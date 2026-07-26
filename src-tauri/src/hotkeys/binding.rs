//! Which chords Oto wants Windows to deliver.
//!
//! Oto binds one chord for ordinary dictation plus, optionally, one per Mode.
//! Keeping the desired set separate from the registration backend means the
//! part that actually has a decision to make — what to bind, and what to refuse
//! because it would be ambiguous — is ordinary testable code.

use crate::config::AppConfig;

/// Shortcut id used for the primary dictation binding.
pub const PRIMARY_ID: &str = "dictation";

/// One chord Oto wants the system to deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// Stable identifier: `dictation`, or `mode:<mode id>`.
    pub id: String,
    /// Normalized chord, e.g. `Ctrl+Shift+Space`.
    pub hotkey: String,
    /// `None` for the primary binding; otherwise the Mode to start under.
    pub mode_id: Option<String>,
    /// Human-readable description, used in logs and diagnostics.
    pub label: String,
}

impl Binding {
    pub fn primary(hotkey: String) -> Self {
        Self {
            id: PRIMARY_ID.to_string(),
            hotkey,
            mode_id: None,
            label: "Start or stop Oto dictation".to_string(),
        }
    }
}

/// Every chord Oto should hold, primary first.
///
/// Modes without a chord, and modes whose chord collides with one already in the
/// list, are skipped: two bindings for the same keys would make which Mode runs
/// depend on registration order.
pub fn desired_bindings(cfg: &AppConfig, normalize: impl Fn(&str) -> String) -> Vec<Binding> {
    let primary = normalize(&cfg.hotkey);
    let mut bindings = vec![Binding::primary(primary.clone())];
    let mut claimed = vec![primary];

    for mode in &cfg.modes {
        if !mode.enabled {
            continue;
        }
        let chord = normalize(&mode.hotkey);
        if chord.is_empty() {
            continue;
        }
        if claimed.iter().any(|existing| existing == &chord) {
            eprintln!(
                "oto hotkey: mode '{}' wants {chord}, which is already bound — skipping",
                mode.name
            );
            continue;
        }
        claimed.push(chord.clone());
        bindings.push(Binding {
            id: format!("mode:{}", mode.id),
            hotkey: chord,
            mode_id: Some(mode.id.clone()),
            label: format!("Oto dictation — {}", mode.name),
        });
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mode;

    /// Stand-in for `hotkeys::normalize_hotkey`, so these tests do not depend on
    /// the parser they are not exercising. Like the real one, it splits on `+`
    /// and drops the spacing — collision detection compares normalized chords,
    /// so `ctrl + alt + c` has to land on `CTRL+ALT+C`.
    fn upper(value: &str) -> String {
        value
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_ascii_uppercase)
            .collect::<Vec<_>>()
            .join("+")
    }

    fn cfg_with_modes(modes: Vec<Mode>) -> AppConfig {
        AppConfig {
            hotkey: "Ctrl+Shift+Space".into(),
            modes,
            ..AppConfig::default()
        }
    }

    fn mode(id: &str, hotkey: &str) -> Mode {
        Mode {
            hotkey: hotkey.into(),
            ..Mode::new(id.into(), id.into())
        }
    }

    #[test]
    fn without_modes_only_the_primary_chord_is_wanted() {
        let bindings = desired_bindings(&cfg_with_modes(vec![]), upper);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].id, PRIMARY_ID);
        assert_eq!(bindings[0].hotkey, "CTRL+SHIFT+SPACE");
        assert!(bindings[0].mode_id.is_none());
    }

    #[test]
    fn a_mode_with_a_chord_gets_its_own_binding() {
        let bindings = desired_bindings(&cfg_with_modes(vec![mode("chat", "Ctrl+Alt+C")]), upper);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[1].id, "mode:chat");
        assert_eq!(bindings[1].mode_id.as_deref(), Some("chat"));
    }

    #[test]
    fn modes_without_a_chord_are_window_matched_only() {
        let bindings = desired_bindings(
            &cfg_with_modes(vec![mode("chat", ""), mode("email", "   ")]),
            upper,
        );
        assert_eq!(bindings.len(), 1, "no chord means no binding");
    }

    #[test]
    fn a_disabled_mode_is_not_bound() {
        let mut disabled = mode("chat", "Ctrl+Alt+C");
        disabled.enabled = false;
        let bindings = desired_bindings(&cfg_with_modes(vec![disabled]), upper);
        assert_eq!(bindings.len(), 1);
    }

    #[test]
    fn a_chord_that_collides_is_refused_rather_than_registered_twice() {
        // Two bindings on one chord would make the winner depend on which one
        // the backend happened to register last.
        let bindings = desired_bindings(
            &cfg_with_modes(vec![
                mode("chat", "Ctrl+Alt+C"),
                mode("email", "ctrl + alt + c"),
            ]),
            upper,
        );
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[1].id, "mode:chat", "first match wins");
    }

    #[test]
    fn a_mode_cannot_steal_the_primary_chord() {
        let bindings = desired_bindings(
            &cfg_with_modes(vec![mode("chat", "Ctrl+Shift+Space")]),
            upper,
        );
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].id, PRIMARY_ID);
    }
}
