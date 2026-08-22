//! Состояние диалога с одним чатом. Ни одна из сущностей `application`/`domain`
//! персистентно не хранит "что бот сейчас ждёт от пользователя" (в схеме
//! SPEC.md такой таблицы нет — это чисто UI-состояние Telegram-адаптера), так
//! что оно живёт только в памяти процесса и не переживает рестарт. Это не
//! нарушает restart-safety проактивных сообщений (SPEC.md п.4): те целиком
//! зависят от данных в БД и опрашиваются `PollLoop` (Задача 10) независимо
//! от этого состояния — см. пояснение в модуле `intent`.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::NaiveDate;
use domain::{HourIntervalId, TaskId, TaskPriority, TenMinCheckId, WorkDayId};
use teloxide::types::ChatId;

/// Что бот ждёт от следующего свободного текстового сообщения этого чата.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Awaiting {
    #[default]
    Nothing,
    /// Ответ на "Что случилось?" после явного "Нет" — закрывает открытую
    /// десятиминутку через `SubmitTenMinAnswer(No, reason)`.
    TenMinReasonExplicit,
    /// Ответ "Готов продолжить?".
    ConfirmContinue,
    /// Ответ на обязательный вопрос "что делали за этот час".
    HourSummary,
    /// Ожидается свободный текст с названием новой задачи.
    NewTaskTitle,
    /// Название уже получено, ждём нажатия одной из кнопок приоритета.
    NewTaskPriority {
        title: String,
    },
    /// Приоритет уже выбран, ждём текст с дедлайном (или "нет").
    NewTaskDeadline {
        title: String,
        priority: Option<TaskPriority>,
    },
    /// Ждём текст с новым названием (или "нет", чтобы оставить прежнее).
    EditTaskTitle {
        task_id: TaskId,
    },
    /// Название решено, ждём нажатия кнопки приоритета.
    EditTaskPriority {
        task_id: TaskId,
        title: Option<String>,
    },
    /// Приоритет решён, ждём текст с дедлайном (или "нет").
    EditTaskDeadline {
        task_id: TaskId,
        title: Option<String>,
        priority: Option<Option<TaskPriority>>,
    },
    /// Дедлайн решён, ждём нажатия кнопки статуса — последний шаг перед
    /// вызовом `UpdateTask`.
    EditTaskStatus {
        task_id: TaskId,
        title: Option<String>,
        priority: Option<Option<TaskPriority>>,
        deadline: Option<Option<NaiveDate>>,
    },
}

/// Состояние одного чата (= один пользователь, приватный чат). Идентификаторы
/// текущего дня/интервала/десятиминутки кэшируются здесь после того, как их
/// вернул соответствующий use case — адаптеру запрещено обращаться к
/// репозиториям напрямую (критерий приёмки Задачи 11), поэтому это
/// единственный способ передать `work_day_id`/`interval_id` из одной команды
/// в следующую, не считая случаев, когда use case сам не требует их (см.
/// `intent.rs`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatSession {
    pub work_day_id: Option<WorkDayId>,
    pub interval_id: Option<HourIntervalId>,
    /// Последняя известная десятиминутка интервала — открытая либо только что
    /// закрытая. Нужна для `SubmitFailureReason`, когда пользователь
    /// откликается на молчание уже после того, как `PollLoop` (Задача
    /// 7/10) сам закрыл десятиминутку как `NoResponse` без участия
    /// адаптера — см. `intent.rs`.
    pub check_id: Option<TenMinCheckId>,
    /// Счётчик успешных (`Worked`) ответов текущего интервала, полученный из
    /// возвращаемых значений `SubmitTenMinAnswer` — используется только для
    /// выбора, какую кнопку показать дальше ("Вернулся" после отдыха или
    /// снова "Да/Нет"), не для проверки бизнес-правила "хватит ли 5" — это
    /// решает domain внутри use case, а не адаптер.
    pub worked_in_interval: u8,
    pub rest_active: bool,
    pub lunch_active: bool,
    pub awaiting: Awaiting,
}

/// Простое in-memory хранилище состояний по чатам. `PollLoop` (Задача 10) и
/// Telegram-адаптер независимы (см. `tasks.md`, Задача 11), поэтому это
/// хранилище нужно только входящим апдейтам этого адаптера.
#[derive(Default)]
pub struct SessionStore {
    sessions: Mutex<HashMap<ChatId, ChatSession>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, chat_id: ChatId) -> ChatSession {
        self.sessions
            .lock()
            .expect("session store mutex poisoned")
            .get(&chat_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set(&self, chat_id: ChatId, session: ChatSession) {
        self.sessions
            .lock()
            .expect("session store mutex poisoned")
            .insert(chat_id, session);
    }
}

/// Хелпер для парсинга свободного текста дедлайна в диалоге создания/
/// редактирования задачи — "нет" (в любом регистре) значит "без дедлайна",
/// иначе ожидается `ГГГГ-ММ-ДД`. Чистая функция, тестируется без сети.
pub fn parse_deadline_input(text: &str) -> Result<Option<NaiveDate>, ()> {
    let trimmed = text.trim();
    // `eq_ignore_ascii_case` не подходит для кириллицы (сравнивает только
    // ASCII-байты), поэтому кириллическое слово сравнивается через
    // `to_lowercase()` — корректный Unicode case-folding.
    if trimmed.to_lowercase() == "нет" || trimmed.eq_ignore_ascii_case("skip") {
        return Ok(None);
    }
    NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_returns_default_session_for_unknown_chat() {
        let store = SessionStore::new();
        assert_eq!(store.get(ChatId(1)), ChatSession::default());
    }

    #[test]
    fn store_roundtrips_session_updates() {
        let store = SessionStore::new();
        let session = ChatSession {
            work_day_id: Some(WorkDayId::new(5)),
            awaiting: Awaiting::HourSummary,
            ..ChatSession::default()
        };
        store.set(ChatId(1), session.clone());

        assert_eq!(store.get(ChatId(1)), session);
        // Другой чат не задет.
        assert_eq!(store.get(ChatId(2)), ChatSession::default());
    }

    #[test]
    fn parses_deadline_skip_keywords_case_insensitively() {
        assert_eq!(parse_deadline_input("нет"), Ok(None));
        assert_eq!(parse_deadline_input("НЕТ"), Ok(None));
        assert_eq!(parse_deadline_input("skip"), Ok(None));
    }

    #[test]
    fn parses_valid_iso_date() {
        assert_eq!(
            parse_deadline_input("2026-12-31"),
            Ok(Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()))
        );
    }

    #[test]
    fn rejects_malformed_date() {
        assert_eq!(parse_deadline_input("31/12/2026"), Err(()));
        assert_eq!(parse_deadline_input("not a date"), Err(()));
    }
}
