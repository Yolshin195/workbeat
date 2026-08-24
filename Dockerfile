# syntax=docker/dockerfile:1

# workbeat — Telegram-бот на tokio/teloxide с локальным хранилищем SQLite.
# Процесс не слушает HTTP-порт (long polling), поэтому EXPOSE не нужен.
#
# Сборка использует cargo-chef, чтобы зависимости воркспейса собирались в
# отдельном, кешируемом слое и не пересобирались при каждом изменении
# исходников приложения. Рантайм-образ — distroless (без shell и пакетного
# менеджера), запускается от непривилегированного пользователя.

################################################################################
# chef: базовый образ с Rust и cargo-chef. Полная (не -slim) версия образа
# rust собрана на buildpack-deps и уже содержит toolchain для сборки C
# (gcc/make) — он нужен, потому что sqlx по умолчанию собирает SQLite из
# исходников через крейт `cc` (bundled-режим), а не линкуется с системной
# libsqlite3.
################################################################################
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

################################################################################
# planner: строит "рецепт" — граф зависимостей всех крейтов воркспейса —
# по манифестам, без компиляции кода приложения.
################################################################################
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

################################################################################
# builder: сначала собирает только зависимости по рецепту (этот слой
# инвалидируется только при изменении Cargo.toml/Cargo.lock, а не исходников),
# затем — сам бинарь workbeat.
################################################################################
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --locked --bin workbeat \
    && strip target/release/workbeat

# Пустая директория для файла SQLite — создаётся здесь (в образе есть shell),
# чтобы в дистролесс-рантайме её можно было скопировать сразу с нужным
# владельцем через COPY --chown (в дистролессе нет ни shell, ни mkdir).
RUN mkdir -p /app/data

################################################################################
# runtime: distroless-образ на той же версии glibc (debian12/bookworm), что и
# сборочный — без shell, пакетного менеджера и лишних библиотек. TLS-стек
# приложения (rustls) использует встроенные корневые сертификаты Mozilla, а
# не системные, поэтому пакет ca-certificates не требуется.
################################################################################
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime

ENV DATABASE_URL=sqlite:///data/workbeat.db

COPY --from=builder --chown=nonroot:nonroot /app/data /data
COPY --from=builder /app/target/release/workbeat /usr/local/bin/workbeat

WORKDIR /data
USER nonroot:nonroot
VOLUME ["/data"]

ENTRYPOINT ["/usr/local/bin/workbeat"]
