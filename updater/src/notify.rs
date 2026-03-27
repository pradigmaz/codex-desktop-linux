//! Desktop notification helpers used by the updater daemon.

use anyhow::Result;

/// Sends a desktop notification through the host notification service.
pub fn send(summary: &str, body: &str) -> Result<()> {
    notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .appname("Codex Update Manager")
        .show()?;
    Ok(())
}
