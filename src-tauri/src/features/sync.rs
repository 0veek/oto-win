use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, Snippet, StylePreset};
use crate::error::{OtoError, OtoResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDocument {
    pub version: u32,
    pub dictionary: Vec<String>,
    pub snippets: Vec<Snippet>,
    pub styles: Vec<StylePreset>,
}

impl From<&AppConfig> for SyncDocument {
    fn from(config: &AppConfig) -> Self {
        Self {
            version: 1,
            dictionary: config.dictionary.clone(),
            snippets: config.snippets.clone(),
            styles: config.styles.clone(),
        }
    }
}

fn validate_endpoint(endpoint: &str) -> OtoResult<()> {
    // Compare the parsed host, not a string prefix: "http://localhost.example.com"
    // starts with "http://localhost" yet is a remote host, which would have sent
    // the bearer token over plaintext.
    let url = reqwest::Url::parse(endpoint.trim())
        .map_err(|error| OtoError::Message(format!("Sync endpoint is not a valid URL: {error}")))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if crate::providers::openai_compat::is_loopback_url(&url) => Ok(()),
        _ => Err(OtoError::Message(
            "Sync endpoint must use HTTPS (HTTP is allowed only for localhost)".into(),
        )),
    }
}

fn merge_unique_strings(local: &mut Vec<String>, remote: Vec<String>) {
    for value in remote {
        if !local
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
        {
            local.push(value);
        }
    }
}

fn merge_by_id<T, F>(local: &mut Vec<T>, remote: Vec<T>, id: F)
where
    F: Fn(&T) -> &str,
{
    // Remote wins on id collision so edits synced from another machine apply.
    // Local-only items (ids not present remotely) are kept.
    for value in remote {
        if let Some(existing) = local.iter_mut().find(|existing| id(existing) == id(&value)) {
            *existing = value;
        } else {
            local.push(value);
        }
    }
}

pub fn merge_into(config: &mut AppConfig, remote: SyncDocument) {
    merge_unique_strings(&mut config.dictionary, remote.dictionary);
    merge_by_id(&mut config.snippets, remote.snippets, |value| &value.id);
    merge_by_id(&mut config.styles, remote.styles, |value| &value.id);
}

pub async fn sync(config: &mut AppConfig, token: Option<&str>) -> OtoResult<String> {
    let endpoint = config.sync.endpoint.trim().to_string();
    validate_endpoint(&endpoint)?;
    let client = reqwest::Client::new();
    let mut get = client.get(&endpoint);
    if let Some(token) = token.filter(|token| !token.trim().is_empty()) {
        get = get.bearer_auth(token);
    }
    let response = get.send().await?;
    if response.status().is_success() {
        let remote: SyncDocument = response.json().await?;
        merge_into(config, remote);
    } else if response.status() != reqwest::StatusCode::NOT_FOUND {
        return Err(OtoError::Message(format!(
            "Sync download failed with {}",
            response.status()
        )));
    }

    let document = SyncDocument::from(&*config);
    let mut put = client.put(&endpoint).json(&document);
    if let Some(token) = token.filter(|token| !token.trim().is_empty()) {
        put = put.bearer_auth(token);
    }
    put.send().await?.error_for_status()?;
    Ok(format!(
        "Synced {} dictionary terms, {} snippets, and {} styles",
        document.dictionary.len(),
        document.snippets.len(),
        document.styles.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_validation_requires_https_or_real_loopback() {
        assert!(validate_endpoint("https://example.com/oto.json").is_ok());
        assert!(validate_endpoint(" http://127.0.0.1:8080/oto.json ").is_ok());
        assert!(validate_endpoint("http://localhost:3000/oto.json").is_ok());
        // Prefix matching used to accept these over plaintext with a bearer token.
        assert!(validate_endpoint("http://localhost.example.com/oto.json").is_err());
        assert!(validate_endpoint("http://127.0.0.1.example.com/oto.json").is_err());
        assert!(validate_endpoint("http://example.com/oto.json").is_err());
        assert!(validate_endpoint("ftp://example.com/oto.json").is_err());
        assert!(validate_endpoint("").is_err());
    }

    #[test]
    fn merge_applies_remote_edits_and_adds_remote_items() {
        let mut config = AppConfig {
            dictionary: vec!["Oto".into()],
            snippets: vec![Snippet {
                id: "same".into(),
                trigger: "local".into(),
                expansion: "local".into(),
                enabled: true,
            }],
            ..AppConfig::default()
        };
        merge_into(
            &mut config,
            SyncDocument {
                version: 1,
                dictionary: vec!["oto".into(), "Tauri".into()],
                snippets: vec![
                    Snippet {
                        id: "same".into(),
                        trigger: "remote".into(),
                        expansion: "remote".into(),
                        enabled: true,
                    },
                    Snippet {
                        id: "new".into(),
                        trigger: "new".into(),
                        expansion: "new".into(),
                        enabled: true,
                    },
                ],
                styles: vec![],
            },
        );
        assert_eq!(config.dictionary, vec!["Oto", "Tauri"]);
        assert_eq!(config.snippets.len(), 2);
        assert_eq!(config.snippets[0].trigger, "remote");
        assert_eq!(config.snippets[0].expansion, "remote");
        assert_eq!(config.snippets[1].id, "new");
    }
}
