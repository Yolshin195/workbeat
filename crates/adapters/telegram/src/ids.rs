//! Маппинг `teloxide::types::{ChatId, UserId}` ↔ `domain::TelegramId` — живёт
//! только в этом адаптере (критерий приёмки Задачи 11), нигде в
//! `application`/`domain` типы teloxide не появляются.
//!
//! Бот работает только в приватных чатах, поэтому `ChatId` пользователя и его
//! `UserId` численно совпадают — это позволяет отправлять проактивные
//! сообщения (`Notifier`, Задача 12) по одному `TelegramId`, не храня
//! отдельно `chat_id` в БД (в схеме SPEC.md его и нет).

use domain::{DomainError, TelegramId};
use teloxide::types::{ChatId, UserId};

pub fn telegram_id_from_user(user_id: UserId) -> Result<TelegramId, DomainError> {
    TelegramId::new(user_id.0 as i64)
}

pub fn chat_id_for(telegram_id: TelegramId) -> ChatId {
    ChatId(telegram_id.value())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_between_user_id_and_telegram_id() {
        let user_id = UserId(42);
        let telegram_id = telegram_id_from_user(user_id).unwrap();
        assert_eq!(telegram_id.value(), 42);
        assert_eq!(chat_id_for(telegram_id), ChatId(42));
    }

    #[test]
    fn rejects_non_positive_ids() {
        // UserId(0) не встречается в реальном Telegram API, но домен всё
        // равно защищает инвариант "telegram id положителен".
        assert!(telegram_id_from_user(UserId(0)).is_err());
    }
}
