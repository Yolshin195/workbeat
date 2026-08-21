use thiserror::Error;

/// Ошибки валидации доменных типов. `domain` не выполняет ввод-вывод, поэтому
/// этот тип покрывает только нарушения инвариантов value objects и сущностей.
#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum DomainError {
    #[error("telegram id must be positive, got {0}")]
    InvalidTelegramId(i64),

    #[error("timezone offset must be between -12:00 and +14:00, got {0} minutes")]
    InvalidTimeZoneOffset(i32),

    #[error("successful_count must be between 0 and 5, got {0}")]
    InvalidSuccessfulCount(u8),

    #[error("task title must not be empty")]
    EmptyTaskTitle,

    #[error("ten-min check is already closed")]
    TenMinCheckAlreadyClosed,

    #[error("reason can only be appended to a NoResponse check")]
    ReasonNotAllowedForStatus,

    #[error("reason has already been set for this check")]
    ReasonAlreadySet,
}
