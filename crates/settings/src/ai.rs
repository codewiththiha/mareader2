//! Which AI explains words, and where; `mareader-assist` asks.

use serde::{Deserialize, Serialize};

/// Who answers. Keys are not here: they live encrypted, apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Apple Intelligence where the Mac has it; otherwise none.
    #[default]
    Auto,
    /// Apple's on-device model, through the bundled helper.
    Apple,
    #[serde(rename = "openai")]
    OpenAi,
    Gemini,
    #[serde(rename = "openrouter")]
    OpenRouter,
    /// A local Ollama server; no key.
    Ollama,
    /// A local LM Studio server; no key.
    LmStudio,
    /// Any OpenAI-compatible server; a key only if it has some.
    Custom,
}

impl Provider {
    pub const ALL: [Provider; 8] = [
        Self::Auto,
        Self::Apple,
        Self::OpenAi,
        Self::Gemini,
        Self::OpenRouter,
        Self::Ollama,
        Self::LmStudio,
        Self::Custom,
    ];

    /// The ones reached over HTTP, each with its own address and keys.
    pub const SERVERS: [Provider; 6] = [
        Self::OpenAi,
        Self::Gemini,
        Self::OpenRouter,
        Self::Ollama,
        Self::LmStudio,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Apple => "Apple Intelligence",
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Google Gemini",
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::Custom => "Custom server",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Auto => "Apple Intelligence on Macs that have it",
            Self::Apple => "On this Mac, private and offline",
            Self::OpenAi => "Needs an API key",
            Self::Gemini => "Needs an API key",
            Self::OpenRouter => "Many models, one API key",
            Self::Ollama => "A model running on this computer",
            Self::LmStudio => "A model running on this computer",
            Self::Custom => "Any OpenAI-compatible address",
        }
    }

    /// The address when none is set; a custom server has none.
    pub fn default_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1/",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai/",
            Self::OpenRouter => "https://openrouter.ai/api/v1/",
            Self::Ollama => "http://localhost:11434/v1/",
            Self::LmStudio => "http://localhost:1234/v1/",
            Self::Auto | Self::Apple | Self::Custom => "",
        }
    }

    /// The model when none is set; empty asks the server for its first.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4.1-mini",
            Self::Gemini => "gemini-2.5-flash",
            Self::OpenRouter => "openai/gpt-4.1-mini",
            _ => "",
        }
    }

    /// Whether requests need a key; a custom server needs one if it has one.
    pub fn needs_key(self) -> bool {
        matches!(self, Self::OpenAi | Self::Gemini | Self::OpenRouter)
    }

    /// Whether it may hold keys at all.
    pub fn takes_keys(self) -> bool {
        self.needs_key() || self == Self::Custom
    }

    /// Whether it is a server on this computer.
    pub fn local(self) -> bool {
        matches!(self, Self::Ollama | Self::LmStudio)
    }

    /// Its keys' scope in the key store; stable, never shown.
    pub fn scope(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Apple => "apple",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::OpenRouter => "openrouter",
            Self::Ollama => "ollama",
            Self::LmStudio => "lm_studio",
            Self::Custom => "custom",
        }
    }
}

/// Where one server is and which model it runs; blank means the default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Endpoint {
    pub url: String,
    pub model: String,
}

impl Endpoint {
    /// Longer text is cut; a pasted page can't bloat the settings file.
    pub const MAX_CHARS: usize = 2_048;

    fn sanitize(&mut self) {
        for s in [&mut self.url, &mut self.model] {
            if let Some((byte, _)) = s.char_indices().nth(Self::MAX_CHARS) {
                s.truncate(byte);
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub provider: Provider,
    pub openai: Endpoint,
    pub gemini: Endpoint,
    pub openrouter: Endpoint,
    pub ollama: Endpoint,
    pub lm_studio: Endpoint,
    pub custom: Endpoint,
}

impl AiSettings {
    pub fn endpoint(&self, p: Provider) -> Option<&Endpoint> {
        match p {
            Provider::OpenAi => Some(&self.openai),
            Provider::Gemini => Some(&self.gemini),
            Provider::OpenRouter => Some(&self.openrouter),
            Provider::Ollama => Some(&self.ollama),
            Provider::LmStudio => Some(&self.lm_studio),
            Provider::Custom => Some(&self.custom),
            Provider::Auto | Provider::Apple => None,
        }
    }

    pub fn endpoint_mut(&mut self, p: Provider) -> Option<&mut Endpoint> {
        match p {
            Provider::OpenAi => Some(&mut self.openai),
            Provider::Gemini => Some(&mut self.gemini),
            Provider::OpenRouter => Some(&mut self.openrouter),
            Provider::Ollama => Some(&mut self.ollama),
            Provider::LmStudio => Some(&mut self.lm_studio),
            Provider::Custom => Some(&mut self.custom),
            Provider::Auto | Provider::Apple => None,
        }
    }

    /// `p`'s address: the one set, else its default; empty if neither.
    pub fn url(&self, p: Provider) -> String {
        let set = self.endpoint(p).map_or("", |e| e.url.trim());
        let url = if set.is_empty() { p.default_url() } else { set };
        url.to_owned()
    }

    /// `p`'s model: the one set, else its default; empty if neither.
    pub fn model(&self, p: Provider) -> String {
        let set = self.endpoint(p).map_or("", |e| e.model.trim());
        let model = if set.is_empty() {
            p.default_model()
        } else {
            set
        };
        model.to_owned()
    }

    /// Runs on every edit, so text being typed is never rewritten.
    pub fn sanitize(&mut self) {
        for p in Provider::SERVERS {
            if let Some(e) = self.endpoint_mut(p) {
                e.sanitize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_fall_back_to_each_provider_s_defaults() {
        let mut ai = AiSettings::default();
        assert_eq!(ai.provider, Provider::Auto);
        assert_eq!(ai.url(Provider::Gemini), Provider::Gemini.default_url());
        assert_eq!(ai.model(Provider::OpenAi), "gpt-4.1-mini");
        assert_eq!(ai.url(Provider::Custom), "");
        ai.ollama.url = "  http://box:11434/v1  ".into();
        ai.ollama.model = " llama3.2 ".into();
        assert_eq!(ai.url(Provider::Ollama), "http://box:11434/v1");
        assert_eq!(ai.model(Provider::Ollama), "llama3.2");
        assert_eq!(ai.endpoint(Provider::Apple), None);
    }

    #[test]
    fn text_being_typed_survives_and_only_huge_text_is_cut() {
        let mut ai = AiSettings::default();
        ai.custom.url = "h".into();
        ai.openai.model = "é".repeat(Endpoint::MAX_CHARS + 9);
        ai.sanitize();
        assert_eq!(ai.custom.url, "h");
        assert_eq!(ai.openai.model.chars().count(), Endpoint::MAX_CHARS);
    }

    #[test]
    fn providers_keep_their_names_in_the_file() {
        let names: Vec<String> = Provider::ALL
            .iter()
            .filter_map(|p| serde_json::to_string(p).ok())
            .collect();
        let want = [
            "auto",
            "apple",
            "openai",
            "gemini",
            "openrouter",
            "ollama",
            "lm_studio",
            "custom",
        ];
        let want: Vec<String> = want.iter().map(|w| format!("\"{w}\"")).collect();
        assert_eq!(names, want);
        let keyed: Vec<_> = Provider::ALL
            .into_iter()
            .filter(|p| p.takes_keys())
            .collect();
        let want = [
            Provider::OpenAi,
            Provider::Gemini,
            Provider::OpenRouter,
            Provider::Custom,
        ];
        assert_eq!(keyed, want);
    }
}
