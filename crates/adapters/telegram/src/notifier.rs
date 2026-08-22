//! Реализация порта `Notifier` (Задача 3) поверх `teloxide::Bot` (Задача 12
//! корневого `tasks.md`): `OutboundMessage::Text` → `send_message`,
//! `OutboundMessage::Document` → `send_document` (используется poll-циклом,
//! Задача 10, и `ExportDayReportCsv`/Задача 9 через Telegram-адаптер для
//! отправки CSV как файла, а не текстом).

use application::{Notifier, NotifierError, OutboundMessage};
use async_trait::async_trait;
use domain::TelegramId;
use teloxide::prelude::*;
use teloxide::types::InputFile;

use crate::ids::chat_id_for;

/// Единственная реализация `Notifier` в системе — composition root (Задача
/// 13) передаёт её use cases, которым нужно слать проактивные сообщения
/// (poll-цикл, Задача 10) или итоги/экспорт (Задачи 7–9).
pub struct TeloxideNotifier {
    bot: Bot,
}

impl TeloxideNotifier {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl Notifier for TeloxideNotifier {
    async fn send(&self, user: TelegramId, message: OutboundMessage) -> Result<(), NotifierError> {
        let chat_id = chat_id_for(user);
        match message {
            OutboundMessage::Text(text) => {
                self.bot
                    .send_message(chat_id, text)
                    .await
                    .map_err(|err| NotifierError::DeliveryFailed(err.to_string()))?;
            }
            OutboundMessage::Document { filename, bytes } => {
                let file = InputFile::memory(bytes).file_name(filename);
                self.bot
                    .send_document(chat_id, file)
                    .await
                    .map_err(|err| NotifierError::DeliveryFailed(err.to_string()))?;
            }
        }
        Ok(())
    }
}
