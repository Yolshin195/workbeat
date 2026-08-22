//! Периодический poll-цикл (Задача 10 корневого `tasks.md`) — единственный
//! механизм проактивных сообщений во всей системе (README.md, принцип
//! таймеров). Вместо отдельного `Scheduler`-порта с job'ами цикл на каждом
//! тике просто вызывает три use case из `application`
//! (`AdvanceOpenTenMinChecks`, `RemindAwaitingResume`,
//! `PromptContinueIfIdle`), которые сами перечитывают текущее состояние из
//! БД через порты. Между тиками `PollLoop` не хранит вообще никакого
//! состояния (ни `HashMap`, ни `JobId`) — это делает его тривиально
//! restart-safe: после рестарта процесса первый же тик отрабатывает как
//! обычно.

use std::sync::Arc;
use std::time::Duration;

use application::{AdvanceOpenTenMinChecks, PromptContinueIfIdle, RemindAwaitingResume};

/// Шаг опроса по умолчанию — совпадает с минимальной частотой
/// `burst_interval` расписания напоминаний (README.md раздел 7).
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

pub struct PollLoop {
    advance_open_ten_min_checks: Arc<AdvanceOpenTenMinChecks>,
    remind_awaiting_resume: Arc<RemindAwaitingResume>,
    prompt_continue_if_idle: Arc<PromptContinueIfIdle>,
    poll_interval: Duration,
}

impl PollLoop {
    pub fn new(
        advance_open_ten_min_checks: Arc<AdvanceOpenTenMinChecks>,
        remind_awaiting_resume: Arc<RemindAwaitingResume>,
        prompt_continue_if_idle: Arc<PromptContinueIfIdle>,
        poll_interval: Duration,
    ) -> Self {
        Self {
            advance_open_ten_min_checks,
            remind_awaiting_resume,
            prompt_continue_if_idle,
            poll_interval,
        }
    }

    /// Бесконечный цикл опроса; не завершается сам — вызывающий код
    /// (composition root, Задача 13) решает, в какой tokio-задаче его
    /// запустить (например, `tokio::spawn`).
    pub async fn run(&self) {
        let mut interval = tokio::time::interval(self.poll_interval);
        loop {
            interval.tick().await;
            self.tick().await;
        }
    }

    /// Один тик опроса. Порядок между use cases не важен — это разные
    /// сущности (открытые десятиминутки, десятиминутки в ожидании возврата,
    /// простаивающие дни), поэтому они вызываются параллельно.
    pub async fn tick(&self) {
        let (advance, remind, idle) = tokio::join!(
            self.advance_open_ten_min_checks.execute(),
            self.remind_awaiting_resume.execute(),
            self.prompt_continue_if_idle.execute(),
        );
        if let Err(err) = advance {
            eprintln!("poll tick: AdvanceOpenTenMinChecks failed: {err}");
        }
        if let Err(err) = remind {
            eprintln!("poll tick: RemindAwaitingResume failed: {err}");
        }
        if let Err(err) = idle {
            eprintln!("poll tick: PromptContinueIfIdle failed: {err}");
        }
    }
}

#[cfg(test)]
mod tests;
