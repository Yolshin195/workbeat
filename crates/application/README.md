# application

Слой use cases и портов (traits). На этом этапе (Задача 3 корневого
`tasks.md`) реализованы только порты — интерфейсы, через которые use cases
(Задачи 5-9) будут общаться с внешним миром. Реализаций-адаптеров здесь нет:
`application` зависит только от `domain`, `async-trait` и `thiserror`.

## Порты

- `UserRepository`, `TaskRepository`, `WorkDayRepository`,
  `HourIntervalRepository`, `TenMinCheckRepository` — CRUD/query-порты для
  сущностей `domain`, все методы `async`, возвращают `Result<_, RepoError>`.
  `create` у сущностей с суррогатным id (`Task`/`WorkDay`/`HourInterval`/
  `TenMinCheck`) игнорирует `id` во входном значении и возвращает сущность с
  реальным id, присвоенным хранилищем.
- Отдельные query-методы под периодический poll-цикл (единственный механизм
  проактивных сообщений, см. корневой README.md п.4 — никакого
  `Scheduler`/`JobId` в системе нет):
  - `WorkDayRepository::find_open_without_active_interval()`;
  - `TenMinCheckRepository::find_open()`;
  - `TenMinCheckRepository::find_awaiting_resume()`.
- `Clock::now() -> UtcTimestamp` — единственный порт для работы со временем.
- `Notifier::send(user, OutboundMessage)` — исходящий порт для интерфейсов;
  `OutboundMessage` (`Text` / `Document`) не привязан к Telegram.

## Тестовые фейки

`src/testing.rs` (доступен под `#[cfg(test)]`) содержит ручные in-memory
реализации всех портов — `FakeClock`, `FakeNotifier`,
`InMemoryUserRepository`, `InMemoryTaskRepository`,
`InMemoryWorkDayRepository`, `InMemoryHourIntervalRepository`,
`InMemoryTenMinCheckRepository`. Ими будут пользоваться сценарные тесты
use cases в Задачах 5-9. Некоторые запросы, требующие состояния из другой
таблицы (`find_open_without_active_interval`, `find_awaiting_resume`), в
реальном SQLite-адаптере (Задача 4) станут JOIN'ом; здесь эквивалентное
состояние тест выставляет явно (`set_has_active_interval`,
`set_interval_closed`).

## Тесты

```sh
cargo test -p application
```
