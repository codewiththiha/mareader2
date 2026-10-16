//! OpenAI-compatible servers, through `ai-pool`: cloud, local or custom.

use std::sync::Arc;

use ai_pool::{AiClient, AiError, ResponseFormat, StreamInterruption, VaultError};
use futures_util::StreamExt;
use mareader_study::{Failure, Why};
use serde_json::Value;

use crate::answer;
use crate::backend::{Backend, BoxFuture, Partial};
use crate::prompt::Prompt;

/// How strictly the answer's shape is asked for; each step is a fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// `json_schema`: the server enforces the schema.
    Schema,
    /// `json_object`: any object; the schema rides in the prompt.
    Object,
    /// No format at all, for servers that know neither.
    Plain,
}

impl Shape {
    fn weaker(self) -> Option<Self> {
        match self {
            Self::Schema => Some(Self::Object),
            Self::Object => Some(Self::Plain),
            Self::Plain => None,
        }
    }
}

/// One server and model; the client brings keys, retries and rotation.
#[derive(Clone)]
pub struct Compat {
    client: Arc<AiClient>,
    /// Blank asks the server for its first model.
    model: String,
}

impl Compat {
    pub fn new(client: Arc<AiClient>, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into().trim().to_owned(),
        }
    }

    async fn model(&self) -> Result<String, Failure> {
        if !self.model.is_empty() {
            return Ok(self.model.clone());
        }
        let models = self.client.models().await.map_err(|e| failure(&e))?;
        let first = models.into_iter().next().map(|m| m.id);
        first.ok_or_else(|| Failure::detailed(Why::Setup, "the server lists no models"))
    }

    async fn answer(&self, prompt: &Prompt, partial: Partial<'_>) -> Result<Value, Failure> {
        let model = self.model().await?;
        let mut shape = Shape::Schema;
        loop {
            match self.stream(&model, prompt, shape, partial).await {
                Err(e) if shape_refused(&e) => match shape.weaker() {
                    Some(weaker) => shape = weaker,
                    None => return Err(failure(&e)),
                },
                Err(e) => return Err(failure_with_keys(&self.client, e).await),
                Ok(text) => {
                    return answer::whole(&text)
                        .ok_or_else(|| Failure::detailed(Why::BadAnswer, clip(&text)));
                }
            }
        }
    }

    async fn stream(
        &self,
        model: &str,
        prompt: &Prompt,
        shape: Shape,
        partial: Partial<'_>,
    ) -> Result<String, AiError> {
        let system = match shape {
            Shape::Schema => prompt.system.clone(),
            Shape::Object | Shape::Plain => prompt.system_with_schema(),
        };
        let mut chat = self
            .client
            .chat_builder()
            .model(model)
            .system(system)
            .user(prompt.user.as_str());
        chat = match shape {
            Shape::Schema => chat.response_format(ResponseFormat::json_schema(
                prompt.name,
                prompt.schema.clone(),
            )),
            Shape::Object => chat.response_format(ResponseFormat::Json),
            Shape::Plain => chat,
        };
        let mut chunks = chat.send_stream().await?;
        let mut text = String::new();
        let mut shown: Option<Value> = None;
        while let Some(chunk) = chunks.next().await {
            for choice in chunk?.choices {
                text.push_str(choice.delta.content.as_deref().unwrap_or_default());
                match choice.finish_reason.as_deref() {
                    Some("length") => return Err(AiError::Truncated),
                    Some("content_filter") => return Err(AiError::Refused(clip(&text))),
                    _ => {}
                }
            }
            let now = answer::partial(&text);
            if now.is_some() && now != shown {
                if let Some(v) = &now {
                    partial(v.clone());
                }
                shown = now;
            }
        }
        Ok(text)
    }
}

impl Backend for Compat {
    fn ask<'a>(
        &'a self,
        prompt: &'a Prompt,
        partial: Partial<'a>,
    ) -> BoxFuture<'a, Result<Value, Failure>> {
        Box::pin(self.answer(prompt, partial))
    }
}

/// Like [`failure`], telling no keys from keys all turned away.
pub async fn failure_with_keys(client: &AiClient, e: AiError) -> Failure {
    if let AiError::AllKeysExhausted { retry_in_ms: None } = e {
        return match client.manager().list_keys().await {
            Ok(keys) if keys.is_empty() => Why::NoKey.into(),
            _ => Why::KeysRejected.into(),
        };
    }
    failure(&e)
}

/// Whether the server refused the answer format rather than the request.
fn shape_refused(e: &AiError) -> bool {
    let AiError::BadRequest { status, body } = e else {
        return false;
    };
    let body = body.to_ascii_lowercase();
    matches!(status, 400 | 422)
        && ["response_format", "json_schema", "json_object", "schema"]
            .iter()
            .any(|w| body.contains(w))
}

/// What the card says for `e`.
pub fn failure(e: &AiError) -> Failure {
    let said = |why, body: &str| Failure::detailed(why, provider_message(body));
    match e {
        AiError::AllKeysExhausted {
            retry_in_ms: Some(ms),
        } => {
            let secs = ms.div_ceil(1000).max(1);
            Failure::detailed(
                Why::Busy,
                format!("every key is resting; one is back in {secs} s"),
            )
        }
        AiError::AllKeysExhausted { retry_in_ms: None } => Why::KeysRejected.into(),
        AiError::Timeout => Why::TimedOut.into(),
        AiError::BadRequest { body, .. } => said(Why::Setup, body),
        AiError::ServerError { status, body } => {
            Failure::detailed(Why::Busy, format!("{status}: {}", provider_message(body)))
        }
        AiError::UnexpectedStatus { status, body } => {
            Failure::detailed(Why::Setup, format!("{status}: {}", provider_message(body)))
        }
        AiError::Network(err) => Failure::detailed(Why::Offline, err.to_string()),
        AiError::InvalidResponse(m) => Failure::detailed(Why::BadAnswer, m.clone()),
        AiError::Refused(m) => Failure::detailed(Why::Refused, m.clone()),
        AiError::Truncated => Failure::detailed(Why::BadAnswer, "the answer was cut off"),
        AiError::StreamInterrupted(StreamInterruption::RateLimited) => Why::Busy.into(),
        AiError::StreamInterrupted(StreamInterruption::Unauthorized) => Why::KeysRejected.into(),
        AiError::Vault(VaultError::KeychainLocked(m)) => Failure::detailed(Why::Locked, m.clone()),
        other => Failure::detailed(Why::Setup, other.to_string()),
    }
}

/// The `error.message` of an OpenAI-style body, else the body itself.
fn provider_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let one = match &parsed {
        // Gemini wraps the error object in an array.
        Some(Value::Array(items)) => items.first(),
        other => other.as_ref(),
    };
    let message = one
        .and_then(|v| v.get("error"))
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(Value::as_str);
    message.map_or_else(|| body.trim().to_owned(), str::to_owned)
}

/// Small print from model text.
fn clip(text: &str) -> String {
    let cut: String = text.chars().take(Failure::DETAIL_CHARS).collect();
    if cut.trim().is_empty() {
        "the answer was empty".to_owned()
    } else {
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_messages_are_found_in_either_shape() {
        let openai = r#"{"error":{"message":"The model `x` does not exist","type":"invalid"}}"#;
        assert_eq!(provider_message(openai), "The model `x` does not exist");
        let gemini = r#"[{"error":{"code":400,"message":"API key not valid"}}]"#;
        assert_eq!(provider_message(gemini), "API key not valid");
        assert_eq!(provider_message(r#"{"error":"plain"}"#), "plain");
        assert_eq!(provider_message(" Bad Gateway "), "Bad Gateway");
    }

    #[test]
    fn only_format_complaints_weaken_the_shape() {
        let bad = |status, body: &str| AiError::BadRequest {
            status,
            body: body.into(),
        };
        assert!(shape_refused(&bad(
            400,
            "response_format json_schema unsupported"
        )));
        assert!(shape_refused(&bad(422, "Invalid schema for function")));
        assert!(!shape_refused(&bad(
            404,
            "model not found: response_format"
        )));
        assert!(!shape_refused(&bad(400, "context length exceeded")));
        assert!(!shape_refused(&AiError::Timeout));
        assert_eq!(Shape::Schema.weaker(), Some(Shape::Object));
        assert_eq!(Shape::Plain.weaker(), None);
    }

    #[test]
    fn errors_become_the_card_s_reasons() {
        let resting = AiError::AllKeysExhausted {
            retry_in_ms: Some(1_200),
        };
        let f = failure(&resting);
        assert_eq!(f.why, Why::Busy);
        assert_eq!(
            f.detail.as_deref(),
            Some("every key is resting; one is back in 2 s")
        );
        assert_eq!(failure(&AiError::Timeout).why, Why::TimedOut);
        let gone = AiError::BadRequest {
            status: 404,
            body: r#"{"error":{"message":"no such model"}}"#.into(),
        };
        assert_eq!(failure(&gone).detail.as_deref(), Some("no such model"));
        assert_eq!(failure(&AiError::Truncated).why, Why::BadAnswer);
    }
}
