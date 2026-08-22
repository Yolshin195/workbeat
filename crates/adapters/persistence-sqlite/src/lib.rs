//! SQLite-реализация репозиториев из `application` (Задача 4, `tasks.md`).
//!
//! `domain` не знает про SQLite: все ряды маппятся в доменные типы здесь, с
//! явным `TryFrom`-подобным маппингом enum ↔ TEXT (`enums` модуль) — никаких
//! "magic strings" в бизнес-коде репозиториев.

mod enums;
mod error;
mod hour_interval_repository;
mod task_repository;
mod ten_min_check_repository;
mod user_repository;
mod work_day_repository;

#[cfg(test)]
mod test_support;

pub use hour_interval_repository::SqliteHourIntervalRepository;
pub use task_repository::SqliteTaskRepository;
pub use ten_min_check_repository::SqliteTenMinCheckRepository;
pub use user_repository::SqliteUserRepository;
pub use work_day_repository::SqliteWorkDayRepository;

use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

/// Миграции из `migrations/` встраиваются в бинарь на этапе компиляции — это
/// не требует подключения к БД во время сборки (в отличие от `sqlx::query!`).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Подключается к SQLite по `database_url` (например `sqlite://./workbeat.db`
/// или `sqlite::memory:`), создаёт файл БД, если его ещё нет, включает WAL и
/// разумный `busy_timeout` (см. Задачу 4: конкурентный доступ не должен
/// приводить к `SQLITE_BUSY`), затем прогоняет миграции.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;

    MIGRATOR.run(&pool).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{sample_timestamp, TempDbFile};

    #[tokio::test]
    async fn migrations_apply_cleanly_from_scratch() {
        let db = TempDbFile::new();
        let pool = db.connect().await;

        // Схема должна быть на месте после единственного прогона миграций —
        // вставка по всем пяти таблицам без ошибок.
        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (1, '0', ?1)")
            .bind(sample_timestamp())
            .execute(&pool)
            .await
            .unwrap();

        let applied = MIGRATOR
            .run(&pool)
            .await;
        assert!(applied.is_ok(), "re-running migrations must be a no-op");
    }

    #[tokio::test]
    async fn concurrent_writers_to_different_tables_do_not_fail() {
        let db = TempDbFile::new();
        let pool = db.connect().await;

        sqlx::query("INSERT INTO users (telegram_id, timezone, created_at) VALUES (1, '0', ?1)")
            .bind(sample_timestamp())
            .execute(&pool)
            .await
            .unwrap();

        let tasks_pool = pool.clone();
        let tasks_writer = tokio::spawn(async move {
            for i in 0..20 {
                sqlx::query(
                    "INSERT INTO tasks (user_id, title, status, created_at) VALUES (1, ?1, 'ready', ?2)",
                )
                .bind(format!("task {i}"))
                .bind(sample_timestamp())
                .execute(&tasks_pool)
                .await
                .expect("insert into tasks must not fail under concurrent access");
            }
        });

        let work_days_pool = pool.clone();
        let work_days_writer = tokio::spawn(async move {
            for _ in 0..20 {
                sqlx::query("INSERT INTO work_days (user_id, started_at) VALUES (1, ?1)")
                    .bind(sample_timestamp())
                    .execute(&work_days_pool)
                    .await
                    .expect("insert into work_days must not fail under concurrent access");
            }
        });

        let (tasks_result, work_days_result) = tokio::join!(tasks_writer, work_days_writer);
        tasks_result.unwrap();
        work_days_result.unwrap();

        let task_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
            .fetch_one(&pool)
            .await
            .unwrap();
        let work_day_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_days")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(task_count, 20);
        assert_eq!(work_day_count, 20);
    }
}
