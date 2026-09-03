use std::sync::Arc;

use domain::{CheckStatus, DomainError, HourInterval, HourIntervalState, SuccessfulCount, TenMinCheck, WorkDayId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, HourIntervalRepository, TenMinCheckRepository};
use crate::use_cases::support::classify_checks;

/// Ошибка `ForceCloseHourInterval`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ForceCloseHourIntervalError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Принудительно закрывает открытый часовой интервал рабочего дня, не
/// дожидаясь 5/5 успешных десятиминуток — та же логика, что раньше жила
/// только внутри `FinishWorkDay` (закрытие интервала как часть завершения
/// дня, issue #38), теперь переиспользуется также командой `/finish_interval`.
/// Обязательный вопрос "что делали за этот час" (`FinishHourInterval`) не
/// блокирует принудительное закрытие — `summary` всегда `None`.
pub struct ForceCloseHourInterval {
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    clock: Arc<dyn Clock>,
}

impl ForceCloseHourInterval {
    pub fn new(
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            hour_interval_repository,
            ten_min_check_repository,
            clock,
        }
    }

    /// `Ok(None)`, если для этого дня нет открытого интервала — закрывать
    /// нечего, вызывающий код сам решает, что это значит (`FinishWorkDay` —
    /// no-op, `/finish_interval` — сообщение пользователю).
    pub async fn execute(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Option<HourInterval>, ForceCloseHourIntervalError> {
        let Some(interval) = self
            .hour_interval_repository
            .find_open_by_work_day(work_day_id)
            .await?
        else {
            return Ok(None);
        };

        let now = self.clock.now();

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
        self.hour_interval_repository
            .update(closed_interval.clone())
            .await?;

        Ok(Some(closed_interval))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, InMemoryHourIntervalRepository, InMemoryTenMinCheckRepository};
    use chrono::{Duration, Utc};
    use domain::{HourIntervalId, TaskId, TenMinCheckId, UtcTimestamp, WorkDayId};

    struct Fixture {
        use_case: ForceCloseHourInterval,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        clock: Arc<FakeClock>,
        work_day_id: WorkDayId,
    }

    fn fixture() -> Fixture {
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let use_case = ForceCloseHourInterval::new(
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            clock.clone(),
        );

        Fixture {
            use_case,
            hour_interval_repository,
            ten_min_check_repository,
            clock,
            work_day_id: WorkDayId::new(1),
        }
    }

    async fn seed_worked(
        repo: &InMemoryTenMinCheckRepository,
        interval_id: HourIntervalId,
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
    fn returns_none_when_no_open_interval() {
        pollster::block_on(async {
            let fixture = fixture();
            let result = fixture.use_case.execute(fixture.work_day_id).await.unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn closes_open_working_check_as_no_response_with_actual_duration() {
        pollster::block_on(async {
            let fixture = fixture();
            let task_id = TaskId::new(1);
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = fixture
                .hour_interval_repository
                .create(interval)
                .await
                .unwrap();
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
            fixture
                .ten_min_check_repository
                .create(open_check)
                .await
                .unwrap();

            fixture.clock.advance(Duration::minutes(35));
            let finish_at = fixture.clock.now();

            let closed = fixture
                .use_case
                .execute(fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();

            assert_eq!(closed.successful_count().value(), 2);
            assert_eq!(closed.ended_at(), Some(finish_at));
            assert_eq!(closed.summary(), None);

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
            assert_eq!(last.ended_at(), Some(finish_at));
        });
    }

    #[test]
    fn closes_open_rest_check_without_changing_its_status() {
        pollster::block_on(async {
            let fixture = fixture();
            let task_id = TaskId::new(1);
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = fixture
                .hour_interval_repository
                .create(interval)
                .await
                .unwrap();
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
            fixture
                .ten_min_check_repository
                .create(rest_check)
                .await
                .unwrap();

            fixture.clock.advance(Duration::minutes(75));
            let finish_at = fixture.clock.now();

            let closed = fixture
                .use_case
                .execute(fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();

            assert_eq!(closed.successful_count().value(), 5);

            let checks = fixture
                .ten_min_check_repository
                .list_by_interval(interval.id())
                .await
                .unwrap();
            let rest = checks
                .iter()
                .find(|c| c.started_at() == rest_started_at)
                .unwrap();
            assert_eq!(rest.status(), CheckStatus::Worked);
            assert_eq!(rest.reason(), None);
            assert_eq!(rest.ended_at(), Some(finish_at));
        });
    }
}
