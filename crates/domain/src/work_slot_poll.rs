use chrono::Duration;

/// Через сколько минут после старта открытой **рабочей** десятиминутки бот
/// впервые спрашивает "Ты работаешь?" (README.md раздел 2, п.2).
pub const WORK_SLOT_ASK_THRESHOLD_MINUTES: i64 = 10;

/// Результат опроса открытой рабочей десятиминутки на одном тике poll-цикла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkSlotPollOutcome {
    /// Ещё рано — ничего не делать.
    NoActionYet,
    /// Пора спросить "Ты работаешь?" (ещё не спрашивали).
    AskAreYouWorking,
    /// Уже спрашивали, ответа так и не было — закрыть немедленно как `NoResponse`.
    ResolveNoResponse,
}

/// Чистая функция опроса открытой **рабочей** десятиминутки (README.md
/// раздел 2, п.2). Управляет только рабочим слотом — десятиминутка отдыха и
/// ожидание после провала обслуживаются `ReminderScheduleDecision`. Никакого
/// "окна ожидания" после закрытия здесь нет: молчание закрывает слот сразу,
/// без второй сущности и без каскада на два проваленных слота.
pub struct WorkSlotPollDecision;

impl WorkSlotPollDecision {
    pub fn decide(elapsed_since_start: Duration, already_prompted: bool) -> WorkSlotPollOutcome {
        if already_prompted {
            return WorkSlotPollOutcome::ResolveNoResponse;
        }
        if elapsed_since_start >= Duration::minutes(WORK_SLOT_ASK_THRESHOLD_MINUTES) {
            return WorkSlotPollOutcome::AskAreYouWorking;
        }
        WorkSlotPollOutcome::NoActionYet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_driven_outcomes() {
        let cases = [
            (
                Duration::minutes(0),
                false,
                WorkSlotPollOutcome::NoActionYet,
            ),
            (
                Duration::minutes(9),
                false,
                WorkSlotPollOutcome::NoActionYet,
            ),
            (
                Duration::minutes(10),
                false,
                WorkSlotPollOutcome::AskAreYouWorking,
            ),
            (
                Duration::minutes(11),
                true,
                WorkSlotPollOutcome::ResolveNoResponse,
            ),
            (
                Duration::minutes(59),
                true,
                WorkSlotPollOutcome::ResolveNoResponse,
            ),
        ];

        for (elapsed, already_prompted, expected) in cases {
            assert_eq!(
                WorkSlotPollDecision::decide(elapsed, already_prompted),
                expected,
                "elapsed={elapsed:?} already_prompted={already_prompted}"
            );
        }
    }

    #[test]
    fn already_prompted_always_resolves_no_response_even_before_threshold() {
        // already_prompted означает, что вопрос уже был задан на предыдущем
        // тике (когда elapsed впервые достиг порога) — на следующем тике,
        // если ответа так и не было, слот закрывается немедленно.
        assert_eq!(
            WorkSlotPollDecision::decide(Duration::minutes(10), true),
            WorkSlotPollOutcome::ResolveNoResponse
        );
    }
}
