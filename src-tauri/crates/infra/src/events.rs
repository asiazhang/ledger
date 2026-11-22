use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

pub const LEDGER_CHANGED: &str = "ledger:changed";

pub const BACKUPS_CHANGED: &str = "ledger:backups-changed";

pub const PRICES_CHANGED: &str = "ledger:prices-changed";

static EVENT_APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init_event_app(app: &AppHandle) {
    let _ = EVENT_APP.set(app.clone());
}

pub fn post_emit_with<R: tauri::Runtime>(
    app: &AppHandle<R>,
    action: impl FnOnce() + Send + 'static,
) {
    let _ = app.run_on_main_thread(action);
}

pub trait SignalEmitter: Send + Sync {
    fn post(&self, event: &'static str);
}

impl<R: tauri::Runtime> SignalEmitter for AppHandle<R> {
    fn post(&self, event: &'static str) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(event, ());
        });
    }
}

pub fn emit_backups_changed(app: &AppHandle) {
    app.post(BACKUPS_CHANGED);
}

pub fn emit_backups_changed_current() {
    if let Some(app) = EVENT_APP.get() {
        emit_backups_changed(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_is_generic() {
        assert_eq!(LEDGER_CHANGED, "ledger:changed");
    }

    #[test]
    fn backups_changed_event_name() {
        assert_eq!(BACKUPS_CHANGED, "ledger:backups-changed");
        assert!(BACKUPS_CHANGED.starts_with("ledger:"));
    }

    #[test]
    fn prices_changed_event_name() {
        assert_eq!(PRICES_CHANGED, "ledger:prices-changed");
        assert!(PRICES_CHANGED.starts_with("ledger:"));
    }

    #[test]
    fn emit_backups_changed_current_without_handle_is_silent() {
        if EVENT_APP.get().is_none() {
            emit_backups_changed_current();
        }
    }
}
