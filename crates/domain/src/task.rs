use chrono::NaiveDate;

use crate::error::DomainError;
use crate::ids::TaskId;
use crate::value_objects::{TelegramId, UtcTimestamp};

/// Статус задачи из пула (`tasks.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    NotReady,
    InProgress,
    Done,
}

/// Приоритет задачи (`tasks.priority`), опциональный.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    High,
    Medium,
    Low,
}

/// Задача из общего пула (`tasks` в SPEC.md п.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    user_id: TelegramId,
    title: String,
    status: TaskStatus,
    priority: Option<TaskPriority>,
    deadline: Option<NaiveDate>,
    created_at: UtcTimestamp,
}

impl Task {
    pub fn new(
        id: TaskId,
        user_id: TelegramId,
        title: impl Into<String>,
        status: TaskStatus,
        priority: Option<TaskPriority>,
        deadline: Option<NaiveDate>,
        created_at: UtcTimestamp,
    ) -> Result<Self, DomainError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(DomainError::EmptyTaskTitle);
        }
        Ok(Self {
            id,
            user_id,
            title,
            status,
            priority,
            deadline,
            created_at,
        })
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn user_id(&self) -> TelegramId {
        self.user_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn status(&self) -> TaskStatus {
        self.status
    }

    pub fn priority(&self) -> Option<TaskPriority> {
        self.priority
    }

    pub fn deadline(&self) -> Option<NaiveDate> {
        self.deadline
    }

    pub fn created_at(&self) -> UtcTimestamp {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn fixture() -> (TaskId, TelegramId, UtcTimestamp) {
        (
            TaskId::new(1),
            TelegramId::new(123).unwrap(),
            UtcTimestamp::new(Utc::now()),
        )
    }

    #[test]
    fn constructs_task_with_optional_fields_none() {
        let (id, user_id, created_at) = fixture();

        let task = Task::new(
            id,
            user_id,
            "Write report",
            TaskStatus::Ready,
            None,
            None,
            created_at,
        )
        .unwrap();

        assert_eq!(task.title(), "Write report");
        assert_eq!(task.status(), TaskStatus::Ready);
        assert_eq!(task.priority(), None);
        assert_eq!(task.deadline(), None);
    }

    #[test]
    fn constructs_task_with_priority_and_deadline() {
        let (id, user_id, created_at) = fixture();
        let deadline = NaiveDate::from_ymd_opt(2026, 12, 31).unwrap();

        let task = Task::new(
            id,
            user_id,
            "Ship MVP",
            TaskStatus::InProgress,
            Some(TaskPriority::High),
            Some(deadline),
            created_at,
        )
        .unwrap();

        assert_eq!(task.priority(), Some(TaskPriority::High));
        assert_eq!(task.deadline(), Some(deadline));
    }

    #[test]
    fn rejects_empty_title() {
        let (id, user_id, created_at) = fixture();

        let result = Task::new(
            id,
            user_id,
            "   ",
            TaskStatus::Ready,
            None,
            None,
            created_at,
        );

        assert_eq!(result, Err(DomainError::EmptyTaskTitle));
    }
}
