//! Сборка `teloxide::Dispatcher`: три ветки апдейтов (команда / callback-кнопка
//! / свободный текст), каждая начинается с `RegisterUserIfNotExists`
//! (SPEC.md — вызывается на любое входящее сообщение), затем разбор через
//! `intent::interpret` и выполнение через `executor::handle_intent`.

use std::sync::Arc;

use teloxide::dispatching::{UpdateFilterExt, UpdateHandler};
use teloxide::prelude::*;
use teloxide::types::Update;

use crate::commands::Command;
use crate::deps::UseCases;
use crate::executor;
use crate::ids::telegram_id_from_user;
use crate::intent::{interpret, IncomingEvent};
use crate::session::SessionStore;

pub fn schema() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync + 'static>> {
    dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(on_command),
        )
        .branch(Update::filter_callback_query().endpoint(on_callback))
        .branch(Update::filter_message().endpoint(on_text))
}

async fn on_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    deps: Arc<UseCases>,
    sessions: Arc<SessionStore>,
) -> HandlerResult {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    let Ok(telegram_id) = telegram_id_from_user(user.id) else {
        return Ok(());
    };
    deps.register_user.execute(telegram_id).await.ok();

    let session = sessions.get(msg.chat.id);
    let intent = interpret(&session, IncomingEvent::Command(cmd));
    executor::handle_intent(&bot, &deps, &sessions, msg.chat.id, telegram_id, intent).await;
    Ok(())
}

async fn on_callback(
    bot: Bot,
    q: CallbackQuery,
    deps: Arc<UseCases>,
    sessions: Arc<SessionStore>,
) -> HandlerResult {
    let _ = bot.answer_callback_query(q.id.clone()).await;

    let Some(message) = q.message.as_ref() else {
        return Ok(());
    };
    let Some(data) = q.data.as_deref() else {
        return Ok(());
    };
    let Some(action) = crate::callback::CallbackAction::parse(data) else {
        return Ok(());
    };
    let Ok(telegram_id) = telegram_id_from_user(q.from.id) else {
        return Ok(());
    };
    deps.register_user.execute(telegram_id).await.ok();

    let chat_id = message.chat().id;
    let session = sessions.get(chat_id);
    let intent = interpret(&session, IncomingEvent::Callback(action));
    executor::handle_intent(&bot, &deps, &sessions, chat_id, telegram_id, intent).await;
    Ok(())
}

async fn on_text(
    bot: Bot,
    msg: Message,
    deps: Arc<UseCases>,
    sessions: Arc<SessionStore>,
) -> HandlerResult {
    let Some(user) = msg.from.as_ref() else {
        return Ok(());
    };
    let Some(text) = msg.text() else {
        return Ok(());
    };
    let Ok(telegram_id) = telegram_id_from_user(user.id) else {
        return Ok(());
    };
    deps.register_user.execute(telegram_id).await.ok();

    let session = sessions.get(msg.chat.id);
    let intent = interpret(&session, IncomingEvent::Text(text.to_string()));
    executor::handle_intent(&bot, &deps, &sessions, msg.chat.id, telegram_id, intent).await;
    Ok(())
}

type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>;
