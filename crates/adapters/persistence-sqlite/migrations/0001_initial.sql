-- Схема БД workbeat (см. README.md, раздел 6). `users` не имеет отдельного
-- суррогатного id — естественный ключ это `telegram_id`. `hour_intervals`
-- намеренно не хранит `task_id`: один часовой интервал может включать
-- десятиминутки разных задач (правило "задача готова"), поэтому привязка к
-- задаче живёт на `ten_min_checks`.

CREATE TABLE users (
    telegram_id         INTEGER PRIMARY KEY,
    timezone            TEXT NOT NULL DEFAULT '0',
    created_at          DATETIME NOT NULL
);

CREATE TABLE tasks (
    id                  INTEGER PRIMARY KEY,
    user_id             INTEGER NOT NULL REFERENCES users(telegram_id),
    title               TEXT NOT NULL,
    status              TEXT NOT NULL,
    priority            TEXT,
    deadline            DATE,
    created_at          DATETIME NOT NULL
);

CREATE INDEX idx_tasks_user_id ON tasks(user_id);

CREATE TABLE work_days (
    id                      INTEGER PRIMARY KEY,
    user_id                 INTEGER NOT NULL REFERENCES users(telegram_id),
    started_at              DATETIME NOT NULL,
    finished_at             DATETIME,
    lunch_started_at        DATETIME,
    lunch_ended_at          DATETIME,
    last_idle_prompt_at     DATETIME
);

CREATE INDEX idx_work_days_user_id ON work_days(user_id);
-- Кандидаты на StartWorkDay/find_open_by_user: открытый день пользователя.
CREATE INDEX idx_work_days_open ON work_days(user_id) WHERE finished_at IS NULL;

CREATE TABLE hour_intervals (
    id                  INTEGER PRIMARY KEY,
    work_day_id         INTEGER NOT NULL REFERENCES work_days(id),
    started_at          DATETIME NOT NULL,
    ended_at            DATETIME,
    successful_count    INTEGER NOT NULL DEFAULT 0 CHECK (successful_count BETWEEN 0 AND 5),
    summary             TEXT
);

CREATE INDEX idx_hour_intervals_work_day_id ON hour_intervals(work_day_id);
-- Кандидаты на StartHourInterval/find_open_by_work_day: незакрытый интервал дня.
CREATE INDEX idx_hour_intervals_open ON hour_intervals(work_day_id) WHERE ended_at IS NULL;

CREATE TABLE ten_min_checks (
    id                  INTEGER PRIMARY KEY,
    hour_interval_id    INTEGER NOT NULL REFERENCES hour_intervals(id),
    task_id             INTEGER NOT NULL REFERENCES tasks(id),
    started_at          DATETIME NOT NULL,
    ended_at            DATETIME,
    status              TEXT,
    reason              TEXT,
    last_reminder_at    DATETIME
);

CREATE INDEX idx_ten_min_checks_hour_interval_id ON ten_min_checks(hour_interval_id);
CREATE INDEX idx_ten_min_checks_task_id ON ten_min_checks(task_id);
-- Кандидаты на AdvanceOpenTenMinChecks (Задача 7/10): все открытые десятиминутки.
CREATE INDEX idx_ten_min_checks_open ON ten_min_checks(hour_interval_id) WHERE ended_at IS NULL;
-- Кандидаты на RemindAwaitingResume/find_awaiting_resume (Задача 3/7): закрытые
-- провальные десятиминутки, дешёво находим последнюю закрытую по интервалу.
CREATE INDEX idx_ten_min_checks_awaiting_resume
    ON ten_min_checks(hour_interval_id, started_at)
    WHERE ended_at IS NOT NULL AND status IN ('failed', 'no_response');
