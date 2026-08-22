use std::sync::Arc;

use domain::WorkDayId;
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{HourIntervalRepository, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::report_dto::{format_timestamp, status_label, ReportDto};
use crate::use_cases::build_day_report::{build_day_report_dto, BuildDayReportError};

impl From<BuildDayReportError> for ExportDayReportCsvError {
    fn from(error: BuildDayReportError) -> Self {
        match error {
            BuildDayReportError::Repo(repo_error) => Self::Repo(repo_error),
        }
    }
}

/// Ошибка `ExportDayReportCsv`.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExportDayReportCsvError {
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Экспортирует дневной отчёт в CSV (README.md раздел 5) — тот же `ReportDto`,
/// что и `BuildDayReport`, сериализованный в CSV вместо текста. Возвращённые
/// байты отдаются наружу через `Notifier` как `OutboundMessage::Document`
/// (Задача 3/12), не текстом.
pub struct ExportDayReportCsv {
    work_day_repository: Arc<dyn WorkDayRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    task_repository: Arc<dyn TaskRepository>,
}

impl ExportDayReportCsv {
    pub fn new(
        work_day_repository: Arc<dyn WorkDayRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        task_repository: Arc<dyn TaskRepository>,
    ) -> Self {
        Self {
            work_day_repository,
            hour_interval_repository,
            ten_min_check_repository,
            task_repository,
        }
    }

    pub async fn execute(&self, work_day_id: WorkDayId) -> Result<Vec<u8>, ExportDayReportCsvError> {
        let report = build_day_report_dto(
            &self.work_day_repository,
            &self.hour_interval_repository,
            &self.ten_min_check_repository,
            &self.task_repository,
            work_day_id,
        )
        .await?;
        Ok(render_csv(&report))
    }
}

/// Экранирование поля по RFC4180: значения с запятой/кавычкой/переводом
/// строки оборачиваются в кавычки, внутренние кавычки удваиваются.
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_row(fields: &[String]) -> String {
    let mut row = fields
        .iter()
        .map(|field| csv_field(field))
        .collect::<Vec<_>>()
        .join(",");
    row.push_str("\r\n");
    row
}

/// Строит CSV из `ReportDto`. Один общий набор колонок для всех разделов
/// (`Раздел` различает строки) — так отчёт остаётся одной плоской таблицей,
/// которую Excel/LibreOffice открывают без специальной обработки. UTF-8 BOM
/// в начале нужен, чтобы Excel на Windows не перепутал кодировку кириллицы.
fn render_csv(report: &ReportDto) -> Vec<u8> {
    let mut out = String::new();
    out.push('\u{feff}');
    out.push_str(&csv_row(&[
        "Раздел".to_string(),
        "Задача".to_string(),
        "Начало".to_string(),
        "Статус".to_string(),
        "Причина".to_string(),
        "Минуты".to_string(),
    ]));

    for entry in &report.task_totals {
        out.push_str(&csv_row(&[
            "Задача".to_string(),
            entry.task_title.clone(),
            String::new(),
            String::new(),
            String::new(),
            entry.duration.num_minutes().to_string(),
        ]));
    }

    for check in &report.failed_checks {
        out.push_str(&csv_row(&[
            "Провал".to_string(),
            check.task_title.clone(),
            format_timestamp(check.started_at),
            status_label(check.status).to_string(),
            check.reason.clone().unwrap_or_default(),
            String::new(),
        ]));
    }

    out.push_str(&csv_row(&[
        "Обед".to_string(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        report.lunch_duration.num_minutes().to_string(),
    ]));

    for (index, summary) in report.interval_summaries.iter().enumerate() {
        out.push_str(&csv_row(&[
            "Интервал".to_string(),
            (index + 1).to_string(),
            String::new(),
            String::new(),
            summary.clone().unwrap_or_default(),
            String::new(),
        ]));
    }

    if let Some(day) = &report.day {
        out.push_str(&csv_row(&[
            "Норма".to_string(),
            String::new(),
            String::new(),
            if day.norm_met {
                "выполнена".to_string()
            } else {
                "не выполнена".to_string()
            },
            String::new(),
            day.closed_intervals.to_string(),
        ]));
    }

    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        InMemoryHourIntervalRepository, InMemoryTaskRepository, InMemoryTenMinCheckRepository,
        InMemoryWorkDayRepository,
    };
    use chrono::{Duration, Utc};
    use domain::{
        CheckStatus, HourInterval, HourIntervalId, SuccessfulCount, Task, TaskId, TaskStatus,
        TelegramId, TenMinCheck, TenMinCheckId, UtcTimestamp, WorkDay, WorkDayId,
    };

    #[test]
    fn exports_golden_csv_for_a_simple_day() {
        pollster::block_on(async {
            let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
            let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
            let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
            let task_repository = Arc::new(InMemoryTaskRepository::new());
            let user_id = TelegramId::new(1).unwrap();
            let started_at = UtcTimestamp::new(Utc::now());

            let work_day = WorkDay::new(WorkDayId::new(0), user_id, started_at, None, None, None, None);
            let work_day = work_day_repository.create(work_day).await.unwrap();

            let task = Task::new(
                TaskId::new(0),
                user_id,
                "Write report",
                TaskStatus::InProgress,
                None,
                None,
                started_at,
            )
            .unwrap();
            let task = task_repository.create(task).await.unwrap();

            let interval = HourInterval::new(
                HourIntervalId::new(0),
                work_day.id(),
                started_at,
                Some(UtcTimestamp::new(started_at.value() + Duration::minutes(60))),
                SuccessfulCount::new(5).unwrap(),
                Some("done".to_string()),
            );
            let interval = hour_interval_repository.create(interval).await.unwrap();

            let mut t = started_at;
            for _ in 0..5 {
                let check = TenMinCheck::new(
                    TenMinCheckId::new(0),
                    interval.id(),
                    task.id(),
                    t,
                    Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
                    CheckStatus::Worked,
                    None,
                    None,
                );
                ten_min_check_repository.create(check).await.unwrap();
                t = UtcTimestamp::new(t.value() + Duration::minutes(10));
            }
            let rest = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task.id(),
                t,
                Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
                CheckStatus::Worked,
                None,
                None,
            );
            ten_min_check_repository.create(rest).await.unwrap();

            let use_case = ExportDayReportCsv::new(
                work_day_repository,
                hour_interval_repository,
                ten_min_check_repository,
                task_repository,
            );
            let csv = use_case.execute(work_day.id()).await.unwrap();
            let csv_text = String::from_utf8(csv).unwrap();

            assert_eq!(
                csv_text,
                "\u{feff}Раздел,Задача,Начало,Статус,Причина,Минуты\r\n\
                 Задача,Write report,,,,60\r\n\
                 Обед,,,,,0\r\n\
                 Интервал,1,,,done,\r\n\
                 Норма,,,не выполнена,,1\r\n"
            );
        });
    }

    #[test]
    fn csv_field_quotes_values_containing_commas_and_quotes() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
