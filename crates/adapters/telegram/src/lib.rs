//! teloxide-адаптер: входящие апдейты → use cases (Задача 11 корневого
//! `tasks.md`). Никакой бизнес-логики здесь нет — только разбор апдейтов
//! (`intent`), их исполнение через use cases (`executor`) и Telegram-специфика
//! (маппинг id, клавиатуры, диспетчер). Реализация `Notifier` поверх
//! `teloxide::Bot` (исходящие сообщения, `notifier.rs`) — Задача 12.

mod callback;
mod commands;
mod deps;
mod dispatch;
mod executor;
mod ids;
mod intent;
mod keyboards;
mod notifier;
mod session;
mod text;

pub use commands::Command;
pub use deps::UseCases;
pub use ids::{chat_id_for, telegram_id_from_user};
pub use notifier::TeloxideNotifier;
pub use session::SessionStore;

use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

/// Запускает long-polling бота: единственная точка входа этого крейта,
/// вызывается composition root'ом (Задача 13). `bot` создаётся вызывающей
/// стороной (нужен токен из конфига), `use_cases` — уже собранный пучок use
/// cases с реальными реализациями портов.
pub async fn run(bot: Bot, use_cases: Arc<UseCases>) {
    let sessions = Arc::new(SessionStore::new());

    // Регистрирует список команд в нативном меню Telegram (кнопка "Menu"
    // рядом с полем ввода и автодополнение по "/") — само по себе не влияет
    // на разбор апдейтов, только на подсказки клиента.
    if let Err(err) = bot.set_my_commands(Command::bot_commands()).await {
        log::warn!("не удалось зарегистрировать команды бота в Telegram: {err}");
    }

    Dispatcher::builder(bot, dispatch::schema())
        .dependencies(dptree::deps![use_cases, sessions])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
