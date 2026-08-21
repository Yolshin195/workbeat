use crate::value_objects::{TelegramId, TimeZoneOffset, UtcTimestamp};

/// Пользователь бота (`users` в SPEC.md п.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    telegram_id: TelegramId,
    timezone: TimeZoneOffset,
    created_at: UtcTimestamp,
}

impl User {
    pub fn new(telegram_id: TelegramId, timezone: TimeZoneOffset, created_at: UtcTimestamp) -> Self {
        Self {
            telegram_id,
            timezone,
            created_at,
        }
    }

    pub fn telegram_id(&self) -> TelegramId {
        self.telegram_id
    }

    pub fn timezone(&self) -> TimeZoneOffset {
        self.timezone
    }

    pub fn created_at(&self) -> UtcTimestamp {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn constructs_user_and_exposes_fields() {
        let telegram_id = TelegramId::new(123).unwrap();
        let timezone = TimeZoneOffset::new(180).unwrap();
        let created_at = UtcTimestamp::new(Utc::now());

        let user = User::new(telegram_id, timezone, created_at);

        assert_eq!(user.telegram_id(), telegram_id);
        assert_eq!(user.timezone(), timezone);
        assert_eq!(user.created_at(), created_at);
    }
}
