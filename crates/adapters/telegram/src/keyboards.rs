//! Инлайн-клавиатуры — чистые функции-билдеры поверх `callback::CallbackAction`
//! и `text.rs`. Не содержат бизнес-логики, только раскладку кнопок.

use domain::Task;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use crate::callback::CallbackAction;
use crate::commands::Command;
use crate::text;

fn button(label: &str, action: CallbackAction) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(label, action.to_data())
}

/// Да/Нет на "Ты работаешь?" плюс "Задача готова" — доступна в любой момент
/// активного интервала (README.md раздел 2, п.6).
pub fn ten_min_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button(text::YES_BUTTON, CallbackAction::TenMinYes),
            button(text::NO_BUTTON, CallbackAction::TenMinNo),
        ],
        vec![button(text::TASK_READY_BUTTON, CallbackAction::TaskReady)],
    ])
}

pub fn confirm_continue_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[button(
        text::READY_TO_CONTINUE_BUTTON,
        CallbackAction::ConfirmContinue,
    )]])
}

pub fn returned_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[button(text::RETURNED_BUTTON, CallbackAction::Returned)]])
}

pub fn after_interval_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([
        [button(
            text::START_NEXT_INTERVAL_BUTTON,
            CallbackAction::RequestStartInterval,
        )],
        [button(text::GO_TO_LUNCH_BUTTON, CallbackAction::GoToLunch)],
    ])
}

pub fn idle_continue_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[button(
        text::IDLE_CONTINUE_BUTTON,
        CallbackAction::RequestStartInterval,
    )]])
}

/// Список задач пула — одна задача на строку, подпись с приоритетом и
/// дедлайном (`text::task_button_label`), `to_action` решает, какое действие
/// повесить на кнопку (старт интервала / переключение задачи / редактирование).
pub fn task_list_keyboard(
    tasks: &[Task],
    to_action: impl Fn(domain::TaskId) -> CallbackAction,
) -> InlineKeyboardMarkup {
    let rows = tasks
        .iter()
        .map(|task| [button(&text::task_button_label(task), to_action(task.id()))]);
    InlineKeyboardMarkup::new(rows)
}

pub fn priority_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([
        [
            button(
                text::PRIORITY_HIGH_BUTTON,
                CallbackAction::Priority(Some(domain::TaskPriority::High)),
            ),
            button(
                text::PRIORITY_MEDIUM_BUTTON,
                CallbackAction::Priority(Some(domain::TaskPriority::Medium)),
            ),
        ],
        [
            button(
                text::PRIORITY_LOW_BUTTON,
                CallbackAction::Priority(Some(domain::TaskPriority::Low)),
            ),
            button(text::PRIORITY_NONE_BUTTON, CallbackAction::Priority(None)),
        ],
    ])
}

pub fn status_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button(
                text::STATUS_READY_BUTTON,
                CallbackAction::Status(domain::TaskStatus::Ready),
            ),
            button(
                text::STATUS_NOT_READY_BUTTON,
                CallbackAction::Status(domain::TaskStatus::NotReady),
            ),
        ],
        vec![button(
            text::STATUS_DONE_BUTTON,
            CallbackAction::Status(domain::TaskStatus::Done),
        )],
    ])
}

pub fn skip_field_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new([[button(text::SKIP_FIELD_BUTTON, CallbackAction::SkipField)]])
}

/// Меню `/help` — по кнопке на команду, чтобы не набирать их руками. Кнопки
/// шлют `CallbackAction::RunCommand`, который проходит через тот же
/// `interpret_command`, что и текстовая команда (см. `intent.rs`).
pub fn help_keyboard() -> InlineKeyboardMarkup {
    fn run(command: Command) -> CallbackAction {
        CallbackAction::RunCommand(command)
    }

    InlineKeyboardMarkup::new([
        vec![button(text::BTN_START_DAY, run(Command::StartDay))],
        vec![button(text::BTN_FINISH_DAY, run(Command::FinishDay))],
        vec![button(text::BTN_REPORT, run(Command::Report))],
        vec![
            button(text::BTN_REPORT_WEEK, run(Command::ReportWeek)),
            button(text::BTN_REPORT_MONTH, run(Command::ReportMonth)),
        ],
        vec![button(text::BTN_EXPORT_CSV, run(Command::ExportCsv))],
        vec![button(text::BTN_NEW_TASK, run(Command::NewTask))],
        vec![button(text::BTN_TASKS, run(Command::Tasks))],
        vec![button(text::BTN_EDIT_TASK, run(Command::EditTask))],
        vec![button(text::BTN_CURRENT_INTERVAL, run(Command::CurrentInterval))],
        vec![button(text::BTN_START_INTERVAL, run(Command::StartInterval))],
        vec![button(text::BTN_FINISH_INTERVAL, run(Command::FinishInterval))],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use domain::{TaskId, TaskStatus, TelegramId, UtcTimestamp};

    fn sample_task(id: i64, title: &str) -> Task {
        Task::new(
            TaskId::new(id),
            TelegramId::new(1).unwrap(),
            title,
            TaskStatus::Ready,
            Some(domain::TaskPriority::High),
            Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            UtcTimestamp::new(chrono::Utc::now()),
        )
        .unwrap()
    }

    #[test]
    fn task_list_keyboard_has_one_row_per_task_with_requested_action() {
        let tasks = vec![sample_task(1, "A"), sample_task(2, "B")];
        let keyboard = task_list_keyboard(&tasks, CallbackAction::StartInterval);

        assert_eq!(keyboard.inline_keyboard.len(), 2);
        assert_eq!(
            keyboard.inline_keyboard[0][0].text,
            text::task_button_label(&tasks[0])
        );
    }

    #[test]
    fn ten_min_keyboard_has_yes_no_row_and_a_task_ready_row() {
        let keyboard = ten_min_keyboard();
        assert_eq!(keyboard.inline_keyboard.len(), 2);
        assert_eq!(keyboard.inline_keyboard[0].len(), 2);
        assert_eq!(keyboard.inline_keyboard[1].len(), 1);
    }

    #[test]
    fn help_keyboard_has_one_button_per_non_start_command() {
        let keyboard = help_keyboard();
        let button_count: usize = keyboard.inline_keyboard.iter().map(Vec::len).sum();
        // Все команды, кроме /start и /help.
        assert_eq!(button_count, 12);
    }
}
