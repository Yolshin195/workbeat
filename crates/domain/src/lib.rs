//! Доменный слой workbeat: сущности и value objects из SPEC.md раздела 6.
//!
//! Этот крейт не содержит бизнес-правил (переходов состояний) и не зависит от
//! tokio/sqlx/teloxide — только типы, конструкторы и геттеры.

mod error;
mod hour_interval;
mod ids;
mod task;
mod ten_min_check;
mod user;
mod value_objects;
mod work_day;

pub use error::DomainError;
pub use hour_interval::{HourInterval, SuccessfulCount};
pub use ids::{HourIntervalId, TaskId, TenMinCheckId, WorkDayId};
pub use task::{Task, TaskPriority, TaskStatus};
pub use ten_min_check::{CheckStatus, TenMinCheck};
pub use user::User;
pub use value_objects::{TelegramId, TimeZoneOffset, UtcTimestamp};
pub use work_day::WorkDay;
