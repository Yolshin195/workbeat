//! Use cases (Задача 5 корневого `tasks.md`): первые сценарии, минимально
//! достаточные для регистрации пользователя и начала рабочего дня. Каждый
//! use case — отдельная структура с `execute()`, получающая нужные порты
//! через конструктор (ручной dependency injection, без DI-фреймворка). Use
//! cases не импортируют ничего из `adapters/*` и не содержат SQL/Telegram-типов.

mod create_task;
mod list_available_tasks;
mod mark_task_done;
mod mark_task_in_progress;
mod register_user_if_not_exists;
mod start_work_day;
mod update_task;

pub use create_task::{CreateTask, CreateTaskError};
pub use list_available_tasks::{ListAvailableTasks, TaskFilter};
pub use mark_task_done::MarkTaskDone;
pub use mark_task_in_progress::MarkTaskInProgress;
pub use register_user_if_not_exists::RegisterUserIfNotExists;
pub use start_work_day::{StartWorkDay, StartWorkDayError};
pub use update_task::{UpdateTask, UpdateTaskError};
