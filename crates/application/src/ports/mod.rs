//! Порты (traits), через которые use cases общаются с внешним миром —
//! реализаций здесь нет, они появятся в адаптерах (Задачи 4, 10, 12).

mod clock;
mod hour_interval_repository;
mod notifier;
mod task_repository;
mod ten_min_check_repository;
mod user_repository;
mod work_day_repository;

pub use clock::Clock;
pub use hour_interval_repository::HourIntervalRepository;
pub use notifier::Notifier;
pub use task_repository::TaskRepository;
pub use ten_min_check_repository::TenMinCheckRepository;
pub use user_repository::UserRepository;
pub use work_day_repository::WorkDayRepository;
