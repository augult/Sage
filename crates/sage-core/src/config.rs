//! Providers, keys and defaults.

use crate::provider::openai::Dialect;
use crate::util::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub struct ProviderDef {
    pub id: &'static str,
    pub name: &'static str,
    /// What it is known for, in a word or two: the models it makes, or the service it is.
    pub serves: &'static str,
    pub base_url: &'static str,
    pub key_env: &'static str,
    pub dialect: Dialect,
    pub models: &'static [&'static str],
    pub note: &'static str,
}

/// Built-in providers, in the order they are listed. Only Logfare has been exercised live by this
/// project; the others follow the vendors' published API contracts, and their model lists are what
/// each vendor documented on 11 November 2026 (Moonshot's, a day later).
pub const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: "openai",
        name: "OpenAI",
        serves: "GPT",
        base_url: "https://api.openai.com/v1",
        key_env: "OPENAI_API_KEY",
        dialect: Dialect::OpenAiPlatform,
        models: &["gpt-6.1-sol", "gpt-6-astra", "gpt-6.1-terra", "gpt-6-luna"],
        note: "",
    },
    ProviderDef {
        id: "anthropic",
        name: "Anthropic",
        serves: "Claude",
        base_url: "https://api.anthropic.com/v1",
        key_env: "ANTHROPIC_API_KEY",
        dialect: Dialect::Anthropic,
        models: &[
            "claude-sonnet-5-5",
            "claude-opus-5-5",
            "claude-fable-5-2",
            "claude-haiku-5-5",
        ],
        note: "Through Anthropic's OpenAI-compatible endpoint: no thinking and no prompt caching.",
    },
    ProviderDef {
        id: "google",
        name: "Google AI",
        serves: "Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        key_env: "GEMINI_API_KEY",
        dialect: Dialect::Gemini,
        models: &[
            "gemini-3.8-flash",
            "gemini-4-argon",
            "gemini-4-carbon",
            "gemini-3.5-flash-lite",
        ],
        note: "",
    },
    ProviderDef {
        id: "deepseek",
        name: "DeepSeek",
        serves: "",
        base_url: "https://api.deepseek.com",
        key_env: "DEEPSEEK_API_KEY",
        dialect: Dialect::DeepSeek,
        models: &["deepseek-flash", "deepseek-v4-pro"],
        note: "Cache hits cost 1/50 of a miss; prices double at weekday peak hours (UTC).",
    },
    ProviderDef {
        id: "openrouter",
        name: "OpenRouter",
        serves: "",
        base_url: "https://openrouter.ai/api/v1",
        key_env: "OPENROUTER_API_KEY",
        dialect: Dialect::OpenRouter,
        models: &[
            "deepseek/deepseek-v4.1-flash",
            "z-ai/glm-5.3",
            "anthropic/claude-sonnet-5.5",
            "anthropic/claude-opus-5.5",
            "openai/gpt-6.1-sol",
            "openai/gpt-6-astra",
            "google/gemini-3.8-flash",
            "google/gemini-4-argon",
            "x-ai/grok-4.7",
            "moonshotai/kimi-k3",
            "mistralai/mistral-large-4-0",
            "qwen/qwen3.8-max-0902",
        ],
        note: "",
    },
    ProviderDef {
        id: "moonshot",
        name: "Moonshot Kimi",
        serves: "",
        base_url: "https://api.moonshot.ai/v1",
        key_env: "MOONSHOT_API_KEY",
        dialect: Dialect::Moonshot,
        models: &["kimi-k3.1-pro", "kimi-k3.1-pro-highspeed", "kimi-k3"],
        note: "",
    },
    ProviderDef {
        id: "xai",
        name: "xAI",
        serves: "Grok",
        base_url: "https://api.x.ai/v1",
        key_env: "XAI_API_KEY",
        dialect: Dialect::Xai,
        models: &["grok-4.7", "grok-4.6", "grok-4.5"],
        note: "",
    },
    ProviderDef {
        id: "groq",
        name: "Groq",
        serves: "High-speed inference",
        base_url: "https://api.groq.com/openai/v1",
        key_env: "GROQ_API_KEY",
        dialect: Dialect::Groq,
        models: &[
            "openai/gpt-oss-120b",
            "openai/gpt-oss-20b",
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
        ],
        note: "",
    },
    ProviderDef {
        id: "together",
        name: "Together AI",
        serves: "Open-source model hosting",
        base_url: "https://api.together.ai/v1",
        key_env: "TOGETHER_API_KEY",
        dialect: Dialect::Together,
        models: &[
            "moonshotai/Kimi-K3",
            "deepseek-ai/DeepSeek-V4-Pro-0813",
            "Qwen/Qwen3.7-Max",
            "zai-org/GLM-5.3",
            "openai/gpt-oss-120b",
        ],
        note: "",
    },
    ProviderDef {
        id: "logfare",
        name: "Logfare",
        serves: "",
        base_url: "https://logfare.ai/v1",
        key_env: "LOGFARE_API_KEY",
        dialect: Dialect::Logfare,
        models: &["deepseek-v4.1-flash-1017", "glm-5.3", "glm-5.3-flash"],
        note: "Free gateway. It logs prompts and completions.",
    },
];

/// The environment variables provider keys are read from. Commands the model runs never see them:
/// whatever a command prints goes to the provider with the next request, and into the session's log.
pub fn key_env_vars() -> impl Iterator<Item = &'static str> {
    PROVIDERS
        .iter()
        .map(|d| d.key_env)
        .chain(std::iter::once("SAGE_API_KEY"))
}

/// A user-defined OpenAI-compatible endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomProvider {
    pub base_url: String,
    #[serde(default)]
    pub dialect: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

/// The id of the one endpoint a config file names under `custom`.
pub const CUSTOM: &str = "custom";

/// An endpoint added in Settings: a custom provider with an id and a name of its own.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddedProvider {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub endpoint: CustomProvider,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Provider id → API key. Environment variables take precedence over these.
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
    /// Providers whose key was removed in Settings: their environment variable is not used
    /// either, until a key is given again.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub env_off: BTreeSet<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Keep retrying a provider that is rate limited or unreachable instead of ending the run.
    #[serde(default)]
    pub auto_retry: bool,
    /// "Guard files": `finish` is held while a file the task says not to edit has changed
    /// (`Features::protect`). For new sessions.
    #[serde(default)]
    pub protect_files: bool,
    #[serde(default)]
    pub custom: Option<CustomProvider>,
    /// The user's own endpoints, in the order they were added.
    #[serde(default)]
    pub added: Vec<AddedProvider>,
}

#[derive(Clone, Debug)]
pub struct Resolved {
    pub id: String,
    pub base_url: String,
    pub dialect: Dialect,
    pub key: String,
    pub model: String,
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("SAGE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".sage"))
}

/// Whether `path` is, however it is spelled and through any link, the file that holds the API keys.
pub fn holds_keys(path: &Path) -> bool {
    let own = data_dir().join("config.json");
    match (crate::util::canonical(path), crate::util::canonical(&own)) {
        (Ok(path), Ok(own)) => path == own,
        _ => crate::workspace::normalize(path) == crate::workspace::normalize(&own),
    }
}

impl Config {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Writes the config readable by the owner only: it may hold API keys.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // A file `load` could not read (a slip in an edit by hand) was taken for an empty one. It is
        // kept beside the new one rather than written over with everything it held, keys and all.
        match std::fs::read(path) {
            Ok(old) if !old.is_empty() && serde_json::from_slice::<Config>(&old).is_err() => {
                crate::resume::write_atomic(&path.with_extension("json.unreadable"), &old)?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        crate::resume::write_atomic(path, body.as_bytes())
    }

    pub fn key_for(&self, id: &str, key_env: &str) -> Option<String> {
        self.key_given(id, |name| std::env::var(name).ok(), key_env)
    }

    /// `key_for`, with the environment read through `var`.
    fn key_given(
        &self,
        id: &str,
        var: impl Fn(&str) -> Option<String>,
        key_env: &str,
    ) -> Option<String> {
        let env = (!self.env_off.contains(id)).then(|| var(key_env)).flatten();
        env.filter(|k| !k.trim().is_empty())
            .or_else(|| self.keys.get(id).cloned())
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }

    pub fn has_key(&self, def: &ProviderDef) -> bool {
        self.key_for(def.id, def.key_env).is_some()
    }

    /// The user's own endpoints as `(id, name, endpoint)`: the one the file names under `custom`,
    /// then those added in Settings.
    pub fn customs(&self) -> impl Iterator<Item = (&str, &str, &CustomProvider)> {
        self.custom
            .iter()
            .map(|c| (CUSTOM, "Custom endpoint", c))
            .chain(
                self.added
                    .iter()
                    .map(|a| (a.id.as_str(), a.name.as_str(), &a.endpoint)),
            )
    }

    /// Whether `id` names a provider there is, built in or the user's own.
    pub fn knows(&self, id: &str) -> bool {
        PROVIDERS.iter().any(|d| d.id == id) || self.customs().any(|(known, ..)| known == id)
    }

    /// The provider to use when none is named: the configured one, else the first that has a key.
    pub fn default_provider(&self) -> Option<String> {
        // A saved choice holds while its provider is there: a build may drop one, and the user's own can be removed.
        if let Some(p) = self.provider.as_ref().filter(|p| self.knows(p)) {
            return Some(p.clone());
        }
        PROVIDERS
            .iter()
            .find(|d| self.has_key(d))
            .map(|d| d.id.to_string())
            .or_else(|| self.customs().next().map(|(id, ..)| id.to_string()))
    }

    /// Adds an OpenAI-compatible endpoint of the user's own, and gives the id it goes by.
    pub fn add_provider(
        &mut self,
        name: &str,
        base_url: &str,
        models: &[String],
    ) -> Result<String, String> {
        let name = name.trim();
        let base_url = base_url.trim().trim_end_matches('/');
        let models: Vec<String> = models
            .iter()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .collect();
        if name.is_empty() {
            return Err("the provider needs a name".into());
        }
        let taken = |other: &str| other.eq_ignore_ascii_case(name);
        if PROVIDERS.iter().any(|d| taken(d.id) || taken(d.name))
            || self
                .customs()
                .any(|(id, known, _)| taken(id) || taken(known))
        {
            return Err(format!("there is already a provider called {name}"));
        }
        if !["http://", "https://"]
            .iter()
            .any(|scheme| base_url.len() > scheme.len() && base_url.starts_with(scheme))
        {
            return Err("the address has to start with http:// or https://".into());
        }
        if models.is_empty() {
            return Err("the provider needs at least one model".into());
        }
        let slug = name
            .to_ascii_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if slug.is_empty() {
            format!("{CUSTOM}-endpoint")
        } else {
            format!("{CUSTOM}-{slug}")
        };
        // Two names can come to one slug ("My LLM", "my-llm"): the later one is numbered.
        let mut id = base.clone();
        let mut n = 1;
        while self.knows(&id) {
            n += 1;
            id = format!("{base}-{n}");
        }
        self.added.push(AddedProvider {
            id: id.clone(),
            name: name.to_string(),
            endpoint: CustomProvider {
                base_url: base_url.to_string(),
                dialect: None,
                models,
            },
        });
        Ok(id)
    }

    /// Takes one of the user's own endpoints away, with its key and the defaults that were its.
    pub fn remove_provider(&mut self, id: &str) -> Result<(), String> {
        let before = self.added.len();
        self.added.retain(|a| a.id != id);
        if id == CUSTOM && self.custom.is_some() {
            self.custom = None;
        } else if self.added.len() == before {
            return Err(format!("{id:?} is not a provider of your own"));
        }
        self.keys.remove(id);
        if self.provider.as_deref() == Some(id) {
            self.provider = None;
            self.model = None;
        }
        Ok(())
    }

    pub fn resolve(&self, provider: Option<&str>, model: Option<&str>) -> Result<Resolved, String> {
        let id = provider
            .map(String::from)
            .or_else(|| self.default_provider())
            .ok_or_else(|| {
                let envs: Vec<&str> = PROVIDERS.iter().map(|d| d.key_env).collect();
                format!(
                    "no provider has an API key yet. Add one in Settings (desktop app) or with `sage key <provider>`, or set one of: {}",
                    envs.join(", ")
                )
            })?;
        if id == CUSTOM && self.custom.is_none() {
            return Err("no custom provider is configured".into());
        }
        if let Some((_, name, c)) = self.customs().find(|(known, ..)| *known == id) {
            // A server of one's own may ask for no key. Only the endpoint the file names takes one from the environment.
            let key = if id == CUSTOM {
                self.key_for(CUSTOM, "SAGE_API_KEY")
            } else {
                self.keys.get(&id).map(|k| k.trim().to_string())
            }
            .unwrap_or_default();
            // A saved default model is not carried over from another provider.
            let saved = self
                .model
                .clone()
                .filter(|_| self.provider.as_deref().is_none_or(|p| p == id));
            let model = model
                .map(String::from)
                .or(saved)
                .or_else(|| c.models.first().cloned())
                .ok_or_else(|| format!("{name} has no model configured"))?;
            let dialect = c
                .dialect
                .as_deref()
                .and_then(Dialect::parse)
                .unwrap_or(Dialect::OpenAi);
            return Ok(Resolved {
                base_url: c.base_url.clone(),
                dialect,
                key,
                model,
                id,
            });
        }
        let known = PROVIDERS
            .iter()
            .map(|d| d.id)
            .chain(self.customs().map(|(id, ..)| id))
            .collect::<Vec<_>>()
            .join(", ");
        let def = PROVIDERS
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| format!("unknown provider {id:?}; known: {known}"))?;
        let key = self.key_for(def.id, def.key_env).ok_or_else(|| {
            format!(
                "no API key for {}. Add it in Settings or set {}",
                def.name, def.key_env
            )
        })?;
        // A saved default model belongs to the saved default provider only.
        let saved = self
            .model
            .clone()
            .filter(|_| self.provider.as_deref() == Some(def.id));
        let model = model
            .map(String::from)
            .or(saved)
            .unwrap_or_else(|| def.models[0].to_string());
        Ok(Resolved {
            id,
            base_url: def.base_url.to_string(),
            dialect: def.dialect,
            key,
            model,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_defaults_and_explains_what_is_missing() {
        let mut c = Config::default();
        // Hermetic: these tests must not depend on the developer's environment.
        let no_env = PROVIDERS.iter().all(|d| std::env::var(d.key_env).is_err());
        if no_env {
            assert!(
                c.resolve(None, None)
                    .unwrap_err()
                    .contains("no provider has an API key yet")
            );
        }
        c.keys.insert("logfare".into(), " k-logfare ".into());
        if no_env {
            let r = c.resolve(None, None).unwrap();
            assert_eq!(
                (r.id.as_str(), r.model.as_str(), r.key.as_str(), r.dialect),
                (
                    "logfare",
                    "deepseek-v4.1-flash-1017",
                    "k-logfare",
                    Dialect::Logfare
                )
            );
        }
        let r = c.resolve(Some("logfare"), Some("glm-5.3-flash")).unwrap();
        assert_eq!(r.model, "glm-5.3-flash");
        assert!(
            c.resolve(Some("nope"), None)
                .unwrap_err()
                .contains("unknown provider")
        );
        if std::env::var("DEEPSEEK_API_KEY").is_err() {
            assert!(
                c.resolve(Some("deepseek"), None)
                    .unwrap_err()
                    .contains("DEEPSEEK_API_KEY")
            );
        }

        c.provider = Some("logfare".into());
        c.model = Some("glm-5.3-flash".into());
        assert_eq!(c.resolve(None, None).unwrap().model, "glm-5.3-flash");
        c.keys.insert("openrouter".into(), "k".into());
        assert_eq!(
            c.resolve(Some("openrouter"), None).unwrap().model,
            "deepseek/deepseek-v4.1-flash",
            "the saved model is not carried to another provider"
        );

        // A default saved by a build that still had that provider gives way to one that is there.
        c.provider = Some("zai".into());
        if no_env {
            let r = c.resolve(None, None).unwrap();
            assert_eq!(
                (r.id.as_str(), r.model.as_str()),
                ("openrouter", "deepseek/deepseek-v4.1-flash")
            );
        }
    }

    #[test]
    fn the_list_is_the_ten_built_in_in_their_order() {
        let ids: Vec<&str> = PROVIDERS.iter().map(|d| d.id).collect();
        assert_eq!(
            ids,
            [
                "openai",
                "anthropic",
                "google",
                "deepseek",
                "openrouter",
                "moonshot",
                "xai",
                "groq",
                "together",
                "logfare"
            ]
        );
        assert!(
            PROVIDERS
                .iter()
                .all(|d| !d.models.is_empty() && d.base_url.starts_with("https://"))
        );
    }

    #[test]
    fn endpoints_of_the_users_own_are_added_resolved_and_removed() {
        let mut c = Config::default();
        let one = |model: &str| vec![model.to_string()];
        assert!(
            c.add_provider(" ", "http://localhost:11434/v1", &one("m"))
                .unwrap_err()
                .contains("name")
        );
        assert!(
            c.add_provider("Ollama", "localhost:11434", &one("m"))
                .unwrap_err()
                .contains("http://")
        );
        assert!(
            c.add_provider("Ollama", "http://localhost:11434/v1", &one(" "))
                .unwrap_err()
                .contains("model")
        );
        assert!(
            c.add_provider("openai", "http://localhost:1/v1", &one("m"))
                .unwrap_err()
                .contains("already")
        );

        let id = c
            .add_provider(
                " Ollama ",
                "http://localhost:11434/v1/",
                &[" qwen3 ".into(), "".into(), "llama4".into()],
            )
            .unwrap();
        assert_eq!(id, "custom-ollama");
        assert!(
            c.add_provider("ollama", "http://localhost:1/v1", &one("m"))
                .unwrap_err()
                .contains("already")
        );
        assert_eq!(
            c.add_provider("My LLM", "https://llm.example/v1", &one("m"))
                .unwrap(),
            "custom-my-llm"
        );
        assert_eq!(
            c.add_provider("my-llm", "https://llm.example/v1", &one("m"))
                .unwrap(),
            "custom-my-llm-2"
        );

        // It asks for no key, and is the default while nothing else can be.
        let r = c.resolve(Some(&id), None).unwrap();
        assert_eq!(
            (
                r.base_url.as_str(),
                r.model.as_str(),
                r.key.as_str(),
                r.dialect
            ),
            ("http://localhost:11434/v1", "qwen3", "", Dialect::OpenAi)
        );
        if PROVIDERS.iter().all(|d| std::env::var(d.key_env).is_err()) {
            assert_eq!(c.default_provider().as_deref(), Some("custom-ollama"));
        }
        c.keys.insert(id.clone(), "k".into());
        c.provider = Some(id.clone());
        c.model = Some("llama4".into());
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        let r = back.resolve(None, None).unwrap();
        assert_eq!(
            (r.id.as_str(), r.model.as_str(), r.key.as_str()),
            ("custom-ollama", "llama4", "k")
        );
        assert_eq!(
            back.resolve(Some("custom-my-llm"), None).unwrap().model,
            "m",
            "the saved model is its provider's alone"
        );

        assert!(c.remove_provider("openai").is_err() && c.remove_provider("custom").is_err());
        c.remove_provider(&id).unwrap();
        assert!(!c.keys.contains_key(&id) && c.provider.is_none() && c.model.is_none());
        assert!(
            c.resolve(Some(&id), None)
                .unwrap_err()
                .contains("unknown provider")
        );
        assert!(c.knows("custom-my-llm") && !c.knows(&id));
    }

    #[test]
    fn custom_endpoints_and_round_trip_with_private_permissions() {
        let path =
            std::env::temp_dir().join(format!("sage-cfg-{}/config.json", crate::util::rand_hex(4)));
        let mut c = Config::default();
        c.keys.insert("logfare".into(), "secret".into());
        c.custom = Some(CustomProvider {
            base_url: "http://localhost:8000/v1".into(),
            dialect: Some("deepseek".into()),
            models: vec!["local-model".into()],
        });
        c.save(&path).unwrap();
        c.save(&path).unwrap();
        let back = Config::load(&path);
        assert_eq!(back.keys.get("logfare").map(String::as_str), Some("secret"));
        let r = back.resolve(Some("custom"), None).unwrap();
        assert_eq!(
            (r.base_url.as_str(), r.model.as_str(), r.dialect),
            ("http://localhost:8000/v1", "local-model", Dialect::DeepSeek)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(
            Config::load(Path::new("/nonexistent/config.json"))
                .keys
                .is_empty()
        );
    }

    #[test]
    fn a_file_that_could_not_be_read_is_kept_when_the_next_one_is_written() {
        let path =
            std::env::temp_dir().join(format!("sage-cfg-{}/config.json", crate::util::rand_hex(4)));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A trailing comma left by an edit by hand.
        let slip = "{ \"keys\": { \"openai\": \"sk-kept\" }, }";
        std::fs::write(&path, slip).unwrap();
        let mut c = Config::load(&path);
        assert!(c.keys.is_empty());
        c.keys.insert("deepseek".into(), "new".into());
        c.save(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.unreadable")).unwrap(),
            slip
        );
        assert_eq!(
            Config::load(&path).keys.get("deepseek").map(String::as_str),
            Some("new")
        );
        // A file that reads is simply replaced.
        std::fs::remove_file(path.with_extension("json.unreadable")).unwrap();
        c.save(&path).unwrap();
        assert!(!path.with_extension("json.unreadable").exists());
    }

    #[test]
    fn a_removed_key_turns_off_the_environment_s_too() {
        // An environment of its own: setting a variable of the process while other tests read
        // theirs is a race.
        let env = |name: &str| (name == "P_API_KEY").then(|| "from-env".to_string());
        let mut c = Config::default();
        assert_eq!(
            c.key_given("p", env, "P_API_KEY").as_deref(),
            Some("from-env")
        );
        c.env_off.insert("p".into());
        assert_eq!(c.key_given("p", env, "P_API_KEY"), None);
        c.keys.insert("p".into(), "stored".into());
        assert_eq!(
            c.key_given("p", env, "P_API_KEY").as_deref(),
            Some("stored")
        );
    }

    #[test]
    fn every_key_variable_is_one_commands_are_kept_from() {
        let hidden: Vec<&str> = key_env_vars().collect();
        assert!(PROVIDERS.iter().all(|d| hidden.contains(&d.key_env)));
        assert!(hidden.contains(&"SAGE_API_KEY"));
    }
}
