use async_trait::async_trait;
use domain::{TelegramId, User};

use crate::error::RepoError;

/// `users` не имеет отдельного суррогатного id — естественный ключ это
/// `telegram_id` (см. `domain::User`).
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, user: User) -> Result<(), RepoError>;

    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, RepoError>;
}
