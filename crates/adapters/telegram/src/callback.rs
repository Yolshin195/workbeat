//! Разбор `callback_data` инлайн-кнопок — чистые функции без зависимости от
//! `teloxide`/сети (критерий приёмки Задачи 11: "разбор апдейта" тестируется
//! без реального Telegram API). `to_data`/`parse` — строгая пара
//! кодирование/декодирование, покрытая табличными тестами ниже.

use domain::{TaskId, TaskPriority, TaskStatus};

use crate::commands::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackAction {
    TenMinYes,
    TenMinNo,
    ConfirmContinue,
    Returned,
    TaskReady,
    RequestStartInterval,
    StartInterval(TaskId),
    SwitchTask(TaskId),
    GoToLunch,
    Priority(Option<TaskPriority>),
    EditTask(TaskId),
    Status(TaskStatus),
    SkipField,
    Cancel,
    /// Кнопка меню `/help` — выполняет ту же команду, что и её текстовый
    /// аналог (проходит через тот же `interpret_command`, поэтому проверки
    /// вроде "день не начат" работают одинаково для кнопки и для `/команды`).
    RunCommand(Command),
}

impl CallbackAction {
    pub fn to_data(self) -> String {
        match self {
            CallbackAction::TenMinYes => "ten_yes".to_string(),
            CallbackAction::TenMinNo => "ten_no".to_string(),
            CallbackAction::ConfirmContinue => "confirm_continue".to_string(),
            CallbackAction::Returned => "returned".to_string(),
            CallbackAction::TaskReady => "task_ready".to_string(),
            CallbackAction::RequestStartInterval => "start_interval_pick".to_string(),
            CallbackAction::StartInterval(id) => format!("start_interval:{}", id.value()),
            CallbackAction::SwitchTask(id) => format!("switch_task:{}", id.value()),
            CallbackAction::GoToLunch => "go_to_lunch".to_string(),
            CallbackAction::Priority(priority) => {
                format!("priority:{}", priority_tag(priority))
            }
            CallbackAction::EditTask(id) => format!("edit_task:{}", id.value()),
            CallbackAction::Status(status) => format!("status:{}", status_tag(status)),
            CallbackAction::SkipField => "skip_field".to_string(),
            CallbackAction::Cancel => "cancel".to_string(),
            CallbackAction::RunCommand(command) => format!("run:{}", command_tag(command)),
        }
    }

    pub fn parse(data: &str) -> Option<CallbackAction> {
        if let Some((prefix, arg)) = data.split_once(':') {
            return match prefix {
                "start_interval" => Some(CallbackAction::StartInterval(TaskId::new(
                    arg.parse().ok()?,
                ))),
                "switch_task" => Some(CallbackAction::SwitchTask(TaskId::new(arg.parse().ok()?))),
                "edit_task" => Some(CallbackAction::EditTask(TaskId::new(arg.parse().ok()?))),
                "priority" => Some(CallbackAction::Priority(parse_priority_tag(arg)?)),
                "status" => Some(CallbackAction::Status(parse_status_tag(arg)?)),
                "run" => Some(CallbackAction::RunCommand(parse_command_tag(arg)?)),
                _ => None,
            };
        }

        match data {
            "ten_yes" => Some(CallbackAction::TenMinYes),
            "ten_no" => Some(CallbackAction::TenMinNo),
            "confirm_continue" => Some(CallbackAction::ConfirmContinue),
            "returned" => Some(CallbackAction::Returned),
            "task_ready" => Some(CallbackAction::TaskReady),
            "start_interval_pick" => Some(CallbackAction::RequestStartInterval),
            "go_to_lunch" => Some(CallbackAction::GoToLunch),
            "skip_field" => Some(CallbackAction::SkipField),
            "cancel" => Some(CallbackAction::Cancel),
            _ => None,
        }
    }
}

fn priority_tag(priority: Option<TaskPriority>) -> &'static str {
    match priority {
        Some(TaskPriority::High) => "high",
        Some(TaskPriority::Medium) => "medium",
        Some(TaskPriority::Low) => "low",
        None => "none",
    }
}

fn parse_priority_tag(tag: &str) -> Option<Option<TaskPriority>> {
    match tag {
        "high" => Some(Some(TaskPriority::High)),
        "medium" => Some(Some(TaskPriority::Medium)),
        "low" => Some(Some(TaskPriority::Low)),
        "none" => Some(None),
        _ => None,
    }
}

fn command_tag(command: Command) -> &'static str {
    match command {
        Command::Start => "start",
        Command::Help => "help",
        Command::StartDay => "start_day",
        Command::FinishDay => "finish_day",
        Command::Report => "report",
        Command::ReportWeek => "report_week",
        Command::ReportMonth => "report_month",
        Command::ExportCsv => "export_csv",
        Command::NewTask => "new_task",
        Command::Tasks => "tasks",
        Command::EditTask => "edit_task",
        Command::CurrentInterval => "current_interval",
        Command::StartInterval => "start_interval_cmd",
        Command::FinishInterval => "finish_interval",
    }
}

fn parse_command_tag(tag: &str) -> Option<Command> {
    match tag {
        "start" => Some(Command::Start),
        "help" => Some(Command::Help),
        "start_day" => Some(Command::StartDay),
        "finish_day" => Some(Command::FinishDay),
        "report" => Some(Command::Report),
        "report_week" => Some(Command::ReportWeek),
        "report_month" => Some(Command::ReportMonth),
        "export_csv" => Some(Command::ExportCsv),
        "new_task" => Some(Command::NewTask),
        "tasks" => Some(Command::Tasks),
        "edit_task" => Some(Command::EditTask),
        "current_interval" => Some(Command::CurrentInterval),
        "start_interval_cmd" => Some(Command::StartInterval),
        "finish_interval" => Some(Command::FinishInterval),
        _ => None,
    }
}

fn status_tag(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Ready => "ready",
        TaskStatus::NotReady => "not_ready",
        TaskStatus::Done => "done",
        TaskStatus::InProgress => "in_progress",
    }
}

fn parse_status_tag(tag: &str) -> Option<TaskStatus> {
    match tag {
        "ready" => Some(TaskStatus::Ready),
        "not_ready" => Some(TaskStatus::NotReady),
        "done" => Some(TaskStatus::Done),
        "in_progress" => Some(TaskStatus::InProgress),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cases() -> Vec<CallbackAction> {
        vec![
            CallbackAction::TenMinYes,
            CallbackAction::TenMinNo,
            CallbackAction::ConfirmContinue,
            CallbackAction::Returned,
            CallbackAction::TaskReady,
            CallbackAction::RequestStartInterval,
            CallbackAction::StartInterval(TaskId::new(7)),
            CallbackAction::SwitchTask(TaskId::new(9)),
            CallbackAction::GoToLunch,
            CallbackAction::Priority(None),
            CallbackAction::Priority(Some(TaskPriority::High)),
            CallbackAction::Priority(Some(TaskPriority::Medium)),
            CallbackAction::Priority(Some(TaskPriority::Low)),
            CallbackAction::EditTask(TaskId::new(3)),
            CallbackAction::Status(TaskStatus::Ready),
            CallbackAction::Status(TaskStatus::NotReady),
            CallbackAction::Status(TaskStatus::Done),
            CallbackAction::SkipField,
            CallbackAction::Cancel,
            CallbackAction::RunCommand(Command::Start),
            CallbackAction::RunCommand(Command::Help),
            CallbackAction::RunCommand(Command::StartDay),
            CallbackAction::RunCommand(Command::FinishDay),
            CallbackAction::RunCommand(Command::Report),
            CallbackAction::RunCommand(Command::ReportWeek),
            CallbackAction::RunCommand(Command::ReportMonth),
            CallbackAction::RunCommand(Command::ExportCsv),
            CallbackAction::RunCommand(Command::NewTask),
            CallbackAction::RunCommand(Command::Tasks),
            CallbackAction::RunCommand(Command::EditTask),
            CallbackAction::RunCommand(Command::CurrentInterval),
            CallbackAction::RunCommand(Command::StartInterval),
            CallbackAction::RunCommand(Command::FinishInterval),
        ]
    }

    #[test]
    fn round_trips_every_action_through_its_encoded_data() {
        for action in cases() {
            let data = action.to_data();
            assert_eq!(CallbackAction::parse(&data), Some(action));
        }
    }

    #[test]
    fn encoded_data_stays_within_telegram_callback_data_limit() {
        // Telegram ограничивает callback_data 64 байтами.
        for action in cases() {
            assert!(action.to_data().len() <= 64);
        }
    }

    #[test]
    fn rejects_unknown_or_malformed_data() {
        assert_eq!(CallbackAction::parse("garbage"), None);
        assert_eq!(CallbackAction::parse("start_interval:not-a-number"), None);
        assert_eq!(CallbackAction::parse("priority:extreme"), None);
        assert_eq!(CallbackAction::parse(""), None);
    }
}
