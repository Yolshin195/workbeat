use std::collections::HashMap;
use std::sync::Arc;

use chrono::Duration;
use domain::{
    daily_norm_progress, CheckStatus, DomainError, HourInterval, HourIntervalState,
    SuccessfulCount, TaskId, TenMinCheck, WorkDay, WorkDayId,
};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{Clock, HourIntervalRepository, Notifier, TenMinCheckRepository, WorkDayRepository};
use crate::use_cases::support::{classify_checks, task_time_totals_for_interval};

/// Ошибка команды `/finish_day`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FinishWorkDayError {
    /// День уже завершён.
    #[error("work day is already finished")]
    AlreadyFinished,

    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Итог дня, возвращаемый `FinishWorkDay` (README.md раздел 1): сколько из 8
/// часовых интервалов закрыто, суммарное время по задачам и обеденное время
/// отдельной строкой (не входит в 8-часовую норму).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDayFinishSummary {
    pub closed_intervals: u8,
    pub norm_met: bool,
    pub task_totals: Vec<(TaskId, Duration)>,
    pub lunch_duration: Option<Duration>,
}

/// Закрывает день (README.md раздел 1). Если на момент вызова есть активный
/// незакрытый часовой интервал, он принудительно закрывается тем же путём,
/// что и `FinishHourInterval` из Задачи 7, но без обязательного `summary` —
/// вопрос "что делали за этот час" не блокирует завершение дня. Три
/// возможных состояния незакрытого интервала обрабатываются раздельно, без
/// каскадов и без "восстановительных" сущностей (см. `tasks.md`, Задача 8):
/// открытая рабочая десятиминутка, открытая десятиминутка отдыха, уже
/// закрытая "жду возврата" (ничего закрывать не нужно).
pub struct FinishWorkDay {
    work_day_repository: Arc<dyn WorkDayRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    clock: Arc<dyn Clock>,
    notifier: Arc<dyn Notifier>,
}

impl FinishWorkDay {
    pub fn new(
        work_day_repository: Arc<dyn WorkDayRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        clock: Arc<dyn Clock>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            clock,
            notifier,
        }
    }

    pub async fn execute(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<WorkDayFinishSummary, FinishWorkDayError> {
        let work_day = self
            .work_day_repository
            .find_by_id(work_day_id)
            .await?
            .ok_or(RepoError::NotFound)?;
        if work_day.finished_at().is_some() {
            return Err(FinishWorkDayError::AlreadyFinished);
        }

        let now = self.clock.now();

        if let Some(open_interval) = self
            .hour_interval_repository
            .find_open_by_work_day(work_day_id)
            .await?
        {
            self.force_close_interval(open_interval, now).await?;
        }

        let intervals = self
            .hour_interval_repository
            .list_by_work_day(work_day_id)
            .await?;
        let closed_intervals = intervals
            .iter()
            .filter(|interval| interval.ended_at().is_some())
            .count() as u8;

        let mut task_totals: HashMap<TaskId, Duration> = HashMap::new();
        for interval in &intervals {
            let checks = self
                .ten_min_check_repository
                .list_by_interval(interval.id())
                .await?;
            for (task_id, duration) in task_time_totals_for_interval(checks) {
                *task_totals.entry(task_id).or_insert_with(Duration::zero) += duration;
            }
        }

        let lunch_duration = match (work_day.lunch_started_at(), work_day.lunch_ended_at()) {
            (Some(started), Some(ended)) => Some(ended.value() - started.value()),
            _ => None,
        };

        let finished_work_day = WorkDay::new(
            work_day.id(),
            work_day.user_id(),
            work_day.started_at(),
            Some(now),
            work_day.lunch_started_at(),
            work_day.lunch_ended_at(),
            work_day.last_idle_prompt_at(),
        );
        self.work_day_repository.update(finished_work_day).await?;

        let progress = daily_norm_progress(closed_intervals);
        let text = format!(
            "День завершён: {}/8 интервалов{}",
            progress.closed_intervals,
            if progress.norm_met { ", норма выполнена" } else { "" }
        );
        self.notifier
            .send(work_day.user_id(), OutboundMessage::Text(text))
            .await?;

        Ok(WorkDayFinishSummary {
            closed_intervals: progress.closed_intervals,
            norm_met: progress.norm_met,
            task_totals: task_totals.into_iter().collect(),
            lunch_duration,
        })
    }

    async fn force_close_interval(
        &self,
        interval: HourInterval,
        now: domain::UtcTimestamp,
    ) -> Result<(), FinishWorkDayError> {
        if let Some(open_check) = self
            .ten_min_check_repository
            .find_open_by_interval(interval.id())
            .await?
        {
            let siblings = self
                .ten_min_check_repository
                .list_by_interval(interval.id())
                .await?;
            let classified = classify_checks(siblings);
            let is_rest = classified
                .rest_check
                .as_ref()
                .is_some_and(|rest| rest.id() == open_check.id());

            let updated = if is_rest {
                // Как `MarkReturnedFromRest`: фактическая длительность, без
                // `reason`, статус не меняется.
                TenMinCheck::new(
                    open_check.id(),
                    open_check.hour_interval_id(),
                    open_check.task_id(),
                    open_check.started_at(),
                    Some(now),
                    open_check.status(),
                    None,
                    None,
                )
            } else {
                // В отличие от обычного `WorkSlotPollDecision`, дальнейшего
                // ожидания уже не будет — длительность фактическая, а не
                // номинальные 10 минут.
                TenMinCheck::new(
                    open_check.id(),
                    open_check.hour_interval_id(),
                    open_check.task_id(),
                    open_check.started_at(),
                    Some(now),
                    CheckStatus::NoResponse,
                    None,
                    None,
                )
            };
            self.ten_min_check_repository.update(updated).await?;
        }
        // Иначе последняя десятиминутка интервала уже закрыта
        // (`find_awaiting_resume()`-состояние) — закрывать нечего.

        let checks_after = self
            .ten_min_check_repository
            .list_by_interval(interval.id())
            .await?;
        let classified_after = classify_checks(checks_after);
        let successful_count = SuccessfulCount::new(HourIntervalState::successful_count(
            &classified_after.closed_work_statuses,
        ))?;

        let closed_interval = HourInterval::new(
            interval.id(),
            interval.work_day_id(),
            interval.started_at(),
            Some(now),
            successful_count,
            None,
        );
        self.hour_interval_repository.update(closed_interval).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        FakeClock, FakeNotifier, InMemoryHourIntervalRepository, InMemoryTenMinCheckRepository,
        InMemoryWorkDayRepository,
    };
    use chrono::Utc;
    use domain::{HourIntervalId, TaskId, TelegramId, TenMinCheckId, UtcTimestamp, WorkDayId};

    struct Fixture {
        use_case: FinishWorkDay,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        notifier: Arc<FakeNotifier>,
        clock: Arc<FakeClock>,
        work_day_id: WorkDayId,
        user_id: TelegramId,
    }

    async fn fixture() -> Fixture {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let notifier = Arc::new(FakeNotifier::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let use_case = FinishWorkDay::new(
            work_day_repository.clone(),
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            clock.clone(),
            notifier.clone(),
        );

        Fixture {
            use_case,
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            notifier,
            clock,
            work_day_id: work_day.id(),
            user_id,
        }
    }

    async fn seed_worked(
        repo: &InMemoryTenMinCheckRepository,
        interval_id: domain::HourIntervalId,
        task_id: TaskId,
        count: i64,
        start: UtcTimestamp,
    ) {
        for i in 0..count {
            let started = UtcTimestamp::new(start.value() + Duration::minutes(i * 10));
            let ended = UtcTimestamp::new(started.value() + Duration::minutes(10));
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                task_id,
                started,
                Some(ended),
                CheckStatus::Worked,
                None,
                None,
            );
            repo.create(draft).await.unwrap();
        }
    }

    #[test]
    fn finish_work_day_excludes_lunch_from_norm_and_reports_lunch_separately() {
        pollster::block_on(async {
            let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
            let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
            let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
            let notifier = Arc::new(FakeNotifier::new());
            let started_at = UtcTimestamp::new(Utc::now());
            let lunch_started_at = UtcTimestamp::new(started_at.value() + Duration::hours(4));
            let lunch_ended_at = UtcTimestamp::new(lunch_started_at.value() + Duration::minutes(30));
            let now = UtcTimestamp::new(lunch_ended_at.value() + Duration::hours(1));
            let clock = Arc::new(FakeClock::new(now));

            let user_id = TelegramId::new(1).unwrap();
            let work_day = WorkDay::new(
                WorkDayId::new(0),
                user_id,
                started_at,
                None,
                Some(lunch_started_at),
                Some(lunch_ended_at),
                None,
            );
            let work_day = work_day_repository.create(work_day).await.unwrap();

            let use_case = FinishWorkDay::new(
                work_day_repository.clone(),
                hour_interval_repository,
                ten_min_check_repository,
                clock,
                notifier,
            );

            let summary = use_case.execute(work_day.id()).await.unwrap();

            assert_eq!(summary.lunch_duration, Some(Duration::minutes(30)));
            assert_eq!(summary.closed_intervals, 0);
        });
    }

    #[test]
    fn forced_finish_with_open_working_check_closes_no_response_with_actual_duration() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = TaskId::new(1);
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = fixture.hour_interval_repository.create(interval).await.unwrap();
            seed_worked(
                &fixture.ten_min_check_repository,
                interval.id(),
                task_id,
                2,
                fixture.clock.now(),
            )
            .await;

            let open_started_at =
                UtcTimestamp::new(fixture.clock.now().value() + Duration::minutes(20));
            let open_check = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task_id,
                open_started_at,
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(open_check).await.unwrap();

            fixture.clock.advance(Duration::minutes(35));
            let finish_at = fixture.clock.now();

            let summary = fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert_eq!(summary.closed_intervals, 1);
            let closed_interval = fixture
                .hour_interval_repository
                .find_by_id(interval.id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(closed_interval.successful_count().value(), 2);
            assert_eq!(closed_interval.ended_at(), Some(finish_at));

            let checks = fixture
                .ten_min_check_repository
                .list_by_interval(interval.id())
                .await
                .unwrap();
            let last = checks
                .iter()
                .find(|c| c.started_at() == open_started_at)
                .unwrap();
            assert_eq!(last.status(), CheckStatus::NoResponse);
            assert_eq!(last.reason(), None);
            // Фактическая, не номинальная (10 мин), длительность.
            assert_eq!(last.ended_at(), Some(finish_at));
            assert_ne!(
                last.ended_at().unwrap().value() - last.started_at().value(),
                Duration::minutes(10)
            );

            assert!(fixture
                .ten_min_check_repository
                .find_open_by_interval(interval.id())
                .await
                .unwrap()
                .is_none());
        });
    }

    #[test]
    fn forced_finish_with_open_rest_check_closes_it_with_actual_duration_and_no_reason() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = TaskId::new(1);
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = fixture.hour_interval_repository.create(interval).await.unwrap();
            seed_worked(
                &fixture.ten_min_check_repository,
                interval.id(),
                task_id,
                5,
                fixture.clock.now(),
            )
            .await;

            let rest_started_at =
                UtcTimestamp::new(fixture.clock.now().value() + Duration::minutes(50));
            let rest_check = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task_id,
                rest_started_at,
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(rest_check).await.unwrap();

            fixture.clock.advance(Duration::minutes(75));
            let finish_at = fixture.clock.now();

            let summary = fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert_eq!(summary.closed_intervals, 1);
            let closed_interval = fixture
                .hour_interval_repository
                .find_by_id(interval.id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(closed_interval.successful_count().value(), 5);

            let checks = fixture
                .ten_min_check_repository
                .list_by_interval(interval.id())
                .await
                .unwrap();
            let rest = checks.iter().find(|c| c.started_at() == rest_started_at).unwrap();
            assert_eq!(rest.status(), CheckStatus::Worked);
            assert_eq!(rest.reason(), None);
            assert_eq!(rest.ended_at(), Some(finish_at));

            // Задача получает 5 успешных + отдых = 60 минут.
            assert_eq!(
                summary.task_totals,
                vec![(task_id, Duration::minutes(60))]
            );
        });
    }

    #[test]
    fn forced_finish_with_already_closed_awaiting_resume_check_closes_nothing_extra() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = TaskId::new(1);
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = fixture.hour_interval_repository.create(interval).await.unwrap();
            seed_worked(
                &fixture.ten_min_check_repository,
                interval.id(),
                task_id,
                2,
                fixture.clock.now(),
            )
            .await;

            let last_started_at =
                UtcTimestamp::new(fixture.clock.now().value() + Duration::minutes(20));
            let last_ended_at = UtcTimestamp::new(last_started_at.value() + Duration::minutes(10));
            let already_closed = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task_id,
                last_started_at,
                Some(last_ended_at),
                CheckStatus::NoResponse,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(already_closed)
                .await
                .unwrap();

            fixture.clock.advance(Duration::minutes(40));
            let finish_at = fixture.clock.now();

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            let checks = fixture
                .ten_min_check_repository
                .list_by_interval(interval.id())
                .await
                .unwrap();
            assert_eq!(checks.len(), 3);
            // Уже закрытая строка не тронута.
            let untouched = checks.iter().find(|c| c.started_at() == last_started_at).unwrap();
            assert_eq!(untouched.ended_at(), Some(last_ended_at));
            assert_ne!(untouched.ended_at(), Some(finish_at));

            let closed_interval = fixture
                .hour_interval_repository
                .find_by_id(interval.id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(closed_interval.ended_at(), Some(finish_at));
            assert_eq!(closed_interval.successful_count().value(), 2);
        });
    }

    #[test]
    fn finishing_marks_work_day_finished_and_notifies_user() {
        pollster::block_on(async {
            let fixture = fixture().await;

            let summary = fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert_eq!(summary.closed_intervals, 0);
            let finished = fixture
                .work_day_repository
                .find_by_id(fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();
            assert!(finished.finished_at().is_some());

            let sent = fixture.notifier.sent_messages();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].0, fixture.user_id);
        });
    }

    #[test]
    fn rejects_finishing_an_already_finished_day() {
        pollster::block_on(async {
            let fixture = fixture().await;

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();
            let error = fixture.use_case.execute(fixture.work_day_id).await.unwrap_err();

            assert_eq!(error, FinishWorkDayError::AlreadyFinished);
        });
    }
}
