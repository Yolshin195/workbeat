use std::sync::Arc;

use chrono::NaiveDate;
use domain::{DomainError, Task, TaskId, TaskPriority, TaskStatus};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::TaskRepository;

/// Ошибка команды редактирования задачи.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UpdateTaskError {
    /// `status: Some(InProgress)` — единственный статус, который пользователь
    /// не может выставить вручную: он проставляется автоматически стартом
    /// часового интервала (`MarkTaskInProgress`). `Ready`/`NotReady`/`Done`
    /// пользователь вправе выставить в любой комбинации через редактирование
    /// (например, отменить неактуальную задачу как `Done` в обход интервала).
    #[error("status can be Ready, NotReady or Done — InProgress is set automatically when an hour interval starts")]
    InProgressNotAllowed,

    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Редактирование полей задачи, задаваемых пользователем вручную (SPEC.md
/// раздел 3). `priority`/`deadline` — `Option<Option<_>>`: внешний `None`
/// значит "не менять", `Some(None)` — "очистить поле", `Some(Some(value))` —
/// "установить значение". `title`/`status` — обычный `Option<_>`, так как
/// пустого значения там не бывает ("не менять" — тоже `None`).
pub struct UpdateTask {
    task_repository: Arc<dyn TaskRepository>,
}

impl UpdateTask {
    pub fn new(task_repository: Arc<dyn TaskRepository>) -> Self {
        Self { task_repository }
    }

    pub async fn execute(
        &self,
        task_id: TaskId,
        title: Option<String>,
        priority: Option<Option<TaskPriority>>,
        deadline: Option<Option<NaiveDate>>,
        status: Option<TaskStatus>,
    ) -> Result<Task, UpdateTaskError> {
        if status == Some(TaskStatus::InProgress) {
            return Err(UpdateTaskError::InProgressNotAllowed);
        }

        let existing = self
            .task_repository
            .find_by_id(task_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        let updated = Task::new(
            existing.id(),
            existing.user_id(),
            title.unwrap_or_else(|| existing.title().to_string()),
            status.unwrap_or_else(|| existing.status()),
            priority.unwrap_or_else(|| existing.priority()),
            deadline.unwrap_or_else(|| existing.deadline()),
            existing.created_at(),
        )?;

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

    fn use_case() -> (UpdateTask, Arc<InMemoryTaskRepository>) {
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        (UpdateTask::new(task_repository.clone()), task_repository)
    }

    async fn seed_task(repo: &InMemoryTaskRepository) -> Task {
        let user_id = TelegramId::new(1).unwrap();
        let draft = Task::new(
            TaskId::new(0),
            user_id,
            "Write report",
            TaskStatus::NotReady,
            Some(TaskPriority::Low),
            Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            UtcTimestamp::new(Utc::now()),
        )
        .unwrap();
        repo.create(draft).await.unwrap()
    }

    #[test]
    fn updates_only_provided_fields() {
        let (use_case, repo) = use_case();
        let task = pollster::block_on(seed_task(&repo));

        let updated = pollster::block_on(use_case.execute(
            task.id(),
            Some("Write final report".to_string()),
            None,
            None,
            None,
        ))
        .unwrap();

        assert_eq!(updated.title(), "Write final report");
        // Не переданные поля остаются как были.
        assert_eq!(updated.status(), TaskStatus::NotReady);
        assert_eq!(updated.priority(), Some(TaskPriority::Low));
        assert_eq!(
            updated.deadline(),
            Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
        );
    }

    #[test]
    fn clears_priority_and_deadline_when_explicitly_set_to_none() {
        let (use_case, repo) = use_case();
        let task = pollster::block_on(seed_task(&repo));

        let updated =
            pollster::block_on(use_case.execute(task.id(), None, Some(None), Some(None), None))
                .unwrap();

        assert_eq!(updated.priority(), None);
        assert_eq!(updated.deadline(), None);
    }

    #[test]
    fn allows_manual_ready_not_ready_and_done_transitions() {
        let (use_case, repo) = use_case();
        let task = pollster::block_on(seed_task(&repo));

        let ready = pollster::block_on(use_case.execute(
            task.id(),
            None,
            None,
            None,
            Some(TaskStatus::Ready),
        ))
        .unwrap();
        assert_eq!(ready.status(), TaskStatus::Ready);

        let done = pollster::block_on(use_case.execute(
            task.id(),
            None,
            None,
            None,
            Some(TaskStatus::Done),
        ))
        .unwrap();
        assert_eq!(done.status(), TaskStatus::Done);
    }

    #[test]
    fn rejects_setting_in_progress_manually() {
        let (use_case, repo) = use_case();
        let task = pollster::block_on(seed_task(&repo));

        let error = pollster::block_on(use_case.execute(
            task.id(),
            None,
            None,
            None,
            Some(TaskStatus::InProgress),
        ))
        .unwrap_err();

        assert_eq!(error, UpdateTaskError::InProgressNotAllowed);
        // Задача в хранилище не изменилась.
        assert_eq!(
            pollster::block_on(repo.find_by_id(task.id())).unwrap(),
            Some(task)
        );
    }

    #[test]
    fn rejects_empty_title() {
        let (use_case, repo) = use_case();
        let task = pollster::block_on(seed_task(&repo));

        let error = pollster::block_on(use_case.execute(
            task.id(),
            Some("   ".to_string()),
            None,
            None,
            None,
        ))
        .unwrap_err();

        assert_eq!(error, UpdateTaskError::Domain(DomainError::EmptyTaskTitle));
    }

    #[test]
    fn errors_when_task_not_found() {
        let (use_case, _repo) = use_case();

        let error = pollster::block_on(use_case.execute(TaskId::new(999), None, None, None, None))
            .unwrap_err();

        assert_eq!(error, UpdateTaskError::Repo(RepoError::NotFound));
    }
}
