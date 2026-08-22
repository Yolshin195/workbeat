//! Слэш-команды бота (teloxide `BotCommands`) — только объявление и разбор,
//! исполнение живёт в `executor`.

use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Debug, Clone, PartialEq, Eq)]
#[command(rename_rule = "snake_case", description = "Команды workbeat:")]
pub enum Command {
    #[command(description = "начать/перезапустить диалог")]
    Start,
    #[command(description = "начать рабочий день")]
    StartDay,
    #[command(description = "завершить рабочий день")]
    FinishDay,
    #[command(description = "отчёт за сегодня")]
    Report,
    #[command(description = "отчёт за текущую неделю")]
    ReportWeek,
    #[command(description = "отчёт за текущий месяц")]
    ReportMonth,
    #[command(description = "экспорт дневного отчёта в CSV")]
    ExportCsv,
    #[command(description = "создать новую задачу")]
    NewTask,
    #[command(description = "показать пул задач")]
    Tasks,
    #[command(description = "отредактировать задачу")]
    EditTask,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_commands() {
        assert_eq!(Command::parse("/start_day", "bot").unwrap(), Command::StartDay);
        assert_eq!(Command::parse("/finish_day", "bot").unwrap(), Command::FinishDay);
        assert_eq!(Command::parse("/report", "bot").unwrap(), Command::Report);
        assert_eq!(
            Command::parse("/report_week", "bot").unwrap(),
            Command::ReportWeek
        );
        assert_eq!(
            Command::parse("/report_month", "bot").unwrap(),
            Command::ReportMonth
        );
        assert_eq!(Command::parse("/new_task", "bot").unwrap(), Command::NewTask);
        assert_eq!(Command::parse("/tasks", "bot").unwrap(), Command::Tasks);
        assert_eq!(Command::parse("/edit_task", "bot").unwrap(), Command::EditTask);
    }

    #[test]
    fn rejects_unknown_commands() {
        assert!(Command::parse("/does_not_exist", "bot").is_err());
    }
}
