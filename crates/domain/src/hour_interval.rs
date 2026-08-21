use crate::error::DomainError;
use crate::ids::{HourIntervalId, TaskId, WorkDayId};
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

/// Часовой интервал работы над одной задачей (`hour_intervals` в SPEC.md п.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourInterval {
    id: HourIntervalId,
    work_day_id: WorkDayId,
    task_id: TaskId,
    started_at: UtcTimestamp,
    ended_at: Option<UtcTimestamp>,
    successful_count: SuccessfulCount,
}

impl HourInterval {
    pub fn new(
        id: HourIntervalId,
        work_day_id: WorkDayId,
        task_id: TaskId,
        started_at: UtcTimestamp,
        ended_at: Option<UtcTimestamp>,
        successful_count: SuccessfulCount,
    ) -> Self {
        Self {
            id,
            work_day_id,
            task_id,
            started_at,
            ended_at,
            successful_count,
        }
    }

    pub fn id(&self) -> HourIntervalId {
        self.id
    }

    pub fn work_day_id(&self) -> WorkDayId {
        self.work_day_id
    }

    pub fn task_id(&self) -> TaskId {
        self.task_id
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
            TaskId::new(1),
            UtcTimestamp::new(Utc::now()),
            None,
            SuccessfulCount::zero(),
        );

        assert_eq!(interval.successful_count().value(), 0);
        assert_eq!(interval.ended_at(), None);
    }
}
