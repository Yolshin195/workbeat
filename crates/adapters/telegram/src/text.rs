//! Все текстовые сообщения и подписи кнопок бота — в одном модуле (критерий
//! приёмки Задачи 11: "Все тексты сообщений вынесены в один модуль, легко
//! менять формулировки"). Никакой бизнес-логики здесь нет — только строки.

pub const WELCOME: &str =
    "Привет! Я помогу дисциплинировать рабочий день по методике 50/10.\n\
     Команды: /start_day, /finish_day, /report, /report_week, /report_month, /new_task, /tasks, /edit_task.";

pub const DAY_ALREADY_OPEN: &str = "День уже начат.";
pub const DAY_STARTED: &str = "День начат! Выбери задачу, чтобы начать интервал.";
pub const NO_TASKS_TO_START: &str =
    "В пуле нет готовых задач. Создай задачу командой /new_task.";
pub const DAY_NOT_OPEN: &str = "День ещё не начат — сначала /start_day.";
pub const DAY_ALREADY_FINISHED: &str = "День уже завершён.";

pub const PREVIOUS_INTERVAL_NOT_CLOSED: &str = "Предыдущий интервал ещё не закрыт.";
pub const INTERVAL_STARTED: &str = "Интервал начат. Через 10 минут спрошу, работаешь ли ты.";

pub const YES_BUTTON: &str = "Да";
pub const NO_BUTTON: &str = "Нет";
pub const WORKED_ACK: &str = "Отлично, продолжай.";
pub const ASK_WHAT_HAPPENED: &str = "Что случилось, почему перестал работать?";
pub const REASON_RECORDED: &str = "Понял. Готов продолжить?";
pub const READY_TO_CONTINUE_BUTTON: &str = "Готов продолжить";
pub const NOTHING_TO_CONFIRM: &str = "Пока нечего подтверждать.";
pub const NO_OPEN_CHECK: &str = "Сейчас нет открытой десятиминутки.";
pub const NEXT_SLOT_STARTED: &str = "Продолжаем.";

pub const RETURNED_BUTTON: &str = "Вернулся";
pub const NO_OPEN_REST_OR_LUNCH: &str = "Сейчас нечего завершать — ни отдых, ни обед не активны.";

pub const ASK_HOUR_SUMMARY: &str = "Что делали за этот час?";
pub const INTERVAL_FINISHED_PREFIX: &str = "Интервал завершён";
pub const START_NEXT_INTERVAL_BUTTON: &str = "Начать следующий интервал";
pub const GO_TO_LUNCH_BUTTON: &str = "Пойти на обед";
pub const TASK_READY_BUTTON: &str = "Задача готова";

pub const LUNCH_STARTED: &str = "Приятного обеда! Нажми «Вернулся», когда будешь готов продолжить.";
pub const LUNCH_ACTIVE_INTERVAL_OPEN: &str = "Нельзя пойти на обед во время активного интервала.";
pub const LUNCH_ALREADY_STARTED: &str = "Обед уже начат.";
pub const LUNCH_NOT_STARTED: &str = "Обед ещё не начат.";
pub const LUNCH_ALREADY_ENDED: &str = "Обед уже завершён.";
pub const LUNCH_ENDED: &str = "С возвращением! Можно начинать следующий интервал.";

pub const IDLE_CONTINUE_BUTTON: &str = "Продолжить работу";

pub const PICK_TASK_TO_START: &str = "Выбери задачу для интервала:";
pub const PICK_TASK_TO_SWITCH: &str = "Текущая задача завершена. Выбери следующую:";
pub const NO_TASKS_AVAILABLE: &str = "В пуле пока нет задач.";
pub const TASK_LIST_HEADER: &str = "Пул задач:";

pub const NEW_TASK_ASK_TITLE: &str = "Название новой задачи?";
pub const NEW_TASK_ASK_PRIORITY: &str = "Приоритет задачи?";
pub const NEW_TASK_ASK_DEADLINE: &str =
    "Дедлайн задачи? Отправь дату в формате ГГГГ-ММ-ДД или «нет».";
pub const NEW_TASK_CREATED: &str = "Задача создана.";
pub const EMPTY_TITLE: &str = "Название не может быть пустым, попробуй ещё раз.";
pub const INVALID_DATE: &str = "Не понял дату. Формат: ГГГГ-ММ-ДД, либо «нет».";

pub const PRIORITY_HIGH_BUTTON: &str = "Высокий";
pub const PRIORITY_MEDIUM_BUTTON: &str = "Средний";
pub const PRIORITY_LOW_BUTTON: &str = "Низкий";
pub const PRIORITY_NONE_BUTTON: &str = "Без приоритета";

pub const PICK_TASK_TO_EDIT: &str = "Какую задачу редактировать?";
pub const EDIT_TASK_ASK_TITLE: &str = "Новое название? Отправь текст или «нет», чтобы оставить прежнее.";
pub const EDIT_TASK_ASK_STATUS: &str = "Новый статус задачи?";
pub const STATUS_READY_BUTTON: &str = "Готова";
pub const STATUS_NOT_READY_BUTTON: &str = "Не готова";
pub const STATUS_DONE_BUTTON: &str = "Завершена";
pub const TASK_UPDATED: &str = "Задача обновлена.";
pub const SKIP_FIELD_BUTTON: &str = "Оставить как есть";

pub const NO_ACTIVE_TASK_TO_SWITCH: &str = "Сейчас нет активной задачи в этом интервале.";

pub const REPORT_EMPTY_DAY: &str = "День ещё не начат.";
pub const CSV_FILENAME: &str = "report.csv";

pub const UNKNOWN_INPUT: &str = "Не понял. Используй кнопки или команды из /start.";
pub const DIALOG_CANCELLED: &str = "Отменено.";
pub const GENERIC_ERROR: &str = "Что-то пошло не так, попробуй ещё раз.";
/// Клавиатура "Вернулся" после отдыха — сам текст "Ты молодец, отдыхай 10
/// минут" уже отправляет `Notifier` изнутри `SubmitTenMinAnswer` (Задача 7),
/// поэтому здесь только подсказка, без дублирования формулировки.
pub const REST_KEYBOARD_HINT: &str = "Кнопка ниже — когда вернёшься с отдыха.";

pub fn task_priority_label(priority: Option<domain::TaskPriority>) -> &'static str {
    match priority {
        Some(domain::TaskPriority::High) => "высокий",
        Some(domain::TaskPriority::Medium) => "средний",
        Some(domain::TaskPriority::Low) => "низкий",
        None => "—",
    }
}

pub fn task_status_label(status: domain::TaskStatus) -> &'static str {
    match status {
        domain::TaskStatus::Ready => "готова",
        domain::TaskStatus::NotReady => "не готова",
        domain::TaskStatus::InProgress => "в работе",
        domain::TaskStatus::Done => "завершена",
    }
}

/// Одна строка пула задач для списков/клавиатур — заголовок кнопки с
/// приоритетом/дедлайном, если заданы (требование Задачи 11: отображение
/// приоритета и дедлайна в пуле задач).
pub fn task_button_label(task: &domain::Task) -> String {
    let mut label = task.title().to_string();
    if let Some(priority) = task.priority() {
        label.push_str(" [");
        label.push_str(task_priority_label(Some(priority)));
        label.push(']');
    }
    if let Some(deadline) = task.deadline() {
        label.push_str(" (до ");
        label.push_str(&deadline.to_string());
        label.push(')');
    }
    label
}

pub fn task_list_line(task: &domain::Task) -> String {
    format!(
        "- {} [{}, приоритет: {}{}]",
        task.title(),
        task_status_label(task.status()),
        task_priority_label(task.priority()),
        task.deadline()
            .map(|d| format!(", дедлайн: {d}"))
            .unwrap_or_default()
    )
}

pub fn interval_finished_summary(closed: u8, needed: u8) -> String {
    format!("{INTERVAL_FINISHED_PREFIX}: {closed}/{needed}")
}

pub fn format_duration(duration: chrono::Duration) -> String {
    let minutes = duration.num_minutes();
    format!("{}ч {:02}м", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_duration_as_hours_and_minutes() {
        assert_eq!(format_duration(chrono::Duration::minutes(90)), "1ч 30м");
        assert_eq!(format_duration(chrono::Duration::minutes(0)), "0ч 00м");
    }
}
