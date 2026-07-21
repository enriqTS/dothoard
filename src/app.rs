//! Application-wide identifiers kept together to make the planned rename
//! atomic and reviewable.

pub const APP_NAME: &str = "config-sync";
pub const BINARY_NAME: &str = "config-sync";
pub const CONFIG_DIR_NAME: &str = "config-sync";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const STATE_DIR_NAME: &str = "config-sync";
pub const MANIFEST_FILE_NAME: &str = ".config-sync-manifest.toml";
pub const SYSTEMD_SERVICE_UNIT: &str = "config-sync-backup.service";
pub const SYSTEMD_TIMER_UNIT: &str = "config-sync-backup.timer";

pub fn trace_identifiers() {
    tracing::trace!(
        app_name = APP_NAME,
        binary_name = BINARY_NAME,
        config_dir = CONFIG_DIR_NAME,
        config_file = CONFIG_FILE_NAME,
        state_dir = STATE_DIR_NAME,
        manifest = MANIFEST_FILE_NAME,
        service_unit = SYSTEMD_SERVICE_UNIT,
        timer_unit = SYSTEMD_TIMER_UNIT,
        "using application identifiers"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_identifiers_are_consistent() {
        assert_eq!(APP_NAME, BINARY_NAME);
        assert_eq!(CONFIG_DIR_NAME, APP_NAME);
        assert_eq!(STATE_DIR_NAME, APP_NAME);
        assert!(MANIFEST_FILE_NAME.starts_with('.'));
        assert!(SYSTEMD_SERVICE_UNIT.starts_with(APP_NAME));
        assert!(SYSTEMD_TIMER_UNIT.starts_with(APP_NAME));
    }
}
