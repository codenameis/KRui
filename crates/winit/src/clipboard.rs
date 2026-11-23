//! OS and in-app clipboard synchronization via `arboard` (`Ctrl+C`, `Ctrl+X`, `Ctrl+V`).

use krui_style::sanitize_untrusted_text;
use std::sync::Mutex;

static INTERNAL_CLIPBOARD: Mutex<String> = Mutex::new(String::new());
static LAST_FAILED_OS_SNAPSHOT: Mutex<Option<String>> = Mutex::new(None);
static OS_CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_os_clipboard<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<R>) -> Option<R> {
    let mut guard = OS_CLIPBOARD.lock().ok()?;
    if guard.is_none() {
        *guard = arboard::Clipboard::new().ok();
    }
    guard.as_mut().and_then(f)
}

/// Copies `text` to the KRui clipboard and synchronizes it with the OS clipboard via `arboard`.
pub fn set_clipboard_text(text: &str) {
    let clean = sanitize_untrusted_text(text);
    if let Ok(mut guard) = INTERNAL_CLIPBOARD.lock() {
        *guard = clean.clone();
    }
    let synced = with_os_clipboard(|cb| cb.set_text(clean).ok()).is_some();
    if let Ok(mut snap) = LAST_FAILED_OS_SNAPSHOT.lock() {
        *snap = if synced {
            None
        } else {
            Some(with_os_clipboard(|cb| cb.get_text().ok()).unwrap_or_default())
        };
    }
}

/// Reads text from the clipboard (always attempting the OS clipboard via `arboard` first,
/// self-healing if external OS clipboard contents changed after a transient write failure,
/// and falling back to the in-app clipboard when unavailable).
pub fn get_clipboard_text() -> String {
    if let Some(os_text) = with_os_clipboard(|cb| cb.get_text().ok()) {
        let use_os = match LAST_FAILED_OS_SNAPSHOT.lock() {
            Ok(mut snap) => match snap.as_ref() {
                None => true,
                Some(stale) if *stale != os_text => {
                    *snap = None;
                    true
                }
                Some(_) => false,
            },
            Err(_) => true,
        };
        if use_os {
            let clean = sanitize_untrusted_text(&os_text);
            if !clean.is_empty() {
                return clean;
            }
        }
    }
    INTERNAL_CLIPBOARD
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}
