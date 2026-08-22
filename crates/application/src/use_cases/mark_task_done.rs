use std::sync::Arc;

use domain::{Task, TaskId, TaskStatus};

use crate::error::RepoError;
use crate::ports::TaskRepository;

/// Внутренний use case, переиспользуемый Задачей 7 по команде "задача
/// готова" — не вызывается напрямую из Telegram-адаптера.
pub struct MarkTaskDone {
    task_repository: Arc<dyn TaskRepository>,
}

impl MarkTaskDone {
    pub fn new(task_repository: Arc<dyn TaskRepository>) -> Self {
        Self { task_repository }
    }

    pub async fn execute(&self, task_id: TaskId) -> Result<Task, RepoError> {
        let existing = self
            .task_repository
            .find_by_id(task_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        let updated = Task::new(
            existing.id(),
            existing.user_id(),
            existing.title(),
            TaskStatus::Done,
            existing.priority(),
            existing.deadline(),
            existing.created_at(),
        )
        .expect("existing task fields were already validated");

        self.task_repository.update(updated.clone()).await?;
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::InMemoryTaskRepository;
    use chrono::Utc;
    use domain::{TelegramId, UtcTimestamp};

    fn use_case() -> (MarkTaskDone, Arc<InMemoryTaskRepository>) {
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        (MarkTaskDone::new(task_repository.clone()), task_repository)
    }

    #[test]
    fn marks_task_done() {
        let (use_case, repo) = use_case();
        let user_id = TelegramId::new(1).unwrap();
        let draft = Task::new(
            TaskId::new(0),
            user_id,
            "Write report",
            TaskStatus::InProgress,
            None,
            None,
            UtcTimestamp::new(Utc::now()),
        )
        .unwrap();
        let created = pollster::block_on(repo.create(draft)).unwrap();

        let updated = pollster::block_on(use_case.execute(created.id())).unwrap();

        assert_eq!(updated.status(), TaskStatus::Done);
        assert_eq!(
            pollster::block_on(repo.find_by_id(created.id())).unwrap(),
            Some(updated)
        );
    }

    #[test]
    fn errors_when_task_not_found() {
        let (use_case, _repo) = use_case();

        let error = pollster::block_on(use_case.execute(TaskId::new(999))).unwrap_err();

        assert_eq!(error, RepoError::NotFound);
    }
}
