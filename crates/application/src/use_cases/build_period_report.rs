use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Datelike, Duration, NaiveDate};
use domain::{daily_norm_progress, TaskId, TelegramId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{
    Clock, HourIntervalRepository, TaskRepository, TenMinCheckRepository, UserRepository,
    WorkDayRepository,
};
use crate::report_dto::{Period, PeriodReportInfo, ReportDto, TaskTimeEntry};
use crate::use_cases::support::task_time_totals_for_interval;

/// Ошибка команды `/report_week`/`/report_month`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BuildPeriodReportError {
    #[error("user not found")]
    UserNotFound,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Строит агрегированный отчёт за неделю/месяц (README.md раздел 5): сумма
/// отработанного времени по задачам, число закрытых дней с выполненной
/// нормой, суммарное обеденное время — по всем `work_days` пользователя,
/// попадающим в период. Границы периода вычисляются по календарной дате
/// `work_days.started_at`, сконвертированной из UTC в `users.timezone` — не
/// по UTC-дате (иначе дни, начавшиеся ближе к полуночи UTC, могли бы
/// попасть не в тот период с точки зрения пользователя). Если `at` не
/// передан, "сегодня" вычисляется здесь же как `Clock.now()`,
/// сконвертированный в таймзону пользователя — это бизнес-правило, а не
/// забота адаптера.
pub struct BuildPeriodReport {
    work_day_repository: Arc<dyn WorkDayRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    task_repository: Arc<dyn TaskRepository>,
    user_repository: Arc<dyn UserRepository>,
    clock: Arc<dyn Clock>,
}

impl BuildPeriodReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        work_day_repository: Arc<dyn WorkDayRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        task_repository: Arc<dyn TaskRepository>,
        user_repository: Arc<dyn UserRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            task_repository,
            user_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: TelegramId,
        period: Period,
        at: Option<NaiveDate>,
    ) -> Result<ReportDto, BuildPeriodReportError> {
        let user = self
            .user_repository
            .find_by_telegram_id(user_id)
            .await?
            .ok_or(BuildPeriodReportError::UserNotFound)?;
        let offset = Duration::minutes(i64::from(user.timezone().minutes()));

        let local_at = match at {
            Some(date) => date,
            None => (self.clock.now().value() + offset).date_naive(),
        };
        let (start_date, end_date) = period_bounds(period, local_at);

        let work_days = self.work_day_repository.list_by_user(user_id).await?;

        let mut task_totals: HashMap<TaskId, Duration> = HashMap::new();
        let mut lunch_total = Duration::zero();
        let mut days_total: u32 = 0;
        let mut days_with_norm_met: u32 = 0;

        for work_day in work_days {
            let local_date = (work_day.started_at().value() + offset).date_naive();
            if local_date < start_date || local_date > end_date {
                continue;
            }
            days_total += 1;

            let intervals = self
                .hour_interval_repository
                .list_by_work_day(work_day.id())
                .await?;
            let mut closed_intervals: u8 = 0;
            for interval in &intervals {
                let checks = self.ten_min_check_repository.list_by_interval(interval.id()).await?;
                for (task_id, duration) in task_time_totals_for_interval(checks) {
                    *task_totals.entry(task_id).or_insert_with(Duration::zero) += duration;
                }
                if interval.ended_at().is_some() {
                    closed_intervals += 1;
                }
            }
            if daily_norm_progress(closed_intervals).norm_met {
                days_with_norm_met += 1;
            }

            if let (Some(started), Some(ended)) =
                (work_day.lunch_started_at(), work_day.lunch_ended_at())
            {
                lunch_total += ended.value() - started.value();
            }
        }

        let mut titles: HashMap<TaskId, String> = HashMap::new();
        for task_id in task_totals.keys().copied() {
            if let Some(task) = self.task_repository.find_by_id(task_id).await? {
                titles.insert(task_id, task.title().to_string());
            }
        }

        let mut task_totals: Vec<TaskTimeEntry> = task_totals
            .into_iter()
            .map(|(task_id, duration)| TaskTimeEntry {
                task_id,
                task_title: titles
                    .get(&task_id)
                    .cloned()
                    .unwrap_or_else(|| format!("#{}", task_id.value())),
                duration,
            })
            .collect();
        task_totals.sort_by_key(|entry| entry.task_id);

        Ok(ReportDto {
            task_totals,
            failed_checks: Vec::new(),
            lunch_duration: lunch_total,
            interval_summaries: Vec::new(),
            day: None,
            period: Some(PeriodReportInfo {
                period,
                start_date,
                end_date,
                days_total,
                days_with_norm_met,
            }),
        })
    }
}

fn period_bounds(period: Period, at: NaiveDate) -> (NaiveDate, NaiveDate) {
    match period {
        Period::Week => {
            let days_from_monday = i64::from(at.weekday().num_days_from_monday());
            let start = at - Duration::days(days_from_monday);
            let end = start + Duration::days(6);
            (start, end)
        }
        Period::Month => {
            let start = NaiveDate::from_ymd_opt(at.year(), at.month(), 1).unwrap();
            let next_month_start = if at.month() == 12 {
                NaiveDate::from_ymd_opt(at.year() + 1, 1, 1).unwrap()
            } else {
                NaiveDate::from_ymd_opt(at.year(), at.month() + 1, 1).unwrap()
            };
            (start, next_month_start - Duration::days(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        FakeClock, InMemoryHourIntervalRepository, InMemoryTaskRepository,
        InMemoryTenMinCheckRepository, InMemoryUserRepository, InMemoryWorkDayRepository,
    };
    use chrono::{TimeZone, Utc};
    use domain::{
        CheckStatus, HourInterval, HourIntervalId, SuccessfulCount, TenMinCheck, TenMinCheckId,
        TimeZoneOffset, User, UtcTimestamp, WorkDay, WorkDayId,
    };

    #[allow(dead_code)]
    struct Fixture {
        use_case: BuildPeriodReport,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        task_repository: Arc<InMemoryTaskRepository>,
        user_repository: Arc<InMemoryUserRepository>,
        clock: Arc<FakeClock>,
        user_id: TelegramId,
    }

    async fn fixture_with_timezone(timezone: TimeZoneOffset, now: UtcTimestamp) -> Fixture {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let user_repository = Arc::new(InMemoryUserRepository::new());
        let clock = Arc::new(FakeClock::new(now));
        let user_id = TelegramId::new(1).unwrap();

        user_repository
            .create(User::new(user_id, timezone, now))
            .await
            .unwrap();

        let use_case = BuildPeriodReport::new(
            work_day_repository.clone(),
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            task_repository.clone(),
            user_repository.clone(),
            clock.clone(),
        );

        Fixture {
            use_case,
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            task_repository,
            user_repository,
            clock,
            user_id,
        }
    }

    fn utc_date(year: i32, month: u32, day: u32, hour: u32) -> UtcTimestamp {
        UtcTimestamp::new(Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap())
    }

    async fn seed_work_day_with_one_closed_worked_interval(
        f: &Fixture,
        started_at: UtcTimestamp,
        task_id: TaskId,
    ) -> WorkDayId {
        let work_day = WorkDay::new(WorkDayId::new(0), f.user_id, started_at, None, None, None, None);
        let work_day = f.work_day_repository.create(work_day).await.unwrap();

        let interval = HourInterval::new(
            HourIntervalId::new(0),
            work_day.id(),
            started_at,
            Some(UtcTimestamp::new(started_at.value() + Duration::minutes(60))),
            SuccessfulCount::new(5).unwrap(),
            Some("done".to_string()),
        );
        let interval = f.hour_interval_repository.create(interval).await.unwrap();

        let mut t = started_at;
        for _ in 0..5 {
            let check = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task_id,
                t,
                Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
                CheckStatus::Worked,
                None,
                None,
            );
            f.ten_min_check_repository.create(check).await.unwrap();
            t = UtcTimestamp::new(t.value() + Duration::minutes(10));
        }
        let rest = TenMinCheck::new(
            TenMinCheckId::new(0),
            interval.id(),
            task_id,
            t,
            Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
            CheckStatus::Worked,
            None,
            None,
        );
        f.ten_min_check_repository.create(rest).await.unwrap();

        work_day.id()
    }

    #[test]
    fn period_bounds_week_covers_monday_to_sunday() {
        // 2026-08-19 is a Wednesday.
        let (start, end) = period_bounds(
            Period::Week,
            NaiveDate::from_ymd_opt(2026, 8, 19).unwrap(),
        );
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 8, 17).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 8, 23).unwrap());
    }

    #[test]
    fn period_bounds_month_covers_full_calendar_month() {
        let (start, end) = period_bounds(
            Period::Month,
            NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
        );
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(end, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }

    #[test]
    fn week_report_excludes_days_outside_period_boundaries() {
        pollster::block_on(async {
            let now = utc_date(2026, 8, 19, 12);
            let f = fixture_with_timezone(TimeZoneOffset::utc(), now).await;
            let task_id = TaskId::new(1);

            // Monday 2026-08-17 (in range), Sunday 2026-08-23 (in range),
            // Monday 2026-08-24 (out of range, next week).
            seed_work_day_with_one_closed_worked_interval(
                &f,
                utc_date(2026, 8, 17, 9),
                task_id,
            )
            .await;
            seed_work_day_with_one_closed_worked_interval(
                &f,
                utc_date(2026, 8, 23, 9),
                task_id,
            )
            .await;
            seed_work_day_with_one_closed_worked_interval(
                &f,
                utc_date(2026, 8, 24, 9),
                task_id,
            )
            .await;

            let report = f
                .use_case
                .execute(f.user_id, Period::Week, Some(NaiveDate::from_ymd_opt(2026, 8, 19).unwrap()))
                .await
                .unwrap();

            let period = report.period.unwrap();
            assert_eq!(period.days_total, 2);
            assert_eq!(period.days_with_norm_met, 0);
            assert_eq!(report.task_totals.len(), 1);
            assert_eq!(report.task_totals[0].duration, Duration::minutes(120));
        });
    }

    #[test]
    fn period_without_explicit_at_uses_user_local_date_east_of_utc() {
        pollster::block_on(async {
            // 2026-08-23 (Sunday) 23:30 UTC is already 2026-08-24 (Monday)
            // 06:30 local time at UTC+7 — a new week has started locally,
            // even though `Clock.now()` is still Sunday in UTC. The period
            // must be computed from the local date, not the UTC date.
            let now = UtcTimestamp::new(Utc.with_ymd_and_hms(2026, 8, 23, 23, 30, 0).unwrap());
            let timezone = TimeZoneOffset::new(7 * 60).unwrap();
            let f = fixture_with_timezone(timezone, now).await;
            let task_id = TaskId::new(1);

            // UTC 2026-08-24 01:00 is local 2026-08-24 08:00 (Monday) — falls
            // in the *new* local week, not the week containing UTC "today".
            seed_work_day_with_one_closed_worked_interval(
                &f,
                utc_date(2026, 8, 24, 1),
                task_id,
            )
            .await;

            let report = f.use_case.execute(f.user_id, Period::Week, None).await.unwrap();

            let period = report.period.unwrap();
            // Week of local "today" (Monday 2026-08-24) is 2026-08-24 ..
            // 2026-08-30 — not the previous week (2026-08-17 .. 2026-08-23)
            // that UTC "today" (still Sunday 2026-08-23) would have produced.
            assert_eq!(period.start_date, NaiveDate::from_ymd_opt(2026, 8, 24).unwrap());
            assert_eq!(period.end_date, NaiveDate::from_ymd_opt(2026, 8, 30).unwrap());
            assert_eq!(period.days_total, 1);
        });
    }

    #[test]
    fn missing_user_is_reported() {
        pollster::block_on(async {
            let f = fixture_with_timezone(TimeZoneOffset::utc(), utc_date(2026, 1, 1, 0)).await;
            let error = f
                .use_case
                .execute(TelegramId::new(999).unwrap(), Period::Week, None)
                .await
                .unwrap_err();
            assert_eq!(error, BuildPeriodReportError::UserNotFound);
        });
    }
}
