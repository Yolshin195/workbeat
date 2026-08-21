//! Слой use cases и портов (traits). Эта задача (Задача 3, см. корневой
//! `tasks.md`) описывает только порты — интерфейсы, через которые будущие
//! use cases (Задачи 5-9) будут общаться с внешним миром. Реализаций-адаптеров
//! здесь нет и быть не должно: `application` собирается, имея в зависимостях
//! только `domain`, `async-trait` и `thiserror` — никакого sqlx/teloxide/
//! tokio-runtime.

mod error;
mod outbound_message;
mod ports;

#[cfg(test)]
pub mod testing;

pub use error::{NotifierError, RepoError};
pub use outbound_message::OutboundMessage;
pub use ports::{
    Clock, HourIntervalRepository, Notifier, TaskRepository, TenMinCheckRepository,
    UserRepository, WorkDayRepository,
};
