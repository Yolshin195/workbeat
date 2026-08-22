//! Слой use cases и портов (traits) — см. корневой `tasks.md`. Порты (Задача
//! 3) — интерфейсы, через которые use cases общаются с внешним миром;
//! реализации-адаптеры (SQLite, Telegram, tokio-poll) живут в других крейтах
//! и здесь не появляются. Use cases (начиная с Задачи 5) получают нужные
//! порты через конструктор и не импортируют ничего из `adapters/*`.
//! `application` собирается, имея в зависимостях только `domain`,
//! `async-trait` и `thiserror` — никакого sqlx/teloxide/tokio-runtime.

mod error;
mod idle_prompt_config;
mod outbound_message;
mod ports;
mod reminder_schedule_config;
mod report_dto;
mod use_cases;

#[cfg(test)]
pub mod testing;

pub use error::{NotifierError, RepoError};
pub use idle_prompt_config::IdlePromptConfig;
pub use outbound_message::OutboundMessage;
pub use ports::{
    Clock, HourIntervalRepository, Notifier, TaskRepository, TenMinCheckRepository, UserRepository,
    WorkDayRepository,
};
pub use reminder_schedule_config::ReminderScheduleConfig;
pub use report_dto::{
    DayReportInfo, FailedCheckEntry, Period, PeriodReportInfo, ReportDto, TaskTimeEntry,
};
pub use use_cases::{
    AdvanceOpenTenMinChecks, AdvanceOpenTenMinChecksError, BuildDayReport, BuildDayReportError,
    BuildPeriodReport, BuildPeriodReportError, ConfirmReadyToContinue,
    ConfirmReadyToContinueError, CreateTask, CreateTaskError, EndLunch, EndLunchError,
    ExportDayReportCsv, ExportDayReportCsvError, FinishHourInterval, FinishHourIntervalError,
    FinishWorkDay, FinishWorkDayError, ListAvailableTasks, MarkReturnedFromRest,
    MarkReturnedFromRestError, MarkTaskDone, MarkTaskInProgress, PromptContinueIfIdle,
    PromptContinueIfIdleError, RegisterUserIfNotExists, RemindAwaitingResume,
    RemindAwaitingResumeError, StartHourInterval, StartHourIntervalError, StartLunch,
    StartLunchError, StartWorkDay, StartWorkDayError, SubmitFailureReason,
    SubmitFailureReasonError, SubmitTenMinAnswer, SubmitTenMinAnswerError,
    SuggestLunchAfterNthInterval, SuggestLunchAfterNthIntervalError, SwitchTaskMidInterval,
    SwitchTaskMidIntervalError, TaskFilter, UpdateTask, UpdateTaskError, WorkDayFinishSummary,
};
