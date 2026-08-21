use async_trait::async_trait;
use application::{RepoError, TenMinCheckRepository};
use chrono::{DateTime, Utc};
use domain::{HourIntervalId, TaskId, TenMinCheck, TenMinCheckId};
use sqlx::{Row, SqlitePool};

use crate::enums::{check_status_from_db, check_status_to_db};
use crate::error::map_sqlx_error;

pub struct SqliteTenMinCheckRepository {
    pool: SqlitePool,
}

impl SqliteTenMinCheckRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_ten_min_check(row: sqlx::sqlite::SqliteRow) -> Result<TenMinCheck, RepoError> {
    let status: String = row.get("status");
    let status = check_status_from_db(&status)?;

    Ok(TenMinCheck::new(
        TenMinCheckId::new(row.get("id")),
        HourIntervalId::new(row.get("hour_interval_id")),
        TaskId::new(row.get("task_id")),
        row.get::<DateTime<Utc>, _>("started_at").into(),
        row.get::<Option<DateTime<Utc>>, _>("ended_at").map(Into::into),
        status,
        row.get::<Option<String>, _>("reason"),
        row.get::<Option<DateTime<Utc>>, _>("last_reminder_at")
            .map(Into::into),
    ))
}

const SELECT_COLUMNS: &str =
    "id, hour_interval_id, task_id, started_at, ended_at, status, reason, last_reminder_at";

#[async_trait]
impl TenMinCheckRepository for SqliteTenMinCheckRepository {
    async fn create(&self, check: TenMinCheck) -> Result<TenMinCheck, RepoError> {
        let id = sqlx::query(
            "INSERT INTO ten_min_checks
             (hour_interval_id, task_id, started_at, ended_at, status, reason, last_reminder_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(check.hour_interval_id().value())
        .bind(check.task_id().value())
        .bind(check.started_at().value())
        .bind(check.ended_at().map(|t| t.value()))
        .bind(check_status_to_db(check.status()))
        .bind(check.reason())
        .bind(check.last_reminder_at().map(|t| t.value()))
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .last_insert_rowid();

        Ok(TenMinCheck::new(
            TenMinCheckId::new(id),
            check.hour_interval_id(),
            check.task_id(),
            check.started_at(),
            check.ended_at(),
            check.status(),
            check.reason().map(str::to_string),
            check.last_reminder_at(),
        ))
    }

    async fn update(&self, check: TenMinCheck) -> Result<(), RepoError> {
        let result = sqlx::query(
            "UPDATE ten_min_checks SET hour_interval_id = ?1, task_id = ?2, started_at = ?3,
             ended_at = ?4, status = ?5, reason = ?6, last_reminder_at = ?7 WHERE id = ?8",
        )
        .bind(check.hour_interval_id().value())
        .bind(check.task_id().value())
        .bind(check.started_at().value())
        .bind(check.ended_at().map(|t| t.value()))
        .bind(check_status_to_db(check.status()))
        .bind(check.reason())
        .bind(check.last_reminder_at().map(|t| t.value()))
        .bind(check.id().value())
        .execute(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn find_by_id(&self, id: TenMinCheckId) -> Result<Option<TenMinCheck>, RepoError> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS} FROM ten_min_checks WHERE id = ?1"
        ))
        .bind(id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_ten_min_check).transpose()
    }

    async fn find_open_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS} FROM ten_min_checks
             WHERE hour_interval_id = ?1 AND ended_at IS NULL"
        ))
        .bind(hour_interval_id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_ten_min_check).transpose()
    }

    async fn find_last_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError> {
        let row = sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS} FROM ten_min_checks
             WHERE hour_interval_id = ?1 ORDER BY started_at DESC LIMIT 1"
        ))
        .bind(hour_interval_id.value())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(row_to_ten_min_check).transpose()
    }

    async fn list_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Vec<TenMinCheck>, RepoError> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS} FROM ten_min_checks
             WHERE hour_interval_id = ?1 ORDER BY started_at ASC"
        ))
        .bind(hour_interval_id.value())
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_ten_min_check).collect()
    }

    async fn find_open(&self) -> Result<Vec<TenMinCheck>, RepoError> {
        let rows = sqlx::query(&format!(
            "SELECT {SELECT_COLUMNS} FROM ten_min_checks WHERE ended_at IS NULL"
        ))
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_ten_min_check).collect()
    }

    async fn find_awaiting_resume(&self) -> Result<Vec<TenMinCheck>, RepoError> {
        // Последняя по времени старта десятиминутка ещё не закрытого интервала:
        // если она сама закрыта как Failed/NoResponse, следующая ещё не
        // началась — интервал "ждёт возврата" (см. `find_awaiting_resume` в
        // application, Задача 3).
        let rows = sqlx::query(
            "SELECT tmc.id, tmc.hour_interval_id, tmc.task_id, tmc.started_at, tmc.ended_at,
                    tmc.status, tmc.reason, tmc.last_reminder_at
             FROM ten_min_checks tmc
             JOIN hour_intervals hi ON hi.id = tmc.hour_interval_id
             WHERE hi.ended_at IS NULL
               AND tmc.ended_at IS NOT NULL
               AND tmc.status IN ('failed', 'no_response')
               AND tmc.started_at = (
                   SELECT MAX(t2.started_at) FROM ten_min_checks t2
                   WHERE t2.hour_interval_id = tmc.hour_interval_id
               )
             ORDER BY tmc.started_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        rows.into_iter().map(row_to_ten_min_check).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{connect_in_memory, sample_timestamp};
    use domain::CheckStatus;

    async fn seed_hour_interval(pool: &SqlitePool) -> (i64, i64) {
        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (1, '0', ?1)")
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO work_days (user_id, started_at) VALUES (1, ?1)")
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tasks (user_id, title, status, created_at) VALUES (1, 'task', 'ready', ?1)",
        )
        .bind(sample_timestamp())
        .execute(pool)
        .await
        .unwrap();
        let hour_interval_id = sqlx::query("INSERT INTO hour_intervals (work_day_id, started_at) VALUES (1, ?1)")
            .bind(sample_timestamp())
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid();

        (hour_interval_id, 1)
    }

    #[tokio::test]
    async fn create_and_find_round_trips() {
        let pool = connect_in_memory().await;
        let (hour_interval_id, task_id) = seed_hour_interval(&pool).await;
        let hour_interval_id = HourIntervalId::new(hour_interval_id);
        let task_id = TaskId::new(task_id);
        let repo = SqliteTenMinCheckRepository::new(pool);

        assert_eq!(repo.find_open().await.unwrap(), Vec::new());

        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval_id,
            task_id,
            sample_timestamp().into(),
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        let created = repo.create(draft).await.unwrap();
        assert_eq!(created.id(), TenMinCheckId::new(1));

        assert_eq!(repo.find_open().await.unwrap(), vec![created.clone()]);
        assert_eq!(
            repo.find_open_by_interval(hour_interval_id).await.unwrap(),
            Some(created.clone())
        );
        assert_eq!(
            repo.find_last_by_interval(hour_interval_id).await.unwrap(),
            Some(created.clone())
        );
        assert_eq!(
            repo.list_by_interval(hour_interval_id).await.unwrap(),
            vec![created.clone()]
        );
        assert_eq!(repo.find_by_id(created.id()).await.unwrap(), Some(created));
    }

    #[tokio::test]
    async fn no_response_close_appends_reason_later_without_touching_ended_at_or_status() {
        let pool = connect_in_memory().await;
        let (hour_interval_id, task_id) = seed_hour_interval(&pool).await;
        let hour_interval_id = HourIntervalId::new(hour_interval_id);
        let task_id = TaskId::new(task_id);
        let repo = SqliteTenMinCheckRepository::new(pool);

        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval_id,
            task_id,
            sample_timestamp().into(),
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        let open_check = repo.create(draft).await.unwrap();

        let ended_at = sample_timestamp().into();
        let closed = TenMinCheck::new(
            open_check.id(),
            hour_interval_id,
            task_id,
            open_check.started_at(),
            Some(ended_at),
            CheckStatus::NoResponse,
            None,
            None,
        );
        repo.update(closed).await.unwrap();

        assert_eq!(repo.find_open().await.unwrap(), Vec::new());
        assert_eq!(
            repo.find_awaiting_resume().await.unwrap().len(),
            1,
            "closed NoResponse check awaiting resume on a still-open interval"
        );

        let with_reason = TenMinCheck::new(
            open_check.id(),
            hour_interval_id,
            task_id,
            open_check.started_at(),
            Some(ended_at),
            CheckStatus::NoResponse,
            Some("was in a meeting".to_string()),
            None,
        );
        repo.update(with_reason).await.unwrap();

        let reloaded = repo.find_by_id(open_check.id()).await.unwrap().unwrap();
        assert_eq!(reloaded.reason(), Some("was in a meeting"));
        assert_eq!(reloaded.ended_at(), Some(ended_at));
        assert_eq!(reloaded.status(), CheckStatus::NoResponse);
    }

    #[tokio::test]
    async fn find_awaiting_resume_ignores_closed_hour_interval() {
        let pool = connect_in_memory().await;
        let (hour_interval_id, task_id) = seed_hour_interval(&pool).await;
        let hour_interval_id_value = hour_interval_id;
        let hour_interval_id = HourIntervalId::new(hour_interval_id);
        let task_id = TaskId::new(task_id);
        let repo = SqliteTenMinCheckRepository::new(pool.clone());

        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval_id,
            task_id,
            sample_timestamp().into(),
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        let open_check = repo.create(draft).await.unwrap();
        let closed = TenMinCheck::new(
            open_check.id(),
            hour_interval_id,
            task_id,
            open_check.started_at(),
            Some(sample_timestamp().into()),
            CheckStatus::Failed,
            Some("distracted".to_string()),
            None,
        );
        repo.update(closed).await.unwrap();

        assert_eq!(repo.find_awaiting_resume().await.unwrap().len(), 1);

        sqlx::query("UPDATE hour_intervals SET ended_at = ?1 WHERE id = ?2")
            .bind(sample_timestamp())
            .bind(hour_interval_id_value)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(repo.find_awaiting_resume().await.unwrap(), Vec::new());
    }

    #[tokio::test]
    async fn find_awaiting_resume_ignores_interval_where_next_slot_already_started() {
        let pool = connect_in_memory().await;
        let (hour_interval_id, task_id) = seed_hour_interval(&pool).await;
        let hour_interval_id = HourIntervalId::new(hour_interval_id);
        let task_id = TaskId::new(task_id);
        let repo = SqliteTenMinCheckRepository::new(pool);

        let first = TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval_id,
            task_id,
            sample_timestamp().into(),
            Some(sample_timestamp().into()),
            CheckStatus::Failed,
            Some("distracted".to_string()),
            None,
        );
        repo.create(first).await.unwrap();

        assert_eq!(repo.find_awaiting_resume().await.unwrap().len(), 1);

        let second_started_at: DateTime<Utc> = sample_timestamp() + chrono::Duration::minutes(15);
        let second = TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval_id,
            task_id,
            second_started_at.into(),
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        repo.create(second).await.unwrap();

        assert_eq!(repo.find_awaiting_resume().await.unwrap(), Vec::new());
    }
}
