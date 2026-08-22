//! Интеграционный smoke-тест `TeloxideNotifier` (Задача 12): проверяет оба
//! варианта `OutboundMessage` через mock-сервер Telegram Bot API (`wiremock`)
//! — без реального токена/сети, `Bot::set_api_url` перенаправляет запросы на
//! локальный сервер.

use adapters_telegram::TeloxideNotifier;
use application::{Notifier, OutboundMessage};
use domain::TelegramId;
use teloxide::Bot;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_TOKEN: &str = "TEST_TOKEN";

/// Минимальный валидный JSON `Message` — единственные обязательные поля,
/// нужные `teloxide` для успешной десериализации ответа Bot API.
const MESSAGE_RESPONSE: &str =
    r#"{"ok":true,"result":{"message_id":1,"chat":{"id":42,"type":"private"},"date":0}}"#;

async fn bot_pointing_to(server: &MockServer) -> Bot {
    let api_url = reqwest::Url::parse(&format!("{}/", server.uri())).expect("valid mock url");
    Bot::new(TEST_TOKEN).set_api_url(api_url)
}

#[tokio::test]
async fn sends_text_message_via_send_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{TEST_TOKEN}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(MESSAGE_RESPONSE, "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let notifier = TeloxideNotifier::new(bot_pointing_to(&server).await);
    let user = TelegramId::new(42).unwrap();

    notifier
        .send(user, OutboundMessage::Text("Ты работаешь?".to_string()))
        .await
        .expect("text message should be delivered");
}

#[tokio::test]
async fn sends_document_via_send_document() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{TEST_TOKEN}/SendDocument")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(MESSAGE_RESPONSE, "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let notifier = TeloxideNotifier::new(bot_pointing_to(&server).await);
    let user = TelegramId::new(42).unwrap();

    notifier
        .send(
            user,
            OutboundMessage::Document {
                filename: "report.csv".to_string(),
                bytes: b"task,minutes\nDemo,50\n".to_vec(),
            },
        )
        .await
        .expect("document should be delivered");
}

#[tokio::test]
async fn propagates_notifier_error_on_api_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{TEST_TOKEN}/SendMessage")))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"ok":false,"description":"Forbidden: bot was blocked by the user"}"#,
            "application/json",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let notifier = TeloxideNotifier::new(bot_pointing_to(&server).await);
    let user = TelegramId::new(42).unwrap();

    let result = notifier
        .send(user, OutboundMessage::Text("Ты работаешь?".to_string()))
        .await;

    assert!(matches!(result, Err(application::NotifierError::DeliveryFailed(_))));
}
