use async_trait::async_trait;
use application::{HourIntervalRepository, RepoError};
use chrono::{DateTime, Utc};
use domain::{HourInterval, HourIntervalId, SuccessfulCount, WorkDayId};
use sqlx::{Row, SqlitePool};

use crate::error::map_sqlx_error;

pub struct SqliteHourIntervalRepository {
    pool: SqlitePool,
}

impl SqliteHourIntervalRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_hour_interval(row: sqlx::sqlite::SqliteRow) -> Result<HourInterval, RepoError> {
    let successful_count: i64 = row.get("successful_count");
    let successful_count = SuccessfulCount::new(successful_count as u8)
        .map_err(|err| RepoError::Storage(err.to_string()))?;

    Ok(HourInterval::new(
        HourIntervalId::new(row.get("id")),
        WorkDayId::new(row.get("work_day_id")),
        row.get::<DateTime<Utc>, _>("started_at").into(),
        row.get::<Option<DateTime<Utc>>, _>("ended_at").map(Into::into),
        successful_count,
        row.get::<Option<String>, _>("summary"),
    ))
}

#[async_trait]
impl HourIntervalRepository for SqliteHourIntervalRepository {
    async fn create(&self, interval: HourInterval) -> Result<HourInterval, RepoError> {
        let id = sqlx::query(
            "INSERT INTO hour_intervals (work_day_id, started_at, ended_at, successful_count, summary)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(interval.work_day_id().value())
        .bind(interval.started_at().value())
        .bind(interval.ended_at().map(|t| t.value()))
        .bind(interval.successful_count().value())
        .bind(interval.summary())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .last_insert_rowid();

        Ok(HourInterval::new(
            HourIntervalId::new(id),
            interval.work_day_id(),
            interval.started_at(),
            interval.ended_at(),
            interval.successful_count(),
            interval.summary().map(str::to_string),
        ))
    }

    async fn update(&self, interval: HourInterval) -> Result<(), RepoError> {
        let result = sqlx::query(
            "UPDATE hour_intervals SET work_day_id = ?1, started_at = ?2, ended_at = ?3,
             successful_count = ?4, summary = ?5 WHERE id = ?6",
        )
        .bind(interval.work_day_id().value())
        .bind(interval.started_at().value())
        .bind(interval.ended_at().map(|t| t.value()))
        .bind(interval.successful_count().value())
        .bind(interval.summary())
        .bind(interval.id().value())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn find_by_id(&self, id: HourIntervalId) -> Result<Option<HourInterval>, RepoError> {
        let row = sqlx::query(
            "SELECT id, work_day_id, started_at, ended_at, successful_count, summary
             FROM hour_intervals WHERE id = ?1",
        )
        .bind(id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_hour_interval).transpose()
    }

    async fn find_open_by_work_day(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Option<HourInterval>, RepoError> {
        let row = sqlx::query(
            "SELECT id, work_day_id, started_at, ended_at, successful_count, summary
             FROM hour_intervals WHERE work_day_id = ?1 AND ended_at IS NULL",
        )
        .bind(work_day_id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_hour_interval).transpose()
    }

    async fn list_by_work_day(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Vec<HourInterval>, RepoError> {
        let rows = sqlx::query(
            "SELECT id, work_day_id, started_at, ended_at, successful_count, summary
             FROM hour_intervals WHERE work_day_id = ?1 ORDER BY started_at ASC",
        )
        .bind(work_day_id.value())
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_hour_interval).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{connect_in_memory, sample_timestamp};

    async fn seed_work_day(pool: &SqlitePool) -> i64 {
        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (1, '0', ?1)")
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO work_days (user_id, started_at) VALUES (1, ?1)")
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid()
    }

    #[tokio::test]
    async fn create_and_find_round_trips() {
        let pool = connect_in_memory().await;
        let work_day_id = WorkDayId::new(seed_work_day(&pool).await);
        let repo = SqliteHourIntervalRepository::new(pool);

        assert_eq!(
            repo.find_open_by_work_day(work_day_id).await.unwrap(),
            None
        );

        let draft = HourInterval::new(
            HourIntervalId::new(0),
            work_day_id,
            sample_timestamp().into(),
            None,
            SuccessfulCount::zero(),
            None,
        );
        let created = repo.create(draft).await.unwrap();
        assert_eq!(created.id(), HourIntervalId::new(1));

        assert_eq!(
            repo.find_open_by_work_day(work_day_id).await.unwrap(),
            Some(created.clone())
        );
        assert_eq!(
            repo.list_by_work_day(work_day_id).await.unwrap(),
            vec![created.clone()]
        );
        assert_eq!(repo.find_by_id(created.id()).await.unwrap(), Some(created));
    }

    #[tokio::test]
    async fn update_closes_interval_with_summary_and_it_leaves_open_query() {
        let pool = connect_in_memory().await;
        let work_day_id = WorkDayId::new(seed_work_day(&pool).await);
        let repo = SqliteHourIntervalRepository::new(pool);

        let draft = HourInterval::new(
            HourIntervalId::new(0),
            work_day_id,
            sample_timestamp().into(),
            None,
            SuccessfulCount::zero(),
            None,
        );
        let created = repo.create(draft).await.unwrap();

        let closed = HourInterval::new(
            created.id(),
            work_day_id,
            created.started_at(),
            Some(sample_timestamp().into()),
            SuccessfulCount::new(5).unwrap(),
            Some("wrote the report".to_string()),
        );
        repo.update(closed.clone()).await.unwrap();

        assert_eq!(
            repo.find_open_by_work_day(work_day_id).await.unwrap(),
            None
        );
        assert_eq!(repo.find_by_id(created.id()).await.unwrap(), Some(closed));
    }

    #[tokio::test]
    async fn update_missing_interval_returns_not_found() {
        let pool = connect_in_memory().await;
        let work_day_id = WorkDayId::new(seed_work_day(&pool).await);
        let repo = SqliteHourIntervalRepository::new(pool);

        let missing = HourInterval::new(
            HourIntervalId::new(999),
            work_day_id,
            sample_timestamp().into(),
            None,
            SuccessfulCount::zero(),
            None,
        );

        assert_eq!(repo.update(missing).await, Err(RepoError::NotFound));
    }
}
