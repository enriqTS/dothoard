use super::*;
use std::path::PathBuf;

#[test]
fn detects_ssh_private_keys() {
    assert!(detect_secret(Path::new("id_rsa")).is_some());
    assert!(detect_secret(Path::new("id_ed25519")).is_some());
    assert!(detect_secret(Path::new("id_ecdsa")).is_some());
    assert!(detect_secret(Path::new("id_dsa")).is_some());
    assert!(detect_secret(Path::new("identity")).is_some());
}

#[test]
fn detects_key_file_extensions() {
    assert!(detect_secret(Path::new("server.pem")).is_some());
    assert!(detect_secret(Path::new("tls.key")).is_some());
    assert!(detect_secret(Path::new("cert.p12")).is_some());
    assert!(detect_secret(Path::new("keystore.pfx")).is_some());
    assert!(detect_secret(Path::new("app.jks")).is_some());
}

#[test]
fn detects_private_key_naming() {
    assert!(detect_secret(Path::new("my_private_key.txt")).is_some());
    assert!(detect_secret(Path::new("private-key")).is_some());
}

#[test]
fn does_not_flag_public_keys() {
    // .pub files in .ssh are not private keys
    assert!(detect_secret(Path::new("id_rsa.pub")).is_none());
    assert!(detect_secret(Path::new("id_ed25519.pub")).is_none());
}

#[test]
fn detects_ssh_directory_private_files() {
    assert!(detect_secret(Path::new(".ssh/my_custom_key")).is_some());
    // But NOT public or known safe files
    assert!(detect_secret(Path::new(".ssh/known_hosts")).is_none());
    assert!(detect_secret(Path::new(".ssh/config")).is_none());
    assert!(detect_secret(Path::new(".ssh/authorized_keys")).is_none());
}

#[test]
fn detects_credential_files() {
    assert!(detect_secret(Path::new(".netrc")).is_some());
    assert!(detect_secret(Path::new(".npmrc")).is_some());
    assert!(detect_secret(Path::new(".pypirc")).is_some());
    assert!(detect_secret(Path::new("credentials")).is_some());
    assert!(detect_secret(Path::new("credentials.json")).is_some());
}

#[test]
fn detects_token_files() {
    assert!(detect_secret(Path::new("auth_token")).is_some());
    assert!(detect_secret(Path::new("access_token.json")).is_some());
    assert!(detect_secret(Path::new("session.json")).is_some());
    assert!(detect_secret(Path::new("refresh_token")).is_some());
}

#[test]
fn detects_cookie_files() {
    assert!(detect_secret(Path::new("cookies")).is_some());
    assert!(detect_secret(Path::new("cookies.txt")).is_some());
    assert!(detect_secret(Path::new("cookies.sqlite")).is_some());
    assert!(detect_secret(Path::new("cookies.db")).is_some());
}

#[test]
fn detects_env_files() {
    assert!(detect_secret(Path::new(".env")).is_some());
    assert!(detect_secret(Path::new(".env.local")).is_some());
    assert!(detect_secret(Path::new(".env.production")).is_some());
}

#[test]
fn detects_sensitive_app_files() {
    assert!(detect_secret(Path::new(".vault-token")).is_some());
    assert!(detect_secret(Path::new("kubeconfig")).is_some());
}

#[test]
fn detects_gpg_private_keys() {
    assert!(detect_secret(Path::new("secring.gpg")).is_some());
    assert!(detect_secret(Path::new("trustdb.gpg")).is_some());
}

#[test]
fn does_not_flag_normal_config_files() {
    assert!(detect_secret(Path::new("config.toml")).is_none());
    assert!(detect_secret(Path::new("settings.json")).is_none());
    assert!(detect_secret(Path::new("init.lua")).is_none());
    assert!(detect_secret(Path::new("config.fish")).is_none());
    assert!(detect_secret(Path::new(".bashrc")).is_none());
    assert!(detect_secret(Path::new(".gitconfig")).is_none());
    assert!(detect_secret(Path::new("starship.toml")).is_none());
    assert!(detect_secret(Path::new("waybar/config")).is_none());
}

#[test]
fn does_not_flag_regular_source_files() {
    assert!(detect_secret(Path::new("main.rs")).is_none());
    assert!(detect_secret(Path::new("README.md")).is_none());
    assert!(detect_secret(Path::new("Cargo.toml")).is_none());
    assert!(detect_secret(Path::new("package.json")).is_none());
}

#[test]
fn make_secret_warning_creates_correct_struct() {
    let path = PathBuf::from("/home/user/.ssh/id_rsa");
    let warning = make_secret_warning(&path, "private key file".to_string());

    assert_eq!(warning.path, path);
    assert!(matches!(
        &warning.kind,
        WarningKind::PossibleSecret { reason } if reason == "private key file"
    ));
}

#[test]
fn detects_token_in_path() {
    assert!(detect_secret(Path::new("tokens/github")).is_some());
    assert!(detect_secret(Path::new("app/token")).is_some());
}

#[test]
fn detects_keychain_files() {
    assert!(detect_secret(Path::new("login.keychain")).is_some());
    assert!(detect_secret(Path::new("gnome-keyring")).is_some());
}
