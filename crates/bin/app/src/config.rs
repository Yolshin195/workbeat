//! Конфигурация composition root (Задача 13 корневого `tasks.md`): путь к
//! SQLite-файлу, Telegram-токен, порог простоя `N` для `PromptContinueIfIdle`
//! (Задача 8) и `burst_window`/`burst_interval`/`steady_interval` для
//! `ReminderScheduleDecision` (Задача 2/7). Значения читаются из переменных
//! окружения (`.env` подхватывается через `dotenvy` в `run()`, см. `lib.rs`);
//! обязательные переменные без значения — понятная типизированная ошибка, а
//! не паника.

use std::time::Duration as StdDuration;

use application::{IdlePromptConfig, ReminderScheduleConfig};
use chrono::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub database_url: String,
    pub telegram_token: String,
    pub poll_interval: StdDuration,
    pub idle_prompt: IdlePromptConfig,
    pub reminder_schedule: ReminderScheduleConfig,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("environment variable {0} is required but not set")]
    Missing(&'static str),
    #[error(
        "environment variable {name} has invalid value {value:?}: expected a positive integer"
    )]
    InvalidPositiveInt { name: &'static str, value: String },
}

impl Config {
    /// Читает конфиг из переменных окружения текущего процесса. Не читает
    /// `.env` сам — это делает вызывающая сторона (`run()` в `lib.rs`) один
    /// раз перед вызовом, через `dotenvy::dotenv()`, чтобы эта функция
    /// оставалась чистой и тестируемой без файловой системы.
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = required_var("DATABASE_URL")?;
        let telegram_token = required_var("TELEGRAM_BOT_TOKEN")?;

        let poll_interval_secs = positive_u64_var("POLL_INTERVAL_SECS", 60)?;
        let idle_threshold_hours = positive_i64_var("IDLE_PROMPT_THRESHOLD_HOURS", 2)?;
        let burst_window_minutes = positive_i64_var("REMINDER_BURST_WINDOW_MINUTES", 5)?;
        let burst_interval_minutes = positive_i64_var("REMINDER_BURST_INTERVAL_MINUTES", 1)?;
        let steady_interval_minutes = positive_i64_var("REMINDER_STEADY_INTERVAL_MINUTES", 5)?;

        Ok(Self {
            database_url,
            telegram_token,
            poll_interval: StdDuration::from_secs(poll_interval_secs),
            idle_prompt: IdlePromptConfig {
                idle_threshold: Duration::hours(idle_threshold_hours),
            },
            reminder_schedule: ReminderScheduleConfig {
                burst_window: Duration::minutes(burst_window_minutes),
                burst_interval: Duration::minutes(burst_interval_minutes),
                steady_interval: Duration::minutes(steady_interval_minutes),
            },
        })
    }
}

fn required_var(name: &'static str) -> Result<String, ConfigError> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(ConfigError::Missing(name)),
    }
}

fn positive_u64_var(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match std::env::var(name) {
        Err(_) => Ok(default),
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|parsed| *parsed > 0)
            .ok_or(ConfigError::InvalidPositiveInt { name, value }),
    }
}

fn positive_i64_var(name: &'static str, default: i64) -> Result<i64, ConfigError> {
    match std::env::var(name) {
        Err(_) => Ok(default),
        Ok(value) => value
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|parsed| *parsed > 0)
            .ok_or(ConfigError::InvalidPositiveInt { name, value }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    // Переменные окружения — общее состояние процесса: тесты этого модуля
    // мутируют их напрямую, поэтому сериализуем их через мьютекс, чтобы не
    // ловить гонки при параллельном запуске `cargo test`.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const REQUIRED_VARS: &[&str] = &["DATABASE_URL", "TELEGRAM_BOT_TOKEN"];
    const OPTIONAL_VARS: &[&str] = &[
        "POLL_INTERVAL_SECS",
        "IDLE_PROMPT_THRESHOLD_HOURS",
        "REMINDER_BURST_WINDOW_MINUTES",
        "REMINDER_BURST_INTERVAL_MINUTES",
        "REMINDER_STEADY_INTERVAL_MINUTES",
    ];

    fn clear_all_vars() {
        for name in REQUIRED_VARS.iter().chain(OPTIONAL_VARS) {
            std::env::remove_var(name);
        }
    }

    fn set_required_vars() {
        std::env::set_var("DATABASE_URL", "sqlite://./workbeat-test.db");
        std::env::set_var("TELEGRAM_BOT_TOKEN", "test-token");
    }

    #[test]
    fn missing_database_url_is_a_clear_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all_vars();
        std::env::set_var("TELEGRAM_BOT_TOKEN", "test-token");

        let result = Config::from_env();

        assert_eq!(result, Err(ConfigError::Missing("DATABASE_URL")));
        clear_all_vars();
    }

    #[test]
    fn missing_telegram_token_is_a_clear_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all_vars();
        std::env::set_var("DATABASE_URL", "sqlite://./workbeat-test.db");

        let result = Config::from_env();

        assert_eq!(result, Err(ConfigError::Missing("TELEGRAM_BOT_TOKEN")));
        clear_all_vars();
    }

    #[test]
    fn defaults_apply_when_optional_vars_are_absent() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all_vars();
        set_required_vars();

        let config = Config::from_env().unwrap();

        assert_eq!(config.poll_interval, StdDuration::from_secs(60));
        assert_eq!(config.idle_prompt, IdlePromptConfig::default());
        assert_eq!(config.reminder_schedule, ReminderScheduleConfig::default());
        clear_all_vars();
    }

    #[test]
    fn optional_vars_override_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all_vars();
        set_required_vars();
        std::env::set_var("POLL_INTERVAL_SECS", "30");
        std::env::set_var("IDLE_PROMPT_THRESHOLD_HOURS", "3");
        std::env::set_var("REMINDER_BURST_WINDOW_MINUTES", "10");
        std::env::set_var("REMINDER_BURST_INTERVAL_MINUTES", "2");
        std::env::set_var("REMINDER_STEADY_INTERVAL_MINUTES", "7");

        let config = Config::from_env().unwrap();

        assert_eq!(config.poll_interval, StdDuration::from_secs(30));
        assert_eq!(config.idle_prompt.idle_threshold, Duration::hours(3));
        assert_eq!(config.reminder_schedule.burst_window, Duration::minutes(10));
        assert_eq!(
            config.reminder_schedule.burst_interval,
            Duration::minutes(2)
        );
        assert_eq!(
            config.reminder_schedule.steady_interval,
            Duration::minutes(7)
        );
        clear_all_vars();
    }

    #[test]
    fn invalid_optional_var_is_a_clear_error_not_a_panic() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all_vars();
        set_required_vars();
        std::env::set_var("POLL_INTERVAL_SECS", "not-a-number");

        let result = Config::from_env();

        assert_eq!(
            result,
            Err(ConfigError::InvalidPositiveInt {
                name: "POLL_INTERVAL_SECS",
                value: "not-a-number".to_string(),
            })
        );
        clear_all_vars();
    }

    #[test]
    fn zero_is_rejected_for_positive_int_vars() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_all_vars();
        set_required_vars();
        std::env::set_var("IDLE_PROMPT_THRESHOLD_HOURS", "0");

        let result = Config::from_env();

        assert_eq!(
            result,
            Err(ConfigError::InvalidPositiveInt {
                name: "IDLE_PROMPT_THRESHOLD_HOURS",
                value: "0".to_string(),
            })
        );
        clear_all_vars();
    }
}
