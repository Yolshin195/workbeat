use async_trait::async_trait;
use domain::TelegramId;

use crate::error::NotifierError;
use crate::outbound_message::OutboundMessage;

/// Исходящий порт для интерфейсов — реализация поверх Telegram появится в
/// Задаче 12.
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn send(
        &self,
        user: TelegramId,
        message: OutboundMessage,
    ) -> Result<(), NotifierError>;
}
