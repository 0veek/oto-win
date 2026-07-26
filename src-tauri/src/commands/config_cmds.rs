use crate::config::{load_config, save_config, secrets, AppConfig, IdleBehavior, ProviderPreset};
use crate::error::OtoError;
use crate::hotkeys;
use crate::pipeline::orchestrator::{apply_overlay_scale, position_overlay};
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

fn preset_account(p: &ProviderPreset) -> &'static str {
    crate::providers::presets::preset_account(p)
}

/// Add or remove Oto's Windows startup entry to match `enabled`.
///
/// Reads the current state first so re-saving unrelated settings does not
/// rewrite the registry on every save.
fn apply_autostart(app: &AppHandle, enabled: bool) -> Result<(), OtoError> {
    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    if current == enabled {
        return Ok(());
    }
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|error| OtoError::Message(format!("could not update Windows startup: {error}")))
}

#[tauri::command]
pub fn get_config() -> Result<AppConfig, OtoError> {
    load_config()
}

#[tauri::command]
pub async fn set_config(app: AppHandle, mut cfg: AppConfig) -> Result<(), OtoError> {
    // Normalize + re-register before saving so invalid hotkeys are rejected without writing.
    cfg.hotkey = hotkeys::normalize_hotkey(&cfg.hotkey);
    cfg.history_limit = cfg.history_limit.clamp(1, 1000);
    cfg.font_scale = cfg.font_scale.clamp(0.85, 1.25);
    cfg.temperature = cfg.temperature.clamp(0.0, 1.0);
    // Normalize the audio sections on the way in, so the stored document always
    // reflects what the pipeline will actually use.
    cfg.audio.input_gain = cfg.audio.effective_gain();
    cfg.audio.noise_gate_threshold = cfg.audio.effective_gate_threshold();
    cfg.vad.silence_ms = cfg.vad.effective_silence_ms();
    cfg.vad.min_speech_ms = cfg.vad.effective_min_speech_ms();
    cfg.sounds.volume = cfg.sounds.effective_volume();
    cfg.hybrid_tap_threshold_ms = cfg.hybrid_tap_threshold_ms.clamp(120, 2_000);
    if let Some(device) = cfg.audio.input_device.as_ref() {
        if device.trim().is_empty() {
            cfg.audio.input_device = None;
        }
    }
    // Drop a dangling active profile pointer so the runtime never reads missing models.
    if let Some(active_id) = cfg.active_custom_provider_id.as_deref() {
        if !cfg
            .custom_providers
            .iter()
            .any(|profile| profile.id == active_id)
        {
            cfg.active_custom_provider_id = None;
        }
    }
    if let Some(style_id) = cfg.active_style_id.as_deref() {
        if !cfg.styles.iter().any(|style| style.id == style_id) {
            cfg.active_style_id = None;
        }
    }

    // A rejected chord must not cost the user every other edit in the form
    // (theme, models, dictionary…). Keep the last chord that actually bound,
    // persist everything else, then report the hotkey failure.
    let previous_hotkey = load_config().ok().map(|saved| saved.hotkey);
    let hotkey_error = hotkeys::register_ptt(&app, &cfg.hotkey).await.err();
    if hotkey_error.is_some() {
        if let Some(previous) = previous_hotkey {
            eprintln!(
                "oto: keeping previously registered hotkey {previous} (rejected {})",
                cfg.hotkey
            );
            cfg.hotkey = previous;
        }
    }

    // Update the startup entry before writing config so a registry failure does
    // not leave the on-disk flag claiming something that did not happen.
    apply_autostart(&app, cfg.autostart_enabled)?;
    save_config(&cfg)?;
    eprintln!(
        "oto: config saved, hotkey active = {}, autostart = {}",
        cfg.hotkey, cfg.autostart_enabled
    );
    // Notify other webviews (overlay) so theme / scale / motion stay in sync.
    let _ = app.emit("config://changed", &cfg);
    // Apply idle appearance immediately when settings change.
    if let Some(overlay) = app.get_webview_window("overlay") {
        // The pill is sized in rem, so the window has to follow the text scale.
        apply_overlay_scale(&overlay, cfg.font_scale);
        if cfg.idle_behavior == IdleBehavior::Minimal {
            position_overlay(&overlay);
            let _ = overlay.show();
        } else if app
            .try_state::<AppState>()
            .map(|s| s.pipeline.is_idle())
            .unwrap_or(true)
        {
            // Hide while idle when switching to Hide; leave visible mid-dictation.
            let _ = overlay.hide();
        }
    }

    match hotkey_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Credential Manager calls are synchronous and can block while the vault is
/// busy. Run them off the main IPC thread so the settings UI stays responsive.
#[tauri::command]
pub async fn set_api_key(preset: ProviderPreset, key: String) -> Result<(), OtoError> {
    let account = preset_account(&preset).to_string();
    tauri::async_runtime::spawn_blocking(move || secrets::set_api_key(&account, &key))
        .await
        .map_err(|error| OtoError::Message(format!("keyring task failed: {error}")))?
}

#[tauri::command]
pub async fn api_key_present(preset: ProviderPreset) -> Result<bool, OtoError> {
    let account = preset_account(&preset).to_string();
    tauri::async_runtime::spawn_blocking(move || Ok(secrets::has_api_key(&account)))
        .await
        .map_err(|error| OtoError::Message(format!("keyring task failed: {error}")))?
}

#[tauri::command]
pub async fn api_key_hint(preset: ProviderPreset) -> Result<Option<String>, OtoError> {
    let account = preset_account(&preset).to_string();
    tauri::async_runtime::spawn_blocking(move || {
        Ok(secrets::get_api_key(&account)?.map(|k| {
            let chars: Vec<char> = k.chars().collect();
            if chars.len() <= 8 {
                "••••".into()
            } else {
                let head: String = chars.iter().take(4).collect();
                let tail: String = chars
                    .iter()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                format!("{head}…{tail}")
            }
        }))
    })
    .await
    .map_err(|error| OtoError::Message(format!("keyring task failed: {error}")))?
}

#[tauri::command]
pub async fn set_provider_api_key(account: String, key: String) -> Result<(), OtoError> {
    tauri::async_runtime::spawn_blocking(move || {
        secrets::validate_account(&account)?;
        secrets::set_api_key(&account, &key)
    })
    .await
    .map_err(|error| OtoError::Message(format!("keyring task failed: {error}")))?
}

#[tauri::command]
pub async fn provider_api_key_present(account: String) -> Result<bool, OtoError> {
    tauri::async_runtime::spawn_blocking(move || {
        secrets::validate_account(&account)?;
        Ok(secrets::has_api_key(&account))
    })
    .await
    .map_err(|error| OtoError::Message(format!("keyring task failed: {error}")))?
}

/// Cargo package version shown in About.
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Whether Windows is currently configured to launch Oto at login.
///
/// Read from the registry rather than from config, so the toggle reflects
/// reality even when the entry was removed by other means — Task Manager's
/// Startup tab, or a reinstall to a different path.
#[tauri::command]
pub fn autostart_active(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// Microphones available for the input-device picker.
#[tauri::command]
pub async fn list_audio_inputs() -> Result<Vec<crate::audio::InputDevice>, OtoError> {
    // Device enumeration opens WASAPI endpoints and can block; keep it off the
    // IPC thread so the settings window never freezes on a wedged audio device.
    tauri::async_runtime::spawn_blocking(crate::audio::list_input_devices)
        .await
        .map_err(|error| OtoError::Message(format!("device enumeration failed: {error}")))?
}

/// Play one cue so the user can audition volume and tone while adjusting them.
#[tauri::command]
pub fn preview_sound_cue(cue: String, config: crate::config::SoundConfig) -> Result<(), OtoError> {
    let cue = match cue.as_str() {
        "start" => crate::audio::Cue::Start,
        "stop" => crate::audio::Cue::Stop,
        "done" => crate::audio::Cue::Done,
        "error" => crate::audio::Cue::Error,
        other => return Err(OtoError::Message(format!("unknown cue: {other}"))),
    };
    // Auditioning has to ignore the per-cue toggles, otherwise the preview
    // button for a disabled cue would do nothing and look broken.
    let config = crate::config::SoundConfig {
        enabled: true,
        on_start: true,
        on_stop: true,
        on_done: true,
        on_error: true,
        ..config
    };
    crate::audio::cues::play(cue, &config);
    Ok(())
}

/// Report the focused window as Oto sees it, for writing Mode match rules.
///
/// Also shows what context would be sent at the current disclosure level, so
/// the privacy setting can be verified rather than trusted.
#[tauri::command]
pub async fn probe_focused_window() -> Result<String, OtoError> {
    let cfg = load_config()?;
    let target = crate::injection::capture_focus_target_async().await;
    let context =
        crate::pipeline::context::build(cfg.context_level, &target, &cfg.context_blocklist, None);
    let class = target.class.as_deref().unwrap_or("(unknown)");
    let title = target.title.as_deref().unwrap_or("(unknown)");
    Ok(format!(
        "class: {class}\ntitle: {title}\n\nContext that would be sent:\n{}",
        context.preview()
    ))
}

/// Infer replacement rules from a transcript the user corrected by hand.
///
/// Returns candidates only — nothing is saved until the user accepts them.
#[tauri::command]
pub fn suggest_replacements(
    raw: String,
    corrected: String,
) -> Vec<crate::features::replacements::ReplacementSuggestion> {
    crate::features::replacements::suggest_replacements(&raw, &corrected)
}

/// Append accepted rules to the stored configuration.
///
/// Writes through `load`/`save` rather than the settings form so accepting a
/// suggestion cannot clobber unrelated edits the user has open elsewhere.
#[tauri::command]
pub fn add_replacement_rules(
    rules: Vec<crate::config::ReplacementRule>,
) -> Result<usize, OtoError> {
    let mut cfg = load_config()?;
    let mut added = 0usize;
    for rule in rules {
        if rule.from.trim().is_empty() {
            continue;
        }
        // Re-adding a rule for the same word would silently shadow the first.
        if cfg
            .replacements
            .iter()
            .any(|existing| existing.from.eq_ignore_ascii_case(rule.from.trim()))
        {
            continue;
        }
        cfg.replacements.push(rule);
        added += 1;
    }
    if added > 0 {
        save_config(&cfg)?;
    }
    Ok(added)
}

/// Persist overlay window coordinates (physical pixels).
#[tauri::command]
pub fn set_overlay_position(x: i32, y: i32) -> Result<(), OtoError> {
    let mut cfg = load_config()?;
    cfg.overlay_x = Some(x);
    cfg.overlay_y = Some(y);
    save_config(&cfg)
}
