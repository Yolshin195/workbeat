# domain

Доменный слой workbeat: сущности и value objects из корневого `README.md`
раздела 6 (схема БД), выраженные как rust-типы. Ноль внешних зависимостей,
кроме `chrono` (только типы даты/времени) и `thiserror` (типизированные
ошибки валидации).

Правило: `domain` не знает про SQLite/Telegram/tokio. Этот крейт содержит
только конструкторы и геттеры — никакой бизнес-логики переходов состояний
(она появится в крейте `application`/бизнес-правилах следующей задачи).

## Value objects

- `TelegramId` — идентификатор пользователя в Telegram (`i64 > 0`).
- `UtcTimestamp` — момент времени в UTC, обёртка над `chrono::DateTime<Utc>`.
- `TimeZoneOffset` — смещение часового пояса пользователя в минутах
  (`-12:00..=+14:00`), используется только для отображения в отчётах.
- `TaskId`, `WorkDayId`, `HourIntervalId`, `TenMinCheckId` — типобезопасные
  идентификаторы сущностей (обёртки над `i64`, соответствуют
  `INTEGER PRIMARY KEY` в SQLite).
- `SuccessfulCount` — число успешных десятиминуток в часовом интервале,
  инвариант `0..=5` гарантирован конструктором (`SuccessfulCount::new`
  возвращает `Result`, значения `> 5` невозможно построить).

## Сущности

| Сущность       | Соответствует таблице | Поля |
|----------------|------------------------|------|
| `User`         | `users`                | `telegram_id`, `timezone`, `created_at` |
| `Task`         | `tasks`                | `id`, `user_id`, `title`, `status: TaskStatus`, `priority: Option<TaskPriority>`, `deadline: Option<NaiveDate>`, `created_at` |
| `WorkDay`      | `work_days`            | `id`, `user_id`, `started_at`, `finished_at`, `lunch_started_at`, `lunch_ended_at` |
| `HourInterval` | `hour_intervals`       | `id`, `work_day_id`, `started_at`, `ended_at`, `successful_count: SuccessfulCount`, `summary: Option<String>` |
| `TenMinCheck`  | `ten_min_checks`       | `id`, `hour_interval_id`, `task_id`, `started_at`, `ended_at`, `status: CheckStatus`, `reason: Option<String>` |

`HourInterval` намеренно не хранит `task_id` — один часовой интервал может
включать десятиминутки разных задач (правило "задача готова"), поэтому
привязка к задаче живёт на уровне `TenMinCheck`. `HourInterval.summary` —
ответ на обязательный вопрос "что делали за этот час".

Перечисления:

- `TaskStatus`: `Ready | NotReady | InProgress | Done`.
- `TaskPriority`: `High | Medium | Low` (опционален на `Task`).
- `CheckStatus`: `Worked | Failed | NoResponse`.

## Тесты

```sh
cargo test -p domain
```

Тесты — чистые unit-тесты на конструкторы (без сети/файловой системы):
корректные значения принимаются, невалидные (`TelegramId <= 0`,
`TimeZoneOffset` вне `-12:00..=+14:00`, `SuccessfulCount > 5`, пустой
`Task::title`) отклоняются через `Result<_, DomainError>`.
