use crate::ids::{HourIntervalId, TaskId, TenMinCheckId};
use crate::value_objects::UtcTimestamp;

/// Итог одной десятиминутки (`ten_min_checks.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Worked,
    Failed,
    NoResponse,
}

/// Десятиминутка внутри часового интервала (`ten_min_checks` в SPEC.md п.6).
/// Хранит `task_id` — задачу, активную в момент этой десятиминутки: один
/// часовой интервал может включать десятиминутки разных задач (правило
/// "задача готова", см. SPEC.md раздел 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenMinCheck {
    id: TenMinCheckId,
    hour_interval_id: HourIntervalId,
    task_id: TaskId,
    started_at: UtcTimestamp,
    ended_at: Option<UtcTimestamp>,
    status: CheckStatus,
    reason: Option<String>,
}

impl TenMinCheck {
    pub fn new(
        id: TenMinCheckId,
        hour_interval_id: HourIntervalId,
        task_id: TaskId,
        started_at: UtcTimestamp,
        ended_at: Option<UtcTimestamp>,
        status: CheckStatus,
        reason: Option<String>,
    ) -> Self {
        Self {
            id,
            hour_interval_id,
            task_id,
            started_at,
            ended_at,
            status,
            reason,
        }
    }

    pub fn id(&self) -> TenMinCheckId {
        self.id
    }

    pub fn hour_interval_id(&self) -> HourIntervalId {
        self.hour_interval_id
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

    pub fn status(&self) -> CheckStatus {
        self.status
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn constructs_worked_check_without_reason() {
        let check = TenMinCheck::new(
            TenMinCheckId::new(1),
            HourIntervalId::new(1),
            TaskId::new(1),
            UtcTimestamp::new(Utc::now()),
            None,
            CheckStatus::Worked,
            None,
        );

        assert_eq!(check.status(), CheckStatus::Worked);
        assert_eq!(check.reason(), None);
        assert_eq!(check.task_id(), TaskId::new(1));
    }

    #[test]
    fn constructs_failed_check_with_reason() {
        let check = TenMinCheck::new(
            TenMinCheckId::new(2),
            HourIntervalId::new(1),
            TaskId::new(1),
            UtcTimestamp::new(Utc::now()),
            None,
            CheckStatus::Failed,
            Some("distracted".to_string()),
        );

        assert_eq!(check.status(), CheckStatus::Failed);
        assert_eq!(check.reason(), Some("distracted"));
    }

    #[test]
    fn checks_in_same_interval_can_reference_different_tasks() {
        let first = TenMinCheck::new(
            TenMinCheckId::new(3),
            HourIntervalId::new(1),
            TaskId::new(1),
            UtcTimestamp::new(Utc::now()),
            None,
            CheckStatus::Worked,
            None,
        );
        let second = TenMinCheck::new(
            TenMinCheckId::new(4),
            HourIntervalId::new(1),
            TaskId::new(2),
            UtcTimestamp::new(Utc::now()),
            None,
            CheckStatus::Worked,
            None,
        );

        assert_eq!(first.hour_interval_id(), second.hour_interval_id());
        assert_ne!(first.task_id(), second.task_id());
    }
}
