//! Хелперы для интеграционных тестов репозиториев: временная SQLite БД
//! (в памяти для обычных round-trip тестов, tempfile для теста конкурентного
//! доступа, где нужны несколько реальных соединений и WAL).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, TimeZone, Utc};
use sqlx::SqlitePool;

use crate::{connect, MIGRATOR};

/// Фиксированная временная метка для тестов round-trip: `chrono::Utc::now()`
/// избыточно точен (наносекунды), и сравнивать его после сериализации в
/// SQLite TEXT менее предсказуемо, чем сравнение с заранее известным
/// значением.
pub fn sample_timestamp() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 15, 9, 30, 0).unwrap()
}

/// Пул на одном in-memory соединении — самый дешёвый вариант для тестов
/// репозиториев, где не нужна настоящая многопоточная конкурентность.
pub async fn connect_in_memory() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite connection");

    MIGRATOR.run(&pool).await.expect("run migrations");

    pool
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Уникальный путь к временному файлу БД — для теста конкурентного доступа,
/// которому нужны настоящие несколько соединений (WAL/busy_timeout не имеют
/// смысла для `:memory:`).
pub struct TempDbFile {
    path: PathBuf,
}

impl TempDbFile {
    pub fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("workbeat-test-{pid}-{unique}.sqlite"));
        Self { path }
    }

    pub fn url(&self) -> String {
        format!("sqlite://{}", self.path.display())
    }

    pub async fn connect(&self) -> SqlitePool {
        connect(&self.url()).await.expect("connect to tempfile db")
    }
}

impl Drop for TempDbFile {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut path = self.path.clone().into_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}
