//! Единственная продовая реализация порта `application::Clock` (Задача 3) —
//! прямая обёртка над `chrono::Utc::now()`. Никакой другой крейт её не
//! предоставляет специально: use cases и `PollLoop` (Задача 10) работают
//! исключительно через порт, а не через `chrono` напрямую, поэтому реальное
//! "сейчас" появляется только здесь, в composition root.

use application::Clock;
use domain::UtcTimestamp;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UtcTimestamp {
        UtcTimestamp::new(chrono::Utc::now())
    }
}
