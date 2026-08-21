use chrono::{DateTime, Utc};

use crate::error::DomainError;

/// Идентификатор пользователя в Telegram (`users.telegram_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TelegramId(i64);

impl TelegramId {
    pub fn new(value: i64) -> Result<Self, DomainError> {
        if value <= 0 {
            return Err(DomainError::InvalidTelegramId(value));
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> i64 {
        self.0
    }
}

/// Момент времени в UTC. Обёртка над `chrono::DateTime<Utc>`, чтобы domain не
/// зависел напрямую от того, как именно адаптеры получают текущее время.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp(DateTime<Utc>);

impl UtcTimestamp {
    pub fn new(value: DateTime<Utc>) -> Self {
        Self(value)
    }

    pub fn value(&self) -> DateTime<Utc> {
        self.0
    }
}

impl From<DateTime<Utc>> for UtcTimestamp {
    fn from(value: DateTime<Utc>) -> Self {
        Self::new(value)
    }
}

/// Смещение часового пояса пользователя относительно UTC, в минутах.
/// Используется только для отображения в отчётах (см. README раздел 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeZoneOffset(i32);

impl TimeZoneOffset {
    const MIN_MINUTES: i32 = -12 * 60;
    const MAX_MINUTES: i32 = 14 * 60;

    pub fn new(minutes: i32) -> Result<Self, DomainError> {
        if !(Self::MIN_MINUTES..=Self::MAX_MINUTES).contains(&minutes) {
            return Err(DomainError::InvalidTimeZoneOffset(minutes));
        }
        Ok(Self(minutes))
    }

    pub fn utc() -> Self {
        Self(0)
    }

    pub fn minutes(&self) -> i32 {
        self.0
    }
}

impl Default for TimeZoneOffset {
    fn default() -> Self {
        Self::utc()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_id_rejects_non_positive_values() {
        assert_eq!(TelegramId::new(0), Err(DomainError::InvalidTelegramId(0)));
        assert_eq!(TelegramId::new(-1), Err(DomainError::InvalidTelegramId(-1)));
    }

    #[test]
    fn telegram_id_accepts_positive_values() {
        let id = TelegramId::new(42).unwrap();
        assert_eq!(id.value(), 42);
    }

    #[test]
    fn timezone_offset_accepts_boundaries() {
        assert!(TimeZoneOffset::new(-12 * 60).is_ok());
        assert!(TimeZoneOffset::new(14 * 60).is_ok());
    }

    #[test]
    fn timezone_offset_rejects_out_of_range() {
        assert!(TimeZoneOffset::new(-12 * 60 - 1).is_err());
        assert!(TimeZoneOffset::new(14 * 60 + 1).is_err());
    }

    #[test]
    fn timezone_offset_default_is_utc() {
        assert_eq!(TimeZoneOffset::default(), TimeZoneOffset::utc());
        assert_eq!(TimeZoneOffset::utc().minutes(), 0);
    }
}
