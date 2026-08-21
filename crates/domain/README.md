# domain

Доменный слой workbeat: сущности и value objects из корневого `README.md`
раздела 6 (схема БД), выраженные как rust-типы. Ноль внешних зависимостей,
кроме `chrono` (только типы даты/времени) и `thiserror` (типизированные
ошибки валидации).

Правило: `domain` не знает про SQLite/Telegram/tokio. Value objects и сущности
содержат только конструкторы и геттеры; переходы состояний живут отдельно —
как чистые функции без ввода-вывода (см. раздел "Бизнес-правила" ниже),
которыми пользуется `application` (следующая задача плана).

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
| `WorkDay`      | `work_days`            | `id`, `user_id`, `started_at`, `finished_at`, `lunch_started_at`, `lunch_ended_at`, `last_idle_prompt_at: Option<UtcTimestamp>` |
| `HourInterval` | `hour_intervals`       | `id`, `work_day_id`, `started_at`, `ended_at`, `successful_count: SuccessfulCount`, `summary: Option<String>` |
| `TenMinCheck`  | `ten_min_checks`       | `id`, `hour_interval_id`, `task_id`, `started_at`, `ended_at`, `status: CheckStatus`, `reason: Option<String>`, `last_reminder_at: Option<UtcTimestamp>` |

Важно: `task_id` живёт на `TenMinCheck`, а не на `HourInterval` — один часовой
интервал может включать десятиминутки разных задач (правило "задача готова",
корневой `README.md` раздел 3). Признака "восстановительная десятиминутка"
(`is_recovery`) в системе нет: молчание закрывает рабочую десятиминутку сразу,
без второй сущности — `reason` может быть дописан в уже закрытую строку
(единственный случай изменения закрытой записи).

Перечисления:

- `TaskStatus`: `Ready | NotReady | InProgress | Done`.
- `TaskPriority`: `High | Medium | Low` (опционален на `Task`).
- `CheckStatus`: `Worked | Failed | NoResponse`.

## Бизнес-правила

Чистые функции `(текущее состояние, событие) -> новое состояние`, без
ввода-вывода и без async — реализуют переходы состояний из корневого
`README.md` раздела 2. Ни одна из них не завязана на порты (`application`
появится в следующей задаче плана) и не хранит собственного состояния между
вызовами — вся нужная информация передаётся аргументами.

- `WorkSlotPollDecision::decide(elapsed_since_start, already_prompted)` —
  опрос открытой **рабочей** десятиминутки: `NoActionYet` / `AskAreYouWorking`
  (один раз через 10 минут) / `ResolveNoResponse` (немедленное закрытие, если
  на следующем тике после вопроса ответа так и не было). Управляет только
  рабочим слотом — без каскада на второй "восстановительный" слот.
- `ReminderScheduleDecision::decide(elapsed_since_last_reminder,
  elapsed_since_wait_started, burst_window, burst_interval, steady_interval)`
  — расписание напоминаний (раз в `burst_interval` первые `burst_window`,
  дальше раз в `steady_interval`), общее для переработанного отдыха и
  ожидания возврата после провала рабочей десятиминутки. Пороги приходят
  параметром — не хардкодятся.
- `IdlePromptDecision::decide(elapsed_since_last_prompt, idle_threshold)` —
  та же логика "elapsed ≥ порог" для `PromptContinueIfIdle` (простой между
  часовыми интервалами), с порогом в часах, а не минутах.
- `TenMinCheckState` — машина состояний одной десятиминутки (`Pending` →
  `Closed(Worked | Failed | NoResponse)`), `submit_answer`/
  `resolve_no_response` отклоняют повторное закрытие уже закрытой строки.
  `append_reason_to_no_response(status, existing_reason)` — единственный
  легальный случай изменения закрытой строки: дозапись `reason` в уже
  закрытую `NoResponse`-десятиминутку (ошибка для любого другого статуса или
  если `reason` уже был проставлен).
- `HourIntervalState` — по срезу статусов уже закрытых рабочих попыток
  интервала считает `successful_count` (только `Worked`), решает, закрыт ли
  рабочий этап (`successful_count == 5`, независимо от числа провалов) и
  положен ли отдых (`is_rest_owed`) — а также позиционный факт "следующая
  создаваемая десятиминутка — это слот отдыха" (`is_next_slot_rest`), который
  не хранится в БД.
- `time_calculations`: `task_time(worked_count, rest_earned)` — время задачи
  как сумма номинальных 10-минутных успешных десятиминуток + 10 минут отдыха,
  если он положен (README.md раздел 3); `daily_norm_progress(closed_intervals)`
  — сколько из 8 интервалов закрыто; `interval_overrun(actual_duration)` и
  `day_overrun(interval_overruns)` — перерасход одного интервала и по дню
  (факт минус номинал, README.md раздел 5).

## Тесты

```sh
cargo test -p domain
```

Тесты — unit- и табличные (`rstest`-стиль вручную) тесты на конструкторы и
бизнес-правила, plus один property-based тест (`proptest`) на инвариант "сумма
номинального времени успешных десятиминуток в интервале ≤ 60 минут". Ни один
тест не требует сети/файловой системы/async — все длительности передаются
как `chrono::Duration`, без реального ожидания.

Конструкторы: корректные значения принимаются, невалидные (`TelegramId <= 0`,
`TimeZoneOffset` вне `-12:00..=+14:00`, `SuccessfulCount > 5`, пустой
`Task::title`) отклоняются через `Result<_, DomainError>`.
