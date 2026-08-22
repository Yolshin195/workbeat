use std::sync::Arc;

use chrono::NaiveDate;
use domain::{DomainError, Task, TaskId, TaskPriority, TaskStatus, TelegramId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, TaskRepository};

/// Ошибка команды создания задачи.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CreateTaskError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Создаёт новую задачу в общем пуле пользователя. Задача всегда создаётся в
/// статусе `Ready` — сразу доступна для выбора в интервал; если она ещё не
/// готова к работе, пользователь переводит её в `NotReady` через `UpdateTask`.
pub struct CreateTask {
    task_repository: Arc<dyn TaskRepository>,
    clock: Arc<dyn Clock>,
}

impl CreateTask {
    pub fn new(task_repository: Arc<dyn TaskRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            task_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        user_id: TelegramId,
        title: impl Into<String>,
        priority: Option<TaskPriority>,
        deadline: Option<NaiveDate>,
    ) -> Result<Task, CreateTaskError> {
        // `TaskId::new(0)` — плейсхолдер, игнорируется репозиторием: id
        // присваивается хранилищем при вставке (см. `TaskRepository::create`).
        let draft = Task::new(
            TaskId::new(0),
            user_id,
            title,
            TaskStatus::Ready,
            priority,
            deadline,
            self.clock.now(),
        )?;

        Ok(self.task_repository.create(draft).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, InMemoryTaskRepository};
    use chrono::Utc;
    use domain::UtcTimestamp;

    fn use_case() -> (CreateTask, Arc<InMemoryTaskRepository>) {
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let clock = Arc::new(FakeClock::new(UtcTimestamp::new(Utc::now())));
        (
            CreateTask::new(task_repository.clone(), clock),
            task_repository,
        )
    }

    #[test]
    fn creates_task_ready_by_default() {
        let (use_case, repo) = use_case();
        let user_id = TelegramId::new(1).unwrap();

        let task =
            pollster::block_on(use_case.execute(user_id, "Write report", None, None)).unwrap();

        assert_eq!(task.title(), "Write report");
        assert_eq!(task.status(), TaskStatus::Ready);
        assert_eq!(task.priority(), None);
        assert_eq!(task.deadline(), None);
        assert_eq!(repo.snapshot(), vec![task]);
    }

    #[test]
    fn creates_task_with_priority_and_deadline() {
        let (use_case, _repo) = use_case();
        let user_id = TelegramId::new(1).unwrap();
        let deadline = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();

        let task = pollster::block_on(use_case.execute(
            user_id,
            "Ship MVP",
            Some(TaskPriority::High),
            Some(deadline),
        ))
        .unwrap();

        assert_eq!(task.priority(), Some(TaskPriority::High));
        assert_eq!(task.deadline(), Some(deadline));
    }

    #[test]
    fn rejects_empty_title() {
        let (use_case, repo) = use_case();
        let user_id = TelegramId::new(1).unwrap();

        let error = pollster::block_on(use_case.execute(user_id, "   ", None, None)).unwrap_err();

        assert_eq!(error, CreateTaskError::Domain(DomainError::EmptyTaskTitle));
        assert!(repo.snapshot().is_empty());
    }
}
