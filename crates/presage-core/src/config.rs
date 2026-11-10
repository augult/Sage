//! Providers, keys and defaults.

use crate::provider::openai::Dialect;
use crate::util::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct ProviderDef {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub key_env: &'static str,
    pub dialect: Dialect,
    pub models: &'static [&'static str],
    pub note: &'static str,
}

/// Built-in providers. Only Logfare has been exercised live by this project; the others follow the
/// vendors' published API contracts.
pub const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: "logfare",
        name: "Logfare",
        base_url: "https://logfare.ai/v1",
        key_env: "LOGFARE_API_KEY",
        dialect: Dialect::OpenRouter,
        models: &["deepseek-v4.1-flash-1017", "glm-5.3", "glm-5.3-flash"],
        note: "Free gateway. It logs prompts and completions.",
    },
    ProviderDef {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        key_env: "DEEPSEEK_API_KEY",
        dialect: Dialect::DeepSeek,
        models: &["deepseek-flash", "deepseek-v4-pro"],
        note: "Cache hits cost 1/50 of a miss; prices double at weekday peak hours (UTC).",
    },
    ProviderDef {
        id: "zai",
        name: "Z.ai",
        base_url: "https://api.z.ai/api/paas/v4",
        key_env: "ZAI_API_KEY",
        dialect: Dialect::Zai,
        models: &["glm-5.3", "glm-5.3-flash"],
        note: "Pay-as-you-go GLM endpoint.",
    },
    ProviderDef {
        id: "zai-coding",
        name: "Z.ai Coding Plan",
        base_url: "https://api.z.ai/api/coding/paas/v4",
        key_env: "ZAI_API_KEY",
        dialect: Dialect::Zai,
        models: &["glm-5.3", "glm-5.3-flash"],
        note: "Subscription endpoint: what counts there is the number of requests.",
    },
    ProviderDef {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        key_env: "OPENROUTER_API_KEY",
        dialect: Dialect::OpenRouter,
        models: &["deepseek/deepseek-v4.1-flash", "z-ai/glm-5.3"],
        note: "",
    },
];

/// A user-defined OpenAI-compatible endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomProvider {
    pub base_url: String,
    #[serde(default)]
    pub dialect: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Provider id → API key. Environment variables take precedence over these.
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub custom: Option<CustomProvider>,
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
    std::env::var_os("PRESAGE_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".presage"))
}

impl Config {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// Writes the config readable by the owner only: it may hold API keys.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
            f.write_all(body.as_bytes())?;
            // `mode` only applies when the file is created; tighten an existing one explicitly.
            std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        std::fs::write(path, body)?;
        Ok(())
    }

    pub fn key_for(&self, id: &str, key_env: &str) -> Option<String> {
        std::env::var(key_env).ok().filter(|k| !k.trim().is_empty()).or_else(|| self.keys.get(id).cloned()).map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn has_key(&self, def: &ProviderDef) -> bool {
        self.key_for(def.id, def.key_env).is_some()
    }

    /// The provider to use when none is named: the configured one, else the first that has a key.
    pub fn default_provider(&self) -> Option<String> {
        if let Some(p) = &self.provider {
            return Some(p.clone());
        }
        PROVIDERS.iter().find(|d| self.has_key(d)).map(|d| d.id.to_string()).or_else(|| self.custom.as_ref().map(|_| "custom".to_string()))
    }

    pub fn resolve(&self, provider: Option<&str>, model: Option<&str>) -> Result<Resolved, String> {
        let id = provider.map(String::from).or_else(|| self.default_provider()).ok_or_else(|| {
            let envs: Vec<&str> = PROVIDERS.iter().map(|d| d.key_env).collect();
            format!("no provider has an API key yet. Add one in Settings, or set one of: {}", envs.join(", "))
        })?;
        if id == "custom" {
            let c = self.custom.as_ref().ok_or("no custom provider is configured")?;
            let key = self.key_for("custom", "PRESAGE_API_KEY").unwrap_or_default();
            let model = model.map(String::from).or_else(|| self.model.clone()).or_else(|| c.models.first().cloned()).ok_or("the custom provider has no model configured")?;
            let dialect = c.dialect.as_deref().and_then(Dialect::parse).unwrap_or(Dialect::OpenAi);
            return Ok(Resolved { id, base_url: c.base_url.clone(), dialect, key, model });
        }
        let def = PROVIDERS.iter().find(|d| d.id == id).ok_or_else(|| format!("unknown provider {id:?}; known: {}", PROVIDERS.iter().map(|d| d.id).collect::<Vec<_>>().join(", ")))?;
        let key = self.key_for(def.id, def.key_env).ok_or_else(|| format!("no API key for {}. Add it in Settings or set {}", def.name, def.key_env))?;
        // A saved default model belongs to the saved default provider only.
        let saved = self.model.clone().filter(|_| self.provider.as_deref() == Some(def.id));
        let model = model.map(String::from).or(saved).unwrap_or_else(|| def.models[0].to_string());
        Ok(Resolved { id, base_url: def.base_url.to_string(), dialect: def.dialect, key, model })
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
            assert!(c.resolve(None, None).unwrap_err().contains("no provider has an API key yet"));
        }
        c.keys.insert("zai".into(), " k-zai ".into());
        if no_env {
            let r = c.resolve(None, None).unwrap();
            assert_eq!((r.id.as_str(), r.model.as_str(), r.key.as_str(), r.dialect), ("zai", "glm-5.3", "k-zai", Dialect::Zai));
        }
        let r = c.resolve(Some("zai"), Some("glm-5.3-flash")).unwrap();
        assert_eq!(r.model, "glm-5.3-flash");
        assert!(c.resolve(Some("nope"), None).unwrap_err().contains("unknown provider"));
        if std::env::var("DEEPSEEK_API_KEY").is_err() {
            assert!(c.resolve(Some("deepseek"), None).unwrap_err().contains("DEEPSEEK_API_KEY"));
        }

        c.provider = Some("zai".into());
        c.model = Some("glm-5.3-flash".into());
        assert_eq!(c.resolve(None, None).unwrap().model, "glm-5.3-flash");
        c.keys.insert("openrouter".into(), "k".into());
        assert_eq!(c.resolve(Some("openrouter"), None).unwrap().model, "deepseek/deepseek-v4.1-flash", "the saved model is not carried to another provider");
    }

    #[test]
    fn custom_endpoints_and_round_trip_with_private_permissions() {
        let path = std::env::temp_dir().join(format!("presage-cfg-{}/config.json", crate::util::rand_hex(4)));
        let mut c = Config::default();
        c.keys.insert("logfare".into(), "secret".into());
        c.custom = Some(CustomProvider { base_url: "http://localhost:8000/v1".into(), dialect: Some("deepseek".into()), models: vec!["local-model".into()] });
        c.save(&path).unwrap();
        c.save(&path).unwrap();
        let back = Config::load(&path);
        assert_eq!(back.keys.get("logfare").map(String::as_str), Some("secret"));
        let r = back.resolve(Some("custom"), None).unwrap();
        assert_eq!((r.base_url.as_str(), r.model.as_str(), r.dialect), ("http://localhost:8000/v1", "local-model", Dialect::DeepSeek));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(Config::load(Path::new("/nonexistent/config.json")).keys.is_empty());
    }
}
