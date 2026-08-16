//! Application-wide identifiers kept together to make the planned rename
//! atomic and reviewable.

pub const APP_NAME: &str = "dothoard";
pub const BINARY_NAME: &str = "dothoard";
pub const CONFIG_DIR_NAME: &str = "dothoard";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const THEME_FILE_NAME: &str = "theme.toml";
pub const SETUP_MARKER_FILE_NAME: &str = "setup-incomplete";
pub const STATE_DIR_NAME: &str = "dothoard";
pub const MANIFEST_FILE_NAME: &str = ".dothoard-manifest.toml";
pub const LOG_DIR_NAME: &str = "logs";
pub const SYSTEMD_SERVICE_UNIT: &str = "dothoard-backup.service";
pub const SYSTEMD_TIMER_UNIT: &str = "dothoard-backup.timer";

pub fn trace_identifiers() {
    tracing::trace!(
        app_name = APP_NAME,
        binary_name = BINARY_NAME,
        config_dir = CONFIG_DIR_NAME,
        config_file = CONFIG_FILE_NAME,
        state_dir = STATE_DIR_NAME,
        log_dir = LOG_DIR_NAME,
        manifest = MANIFEST_FILE_NAME,
        service_unit = SYSTEMD_SERVICE_UNIT,
        timer_unit = SYSTEMD_TIMER_UNIT,
        "using application identifiers"
    );
}

#[cfg(test)]
#[path = "../tests/unit/app.rs"]
mod tests;
