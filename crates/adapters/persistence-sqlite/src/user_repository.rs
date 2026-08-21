use async_trait::async_trait;
use application::{RepoError, UserRepository};
use chrono::{DateTime, Utc};
use domain::{TelegramId, TimeZoneOffset, User, UtcTimestamp};
use sqlx::{Row, SqlitePool};

use crate::error::map_sqlx_error;

pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn timezone_to_db(timezone: TimeZoneOffset) -> String {
    timezone.minutes().to_string()
}

fn timezone_from_db(value: &str) -> Result<TimeZoneOffset, RepoError> {
    let minutes: i32 = value
        .parse()
        .map_err(|_| RepoError::Storage(format!("invalid timezone offset stored: {value}")))?;
    TimeZoneOffset::new(minutes).map_err(|err| RepoError::Storage(err.to_string()))
}

fn row_to_user(telegram_id: i64, timezone: String, created_at: DateTime<Utc>) -> Result<User, RepoError> {
    let telegram_id =
        TelegramId::new(telegram_id).map_err(|err| RepoError::Storage(err.to_string()))?;
    let timezone = timezone_from_db(&timezone)?;
    Ok(User::new(telegram_id, timezone, UtcTimestamp::new(created_at)))
}

#[async_trait]
impl UserRepository for SqliteUserRepository {
    async fn create(&self, user: User) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (?1, ?2, ?3)")
            .bind(user.telegram_id().value())
            .bind(timezone_to_db(user.timezone()))
            .bind(user.created_at().value())
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(())
    }

    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, RepoError> {
        let row = sqlx::query("SELECT telegram_id, timezone, created_at FROM users WHERE telegram_id = ?1")
            .bind(telegram_id.value())
            .fetch_optional(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

        row.map(|row| {
            row_to_user(
                row.get::<i64, _>("telegram_id"),
                row.get::<String, _>("timezone"),
                row.get::<DateTime<Utc>, _>("created_at"),
            )
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{connect_in_memory, sample_timestamp};

    #[tokio::test]
    async fn create_and_find_round_trips() {
        let pool = connect_in_memory().await;
        let repo = SqliteUserRepository::new(pool);

        let telegram_id = TelegramId::new(42).unwrap();
        assert_eq!(repo.find_by_telegram_id(telegram_id).await.unwrap(), None);

        let user = User::new(
            telegram_id,
            TimeZoneOffset::new(180).unwrap(),
            UtcTimestamp::new(sample_timestamp()),
        );
        repo.create(user.clone()).await.unwrap();

        let found = repo.find_by_telegram_id(telegram_id).await.unwrap();
        assert_eq!(found, Some(user));
    }

    #[tokio::test]
    async fn find_missing_user_returns_none() {
        let pool = connect_in_memory().await;
        let repo = SqliteUserRepository::new(pool);

        assert_eq!(
            repo.find_by_telegram_id(TelegramId::new(1).unwrap())
                .await
                .unwrap(),
            None
        );
    }
}
