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
