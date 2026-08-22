use std::sync::Arc;

use domain::{TelegramId, WorkDay, WorkDayId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, WorkDayRepository};

/// Ошибка команды `/start_day`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StartWorkDayError {
    /// У пользователя уже есть открытый (`finished_at IS NULL`) день —
    /// бизнес-правило, а не паника/сбой хранилища.
    #[error("work day is already open")]
    AlreadyOpen,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Команда `/start_day`: открывает новый рабочий день пользователя. Ошибка,
/// если день уже открыт.
pub struct StartWorkDay {
    work_day_repository: Arc<dyn WorkDayRepository>,
    clock: Arc<dyn Clock>,
}

impl StartWorkDay {
    pub fn new(work_day_repository: Arc<dyn WorkDayRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            work_day_repository,
            clock,
        }
    }

    pub async fn execute(&self, user_id: TelegramId) -> Result<WorkDay, StartWorkDayError> {
        if self
            .work_day_repository
            .find_open_by_user(user_id)
            .await?
            .is_some()
        {
            return Err(StartWorkDayError::AlreadyOpen);
        }

        // `WorkDayId::new(0)` — плейсхолдер, игнорируется репозиторием: id
        // присваивается хранилищем при вставке (см. `WorkDayRepository::create`).
        let draft = WorkDay::new(
            WorkDayId::new(0),
            user_id,
            self.clock.now(),
            None,
            None,
            None,
            None,
        );

        Ok(self.work_day_repository.create(draft).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, InMemoryWorkDayRepository};
    use chrono::Utc;
    use domain::UtcTimestamp;

    fn use_case() -> (StartWorkDay, Arc<InMemoryWorkDayRepository>, UtcTimestamp) {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));
        (
            StartWorkDay::new(work_day_repository.clone(), clock),
            work_day_repository,
            now,
        )
    }

    #[test]
    fn opens_a_new_work_day_for_user() {
        let (use_case, repo, now) = use_case();
        let user_id = TelegramId::new(1).unwrap();

        let work_day = pollster::block_on(use_case.execute(user_id)).unwrap();

        assert_eq!(work_day.user_id(), user_id);
        assert_eq!(work_day.started_at(), now);
        assert_eq!(work_day.finished_at(), None);
        assert_eq!(repo.snapshot(), vec![work_day]);
    }

    #[test]
    fn rejects_start_day_when_a_day_is_already_open() {
        let (use_case, _repo, _now) = use_case();
        let user_id = TelegramId::new(1).unwrap();

        pollster::block_on(use_case.execute(user_id)).unwrap();
        let error = pollster::block_on(use_case.execute(user_id)).unwrap_err();

        assert_eq!(error, StartWorkDayError::AlreadyOpen);
    }
}
