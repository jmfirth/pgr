//! `LESS_TERMCAP` environment variable overrides for terminal capabilities.
//!
//! GNU less supports `LESS_TERMCAP_xx` environment variables that override
//! terminal capabilities. The most common use case is colored man pages,
//! where shells set these variables to inject ANSI color codes for bold,
//! underline, standout, and other text attributes.

use std::env;

/// Terminal capability names recognized by `LESS_TERMCAP_xx` variables.
const CAPABILITY_NAMES: &[&str] = &["md", "me", "us", "ue", "so", "se", "mb", "mr"];

/// Overrides for terminal capabilities read from `LESS_TERMCAP_xx` environment variables.
///
/// Each field corresponds to a termcap capability:
/// - `md` — start bold
/// - `me` — end bold (and all attributes)
/// - `us` — start underline
/// - `ue` — end underline
/// - `so` — start standout (reverse video)
/// - `se` — end standout
/// - `mb` — start blink
/// - `mr` — start reverse
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermcapOverrides {
    /// Start bold mode (`LESS_TERMCAP_md`).
    pub md: Option<String>,
    /// End bold/all-attributes mode (`LESS_TERMCAP_me`).
    pub me: Option<String>,
    /// Start underline mode (`LESS_TERMCAP_us`).
    pub us: Option<String>,
    /// End underline mode (`LESS_TERMCAP_ue`).
    pub ue: Option<String>,
    /// Start standout (reverse video) mode (`LESS_TERMCAP_so`).
    pub so: Option<String>,
    /// End standout mode (`LESS_TERMCAP_se`).
    pub se: Option<String>,
    /// Start blink mode (`LESS_TERMCAP_mb`).
    pub mb: Option<String>,
    /// Start reverse mode (`LESS_TERMCAP_mr`).
    pub mr: Option<String>,
}

impl TermcapOverrides {
    /// Read all `LESS_TERMCAP_xx` overrides from the current environment.
    ///
    /// Empty environment variable values are treated as unset (returns `None`
    /// for that capability).
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            md: read_termcap_var("md"),
            me: read_termcap_var("me"),
            us: read_termcap_var("us"),
            ue: read_termcap_var("ue"),
            so: read_termcap_var("so"),
            se: read_termcap_var("se"),
            mb: read_termcap_var("mb"),
            mr: read_termcap_var("mr"),
        }
    }

    /// Look up an override for the given capability name.
    ///
    /// Returns the override value if the corresponding `LESS_TERMCAP_xx`
    /// variable was set (and non-empty), or `None` otherwise.
    ///
    /// Valid capability names: `md`, `me`, `us`, `ue`, `so`, `se`, `mb`, `mr`.
    #[must_use]
    pub fn override_capability(&self, name: &str) -> Option<&str> {
        match name {
            "md" => self.md.as_deref(),
            "me" => self.me.as_deref(),
            "us" => self.us.as_deref(),
            "ue" => self.ue.as_deref(),
            "so" => self.so.as_deref(),
            "se" => self.se.as_deref(),
            "mb" => self.mb.as_deref(),
            "mr" => self.mr.as_deref(),
            _ => None,
        }
    }

    /// Returns `true` if no overrides are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.md.is_none()
            && self.me.is_none()
            && self.us.is_none()
            && self.ue.is_none()
            && self.so.is_none()
            && self.se.is_none()
            && self.mb.is_none()
            && self.mr.is_none()
    }

    /// Returns the list of recognized capability names.
    #[must_use]
    pub fn capability_names() -> &'static [&'static str] {
        CAPABILITY_NAMES
    }
}

/// Read a single `LESS_TERMCAP_xx` variable from the environment.
/// Returns `None` if the variable is missing or empty.
fn read_termcap_var(suffix: &str) -> Option<String> {
    let var_name = format!("LESS_TERMCAP_{suffix}");
    match env::var(&var_name) {
        Ok(val) if !val.is_empty() => Some(val),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Environment variable tests must be serialized since env vars are process-global.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Helper to run a closure with specific env vars set, cleaning up afterward.
    fn with_env_vars<F, R>(vars: &[(&str, &str)], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_LOCK.lock();
        // Set vars
        for (key, value) in vars {
            env::set_var(key, value);
        }
        let result = f();
        // Clean up
        for (key, _) in vars {
            env::remove_var(key);
        }
        result
    }

    /// Helper to run a closure with specific env vars removed.
    fn without_env_vars<F, R>(vars: &[&str], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ENV_LOCK.lock();
        // Remove vars
        for key in vars {
            env::remove_var(key);
        }
        let result = f();
        result
    }

    #[test]
    fn test_from_env_reads_all_termcap_variables() {
        let overrides = with_env_vars(
            &[
                ("LESS_TERMCAP_md", "\x1b[1;31m"),
                ("LESS_TERMCAP_me", "\x1b[0m"),
                ("LESS_TERMCAP_us", "\x1b[4;32m"),
                ("LESS_TERMCAP_ue", "\x1b[0m"),
                ("LESS_TERMCAP_so", "\x1b[7m"),
                ("LESS_TERMCAP_se", "\x1b[0m"),
                ("LESS_TERMCAP_mb", "\x1b[5m"),
                ("LESS_TERMCAP_mr", "\x1b[7m"),
            ],
            TermcapOverrides::from_env,
        );

        assert_eq!(overrides.md.as_deref(), Some("\x1b[1;31m"));
        assert_eq!(overrides.me.as_deref(), Some("\x1b[0m"));
        assert_eq!(overrides.us.as_deref(), Some("\x1b[4;32m"));
        assert_eq!(overrides.ue.as_deref(), Some("\x1b[0m"));
        assert_eq!(overrides.so.as_deref(), Some("\x1b[7m"));
        assert_eq!(overrides.se.as_deref(), Some("\x1b[0m"));
        assert_eq!(overrides.mb.as_deref(), Some("\x1b[5m"));
        assert_eq!(overrides.mr.as_deref(), Some("\x1b[7m"));
    }

    #[test]
    fn test_from_env_missing_variables_return_none() {
        let overrides = without_env_vars(
            &[
                "LESS_TERMCAP_md",
                "LESS_TERMCAP_me",
                "LESS_TERMCAP_us",
                "LESS_TERMCAP_ue",
                "LESS_TERMCAP_so",
                "LESS_TERMCAP_se",
                "LESS_TERMCAP_mb",
                "LESS_TERMCAP_mr",
            ],
            TermcapOverrides::from_env,
        );

        assert_eq!(overrides, TermcapOverrides::default());
    }

    #[test]
    fn test_from_env_empty_variables_treated_as_unset() {
        let overrides = with_env_vars(
            &[
                ("LESS_TERMCAP_md", ""),
                ("LESS_TERMCAP_us", ""),
                ("LESS_TERMCAP_so", ""),
            ],
            TermcapOverrides::from_env,
        );

        assert!(overrides.md.is_none());
        assert!(overrides.us.is_none());
        assert!(overrides.so.is_none());
    }

    #[test]
    fn test_from_env_partial_variables_set() {
        let overrides = with_env_vars(
            &[
                ("LESS_TERMCAP_md", "\x1b[1m"),
                ("LESS_TERMCAP_me", "\x1b[0m"),
            ],
            || {
                // Make sure the others are unset
                env::remove_var("LESS_TERMCAP_us");
                env::remove_var("LESS_TERMCAP_ue");
                env::remove_var("LESS_TERMCAP_so");
                env::remove_var("LESS_TERMCAP_se");
                env::remove_var("LESS_TERMCAP_mb");
                env::remove_var("LESS_TERMCAP_mr");
                TermcapOverrides::from_env()
            },
        );

        assert_eq!(overrides.md.as_deref(), Some("\x1b[1m"));
        assert_eq!(overrides.me.as_deref(), Some("\x1b[0m"));
        assert!(overrides.us.is_none());
        assert!(overrides.ue.is_none());
        assert!(overrides.so.is_none());
        assert!(overrides.se.is_none());
        assert!(overrides.mb.is_none());
        assert!(overrides.mr.is_none());
    }

    #[test]
    fn test_override_capability_returns_correct_values() {
        let overrides = TermcapOverrides {
            md: Some("\x1b[1;31m".to_string()),
            me: Some("\x1b[0m".to_string()),
            us: Some("\x1b[4;32m".to_string()),
            ue: Some("\x1b[24m".to_string()),
            so: Some("\x1b[7m".to_string()),
            se: Some("\x1b[27m".to_string()),
            mb: Some("\x1b[5m".to_string()),
            mr: Some("\x1b[7m".to_string()),
        };

        assert_eq!(overrides.override_capability("md"), Some("\x1b[1;31m"));
        assert_eq!(overrides.override_capability("me"), Some("\x1b[0m"));
        assert_eq!(overrides.override_capability("us"), Some("\x1b[4;32m"));
        assert_eq!(overrides.override_capability("ue"), Some("\x1b[24m"));
        assert_eq!(overrides.override_capability("so"), Some("\x1b[7m"));
        assert_eq!(overrides.override_capability("se"), Some("\x1b[27m"));
        assert_eq!(overrides.override_capability("mb"), Some("\x1b[5m"));
        assert_eq!(overrides.override_capability("mr"), Some("\x1b[7m"));
    }

    #[test]
    fn test_override_capability_none_fields_return_none() {
        let overrides = TermcapOverrides::default();

        assert!(overrides.override_capability("md").is_none());
        assert!(overrides.override_capability("me").is_none());
        assert!(overrides.override_capability("us").is_none());
        assert!(overrides.override_capability("ue").is_none());
        assert!(overrides.override_capability("so").is_none());
        assert!(overrides.override_capability("se").is_none());
        assert!(overrides.override_capability("mb").is_none());
        assert!(overrides.override_capability("mr").is_none());
    }

    #[test]
    fn test_override_capability_unknown_name_returns_none() {
        let overrides = TermcapOverrides {
            md: Some("value".to_string()),
            ..TermcapOverrides::default()
        };

        assert!(overrides.override_capability("xx").is_none());
        assert!(overrides.override_capability("").is_none());
        assert!(overrides.override_capability("MD").is_none());
        assert!(overrides.override_capability("bold").is_none());
    }

    #[test]
    fn test_is_empty_default_returns_true() {
        let overrides = TermcapOverrides::default();
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_is_empty_with_any_field_set_returns_false() {
        let test_cases = vec![
            TermcapOverrides {
                md: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                me: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                us: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                ue: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                so: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                se: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                mb: Some("x".to_string()),
                ..Default::default()
            },
            TermcapOverrides {
                mr: Some("x".to_string()),
                ..Default::default()
            },
        ];

        for (i, tc) in test_cases.iter().enumerate() {
            assert!(!tc.is_empty(), "test case {i} should not be empty");
        }
    }

    #[test]
    fn test_capability_names_returns_all_recognized_names() {
        let names = TermcapOverrides::capability_names();
        assert_eq!(names, &["md", "me", "us", "ue", "so", "se", "mb", "mr"]);
    }

    #[test]
    fn test_read_termcap_var_with_value() {
        let _guard = ENV_LOCK.lock();
        env::set_var("LESS_TERMCAP_md", "\x1b[1m");
        let result = read_termcap_var("md");
        env::remove_var("LESS_TERMCAP_md");
        assert_eq!(result, Some("\x1b[1m".to_string()));
    }

    #[test]
    fn test_read_termcap_var_missing() {
        let _guard = ENV_LOCK.lock();
        env::remove_var("LESS_TERMCAP_zz");
        let result = read_termcap_var("zz");
        assert!(result.is_none());
    }

    #[test]
    fn test_read_termcap_var_empty() {
        let _guard = ENV_LOCK.lock();
        env::set_var("LESS_TERMCAP_md", "");
        let result = read_termcap_var("md");
        env::remove_var("LESS_TERMCAP_md");
        assert!(result.is_none());
    }

    #[test]
    fn test_termcap_overrides_clone_produces_equal_value() {
        let overrides = TermcapOverrides {
            md: Some("\x1b[1m".to_string()),
            me: Some("\x1b[0m".to_string()),
            ..Default::default()
        };

        let cloned = overrides.clone();
        assert_eq!(overrides, cloned);
    }

    #[test]
    fn test_termcap_overrides_debug_format() {
        let overrides = TermcapOverrides::default();
        let debug = format!("{overrides:?}");
        assert!(debug.contains("TermcapOverrides"));
    }
}
