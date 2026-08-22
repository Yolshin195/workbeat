//! Слой use cases и портов (traits) — см. корневой `tasks.md`. Порты (Задача
//! 3) — интерфейсы, через которые use cases общаются с внешним миром;
//! реализации-адаптеры (SQLite, Telegram, tokio-poll) живут в других крейтах
//! и здесь не появляются. Use cases (начиная с Задачи 5) получают нужные
//! порты через конструктор и не импортируют ничего из `adapters/*`.
//! `application` собирается, имея в зависимостях только `domain`,
//! `async-trait` и `thiserror` — никакого sqlx/teloxide/tokio-runtime.

mod error;
mod outbound_message;
mod ports;
mod reminder_schedule_config;
mod use_cases;

#[cfg(test)]
pub mod testing;

pub use error::{NotifierError, RepoError};
pub use outbound_message::OutboundMessage;
pub use ports::{
    Clock, HourIntervalRepository, Notifier, TaskRepository, TenMinCheckRepository, UserRepository,
    WorkDayRepository,
};
pub use reminder_schedule_config::ReminderScheduleConfig;
pub use use_cases::{
    AdvanceOpenTenMinChecks, AdvanceOpenTenMinChecksError, ConfirmReadyToContinue,
    ConfirmReadyToContinueError, CreateTask, CreateTaskError, FinishHourInterval,
    FinishHourIntervalError, ListAvailableTasks, MarkReturnedFromRest, MarkReturnedFromRestError,
    MarkTaskDone, MarkTaskInProgress, RegisterUserIfNotExists, RemindAwaitingResume,
    RemindAwaitingResumeError, StartHourInterval, StartHourIntervalError, StartWorkDay,
    StartWorkDayError, SubmitFailureReason, SubmitFailureReasonError, SubmitTenMinAnswer,
    SubmitTenMinAnswerError, SwitchTaskMidInterval, SwitchTaskMidIntervalError, TaskFilter,
    UpdateTask, UpdateTaskError,
};
