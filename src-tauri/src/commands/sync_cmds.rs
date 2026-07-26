use crate::config::{load_config, save_config, secrets};
use crate::error::{OtoError, OtoResult};
use crate::features::sync;

const SYNC_ACCOUNT: &str = "sync";

// Credential Manager calls are synchronous and can block while the vault is
// busy. Keep them off the IPC thread, matching the provider key commands.
#[tauri::command]
pub async fn set_sync_token(token: String) -> Result<(), OtoError> {
    tauri::async_runtime::spawn_blocking(move || secrets::set_api_key(SYNC_ACCOUNT, &token))
        .await
        .map_err(|error| OtoError::Message(format!("keyring task failed: {error}")))?
}

#[tauri::command]
pub async fn sync_token_present() -> bool {
    tauri::async_runtime::spawn_blocking(|| secrets::has_api_key(SYNC_ACCOUNT))
        .await
        .unwrap_or(false)
}

#[tauri::command]
pub async fn sync_now() -> Result<String, OtoError> {
    let (mut config, token) = tauri::async_runtime::spawn_blocking(|| -> OtoResult<_> {
        Ok((load_config()?, secrets::get_api_key(SYNC_ACCOUNT)?))
    })
    .await
    .map_err(|error| OtoError::Message(format!("sync setup task failed: {error}")))??;
    if !config.sync.enabled {
        return Err(OtoError::Message(
            "Enable sync and save an endpoint first".into(),
        ));
    }
    let summary = sync::sync(&mut config, token.as_deref()).await?;
    tauri::async_runtime::spawn_blocking(move || save_config(&config))
        .await
        .map_err(|error| OtoError::Message(format!("sync save task failed: {error}")))??;
    Ok(summary)
}
