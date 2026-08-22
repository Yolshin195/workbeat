use std::sync::Arc;

use domain::{WorkDay, WorkDayId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, WorkDayRepository};

/// Ошибка команды "вернулся" (с обеда).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EndLunchError {
    /// Обед не был начат — завершать нечего.
    #[error("lunch was not started")]
    NotStarted,

    /// Обед уже завершён ранее.
    #[error("lunch is already ended")]
    AlreadyEnded,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Команда "вернулся" (с обеда): фиксирует `lunch_ended_at = now`.
pub struct EndLunch {
    work_day_repository: Arc<dyn WorkDayRepository>,
    clock: Arc<dyn Clock>,
}

impl EndLunch {
    pub fn new(work_day_repository: Arc<dyn WorkDayRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            work_day_repository,
            clock,
        }
    }

    pub async fn execute(&self, work_day_id: WorkDayId) -> Result<WorkDay, EndLunchError> {
        let work_day = self
            .work_day_repository
            .find_by_id(work_day_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        if work_day.lunch_started_at().is_none() {
            return Err(EndLunchError::NotStarted);
        }
        if work_day.lunch_ended_at().is_some() {
            return Err(EndLunchError::AlreadyEnded);
        }

        let updated = WorkDay::new(
            work_day.id(),
            work_day.user_id(),
            work_day.started_at(),
            work_day.finished_at(),
            work_day.lunch_started_at(),
            Some(self.clock.now()),
            work_day.last_idle_prompt_at(),
        );
        self.work_day_repository.update(updated.clone()).await?;

        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, InMemoryWorkDayRepository};
    use chrono::Utc;
    use domain::{TelegramId, UtcTimestamp};

    struct Fixture {
        use_case: EndLunch,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        work_day_id: WorkDayId,
        lunch_started_at: UtcTimestamp,
        now: UtcTimestamp,
    }

    async fn fixture() -> Fixture {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let started_at = UtcTimestamp::new(Utc::now());
        let lunch_started_at = UtcTimestamp::new(started_at.value() + chrono::Duration::hours(4));
        let now = UtcTimestamp::new(lunch_started_at.value() + chrono::Duration::minutes(30));
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(
            WorkDayId::new(0),
            user_id,
            started_at,
            None,
            Some(lunch_started_at),
            None,
            None,
        );
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let use_case = EndLunch::new(work_day_repository.clone(), clock);

        Fixture {
            use_case,
            work_day_repository,
            work_day_id: work_day.id(),
            lunch_started_at,
            now,
        }
    }

    #[test]
    fn ends_lunch_and_keeps_start_time() {
        pollster::block_on(async {
            let fixture = fixture().await;

            let updated = fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert_eq!(updated.lunch_started_at(), Some(fixture.lunch_started_at));
            assert_eq!(updated.lunch_ended_at(), Some(fixture.now));
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
    fn rejects_ending_lunch_that_was_never_started() {
        pollster::block_on(async {
            let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
            let now = UtcTimestamp::new(Utc::now());
            let clock = Arc::new(FakeClock::new(now));
            let user_id = TelegramId::new(1).unwrap();
            let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
            let work_day = work_day_repository.create(work_day).await.unwrap();
            let use_case = EndLunch::new(work_day_repository, clock);

            let error = use_case.execute(work_day.id()).await.unwrap_err();

            assert_eq!(error, EndLunchError::NotStarted);
        });
    }

    #[test]
    fn rejects_ending_lunch_twice() {
        pollster::block_on(async {
            let fixture = fixture().await;

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();
            let error = fixture.use_case.execute(fixture.work_day_id).await.unwrap_err();

            assert_eq!(error, EndLunchError::AlreadyEnded);
        });
    }
}
