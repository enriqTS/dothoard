//! Secret detection heuristics for backup warnings.
//!
//! Scans source file paths for patterns that suggest private keys,
//! credentials, tokens, cookies, and other secrets. Detection is purely
//! path-based — file contents are not read for this purpose.
//!
//! This produces warnings in the change-set so the user can review what
//! sensitive files will be included in a Git-backed repository.

use std::path::Path;

use super::changeset::{PlanWarning, WarningKind};

/// Check whether a file path looks like it might contain secrets.
///
/// Returns `Some(reason)` describing why it was flagged, or `None` if
/// the path doesn't match any known secret patterns.
///
/// The `relative_path` is the path relative to the source root.
pub fn detect_secret(relative_path: &Path) -> Option<String> {
    let path_str = relative_path.to_string_lossy();
    let file_name = relative_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_name_lower = file_name.to_lowercase();

    // Private key files
    if is_private_key(&file_name_lower, &path_str) {
        return Some("private key file".to_string());
    }

    // Credential/auth files
    if is_credential_file(&file_name_lower) {
        return Some("credential file".to_string());
    }

    // Token files
    if is_token_file(&file_name_lower, &path_str) {
        return Some("token or session file".to_string());
    }

    // Cookie files
    if is_cookie_file(&file_name_lower) {
        return Some("cookie file".to_string());
    }

    // Environment files with secrets
    if is_env_file(&file_name_lower) {
        return Some("environment file (may contain secrets)".to_string());
    }

    // Known sensitive application files
    if is_sensitive_app_file(&file_name_lower, &path_str) {
        return Some("sensitive application data".to_string());
    }

    None
}

/// Generate a PlanWarning for a detected secret.
pub fn make_secret_warning(source_path: &Path, reason: String) -> PlanWarning {
    PlanWarning {
        path: source_path.to_path_buf(),
        kind: WarningKind::PossibleSecret { reason },
    }
}

/// Check for private key patterns.
fn is_private_key(file_name: &str, path_str: &str) -> bool {
    // SSH private keys
    if file_name == "id_rsa"
        || file_name == "id_ed25519"
        || file_name == "id_ecdsa"
        || file_name == "id_dsa"
        || file_name == "identity"
    {
        return true;
    }

    // Generic key file extensions
    if file_name.ends_with(".pem")
        || file_name.ends_with(".key")
        || file_name.ends_with(".p12")
        || file_name.ends_with(".pfx")
        || file_name.ends_with(".jks")
    {
        return true;
    }

    // Private key naming patterns
    if file_name.contains("private") && file_name.contains("key") {
        return true;
    }

    // GPG private keys
    if file_name == "secring.gpg" || file_name == "trustdb.gpg" {
        return true;
    }

    // SSH directory private keys (path-based)
    if path_str.contains(".ssh/") && !file_name.ends_with(".pub") && !file_name.contains("config") {
        // Files in .ssh that aren't .pub or config are likely private keys
        if file_name != "known_hosts"
            && file_name != "known_hosts.old"
            && file_name != "authorized_keys"
        {
            return true;
        }
    }

    false
}

/// Check for credential/authentication files.
fn is_credential_file(file_name: &str) -> bool {
    file_name == ".netrc"
        || file_name == ".npmrc"
        || file_name == ".pypirc"
        || file_name == "credentials"
        || file_name == "credentials.json"
        || file_name == "credentials.yml"
        || file_name == "credentials.yaml"
        || file_name == ".docker/config.json"
        || file_name == "service-account.json"
        || file_name == "service_account.json"
        || file_name == ".aws_credentials"
        || file_name.starts_with("credential")
}

/// Check for token/session files.
fn is_token_file(file_name: &str, path_str: &str) -> bool {
    if file_name.contains("token") || file_name.contains("session") {
        return true;
    }

    // OAuth files
    if file_name.contains("oauth") && (file_name.ends_with(".json") || file_name.ends_with(".yml"))
    {
        return true;
    }

    // Bearer token patterns
    if file_name.contains("bearer") || file_name.contains("refresh_token") {
        return true;
    }

    // Application-specific token locations
    if path_str.contains("tokens/") || path_str.contains("/token") {
        return true;
    }

    false
}

/// Check for cookie files.
fn is_cookie_file(file_name: &str) -> bool {
    file_name == "cookies"
        || file_name == "cookies.txt"
        || file_name == "cookies.sqlite"
        || file_name == "cookies.db"
        || file_name.contains("cookie")
}

/// Check for environment files that commonly contain secrets.
fn is_env_file(file_name: &str) -> bool {
    file_name == ".env"
        || file_name == ".env.local"
        || file_name == ".env.production"
        || file_name == ".env.development"
        || file_name.starts_with(".env.")
}

/// Check for known sensitive application data files.
fn is_sensitive_app_file(file_name: &str, path_str: &str) -> bool {
    // AWS credentials
    if path_str.contains(".aws/") && file_name == "credentials" {
        return true;
    }

    // Kubernetes secrets
    if file_name == "kubeconfig" || file_name.contains("kube") && file_name.ends_with("config") {
        return true;
    }

    // Database connection strings
    if file_name == "database.yml" && path_str.contains("config/") {
        return true;
    }

    // Vault tokens
    if file_name == ".vault-token" || file_name == "vault-token" {
        return true;
    }

    // Age/SOPS keys
    if file_name == "keys.txt" && path_str.contains("sops/") {
        return true;
    }

    // Keychain/keyring files
    if file_name.contains("keychain") || file_name.contains("keyring") {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
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
}
