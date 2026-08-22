//! Use cases (Задача 5 корневого `tasks.md`): первые сценарии, минимально
//! достаточные для регистрации пользователя и начала рабочего дня. Каждый
//! use case — отдельная структура с `execute()`, получающая нужные порты
//! через конструктор (ручной dependency injection, без DI-фреймворка). Use
//! cases не импортируют ничего из `adapters/*` и не содержат SQL/Telegram-типов.

mod advance_open_ten_min_checks;
mod confirm_ready_to_continue;
mod create_task;
mod end_lunch;
mod finish_hour_interval;
mod finish_work_day;
mod list_available_tasks;
mod mark_returned_from_rest;
mod mark_task_done;
mod mark_task_in_progress;
mod prompt_continue_if_idle;
mod register_user_if_not_exists;
mod remind_awaiting_resume;
mod start_hour_interval;
mod start_lunch;
mod start_work_day;
mod submit_failure_reason;
mod submit_ten_min_answer;
mod suggest_lunch_after_nth_interval;
pub(crate) mod support;
mod switch_task_mid_interval;
mod update_task;

#[cfg(test)]
mod hour_interval_scenario_tests;
#[cfg(test)]
mod work_day_finish_scenario_tests;

pub use advance_open_ten_min_checks::{AdvanceOpenTenMinChecks, AdvanceOpenTenMinChecksError};
pub use confirm_ready_to_continue::{ConfirmReadyToContinue, ConfirmReadyToContinueError};
pub use create_task::{CreateTask, CreateTaskError};
pub use end_lunch::{EndLunch, EndLunchError};
pub use finish_hour_interval::{FinishHourInterval, FinishHourIntervalError};
pub use finish_work_day::{FinishWorkDay, FinishWorkDayError, WorkDayFinishSummary};
pub use list_available_tasks::{ListAvailableTasks, TaskFilter};
pub use mark_returned_from_rest::{MarkReturnedFromRest, MarkReturnedFromRestError};
pub use mark_task_done::MarkTaskDone;
pub use mark_task_in_progress::MarkTaskInProgress;
pub use prompt_continue_if_idle::{PromptContinueIfIdle, PromptContinueIfIdleError};
pub use register_user_if_not_exists::RegisterUserIfNotExists;
pub use remind_awaiting_resume::{RemindAwaitingResume, RemindAwaitingResumeError};
pub use start_hour_interval::{StartHourInterval, StartHourIntervalError};
pub use start_lunch::{StartLunch, StartLunchError};
pub use start_work_day::{StartWorkDay, StartWorkDayError};
pub use submit_failure_reason::{SubmitFailureReason, SubmitFailureReasonError};
pub use submit_ten_min_answer::{SubmitTenMinAnswer, SubmitTenMinAnswerError};
pub use suggest_lunch_after_nth_interval::{
    SuggestLunchAfterNthInterval, SuggestLunchAfterNthIntervalError,
};
pub use switch_task_mid_interval::{SwitchTaskMidInterval, SwitchTaskMidIntervalError};
pub use update_task::{UpdateTask, UpdateTaskError};
