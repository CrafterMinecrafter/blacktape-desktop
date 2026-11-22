use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use sentry::{protocol::Event, Breadcrumb, ClientInitGuard, ClientOptions, Level};

pub const TELEMETRY_SETTING_KEY: &str = "telemetry_enabled";

static TELEMETRY_ENABLED: AtomicBool = AtomicBool::new(true);
static INSTALL_ID: OnceLock<String> = OnceLock::new();

pub fn set_enabled(enabled: bool) {
    TELEMETRY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    TELEMETRY_ENABLED.load(Ordering::Relaxed)
}

/// Loads or creates a random UUIDv4 in `config_dir/install_id` (no hardware or IP fingerprinting).
pub fn init_install_id(config_dir: &Path) {
    let id_path = config_dir.join("install_id");
    let id = std::fs::read_to_string(&id_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let new_id = uuid::Uuid::new_v4().to_string();
            let _ = std::fs::create_dir_all(config_dir);
            let _ = std::fs::write(&id_path, &new_id);
            new_id
        });
    let _ = INSTALL_ID.set(id);
}

fn scrub_home_dir(text: &str) -> String {
    let mut cleaned = text.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var) {
            if !home.is_empty() {
                cleaned = cleaned.replace(&home, "<HOME>");
                // Also catch escaped Windows backslashes in JSON/debug strings
                let escaped = home.replace('\\', "\\\\");
                cleaned = cleaned.replace(&escaped, "<HOME>");
            }
        }
    }
    cleaned
}

fn sanitize_event(mut event: Event<'static>) -> Option<Event<'static>> {
    if !is_enabled() {
        return None;
    }

    // Strip machine hostname and any accidental user identifiers
    event.server_name = None;
    let anon_id = INSTALL_ID.get().cloned();
    event.user = Some(sentry::User {
        id: anon_id,
        ..Default::default()
    });

    if let Some(ref mut msg) = event.message {
        *msg = scrub_home_dir(msg);
    }

    for exc in &mut event.exception.values {
        if let Some(ref mut val) = exc.value {
            *val = scrub_home_dir(val);
        }
    }

    for crumb in &mut event.breadcrumbs.values {
        if let Some(ref mut msg) = crumb.message {
            *msg = scrub_home_dir(msg);
        }
    }

    Some(event)
}

fn sanitize_breadcrumb(mut crumb: Breadcrumb) -> Option<Breadcrumb> {
    if !is_enabled() {
        return None;
    }
    if let Some(ref mut msg) = crumb.message {
        *msg = scrub_home_dir(msg);
    }
    Some(crumb)
}

pub fn init() -> Option<ClientInitGuard> {
    // Only active in release/CI builds where SENTRY_DSN is injected at compile time
    let dsn = option_env!("SENTRY_DSN")?;
    if dsn.trim().is_empty() {
        return None;
    }

    let mut options = ClientOptions::default();
    options.release = sentry::release_name!();
    options.send_default_pii = false;
    options.before_send = Some(Arc::new(sanitize_event));
    options.before_breadcrumb = Some(Arc::new(sanitize_breadcrumb));

    let guard = sentry::init((dsn, options));

    Some(guard)
}

/// Reports a non-fatal platform/parser failure without leaking track names or URLs.
pub fn report_parser_error(platform: &str, stage: &str, error_kind: &str) {
    if !is_enabled() {
        return;
    }

    sentry::with_scope(
        |scope| {
            scope.set_tag("platform", platform);
            scope.set_tag("stage", stage);
            scope.set_tag("error_kind", error_kind);
            // Group issues by (platform, stage, error_kind) instead of raw error strings
            scope.set_fingerprint(Some(&[platform, stage, error_kind]));
        },
        || {
            sentry::capture_message(
                &format!("[{platform}] {stage} failed: {error_kind}"),
                Level::Error,
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentry::protocol::{Exception, Values};
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_toggle_enabled() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
        assert!(!is_enabled());
        set_enabled(true);
    }

    #[test]
    fn test_scrub_home_dir() {
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            let input = format!("Error at {home}\\test\\path");
            let scrubbed = scrub_home_dir(&input);
            assert!(!scrubbed.contains(&home));
            assert!(scrubbed.contains("<HOME>"));

            let escaped_home = home.replace('\\', "\\\\");
            let input_escaped = format!("Error at {escaped_home}\\\\test\\\\path");
            let scrubbed_escaped = scrub_home_dir(&input_escaped);
            assert!(!scrubbed_escaped.contains(&escaped_home));
            assert!(scrubbed_escaped.contains("<HOME>"));
        }
    }

    #[test]
    fn test_sanitize_breadcrumb() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_enabled(false);
        let crumb = Breadcrumb {
            message: Some("test message".to_string()),
            ..Default::default()
        };
        assert!(sanitize_breadcrumb(crumb).is_none());

        set_enabled(true);
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            let crumb = Breadcrumb {
                message: Some(format!("Error at {home}/something")),
                ..Default::default()
            };
            let res = sanitize_breadcrumb(crumb).expect("breadcrumb should be retained");
            assert!(res.message.unwrap().contains("<HOME>"));
        }
    }

    #[test]
    fn test_sanitize_event() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_enabled(false);
        let event = Event::default();
        assert!(sanitize_event(event).is_none());

        set_enabled(true);
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| "C:\\Users\\dummy".to_string());

        let mut event = Event::default();
        event.server_name = Some("secret-workstation".into());
        event.message = Some(format!("Crash in {home}/app"));
        event.exception = Values {
            values: vec![Exception {
                value: Some(format!("File not found: {home}/data")),
                ..Default::default()
            }],
        };
        event.breadcrumbs = Values {
            values: vec![Breadcrumb {
                message: Some(format!("Action at {home}/lib")),
                ..Default::default()
            }],
        };

        let sanitized = sanitize_event(event).expect("event should be kept");
        assert!(sanitized.server_name.is_none());
        assert!(!sanitized.message.as_ref().unwrap().contains(&home));
        assert!(sanitized.message.as_ref().unwrap().contains("<HOME>"));
        assert!(!sanitized.exception.values[0].value.as_ref().unwrap().contains(&home));
        assert!(sanitized.exception.values[0].value.as_ref().unwrap().contains("<HOME>"));
        assert!(!sanitized.breadcrumbs.values[0].message.as_ref().unwrap().contains(&home));
        assert!(sanitized.breadcrumbs.values[0].message.as_ref().unwrap().contains("<HOME>"));
    }

    #[test]
    fn test_init_install_id_file_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_install_id(temp_dir.path());
        let id_file = temp_dir.path().join("install_id");
        assert!(id_file.exists());
        let content = std::fs::read_to_string(&id_file).unwrap();
        assert!(!content.trim().is_empty());
        assert!(uuid::Uuid::parse_str(content.trim()).is_ok());
    }

    #[test]
    #[ignore = "requires network and live SENTRY_DSN; run with: cargo test --manifest-path src-tauri/Cargo.toml test_live_sentry_ping -- --ignored --nocapture"]
    fn test_live_sentry_ping() {
        let guard = init().expect("SENTRY_DSN must be set at compile time");
        sentry::capture_message("Blacktape telemetry test ping", Level::Info);
        drop(guard); // flushes queue to Sentry
        println!("Sentry test event dispatched successfully.");
    }
}
