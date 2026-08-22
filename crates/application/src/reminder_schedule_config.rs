use chrono::Duration;

/// Три настраиваемых порога расписания напоминаний из README.md раздела 2,
/// п.3 / раздела 7 — общие для переработанного отдыха (`AdvanceOpenTenMinChecks`)
/// и ожидания возврата после провала рабочей десятиминутки
/// (`RemindAwaitingResume`). Реальные значения читаются из `.env` в Задаче 13
/// composition root; `Default` здесь — это дефолты по SPEC.md раздела 7,
/// используемые до тех пор, пока конфигурация не подключена явно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderScheduleConfig {
    pub burst_window: Duration,
    pub burst_interval: Duration,
    pub steady_interval: Duration,
}

impl Default for ReminderScheduleConfig {
    fn default() -> Self {
        Self {
            burst_window: Duration::minutes(5),
            burst_interval: Duration::minutes(1),
            steady_interval: Duration::minutes(5),
        }
    }
}
