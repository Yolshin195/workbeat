use thiserror::Error;

/// Ошибки портов-репозиториев. `application` не знает, что именно за
/// хранилище скрывается за портом (SQLite, память для тестов и т.п.) —
/// адаптеры (Задача 4) маппят свои специфичные ошибки в эти варианты.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RepoError {
    #[error("entity not found")]
    NotFound,

    #[error("storage error: {0}")]
    Storage(String),
}

/// Ошибка отправки исходящего сообщения через `Notifier` (Задача 12 —
/// реализация поверх Telegram).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NotifierError {
    #[error("failed to deliver message: {0}")]
    DeliveryFailed(String),
}
