use std::sync::Arc;

use domain::{WorkDay, WorkDayId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, HourIntervalRepository, WorkDayRepository};

/// Ошибка команды "пошёл на обед".
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StartLunchError {
    /// Обед нельзя начать во время активного незакрытого часового интервала —
    /// только между интервалами (README.md раздел 1).
    #[error("cannot start lunch while an hour interval is active")]
    ActiveIntervalOpen,

    /// Обед на этот день уже начат и ещё не завершён.
    #[error("lunch is already in progress")]
    AlreadyStarted,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Команда "пошёл на обед": фиксирует `lunch_started_at = now`. Запрещена во
/// время активного часового интервала.
pub struct StartLunch {
    work_day_repository: Arc<dyn WorkDayRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    clock: Arc<dyn Clock>,
}

impl StartLunch {
    pub fn new(
        work_day_repository: Arc<dyn WorkDayRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            work_day_repository,
            hour_interval_repository,
            clock,
        }
    }

    pub async fn execute(&self, work_day_id: WorkDayId) -> Result<WorkDay, StartLunchError> {
        let work_day = self
            .work_day_repository
            .find_by_id(work_day_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        if work_day.lunch_started_at().is_some() && work_day.lunch_ended_at().is_none() {
            return Err(StartLunchError::AlreadyStarted);
        }

        if self
            .hour_interval_repository
            .find_open_by_work_day(work_day_id)
            .await?
            .is_some()
        {
            return Err(StartLunchError::ActiveIntervalOpen);
        }

        let updated = WorkDay::new(
            work_day.id(),
            work_day.user_id(),
            work_day.started_at(),
            work_day.finished_at(),
            Some(self.clock.now()),
            None,
            work_day.last_idle_prompt_at(),
        );
        self.work_day_repository.update(updated.clone()).await?;

        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, InMemoryHourIntervalRepository, InMemoryWorkDayRepository};
    use chrono::Utc;
    use domain::{HourInterval, HourIntervalId, SuccessfulCount, TelegramId, UtcTimestamp};

    struct Fixture {
        use_case: StartLunch,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        work_day_id: WorkDayId,
        now: UtcTimestamp,
    }

    async fn fixture() -> Fixture {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let use_case = StartLunch::new(
            work_day_repository.clone(),
            hour_interval_repository.clone(),
            clock,
        );

        Fixture {
            use_case,
            work_day_repository,
            hour_interval_repository,
            work_day_id: work_day.id(),
            now,
        }
    }

    #[test]
    fn starts_lunch_between_intervals() {
        pollster::block_on(async {
            let fixture = fixture().await;

            let updated = fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert_eq!(updated.lunch_started_at(), Some(fixture.now));
            assert_eq!(updated.lunch_ended_at(), None);
            assert_eq!(
                fixture
                    .work_day_repository
                    .find_by_id(fixture.work_day_id)
                    .await
                    .unwrap()
                    .unwrap(),
                updated
            );
        });
    }

    #[test]
    fn rejects_starting_lunch_during_an_active_interval() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.now,
                None,
                SuccessfulCount::zero(),
                None,
            );
            fixture
                .hour_interval_repository
                .create(interval)
                .await
                .unwrap();

            let error = fixture.use_case.execute(fixture.work_day_id).await.unwrap_err();

            assert_eq!(error, StartLunchError::ActiveIntervalOpen);
        });
    }

    #[test]
    fn rejects_starting_lunch_twice() {
        pollster::block_on(async {
            let fixture = fixture().await;

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();
            let error = fixture.use_case.execute(fixture.work_day_id).await.unwrap_err();

            assert_eq!(error, StartLunchError::AlreadyStarted);
        });
    }
}
