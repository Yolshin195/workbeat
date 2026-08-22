use async_trait::async_trait;
use application::{RepoError, WorkDayRepository};
use chrono::{DateTime, Utc};
use domain::{TelegramId, WorkDay, WorkDayId};
use sqlx::{Row, SqlitePool};

use crate::error::map_sqlx_error;

pub struct SqliteWorkDayRepository {
    pool: SqlitePool,
}

impl SqliteWorkDayRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_work_day(row: sqlx::sqlite::SqliteRow) -> Result<WorkDay, RepoError> {
    let id: i64 = row.get("id");
    let user_id: i64 = row.get("user_id");
    let user_id = TelegramId::new(user_id).map_err(|err| RepoError::Storage(err.to_string()))?;

    Ok(WorkDay::new(
        WorkDayId::new(id),
        user_id,
        row.get::<DateTime<Utc>, _>("started_at").into(),
        row.get::<Option<DateTime<Utc>>, _>("finished_at")
            .map(Into::into),
        row.get::<Option<DateTime<Utc>>, _>("lunch_started_at")
            .map(Into::into),
        row.get::<Option<DateTime<Utc>>, _>("lunch_ended_at")
            .map(Into::into),
        row.get::<Option<DateTime<Utc>>, _>("last_idle_prompt_at")
            .map(Into::into),
    ))
}

#[async_trait]
impl WorkDayRepository for SqliteWorkDayRepository {
    async fn create(&self, work_day: WorkDay) -> Result<WorkDay, RepoError> {
        let id = sqlx::query(
            "INSERT INTO work_days
             (user_id, started_at, finished_at, lunch_started_at, lunch_ended_at, last_idle_prompt_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(work_day.user_id().value())
        .bind(work_day.started_at().value())
        .bind(work_day.finished_at().map(|t| t.value()))
        .bind(work_day.lunch_started_at().map(|t| t.value()))
        .bind(work_day.lunch_ended_at().map(|t| t.value()))
        .bind(work_day.last_idle_prompt_at().map(|t| t.value()))
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .last_insert_rowid();

        Ok(WorkDay::new(
            WorkDayId::new(id),
            work_day.user_id(),
            work_day.started_at(),
            work_day.finished_at(),
            work_day.lunch_started_at(),
            work_day.lunch_ended_at(),
            work_day.last_idle_prompt_at(),
        ))
    }

    async fn update(&self, work_day: WorkDay) -> Result<(), RepoError> {
        let result = sqlx::query(
            "UPDATE work_days SET user_id = ?1, started_at = ?2, finished_at = ?3,
             lunch_started_at = ?4, lunch_ended_at = ?5, last_idle_prompt_at = ?6
             WHERE id = ?7",
        )
        .bind(work_day.user_id().value())
        .bind(work_day.started_at().value())
        .bind(work_day.finished_at().map(|t| t.value()))
        .bind(work_day.lunch_started_at().map(|t| t.value()))
        .bind(work_day.lunch_ended_at().map(|t| t.value()))
        .bind(work_day.last_idle_prompt_at().map(|t| t.value()))
        .bind(work_day.id().value())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn find_by_id(&self, id: WorkDayId) -> Result<Option<WorkDay>, RepoError> {
        let row = sqlx::query(
            "SELECT id, user_id, started_at, finished_at, lunch_started_at, lunch_ended_at,
             last_idle_prompt_at FROM work_days WHERE id = ?1",
        )
        .bind(id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_work_day).transpose()
    }

    async fn find_open_by_user(&self, user_id: TelegramId) -> Result<Option<WorkDay>, RepoError> {
        let row = sqlx::query(
            "SELECT id, user_id, started_at, finished_at, lunch_started_at, lunch_ended_at,
             last_idle_prompt_at FROM work_days WHERE user_id = ?1 AND finished_at IS NULL",
        )
        .bind(user_id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_work_day).transpose()
    }

    async fn list_by_user(&self, user_id: TelegramId) -> Result<Vec<WorkDay>, RepoError> {
        let rows = sqlx::query(
            "SELECT id, user_id, started_at, finished_at, lunch_started_at, lunch_ended_at,
             last_idle_prompt_at FROM work_days WHERE user_id = ?1 ORDER BY started_at ASC",
        )
        .bind(user_id.value())
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_work_day).collect()
    }

    async fn find_open_without_active_interval(&self) -> Result<Vec<WorkDay>, RepoError> {
        let rows = sqlx::query(
            "SELECT wd.id, wd.user_id, wd.started_at, wd.finished_at, wd.lunch_started_at,
             wd.lunch_ended_at, wd.last_idle_prompt_at
             FROM work_days wd
             WHERE wd.finished_at IS NULL
             AND NOT EXISTS (
                 SELECT 1 FROM hour_intervals hi
                 WHERE hi.work_day_id = wd.id AND hi.ended_at IS NULL
             )
             ORDER BY wd.started_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_work_day).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{connect_in_memory, sample_timestamp};

    async fn seed_user(pool: &SqlitePool, telegram_id: i64) {
        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (?1, '0', ?2)")
            .bind(telegram_id)
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_and_find_round_trips() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let repo = SqliteWorkDayRepository::new(pool);
        let user_id = TelegramId::new(1).unwrap();

        let draft = WorkDay::new(
            WorkDayId::new(0),
            user_id,
            sample_timestamp().into(),
            None,
            None,
            None,
            None,
        );
        let created = repo.create(draft).await.unwrap();
        assert_eq!(created.id(), WorkDayId::new(1));

        assert_eq!(
            repo.find_by_id(created.id()).await.unwrap(),
            Some(created.clone())
        );
        assert_eq!(
            repo.find_open_by_user(user_id).await.unwrap(),
            Some(created.clone())
        );
        assert_eq!(
            repo.find_open_without_active_interval().await.unwrap(),
            vec![created]
        );
    }

    #[tokio::test]
    async fn find_open_without_active_interval_excludes_day_with_open_interval() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let user_id = TelegramId::new(1).unwrap();

        let work_day_id: i64 = sqlx::query(
            "INSERT INTO work_days (user_id, started_at) VALUES (?1, ?2)",
        )
        .bind(user_id.value())
        .bind(sample_timestamp())
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_rowid();

        sqlx::query("INSERT INTO hour_intervals (work_day_id, started_at) VALUES (?1, ?2)")
            .bind(work_day_id)
            .bind(sample_timestamp())
            .execute(&pool)
            .await
            .unwrap();

        let repo = SqliteWorkDayRepository::new(pool);
        assert_eq!(
            repo.find_open_without_active_interval().await.unwrap(),
            Vec::new()
        );
    }

    #[tokio::test]
    async fn update_finishes_day_and_it_disappears_from_open_queries() {
        let pool = connect_in_memory().await;
        seed_user(&pool, 1).await;
        let repo = SqliteWorkDayRepository::new(pool);
        let user_id = TelegramId::new(1).unwrap();

        let draft = WorkDay::new(
            WorkDayId::new(0),
            user_id,
            sample_timestamp().into(),
            None,
            None,
            None,
            None,
        );
        let created = repo.create(draft).await.unwrap();

        let finished = WorkDay::new(
            created.id(),
            user_id,
            created.started_at(),
            Some(sample_timestamp().into()),
            None,
            None,
            None,
        );
        repo.update(finished).await.unwrap();

        assert_eq!(repo.find_open_by_user(user_id).await.unwrap(), None);
        assert_eq!(
            repo.find_open_without_active_interval().await.unwrap(),
            Vec::new()
        );
    }
}
