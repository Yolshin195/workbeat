//! DTO отчётов (Задача 9 корневого `tasks.md`) — общий для `/report`
//! (`BuildDayReport`) и `/report_week`/`/report_month` (`BuildPeriodReport`).
//! Не привязан к конкретному формату вывода интерфейса: Telegram-адаптер
//! (Задача 11) просто отправляет `to_text()`, а `ExportDayReportCsv`
//! сериализует те же данные в CSV.

use chrono::{Duration, NaiveDate};
use domain::{CheckStatus, TaskId, UtcTimestamp};

/// Период агрегированного отчёта (`BuildPeriodReport`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Week,
    Month,
}

/// Суммарное время одной задачи (README.md раздел 3): сумма успешных
/// десятиминуток + отдых, если он положен, по всем интервалам отчёта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTimeEntry {
    pub task_id: TaskId,
    pub task_title: String,
    pub duration: Duration,
}

/// Одна провальная (`Failed`/`NoResponse`) десятиминутка с причиной —
/// только для дневного отчёта (README.md раздел 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedCheckEntry {
    pub task_id: TaskId,
    pub task_title: String,
    pub status: CheckStatus,
    pub reason: Option<String>,
    pub started_at: UtcTimestamp,
}

/// Норма дня — присутствует только у дневного отчёта (`ReportDto::day`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayReportInfo {
    pub closed_intervals: u8,
    pub norm_met: bool,
}

/// Сведения о периоде — присутствуют только у агрегированного отчёта
/// (`ReportDto::period`), см. `BuildPeriodReport`. Границы периода уже
/// вычислены по календарной дате в таймзоне пользователя.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeriodReportInfo {
    pub period: Period,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days_total: u32,
    pub days_with_norm_met: u32,
}

/// Общий DTO для `/report` (день) и `/report_week`/`/report_month`
/// (период) — README.md раздел 5. `day`/`period` взаимоисключающие: ровно
/// одно из двух всегда `Some` (день — для `BuildDayReport`, период — для
/// `BuildPeriodReport`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDto {
    pub task_totals: Vec<TaskTimeEntry>,
    pub failed_checks: Vec<FailedCheckEntry>,
    pub lunch_duration: Duration,
    pub interval_summaries: Vec<Option<String>>,
    pub day: Option<DayReportInfo>,
    pub period: Option<PeriodReportInfo>,
}

impl ReportDto {
    /// Текстовое представление отчёта — общий рендер для дневного и
    /// периодического отчёта. Формат стабилен и покрыт golden-тестами.
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();

        if let Some(day) = &self.day {
            lines.push(format!(
                "Отчёт за день: {}/8 интервалов{}",
                day.closed_intervals,
                if day.norm_met {
                    ", норма выполнена"
                } else {
                    ""
                }
            ));
        }
        if let Some(period) = &self.period {
            lines.push(format!(
                "Отчёт за {} ({} — {}): {} из {} дней с выполненной нормой",
                period_label(period.period),
                period.start_date,
                period.end_date,
                period.days_with_norm_met,
                period.days_total,
            ));
        }

        lines.push("Время по задачам:".to_string());
        if self.task_totals.is_empty() {
            lines.push("  (нет)".to_string());
        } else {
            for entry in &self.task_totals {
                lines.push(format!(
                    "  {}: {}",
                    entry.task_title,
                    format_duration(entry.duration)
                ));
            }
        }

        if !self.failed_checks.is_empty() {
            lines.push("Провальные десятиминутки:".to_string());
            for check in &self.failed_checks {
                lines.push(format!(
                    "  {} [{}] {}: {}",
                    format_timestamp(check.started_at),
                    check.task_title,
                    status_label(check.status),
                    check.reason.as_deref().unwrap_or("(без причины)")
                ));
            }
        }

        lines.push(format!("Обед: {}", format_duration(self.lunch_duration)));

        if !self.interval_summaries.is_empty() {
            lines.push("Итоги часов:".to_string());
            for (index, summary) in self.interval_summaries.iter().enumerate() {
                lines.push(format!(
                    "  {}. {}",
                    index + 1,
                    summary.as_deref().unwrap_or("(без ответа)")
                ));
            }
        }

        lines.join("\n")
    }
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let minutes = duration.num_minutes();
    format!("{}ч {:02}м", minutes / 60, minutes % 60)
}

pub(crate) fn format_timestamp(ts: UtcTimestamp) -> String {
    ts.value().format("%Y-%m-%d %H:%M").to_string()
}

pub(crate) fn status_label(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Worked => "Успешно",
        CheckStatus::Failed => "Провалено",
        CheckStatus::NoResponse => "Нет ответа",
    }
}

fn period_label(period: Period) -> &'static str {
    match period {
        Period::Week => "неделю",
        Period::Month => "месяц",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::TaskId;

    #[test]
    fn to_text_renders_day_report_without_failures() {
        let dto = ReportDto {
            task_totals: vec![TaskTimeEntry {
                task_id: TaskId::new(1),
                task_title: "Write report".to_string(),
                duration: Duration::minutes(60),
            }],
            failed_checks: vec![],
            lunch_duration: Duration::zero(),
            interval_summaries: vec![Some("wrote the report".to_string())],
            day: Some(DayReportInfo {
                closed_intervals: 1,
                norm_met: false,
            }),
            period: None,
        };

        let text = dto.to_text();

        assert_eq!(
            text,
            "Отчёт за день: 1/8 интервалов\n\
             Время по задачам:\n\
             \x20\x20Write report: 1ч 00м\n\
             Обед: 0ч 00м\n\
             Итоги часов:\n\
             \x20\x201. wrote the report"
        );
    }
}
