use chrono::Duration;

/// Порог простоя `N` из README.md раздела 1/7: как часто (не чаще одного раза
/// в `N` часов) `PromptContinueIfIdle` спрашивает "Готовы продолжить работу?"
/// у дня без активного часового интервала. Реальное значение читается из
/// `.env` в Задаче 13 composition root; `Default` здесь — значение по
/// умолчанию, ориентировочное (README.md раздел 7 не фиксирует конкретное
/// число), используемое до тех пор, пока конфигурация не подключена явно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdlePromptConfig {
    pub idle_threshold: Duration,
}

impl Default for IdlePromptConfig {
    fn default() -> Self {
        Self {
            idle_threshold: Duration::hours(2),
        }
    }
}
