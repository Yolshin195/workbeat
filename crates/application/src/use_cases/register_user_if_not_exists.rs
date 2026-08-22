use std::sync::Arc;

use domain::{TelegramId, TimeZoneOffset, User};

use crate::error::RepoError;
use crate::ports::{Clock, UserRepository};

/// Вызывается на любое входящее сообщение от пользователя (SPEC.md,
/// middleware Telegram-адаптера — Задача 11): идемпотентно гарантирует, что
/// для `telegram_id` есть строка в `users`. Часовой пояс новому пользователю
/// проставляется в UTC по умолчанию — его можно изменить позже отдельной
/// командой (вне рамок этой задачи).
pub struct RegisterUserIfNotExists {
    user_repository: Arc<dyn UserRepository>,
    clock: Arc<dyn Clock>,
}

impl RegisterUserIfNotExists {
    pub fn new(user_repository: Arc<dyn UserRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            user_repository,
            clock,
        }
    }

    pub async fn execute(&self, telegram_id: TelegramId) -> Result<User, RepoError> {
        if let Some(existing) = self
            .user_repository
            .find_by_telegram_id(telegram_id)
            .await?
        {
            return Ok(existing);
        }

        let user = User::new(telegram_id, TimeZoneOffset::default(), self.clock.now());
        self.user_repository.create(user.clone()).await?;
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, InMemoryUserRepository};
    use chrono::Utc;
    use domain::UtcTimestamp;

    fn use_case() -> (RegisterUserIfNotExists, Arc<InMemoryUserRepository>) {
        let user_repository = Arc::new(InMemoryUserRepository::new());
        let clock = Arc::new(FakeClock::new(UtcTimestamp::new(Utc::now())));
        (
            RegisterUserIfNotExists::new(user_repository.clone(), clock),
            user_repository,
        )
    }

    #[test]
    fn creates_new_user_on_first_message() {
        let (use_case, repo) = use_case();
        let telegram_id = TelegramId::new(1).unwrap();

        let user = pollster::block_on(use_case.execute(telegram_id)).unwrap();

        assert_eq!(user.telegram_id(), telegram_id);
        assert_eq!(repo.snapshot(), vec![user]);
    }

    #[test]
    fn repeated_calls_do_not_duplicate_user() {
        let (use_case, repo) = use_case();
        let telegram_id = TelegramId::new(1).unwrap();

        let first = pollster::block_on(use_case.execute(telegram_id)).unwrap();
        let second = pollster::block_on(use_case.execute(telegram_id)).unwrap();

        assert_eq!(first, second);
        assert_eq!(repo.snapshot(), vec![first]);
    }
}
