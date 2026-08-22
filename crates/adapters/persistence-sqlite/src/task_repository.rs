use async_trait::async_trait;
use application::{RepoError, TaskRepository};
use chrono::{DateTime, NaiveDate, Utc};
use domain::{Task, TaskId, TelegramId};
use sqlx::{Row, SqlitePool};

use crate::enums::{task_priority_from_db, task_priority_to_db, task_status_from_db, task_status_to_db};
use crate::error::map_sqlx_error;

pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

impl SqliteTaskRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[allow(clippy::too_many_arguments)]
fn row_to_task(
    id: i64,
    user_id: i64,
    title: String,
    status: String,
    priority: Option<String>,
    deadline: Option<NaiveDate>,
    created_at: DateTime<Utc>,
) -> Result<Task, RepoError> {
    let user_id = TelegramId::new(user_id).map_err(|err| RepoError::Storage(err.to_string()))?;
    let status = task_status_from_db(&status)?;
    let priority = task_priority_from_db(priority)?;
    Task::new(
        TaskId::new(id),
        user_id,
        title,
        status,
        priority,
        deadline,
        created_at.into(),
    )
    .map_err(|err| RepoError::Storage(err.to_string()))
}

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn create(&self, task: Task) -> Result<Task, RepoError> {
        let id = sqlx::query(
            "INSERT INTO tasks (user_id, title, status, priority, deadline, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(task.user_id().value())
        .bind(task.title())
        .bind(task_status_to_db(task.status()))
        .bind(task_priority_to_db(task.priority()))
        .bind(task.deadline())
        .bind(task.created_at().value())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .last_insert_rowid();

        Task::new(
            TaskId::new(id),
            task.user_id(),
            task.title(),
            task.status(),
            task.priority(),
            task.deadline(),
            task.created_at(),
        )
        .map_err(|err| RepoError::Storage(err.to_string()))
    }

    async fn update(&self, task: Task) -> Result<(), RepoError> {
        let result = sqlx::query(
            "UPDATE tasks SET user_id = ?1, title = ?2, status = ?3, priority = ?4, deadline = ?5,
             created_at = ?6 WHERE id = ?7",
        )
        .bind(task.user_id().value())
        .bind(task.title())
        .bind(task_status_to_db(task.status()))
        .bind(task_priority_to_db(task.priority()))
        .bind(task.deadline())
        .bind(task.created_at().value())
        .bind(task.id().value())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn find_by_id(&self, id: TaskId) -> Result<Option<Task>, RepoError> {
        let row = sqlx::query(
            "SELECT id, user_id, title, status, priority, deadline, created_at
             FROM tasks WHERE id = ?1",
        )
        .bind(id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_task_row).transpose()
    }

    async fn list_by_user(&self, user_id: TelegramId) -> Result<Vec<Task>, RepoError> {
        let rows = sqlx::query(
            "SELECT id, user_id, title, status, priority, deadline, created_at
             FROM tasks WHERE user_id = ?1 ORDER BY id ASC",
        )
        .bind(user_id.value())
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_task_row).collect()
    }
}

fn row_to_task_row(row: sqlx::sqlite::SqliteRow) -> Result<Task, RepoError> {
    row_to_task(
        row.get::<i64, _>("id"),
        row.get::<i64, _>("user_id"),
        row.get::<String, _>("title"),
        row.get::<String, _>("status"),
        row.get::<Option<String>, _>("priority"),
        row.get::<Option<NaiveDate>, _>("deadline"),
        row.get::<DateTime<Utc>, _>("created_at"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{connect_in_memory, sample_timestamp};
    use domain::{TaskPriority, TaskStatus};

    async fn seed_user(pool: &SqlitePool, telegram_id: i64) {
        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (?1, '0', ?2)")
            .bind(telegram_id)
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_assigns_id_and_round_trips() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let repo = SqliteTaskRepository::new(pool);
        let user_id = TelegramId::new(1).unwrap();

        let draft = Task::new(
            TaskId::new(0),
            user_id,
            "write report",
            TaskStatus::Ready,
            Some(TaskPriority::High),
            NaiveDate::from_ymd_opt(2026, 12, 31),
            sample_timestamp().into(),
        )
        .unwrap();

        let created = repo.create(draft).await.unwrap();
        assert_eq!(created.id(), TaskId::new(1));

        let found = repo.find_by_id(created.id()).await.unwrap();
        assert_eq!(found, Some(created));
    }

    #[tokio::test]
    async fn create_without_priority_or_deadline_round_trips() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let repo = SqliteTaskRepository::new(pool);
        let user_id = TelegramId::new(1).unwrap();

        let draft = Task::new(
            TaskId::new(0),
            user_id,
            "no priority",
            TaskStatus::NotReady,
            None,
            None,
            sample_timestamp().into(),
        )
        .unwrap();

        let created = repo.create(draft).await.unwrap();
        let found = repo.find_by_id(created.id()).await.unwrap().unwrap();
        assert_eq!(found.priority(), None);
        assert_eq!(found.deadline(), None);
    }

    #[tokio::test]
    async fn update_changes_status_and_list_by_user_reflects_it() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let repo = SqliteTaskRepository::new(pool);
        let user_id = TelegramId::new(1).unwrap();

        let draft = Task::new(
            TaskId::new(0),
            user_id,
            "task",
            TaskStatus::Ready,
            None,
            None,
            sample_timestamp().into(),
        )
        .unwrap();
        let created = repo.create(draft).await.unwrap();

        let in_progress = Task::new(
            created.id(),
            user_id,
            created.title(),
            TaskStatus::InProgress,
            created.priority(),
            created.deadline(),
            created.created_at(),
        )
        .unwrap();
        repo.update(in_progress.clone()).await.unwrap();

        assert_eq!(
            repo.find_by_id(created.id()).await.unwrap(),
            Some(in_progress.clone())
        );
        assert_eq!(repo.list_by_user(user_id).await.unwrap(), vec![in_progress]);
    }

    #[tokio::test]
    async fn update_missing_task_returns_not_found() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let repo = SqliteTaskRepository::new(pool);
        let user_id = TelegramId::new(1).unwrap();

        let missing = Task::new(
            TaskId::new(999),
            user_id,
            "ghost",
            TaskStatus::Ready,
            None,
            None,
            sample_timestamp().into(),
        )
        .unwrap();

        assert_eq!(repo.update(missing).await, Err(RepoError::NotFound));
    }
}
