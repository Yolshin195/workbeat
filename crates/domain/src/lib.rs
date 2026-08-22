//! Доменный слой workbeat: сущности, value objects и бизнес-правила из
//! корневого README.md раздела 6 (сущности) и раздела 2 (переходы состояний).
//!
//! Бизнес-правила (модули `work_slot_poll`, `reminder_schedule`,
//! `idle_prompt`, `ten_min_check_state`, `hour_interval_state`,
//! `time_calculations`) — чистые функции без ввода-вывода: `(текущее
//! состояние, событие) -> новое состояние`. Этот крейт не зависит от
//! tokio/sqlx/teloxide.

mod error;
mod hour_interval;
mod hour_interval_state;
mod idle_prompt;
mod ids;
mod lunch_suggestion;
mod reminder_schedule;
mod task;
mod ten_min_check;
mod ten_min_check_state;
mod time_calculations;
mod user;
mod value_objects;
mod work_day;
mod work_slot_poll;

pub use error::DomainError;
pub use hour_interval::{HourInterval, SuccessfulCount};
pub use hour_interval_state::HourIntervalState;
pub use idle_prompt::IdlePromptDecision;
pub use ids::{HourIntervalId, TaskId, TenMinCheckId, WorkDayId};
pub use lunch_suggestion::{LunchSuggestionDecision, LUNCH_SUGGESTION_AFTER_INTERVALS};
pub use reminder_schedule::ReminderScheduleDecision;
pub use task::{Task, TaskPriority, TaskStatus};
pub use ten_min_check::{CheckStatus, TenMinCheck};
pub use ten_min_check_state::{append_reason_to_no_response, TenMinCheckAnswer, TenMinCheckState};
pub use time_calculations::{
    daily_norm_progress, day_overrun, interval_overrun, task_time, DailyNormProgress,
    NOMINAL_INTERVAL_MINUTES, NOMINAL_WORK_DAY_INTERVALS, TEN_MIN_NOMINAL_MINUTES,
};
pub use user::User;
pub use value_objects::{TelegramId, TimeZoneOffset, UtcTimestamp};
pub use work_day::WorkDay;
pub use work_slot_poll::{
    WorkSlotPollDecision, WorkSlotPollOutcome, WORK_SLOT_ASK_THRESHOLD_MINUTES,
};
