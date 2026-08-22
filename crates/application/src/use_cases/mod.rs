//! Use cases (Задача 5 корневого `tasks.md`): первые сценарии, минимально
//! достаточные для регистрации пользователя и начала рабочего дня. Каждый
//! use case — отдельная структура с `execute()`, получающая нужные порты
//! через конструктор (ручной dependency injection, без DI-фреймворка). Use
//! cases не импортируют ничего из `adapters/*` и не содержат SQL/Telegram-типов.

mod register_user_if_not_exists;
mod start_work_day;

pub use register_user_if_not_exists::RegisterUserIfNotExists;
pub use start_work_day::{StartWorkDay, StartWorkDayError};
