/// Исходящее сообщение пользователю, отправляемое через `Notifier`. Не
/// привязано к Telegram — никаких `ChatId`/типов `teloxide` здесь, адаптер
/// (Задача 12) сам решает, как превратить вариант в конкретный Telegram-вызов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundMessage {
    Text(String),
    /// Нужен для отправки CSV-экспорта (Задача 9) как файла, а не текстом.
    Document { filename: String, bytes: Vec<u8> },
}
