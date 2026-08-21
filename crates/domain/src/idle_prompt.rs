use chrono::Duration;

/// Чистая функция для `PromptContinueIfIdle` (README.md раздел 1 и п.6):
/// "пора ли снова спросить 'Готовы продолжить работу?'". Концептуально
/// похожа на `ReminderScheduleDecision`, но не унифицируется с ней
/// специально: разный масштаб порога (часы простоя между интервалами, а не
/// минуты ожидания внутри активного интервала) и разная точка отсчёта
/// (`last_idle_prompt_at`).
pub struct IdlePromptDecision;

impl IdlePromptDecision {
    pub fn decide(elapsed_since_last_prompt: Duration, idle_threshold: Duration) -> bool {
        elapsed_since_last_prompt >= idle_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_is_false() {
        let threshold = Duration::hours(2);
        assert!(!IdlePromptDecision::decide(
            threshold - Duration::minutes(1),
            threshold
        ));
    }

    #[test]
    fn at_or_above_threshold_is_true() {
        let threshold = Duration::hours(2);
        assert!(IdlePromptDecision::decide(threshold, threshold));
        assert!(IdlePromptDecision::decide(
            threshold + Duration::hours(1),
            threshold
        ));
    }
}
