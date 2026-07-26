use super::model::AppConfig;
use crate::error::{OtoError, OtoResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Serializes read-modify-write cycles inside this process. Several call sites
/// (settings save, overlay drag persistence, sync) load, mutate, and store the
/// config, and interleaving them would silently drop fields.
static CONFIG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn config_lock() -> &'static Mutex<()> {
    CONFIG_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn config_path() -> OtoResult<PathBuf> {
    let base = directories::ProjectDirs::from("dev", "Oto", "oto")
        .ok_or_else(|| OtoError::Message("could not resolve config dir".into()))?;
    Ok(base.config_dir().join("config.json"))
}

pub fn load_config() -> OtoResult<AppConfig> {
    let _guard = config_lock()
        .lock()
        .map_err(|_| OtoError::Message("config lock poisoned".into()))?;
    read_config_from(&config_path()?)
}

fn read_config_from(path: &Path) -> OtoResult<AppConfig> {
    if !path.exists() {
        // A missing file is the one true first run. Every other path — including
        // a config that predates the field — keeps `onboarding_complete: true`,
        // so upgrading never reopens the wizard.
        return Ok(AppConfig {
            onboarding_complete: false,
            ..AppConfig::default()
        });
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        // An interrupted write can leave a zero-length file; defaults beat a hard error.
        return Ok(AppConfig::default());
    }
    // Unknown/legacy fields (including any api_key written by older builds) are
    // ignored by serde; secrets only ever live in the OS keyring.
    Ok(serde_json::from_str(&raw)?)
}

/// Reject secret-looking top-level fields. Checking the serialized *keys* — not
/// the whole document — keeps ordinary user content ("api_key" as a dictionary
/// term or snippet body) from making the config unsavable.
fn assert_no_secret_fields(value: &serde_json::Value) -> OtoResult<()> {
    let Some(object) = value.as_object() else {
        return Ok(());
    };
    for key in object.keys() {
        let normalized = key.to_ascii_lowercase().replace(['_', '-'], "");
        if normalized.contains("apikey") || normalized.contains("secret") {
            return Err(OtoError::Message(format!(
                "refusing to write config that contains a secret field: {key}"
            )));
        }
    }
    Ok(())
}

pub fn save_config(cfg: &AppConfig) -> OtoResult<()> {
    let _guard = config_lock()
        .lock()
        .map_err(|_| OtoError::Message("config lock poisoned".into()))?;
    write_config_to(&config_path()?, cfg)
}

fn write_config_to(path: &Path, cfg: &AppConfig) -> OtoResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let value = serde_json::to_value(cfg)?;
    assert_no_secret_fields(&value)?;
    let raw = serde_json::to_string_pretty(&value)?;
    // Write-then-rename: a crash or a concurrent reader can never observe a
    // half-written config (which would silently reset every setting).
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, raw)?;
    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::*;

    #[test]
    fn default_roundtrip_json_has_no_api_key() {
        let cfg = AppConfig::default();
        let raw = serde_json::to_string(&cfg).unwrap();
        assert!(!raw.contains("api_key"));
        let back: AppConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn save_load_roundtrip_tmp() {
        let cfg = AppConfig {
            dictionary: vec!["Oto".into(), "Tauri".into()],
            polish_enabled: false,
            ..AppConfig::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_config_to(&path, &cfg).unwrap();
        let loaded = read_config_from(&path).unwrap();
        assert_eq!(loaded.dictionary, vec!["Oto", "Tauri"]);
        assert!(!loaded.polish_enabled);
        // Atomic write must not leave the staging file behind.
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn user_content_mentioning_api_key_still_saves() {
        let cfg = AppConfig {
            dictionary: vec!["api_key".into()],
            tone_hint: "Never spell out api_key".into(),
            ..AppConfig::default()
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        write_config_to(&path, &cfg).expect("user content is not a secret field");
        assert_eq!(read_config_from(&path).unwrap().dictionary, vec!["api_key"]);
    }

    #[test]
    fn secret_top_level_fields_are_rejected() {
        let value = serde_json::json!({ "hotkey": "Ctrl+Shift+Space", "api-key": "sk-live" });
        assert!(assert_no_secret_fields(&value).is_err());
        let clean = serde_json::to_value(AppConfig::default()).unwrap();
        assert!(assert_no_secret_fields(&clean).is_ok());
    }

    #[test]
    fn empty_or_missing_config_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("config.json");
        // Only the absent file is treated as a first run.
        let fresh = read_config_from(&missing).unwrap();
        assert!(!fresh.onboarding_complete);
        assert_eq!(
            fresh,
            AppConfig {
                onboarding_complete: false,
                ..AppConfig::default()
            }
        );
        // A truncated write is a damaged config, not a new install: sending the
        // user through onboarding again would be a confusing way to report it.
        fs::write(&missing, "   \n").unwrap();
        assert_eq!(read_config_from(&missing).unwrap(), AppConfig::default());
    }

    #[test]
    fn an_existing_config_never_reopens_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // A 0.1.0 document has no `onboarding_complete` at all.
        fs::write(&path, r#"{"hotkey":"Ctrl+Shift+Space"}"#).unwrap();
        assert!(read_config_from(&path).unwrap().onboarding_complete);
    }
}
