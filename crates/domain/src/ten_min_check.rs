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
/// `task_id` — задача, активная в момент этой десятиминутки: живёт здесь, а не
/// на `HourInterval`, потому что один часовой интервал может включать
/// десятиминутки разных задач (правило "задача готова"). Признака
/// "восстановительная десятиминутка" (`is_recovery`) в системе нет: молчание
/// закрывает рабочую десятиминутку сразу, без второй сущности.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenMinCheck {
    id: TenMinCheckId,
    hour_interval_id: HourIntervalId,
    task_id: TaskId,
    started_at: UtcTimestamp,
    ended_at: Option<UtcTimestamp>,
    status: CheckStatus,
    reason: Option<String>,
    last_reminder_at: Option<UtcTimestamp>,
}

impl TenMinCheck {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TenMinCheckId,
        hour_interval_id: HourIntervalId,
        task_id: TaskId,
        started_at: UtcTimestamp,
        ended_at: Option<UtcTimestamp>,
        status: CheckStatus,
        reason: Option<String>,
        last_reminder_at: Option<UtcTimestamp>,
    ) -> Self {
        Self {
            id,
            hour_interval_id,
            task_id,
            started_at,
            ended_at,
            status,
            reason,
            last_reminder_at,
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

    pub fn last_reminder_at(&self) -> Option<UtcTimestamp> {
        self.last_reminder_at
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
            None,
        );

        assert_eq!(check.status(), CheckStatus::Worked);
        assert_eq!(check.reason(), None);
        assert_eq!(check.task_id(), TaskId::new(1));
        assert_eq!(check.last_reminder_at(), None);
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
            None,
        );

        assert_eq!(check.status(), CheckStatus::Failed);
        assert_eq!(check.reason(), Some("distracted"));
    }

    #[test]
    fn constructs_no_response_check_with_reason_added_after_close() {
        let started_at = UtcTimestamp::new(Utc::now());
        let ended_at = UtcTimestamp::new(Utc::now());

        let check = TenMinCheck::new(
            TenMinCheckId::new(3),
            HourIntervalId::new(1),
            TaskId::new(1),
            started_at,
            Some(ended_at),
            CheckStatus::NoResponse,
            None,
            Some(UtcTimestamp::new(Utc::now())),
        );

        assert_eq!(check.status(), CheckStatus::NoResponse);
        assert_eq!(check.reason(), None);
        assert_eq!(check.ended_at(), Some(ended_at));
        assert!(check.last_reminder_at().is_some());
    }
}
