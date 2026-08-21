use crate::error::DomainError;
use crate::ids::{HourIntervalId, WorkDayId};
use crate::value_objects::UtcTimestamp;

/// Число успешных десятиминуток в часовом интервале (`hour_intervals.successful_count`).
/// Инвариант 0..=5 обеспечен на уровне типа — построить `SuccessfulCount(6)`
/// невозможно.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SuccessfulCount(u8);

impl SuccessfulCount {
    pub const MAX: u8 = 5;

    pub fn new(value: u8) -> Result<Self, DomainError> {
        if value > Self::MAX {
            return Err(DomainError::InvalidSuccessfulCount(value));
        }
        Ok(Self(value))
    }

    pub fn zero() -> Self {
        Self(0)
    }

    pub fn value(&self) -> u8 {
        self.0
    }
}

/// Часовой интервал (`hour_intervals` в SPEC.md п.6). Намеренно не хранит
/// `task_id` — один часовой интервал может включать десятиминутки разных
/// задач (правило "задача готова", см. корневой README.md раздел 3), поэтому
/// привязка к задаче живёт на уровне `TenMinCheck`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourInterval {
    id: HourIntervalId,
    work_day_id: WorkDayId,
    started_at: UtcTimestamp,
    ended_at: Option<UtcTimestamp>,
    successful_count: SuccessfulCount,
    summary: Option<String>,
}

impl HourInterval {
    pub fn new(
        id: HourIntervalId,
        work_day_id: WorkDayId,
        started_at: UtcTimestamp,
        ended_at: Option<UtcTimestamp>,
        successful_count: SuccessfulCount,
        summary: Option<String>,
    ) -> Self {
        Self {
            id,
            work_day_id,
            started_at,
            ended_at,
            successful_count,
            summary,
        }
    }

    pub fn id(&self) -> HourIntervalId {
        self.id
    }

    pub fn work_day_id(&self) -> WorkDayId {
        self.work_day_id
    }

    pub fn started_at(&self) -> UtcTimestamp {
        self.started_at
    }

    pub fn ended_at(&self) -> Option<UtcTimestamp> {
        self.ended_at
    }

    pub fn successful_count(&self) -> SuccessfulCount {
        self.successful_count
    }

    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn successful_count_accepts_zero_to_five() {
        for value in 0..=5u8 {
            assert!(SuccessfulCount::new(value).is_ok());
        }
    }

    #[test]
    fn successful_count_rejects_more_than_five() {
        assert_eq!(
            SuccessfulCount::new(6),
            Err(DomainError::InvalidSuccessfulCount(6))
        );
    }

    #[test]
    fn constructs_hour_interval() {
        let interval = HourInterval::new(
            HourIntervalId::new(1),
            WorkDayId::new(1),
            UtcTimestamp::new(Utc::now()),
            None,
            SuccessfulCount::zero(),
            None,
        );

        assert_eq!(interval.successful_count().value(), 0);
        assert_eq!(interval.ended_at(), None);
        assert_eq!(interval.summary(), None);
    }

    #[test]
    fn constructs_hour_interval_with_summary() {
        let interval = HourInterval::new(
            HourIntervalId::new(1),
            WorkDayId::new(1),
            UtcTimestamp::new(Utc::now()),
            Some(UtcTimestamp::new(Utc::now())),
            SuccessfulCount::new(5).unwrap(),
            Some("wrote the report".to_string()),
        );

        assert_eq!(interval.summary(), Some("wrote the report"));
    }
}
