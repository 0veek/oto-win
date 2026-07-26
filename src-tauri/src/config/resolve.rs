//! Effective-configuration resolution.
//!
//! Every pipeline decision reads a [`ResolvedConfig`] rather than [`AppConfig`]
//! directly, so a per-application Mode can change the provider, the model, the
//! prompt, or where the text goes without any call site knowing Modes exist.

use super::model::AppConfig;

/// The focused window, described in terms `config` can match against without
/// depending on the `injection` module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppContext {
    /// Application class / WM_CLASS / app-id, lowercased by the producer.
    pub app_class: Option<String>,
    pub window_title: Option<String>,
}

impl AppContext {
    pub fn new(app_class: Option<String>, window_title: Option<String>) -> Self {
        Self {
            app_class: app_class
                .map(|c| c.trim().to_ascii_lowercase())
                .filter(|c| !c.is_empty()),
            window_title: window_title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
        }
    }
}

/// Configuration for one dictation session, with any matching Mode folded in.
///
/// `cfg` is a complete [`AppConfig`], so existing helpers that take `&AppConfig`
/// keep working unchanged and automatically observe the override.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub cfg: AppConfig,
    /// `None` when the built-in default (the flat config) applied.
    pub mode_id: Option<String>,
    pub mode_name: String,
}

impl ResolvedConfig {
    /// The unmodified global configuration.
    pub fn global(cfg: AppConfig) -> Self {
        Self {
            cfg,
            mode_id: None,
            mode_name: "Default".to_string(),
        }
    }
}

impl std::ops::Deref for ResolvedConfig {
    type Target = AppConfig;

    fn deref(&self) -> &Self::Target {
        &self.cfg
    }
}

impl AppConfig {
    /// Effective configuration for a session started against `context`.
    ///
    /// Modes are evaluated in the order the user arranged them and the first
    /// match wins, so overlapping rules resolve predictably from the list.
    pub fn resolve(&self, context: Option<&AppContext>) -> ResolvedConfig {
        let Some(context) = context else {
            return ResolvedConfig::global(self.clone());
        };

        let matched = self.modes.iter().find(|mode| {
            mode.matches(
                context.app_class.as_deref(),
                context.window_title.as_deref(),
            )
        });

        match matched {
            Some(mode) => {
                let mut cfg = self.clone();
                mode.apply(&mut cfg);
                ResolvedConfig {
                    cfg,
                    mode_id: Some(mode.id.clone()),
                    mode_name: mode.name.clone(),
                }
            }
            None => ResolvedConfig::global(self.clone()),
        }
    }

    /// Configuration as if `mode_id` had matched, for a mode invoked by its own
    /// hotkey rather than by the focused window.
    pub fn resolve_mode(&self, mode_id: &str) -> ResolvedConfig {
        match self.modes.iter().find(|mode| mode.id == mode_id) {
            Some(mode) => {
                let mut cfg = self.clone();
                mode.apply(&mut cfg);
                ResolvedConfig {
                    cfg,
                    mode_id: Some(mode.id.clone()),
                    mode_name: mode.name.clone(),
                }
            }
            None => ResolvedConfig::global(self.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::model::{ContextLevel, Mode, ModeMatch, SttBackend};

    fn mode_for(id: &str, classes: &[&str]) -> Mode {
        Mode {
            match_rule: ModeMatch {
                app_classes: classes.iter().map(|c| c.to_string()).collect(),
                title_contains: String::new(),
            },
            ..Mode::new(id.into(), id.into())
        }
    }

    #[test]
    fn resolution_without_modes_is_the_global_config() {
        let cfg = AppConfig {
            stt_model: "nova-3".into(),
            ..AppConfig::default()
        };
        let resolved = cfg.resolve(None);
        assert_eq!(resolved.cfg, cfg);
        assert!(resolved.mode_id.is_none());
        // Deref keeps call sites reading like the plain config.
        assert_eq!(resolved.stt_model, "nova-3");
    }

    #[test]
    fn a_matching_mode_overrides_only_what_it_sets() {
        let cfg = AppConfig {
            polish_enabled: true,
            stt_model: "nova-3".into(),
            tone_hint: "global tone".into(),
            modes: vec![Mode {
                polish_enabled: Some(false),
                ..mode_for("terminal", &["WindowsTerminal", "powershell"])
            }],
            ..AppConfig::default()
        };

        let ctx = AppContext::new(Some("WindowsTerminal".into()), None);
        let resolved = cfg.resolve(Some(&ctx));
        assert_eq!(resolved.mode_id.as_deref(), Some("terminal"));
        assert!(!resolved.polish_enabled, "the mode disabled polish");
        // Untouched fields still come from the global config.
        assert_eq!(resolved.stt_model, "nova-3");
        assert_eq!(resolved.tone_hint, "global tone");
    }

    #[test]
    fn a_non_matching_window_falls_back_to_global() {
        let cfg = AppConfig {
            modes: vec![Mode {
                polish_enabled: Some(false),
                ..mode_for("terminal", &["WindowsTerminal"])
            }],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("firefox".into()), None);
        let resolved = cfg.resolve(Some(&ctx));
        assert!(resolved.mode_id.is_none());
        assert!(resolved.polish_enabled);
    }

    #[test]
    fn the_first_matching_mode_wins() {
        let cfg = AppConfig {
            modes: vec![
                Mode {
                    tone_hint: Some("first".into()),
                    ..mode_for("a", &["code"])
                },
                Mode {
                    tone_hint: Some("second".into()),
                    ..mode_for("b", &["code"])
                },
            ],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("code".into()), None);
        assert_eq!(cfg.resolve(Some(&ctx)).tone_hint, "first");
    }

    #[test]
    fn a_disabled_mode_is_skipped() {
        let cfg = AppConfig {
            modes: vec![
                Mode {
                    enabled: false,
                    tone_hint: Some("disabled".into()),
                    ..mode_for("a", &["code"])
                },
                Mode {
                    tone_hint: Some("enabled".into()),
                    ..mode_for("b", &["code"])
                },
            ],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("code".into()), None);
        assert_eq!(cfg.resolve(Some(&ctx)).tone_hint, "enabled");
    }

    #[test]
    fn mode_dictionary_extends_rather_than_replaces() {
        let cfg = AppConfig {
            dictionary: vec!["Oto".into(), "Tauri".into()],
            modes: vec![Mode {
                dictionary: vec!["kubectl".into(), "Oto".into()],
                ..mode_for("terminal", &["WindowsTerminal"])
            }],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("WindowsTerminal".into()), None);
        let resolved = cfg.resolve(Some(&ctx));
        assert_eq!(resolved.dictionary, vec!["Oto", "Tauri", "kubectl"]);
    }

    #[test]
    fn switching_preset_also_switches_the_base_url() {
        // Inheriting a base URL across providers would send a Deepgram key to
        // an OpenAI host and fail with a confusing 401.
        let cfg = AppConfig {
            provider_preset: crate::config::ProviderPreset::Groq,
            base_url: "https://api.groq.com/openai/v1".into(),
            modes: vec![Mode {
                provider_preset: Some(crate::config::ProviderPreset::Deepgram),
                ..mode_for("fast", &["slack"])
            }],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("slack".into()), None);
        let resolved = cfg.resolve(Some(&ctx));
        assert_eq!(resolved.base_url, "https://api.deepgram.com");
    }

    #[test]
    fn a_mode_with_no_rule_never_matches_automatically() {
        // Such a mode is hotkey-only. Matching everything would shadow the
        // global config for every window.
        let cfg = AppConfig {
            modes: vec![Mode {
                hotkey: "Ctrl+Shift+J".into(),
                tone_hint: Some("hotkey only".into()),
                ..Mode::new("manual".into(), "Manual".into())
            }],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("anything".into()), Some("any title".into()));
        assert!(cfg.resolve(Some(&ctx)).mode_id.is_none());
        // …but it still resolves when invoked directly.
        assert_eq!(cfg.resolve_mode("manual").tone_hint, "hotkey only");
    }

    #[test]
    fn resolve_mode_falls_back_to_global_for_an_unknown_id() {
        let cfg = AppConfig::default();
        assert!(cfg.resolve_mode("nope").mode_id.is_none());
    }

    #[test]
    fn mode_can_pin_a_local_backend_and_silence_context() {
        let cfg = AppConfig {
            stt_backend: SttBackend::Cloud,
            context_level: ContextLevel::Window,
            modes: vec![Mode {
                stt_backend: Some(SttBackend::LocalWhisper),
                context_level: Some(ContextLevel::None),
                ..mode_for("private", &["keepassxc"])
            }],
            ..AppConfig::default()
        };
        let ctx = AppContext::new(Some("keepassxc".into()), None);
        let resolved = cfg.resolve(Some(&ctx));
        assert_eq!(resolved.stt_backend, SttBackend::LocalWhisper);
        assert_eq!(resolved.context_level, ContextLevel::None);
    }

    #[test]
    fn app_context_normalizes_class_and_drops_blanks() {
        let ctx = AppContext::new(Some("  Firefox  ".into()), Some("   ".into()));
        assert_eq!(ctx.app_class.as_deref(), Some("firefox"));
        assert!(ctx.window_title.is_none());
    }
}
