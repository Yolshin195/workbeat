//! Composition root (Задача 13 корневого `tasks.md`): единственное место, где
//! собираются конкретные реализации всех портов (`SqliteUserRepository` и
//! другие репозитории, `TeloxideNotifier`, `SystemClock`) и передаются в use
//! cases, а use cases — в Telegram-адаптер (Задача 11) и `PollLoop` (Задача
//! 10). Вынесено в библиотеку (а не только в `main.rs`), чтобы
//! интеграционные тесты (restart-safety) могли собрать то же самое дерево
//! зависимостей без реального Telegram-бота — см. `tests/`.

pub mod clock;
pub mod config;
pub mod wiring;

use std::sync::Arc;

use application::{Clock, Notifier};
use teloxide::Bot;

pub use clock::SystemClock;
pub use config::{Config, ConfigError};
pub use wiring::{wire, Adapters, Wired};

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("failed to connect to the database: {0}")]
    Database(#[from] sqlx::Error),
}

/// Запускает бота: читает `.env`/окружение, поднимает SQLite-пул, собирает
/// use cases (`wiring::wire`) и запускает `PollLoop` (Задача 10) фоновой
/// tokio-задачей параллельно с Telegram-ботом (Задача 11). Никакого
/// отдельного шага восстановления состояния при старте нет — первый же тик
/// `PollLoop` видит текущее состояние БД, что после холодного старта, что
/// после рестарта посреди дня (см. `README.md`, принцип таймеров).
///
/// Единственные места, где допустимы `unwrap()`/`expect()` для этого крейта —
/// сборка тестовых фикстур в `tests/`; в самом `run()` их нет: все ошибки
/// (отсутствующий конфиг, недоступная БД) — типизированный `RunError`.
pub async fn run() -> Result<(), RunError> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    let pool = adapters_persistence_sqlite::connect(&config.database_url).await?;

    let bot = Bot::new(&config.telegram_token);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let notifier: Arc<dyn Notifier> =
        Arc::new(adapters_telegram::TeloxideNotifier::new(bot.clone()));

    let Wired {
        telegram_use_cases,
        poll_loop,
    } = wire(Adapters {
        pool: pool.clone(),
        clock,
        notifier,
        idle_prompt: config.idle_prompt,
        reminder_schedule: config.reminder_schedule,
        poll_interval: config.poll_interval,
    });

    // Poll-цикл — фоновая задача параллельно с ботом; Ctrl+C останавливает
    // `adapters_telegram::run` (у него уже включён `enable_ctrlc_handler`),
    // после чего мы явно останавливаем poll-цикл и закрываем пул.
    let poll_handle = tokio::spawn(async move { poll_loop.run().await });

    adapters_telegram::run(bot, telegram_use_cases).await;

    poll_handle.abort();
    pool.close().await;

    Ok(())
}
