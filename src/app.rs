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
