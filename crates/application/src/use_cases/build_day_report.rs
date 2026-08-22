use std::collections::HashMap;
use std::sync::Arc;

use chrono::Duration;
use domain::{daily_norm_progress, task_time, CheckStatus, HourIntervalState, TaskId, WorkDayId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{HourIntervalRepository, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::report_dto::{DayReportInfo, FailedCheckEntry, ReportDto, TaskTimeEntry};
use crate::use_cases::support::classify_checks;

/// Ошибка команды `/report`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BuildDayReportError {
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Строит дневной отчёт (README.md раздел 5): время по задачам, провальные
/// десятиминутки с причинами, обед, норма 8ч, `summary` каждого завершённого
/// часового интервала. Если день ещё не закрыт и текущий часовой интервал не
/// завершён — в отчёт попадают только уже завершённые десятиминутки этого
/// интервала (см. `classify_checks`), сам интервал не считается «закрытым»
/// для нормы 8 часов и не получает строку в `interval_summaries`.
pub struct BuildDayReport {
    work_day_repository: Arc<dyn WorkDayRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    task_repository: Arc<dyn TaskRepository>,
}

impl BuildDayReport {
    pub fn new(
        work_day_repository: Arc<dyn WorkDayRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        task_repository: Arc<dyn TaskRepository>,
    ) -> Self {
        Self {
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            task_repository,
        }
    }

    pub async fn execute(&self, work_day_id: WorkDayId) -> Result<ReportDto, BuildDayReportError> {
        build_day_report_dto(
            &self.work_day_repository,
            &self.hour_interval_repository,
            &self.ten_min_check_repository,
            &self.task_repository,
            work_day_id,
        )
        .await
    }
}

/// Общая реализация, переиспользуемая `ExportDayReportCsv` — оба используют
/// один и тот же `ReportDto`, отличается только формат сериализации на
/// выходе (текст vs CSV), а не логика построения отчёта.
pub(crate) async fn build_day_report_dto(
    work_day_repository: &Arc<dyn WorkDayRepository>,
    hour_interval_repository: &Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: &Arc<dyn TenMinCheckRepository>,
    task_repository: &Arc<dyn TaskRepository>,
    work_day_id: WorkDayId,
) -> Result<ReportDto, BuildDayReportError> {
    let work_day = work_day_repository
        .find_by_id(work_day_id)
        .await?
        .ok_or(RepoError::NotFound)?;

    let mut intervals = hour_interval_repository.list_by_work_day(work_day_id).await?;
    intervals.sort_by_key(|interval| interval.started_at());

    let mut task_totals: HashMap<TaskId, Duration> = HashMap::new();
    let mut failed_checks: Vec<FailedCheckEntry> = Vec::new();
    let mut interval_summaries: Vec<Option<String>> = Vec::new();
    let mut closed_intervals: u8 = 0;

    for interval in &intervals {
        let checks = ten_min_check_repository.list_by_interval(interval.id()).await?;
        let classified = classify_checks(checks);
        let rest_earned = HourIntervalState::is_rest_owed(&classified.closed_work_statuses);
        let rest_task_id = classified.rest_check.as_ref().map(domain::TenMinCheck::task_id);

        let mut worked_by_task: HashMap<TaskId, u8> = HashMap::new();
        for check in &classified.closed_work_checks {
            if check.status() == CheckStatus::Worked {
                *worked_by_task.entry(check.task_id()).or_insert(0) += 1;
            } else {
                failed_checks.push(FailedCheckEntry {
                    task_id: check.task_id(),
                    task_title: String::new(),
                    status: check.status(),
                    reason: check.reason().map(str::to_string),
                    started_at: check.started_at(),
                });
            }
        }
        if rest_earned {
            if let Some(task_id) = rest_task_id {
                worked_by_task.entry(task_id).or_insert(0);
            }
        }
        for (task_id, worked_count) in worked_by_task {
            let earns_rest = rest_earned && rest_task_id == Some(task_id);
            *task_totals.entry(task_id).or_insert_with(Duration::zero) +=
                task_time(worked_count, earns_rest);
        }

        if interval.ended_at().is_some() {
            closed_intervals += 1;
            interval_summaries.push(interval.summary().map(str::to_string));
        }
    }

    let mut titles: HashMap<TaskId, String> = HashMap::new();
    let mut task_ids: Vec<TaskId> = task_totals.keys().copied().collect();
    for entry in &failed_checks {
        if !task_ids.contains(&entry.task_id) {
            task_ids.push(entry.task_id);
        }
    }
    for task_id in task_ids {
        if let Some(task) = task_repository.find_by_id(task_id).await? {
            titles.insert(task_id, task.title().to_string());
        }
    }
    let title_of = |task_id: TaskId| {
        titles
            .get(&task_id)
            .cloned()
            .unwrap_or_else(|| format!("#{}", task_id.value()))
    };

    let mut task_totals: Vec<TaskTimeEntry> = task_totals
        .into_iter()
        .map(|(task_id, duration)| TaskTimeEntry {
            task_id,
            task_title: title_of(task_id),
            duration,
        })
        .collect();
    task_totals.sort_by_key(|entry| entry.task_id);

    for entry in &mut failed_checks {
        entry.task_title = title_of(entry.task_id);
    }
    failed_checks.sort_by_key(|entry| entry.started_at);

    let lunch_duration = match (work_day.lunch_started_at(), work_day.lunch_ended_at()) {
        (Some(started), Some(ended)) => ended.value() - started.value(),
        _ => Duration::zero(),
    };

    let progress = daily_norm_progress(closed_intervals);

    Ok(ReportDto {
        task_totals,
        failed_checks,
        lunch_duration,
        interval_summaries,
        day: Some(DayReportInfo {
            closed_intervals: progress.closed_intervals,
            norm_met: progress.norm_met,
        }),
        period: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        InMemoryHourIntervalRepository, InMemoryTaskRepository, InMemoryTenMinCheckRepository,
        InMemoryWorkDayRepository,
    };
    use chrono::Utc;
    use domain::{
        HourInterval, HourIntervalId, SuccessfulCount, Task, TaskPriority, TaskStatus, TelegramId,
        TenMinCheck, TenMinCheckId, UtcTimestamp, WorkDay,
    };

    struct Fixture {
        use_case: BuildDayReport,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        task_repository: Arc<InMemoryTaskRepository>,
        user_id: TelegramId,
        started_at: UtcTimestamp,
    }

    async fn fixture() -> Fixture {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let user_id = TelegramId::new(1).unwrap();
        let started_at = UtcTimestamp::new(Utc::now());

        let use_case = BuildDayReport::new(
            work_day_repository.clone(),
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            task_repository.clone(),
        );

        Fixture {
            use_case,
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            task_repository,
            user_id,
            started_at,
        }
    }

    fn worked_check(
        interval_id: HourIntervalId,
        task_id: TaskId,
        started: UtcTimestamp,
    ) -> TenMinCheck {
        TenMinCheck::new(
            TenMinCheckId::new(0),
            interval_id,
            task_id,
            started,
            Some(UtcTimestamp::new(started.value() + Duration::minutes(10))),
            CheckStatus::Worked,
            None,
            None,
        )
    }

    #[test]
    fn day_report_golden_scenario() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let work_day = WorkDay::new(
                domain::WorkDayId::new(0),
                fixture.user_id,
                fixture.started_at,
                None,
                Some(fixture.started_at),
                Some(UtcTimestamp::new(
                    fixture.started_at.value() + Duration::minutes(30),
                )),
                None,
            );
            let work_day = fixture.work_day_repository.create(work_day).await.unwrap();

            let task = Task::new(
                TaskId::new(0),
                fixture.user_id,
                "Write report",
                TaskStatus::InProgress,
                Some(TaskPriority::High),
                None,
                fixture.started_at,
            )
            .unwrap();
            let task = fixture.task_repository.create(task).await.unwrap();

            // Closed interval: 2 Worked, 1 Failed, 3 Worked, 1 Worked (rest) = 7
            // строк, 5 успешных -> rest earned.
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                work_day.id(),
                fixture.started_at,
                Some(UtcTimestamp::new(fixture.started_at.value() + Duration::minutes(70))),
                SuccessfulCount::new(5).unwrap(),
                Some("wrote the report".to_string()),
            );
            let interval = fixture
                .hour_interval_repository
                .create(interval)
                .await
                .unwrap();

            let mut t = fixture.started_at;
            for _ in 0..2 {
                fixture
                    .ten_min_check_repository
                    .create(worked_check(interval.id(), task.id(), t))
                    .await
                    .unwrap();
                t = UtcTimestamp::new(t.value() + Duration::minutes(10));
            }
            let failed = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task.id(),
                t,
                Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            );
            fixture.ten_min_check_repository.create(failed).await.unwrap();
            t = UtcTimestamp::new(t.value() + Duration::minutes(10));
            for _ in 0..3 {
                fixture
                    .ten_min_check_repository
                    .create(worked_check(interval.id(), task.id(), t))
                    .await
                    .unwrap();
                t = UtcTimestamp::new(t.value() + Duration::minutes(10));
            }
            // Rest, closed.
            let rest = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task.id(),
                t,
                Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(rest).await.unwrap();

            // Open second interval with 2 already-closed Worked checks and one
            // still open — only the closed ones must appear in the report,
            // and this interval must not count towards the norm.
            let open_interval = HourInterval::new(
                HourIntervalId::new(0),
                work_day.id(),
                UtcTimestamp::new(fixture.started_at.value() + Duration::hours(2)),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let open_interval = fixture
                .hour_interval_repository
                .create(open_interval)
                .await
                .unwrap();
            let mut t2 = UtcTimestamp::new(fixture.started_at.value() + Duration::hours(2));
            for _ in 0..2 {
                fixture
                    .ten_min_check_repository
                    .create(worked_check(open_interval.id(), task.id(), t2))
                    .await
                    .unwrap();
                t2 = UtcTimestamp::new(t2.value() + Duration::minutes(10));
            }
            let open_check = TenMinCheck::new(
                TenMinCheckId::new(0),
                open_interval.id(),
                task.id(),
                t2,
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(open_check).await.unwrap();

            let report = fixture.use_case.execute(work_day.id()).await.unwrap();

            assert_eq!(report.day, Some(DayReportInfo { closed_intervals: 1, norm_met: false }));
            assert_eq!(report.lunch_duration, Duration::minutes(30));
            assert_eq!(report.interval_summaries, vec![Some("wrote the report".to_string())]);
            // 5 worked + rest (60) from the closed interval, + 2 worked (20)
            // from the open interval's already-closed checks.
            assert_eq!(report.task_totals.len(), 1);
            assert_eq!(report.task_totals[0].task_title, "Write report");
            assert_eq!(report.task_totals[0].duration, Duration::minutes(80));
            assert_eq!(report.failed_checks.len(), 1);
            assert_eq!(report.failed_checks[0].task_title, "Write report");
            assert_eq!(report.failed_checks[0].status, CheckStatus::Failed);
            assert_eq!(report.failed_checks[0].reason.as_deref(), Some("distracted"));

            let text = report.to_text();
            assert_eq!(
                text,
                "Отчёт за день: 1/8 интервалов\n\
                 Время по задачам:\n\
                 \x20\x20Write report: 1ч 20м\n\
                 Провальные десятиминутки:\n\
                 \x20\x20{time} [Write report] Провалено: distracted\n\
                 Обед: 0ч 30м\n\
                 Итоги часов:\n\
                 \x20\x201. wrote the report"
                    .replace(
                        "{time}",
                        &crate::report_dto::format_timestamp(report.failed_checks[0].started_at)
                    )
            );
        });
    }

    #[test]
    fn missing_work_day_is_not_found() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let error = fixture
                .use_case
                .execute(domain::WorkDayId::new(999))
                .await
                .unwrap_err();
            assert_eq!(error, BuildDayReportError::Repo(RepoError::NotFound));
        });
    }
}
