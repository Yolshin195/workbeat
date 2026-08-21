use crate::ids::WorkDayId;
use crate::value_objects::{TelegramId, UtcTimestamp};

/// Рабочий день пользователя (`work_days` в SPEC.md п.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDay {
    id: WorkDayId,
    user_id: TelegramId,
    started_at: UtcTimestamp,
    finished_at: Option<UtcTimestamp>,
    lunch_started_at: Option<UtcTimestamp>,
    lunch_ended_at: Option<UtcTimestamp>,
}

impl WorkDay {
    pub fn new(
        id: WorkDayId,
        user_id: TelegramId,
        started_at: UtcTimestamp,
        finished_at: Option<UtcTimestamp>,
        lunch_started_at: Option<UtcTimestamp>,
        lunch_ended_at: Option<UtcTimestamp>,
    ) -> Self {
        Self {
            id,
            user_id,
            started_at,
            finished_at,
            lunch_started_at,
            lunch_ended_at,
        }
    }

    pub fn id(&self) -> WorkDayId {
        self.id
    }

    pub fn user_id(&self) -> TelegramId {
        self.user_id
    }

    pub fn started_at(&self) -> UtcTimestamp {
        self.started_at
    }

    pub fn finished_at(&self) -> Option<UtcTimestamp> {
        self.finished_at
    }

    pub fn lunch_started_at(&self) -> Option<UtcTimestamp> {
        self.lunch_started_at
    }

    pub fn lunch_ended_at(&self) -> Option<UtcTimestamp> {
        self.lunch_ended_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn constructs_open_work_day_with_no_finish_or_lunch() {
        let work_day = WorkDay::new(
            WorkDayId::new(1),
            TelegramId::new(123).unwrap(),
            UtcTimestamp::new(Utc::now()),
            None,
            None,
            None,
        );

        assert_eq!(work_day.finished_at(), None);
        assert_eq!(work_day.lunch_started_at(), None);
        assert_eq!(work_day.lunch_ended_at(), None);
    }
}
