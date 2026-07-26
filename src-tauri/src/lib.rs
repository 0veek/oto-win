mod audio;
mod commands;
mod config;
mod error;
mod features;
mod hotkeys;
mod injection;
mod pipeline;
mod providers;
mod state;

use std::sync::Arc;

use config::{load_config, save_config, IdleBehavior};
use pipeline::orchestrator::{apply_overlay_scale, position_overlay};
use pipeline::Pipeline;
use state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, Runtime,
};

fn setup_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let start = MenuItem::with_id(
        app,
        "start_listening",
        "Start Listening",
        true,
        None::<&str>,
    )?;
    let stop = MenuItem::with_id(app, "stop_listening", "Stop Listening", true, None::<&str>)?;
    let command = MenuItem::with_id(
        app,
        "command_mode",
        "Command Mode (selected text)",
        true,
        None::<&str>,
    )?;
    let undo = MenuItem::with_id(
        app,
        "undo_insertion",
        "Undo Last Insertion",
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(app, "open_settings", "Open Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&start, &stop, &command, &undo, &open, &quit])?;

    let icon = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::FailedToReceiveMessage)?
        .clone();

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "start_listening" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let pipeline = state.pipeline.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = pipeline.ptt_down().await {
                            eprintln!("ptt_down (tray): {e}");
                        }
                    });
                }
            }
            "stop_listening" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let pipeline = state.pipeline.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = pipeline.ptt_up().await {
                            eprintln!("ptt_up (tray): {e}");
                        }
                    });
                }
            }
            "command_mode" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let pipeline = state.pipeline.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = pipeline.command_down(0).await {
                            eprintln!("command mode (tray): {e}");
                        }
                    });
                }
            }
            "undo_insertion" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let pipeline = state.pipeline.clone();
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        match pipeline.undo_last_insertion().await {
                            Ok(message) => eprintln!("oto undo: {message}"),
                            Err(error) => {
                                // Surface the refusal on the overlay: silently
                                // doing nothing reads as a broken menu item.
                                eprintln!("oto undo: {error}");
                                let _ = app.emit(
                                    "pipeline://event",
                                    pipeline::events::PipelineEvent::Error {
                                        message: error.to_string(),
                                    },
                                );
                            }
                        }
                    });
                }
            }
            "open_settings" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // Quiet tray launch when Windows starts Oto at login.
            Some(vec!["--from-autostart".into()]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::config_cmds::get_config,
            commands::config_cmds::set_config,
            commands::config_cmds::set_api_key,
            commands::config_cmds::api_key_present,
            commands::config_cmds::api_key_hint,
            commands::config_cmds::set_provider_api_key,
            commands::config_cmds::provider_api_key_present,
            commands::config_cmds::get_app_version,
            commands::config_cmds::autostart_active,
            commands::config_cmds::set_overlay_position,
            commands::config_cmds::list_audio_inputs,
            commands::config_cmds::preview_sound_cue,
            commands::config_cmds::probe_focused_window,
            commands::config_cmds::suggest_replacements,
            commands::config_cmds::add_replacement_rules,
            commands::history_cmds::get_history,
            commands::history_cmds::delete_history_entry,
            commands::history_cmds::clear_history,
            commands::history_cmds::get_usage_stats,
            commands::history_cmds::get_history_audio,
            commands::history_cmds::retranscribe_history,
            commands::history_cmds::reinsert_history,
            commands::audio_cmds::transcribe_audio_file,
            commands::history_cmds::copy_history_text,
            commands::pipeline_cmds::ptt_down,
            commands::pipeline_cmds::ptt_up,
            commands::pipeline_cmds::undo_last_insertion,
            commands::pipeline_cmds::start_command_mode,
            commands::pipeline_cmds::cancel_dictation,
            commands::pipeline_cmds::debug_preview_listening,
            commands::test_cmds::test_transcription,
            commands::test_cmds::test_injection,
            commands::test_cmds::test_microphone,
            commands::sync_cmds::set_sync_token,
            commands::sync_cmds::sync_token_present,
            commands::sync_cmds::sync_now,
        ])
        .setup(|app| {
            app.manage(hotkeys::HotkeyManager::default());
            let pipeline = Arc::new(Pipeline::new(app.handle().clone()));
            app.manage(AppState { pipeline });

            setup_tray(app.handle())?;

            let from_autostart = std::env::args().any(|arg| arg == "--from-autostart");

            if let Some(settings) = app.get_webview_window("settings") {
                let settings_for_event = settings.clone();
                settings.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = settings_for_event.hide();
                    }
                });
                // Login autostart should land in the tray only; open Settings from the tray.
                if !from_autostart {
                    let _ = settings.show();
                }
            }

            // Overlay: preload webview, restore position, persist user drags.
            if let Some(overlay) = app.get_webview_window("overlay") {
                // The pill is laid out in rem, so the window has to follow the
                // saved text-size preference or it clips at larger scales.
                if let Ok(cfg) = load_config() {
                    apply_overlay_scale(&overlay, cfg.font_scale);
                }
                position_overlay(&overlay);
                // Overlay is visual-only; if it accepts focus, injected keys land in Oto.
                let _ = overlay.set_focusable(false);

                // Warm the webview so the first PTT does not race a cold load.
                let _ = overlay.show();
                let overlay_hide = overlay.clone();
                let keep_minimal = load_config()
                    .map(|c| c.idle_behavior == IdleBehavior::Minimal)
                    .unwrap_or(false);
                if !keep_minimal {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(400));
                        let _ = overlay_hide.hide();
                    });
                }

                // Debounced position save on WindowEvent::Moved (ignore 0,0 noise).
                let save_pending = Arc::new(AtomicBool::new(false));
                let last_pos = Arc::new(std::sync::Mutex::new((0i32, 0i32)));
                overlay.on_window_event(move |event| {
                    if let tauri::WindowEvent::Moved(pos) = event {
                        if pos.x == 0 && pos.y == 0 {
                            return;
                        }
                        if let Ok(mut g) = last_pos.lock() {
                            *g = (pos.x, pos.y);
                        }
                        if save_pending
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                        {
                            let pending = Arc::clone(&save_pending);
                            let pos_slot = Arc::clone(&last_pos);
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_millis(400));
                                if let Ok(g) = pos_slot.lock() {
                                    let (x, y) = *g;
                                    if x == 0 && y == 0 {
                                        pending.store(false, Ordering::SeqCst);
                                        return;
                                    }
                                    if let Ok(mut cfg) = load_config() {
                                        cfg.overlay_x = Some(x);
                                        cfg.overlay_y = Some(y);
                                        let _ = save_config(&cfg);
                                    }
                                }
                                pending.store(false, Ordering::SeqCst);
                            });
                        }
                    }
                });
            }

            // Clear stale (0,0) overlay coords from earlier Moved bugs.
            if let Ok(mut cfg) = load_config() {
                if cfg.overlay_x == Some(0) && cfg.overlay_y == Some(0) {
                    cfg.overlay_x = None;
                    cfg.overlay_y = None;
                    let _ = save_config(&cfg);
                }
            }

            let (hotkey, load_error) = match load_config() {
                Ok(cfg) => (cfg.hotkey, None),
                Err(error) => ("Ctrl+Shift+Space".to_string(), Some(error)),
            };
            if let Some(error) = load_error {
                eprintln!("could not load config for hotkey: {error}");
            }
            let hotkey_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = hotkeys::register_ptt(&hotkey_app, &hotkey).await {
                    const FALLBACK_HOTKEY: &str = "Ctrl+Shift+Space";
                    eprintln!("hotkey registration failed for {hotkey}: {error}");

                    // A desktop shortcut can be reserved after Oto saved its config.
                    // Recover on the next launch instead of leaving PTT inactive.
                    if hotkey != FALLBACK_HOTKEY {
                        match hotkeys::register_ptt(&hotkey_app, FALLBACK_HOTKEY).await {
                            Ok(()) => {
                                if let Ok(mut cfg) = load_config() {
                                    cfg.hotkey = FALLBACK_HOTKEY.to_string();
                                    if let Err(save_error) = save_config(&cfg) {
                                        eprintln!(
                                            "fallback hotkey active but could not save config: {save_error}"
                                        );
                                    }
                                }
                                eprintln!(
                                    "hotkey fallback active: {FALLBACK_HOTKEY} (the saved {hotkey} shortcut is unavailable)"
                                );
                            }
                            Err(fallback_error) => eprintln!(
                                "fallback hotkey registration failed (use tray Start/Stop): {fallback_error}"
                            ),
                        }
                    } else {
                        eprintln!("use tray Start/Stop until the shortcut conflict is resolved");
                    }
                } else {
                    eprintln!("hotkey registered: {hotkey}");
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
