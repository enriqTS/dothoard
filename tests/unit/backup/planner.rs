use super::*;
use std::os::unix::fs::PermissionsExt;

/// Helper to set up a test environment with home and repository.
struct TestEnv {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    repository: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let repository = tmp.path().join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(repository.join("desktop/home")).unwrap();
        Self {
            _tmp: tmp,
            home,
            repository,
        }
    }

    fn plan(&self, sources: &[SourceConfig]) -> ChangeSet {
        let inputs = PlanInputs {
            home: &self.home,
            repository: &self.repository,
            namespace: "desktop",
            sources,
        };
        plan_backup(&inputs).unwrap()
    }
}

fn source(path: &str, ignore: &[&str]) -> SourceConfig {
    SourceConfig {
        path: path.to_string(),
        ignore: ignore.iter().map(|s| s.to_string()).collect(),
    }
}

// --- Basic planning ---

#[test]
fn empty_source_directory_produces_empty_changeset() {
    let env = TestEnv::new();
    std::fs::create_dir_all(env.home.join(".config/empty")).unwrap();

    let cs = env.plan(&[source(".config/empty", &[])]);

    assert!(cs.is_empty());
    assert!(cs.exclusions.is_empty());
}

#[test]
fn new_files_appear_as_additions() {
    let env = TestEnv::new();
    let src = env.home.join(".config/fish");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("config.fish"), "set -x PATH").unwrap();
    std::fs::write(src.join("functions.fish"), "function hello").unwrap();

    let cs = env.plan(&[source(".config/fish", &[])]);

    assert_eq!(cs.additions.len(), 2);
    assert!(cs.modifications.is_empty());
    assert!(cs.deletions.is_empty());
}

#[test]
fn identical_files_produce_no_changes() {
    let env = TestEnv::new();
    let src = env.home.join(".config/fish");
    let dst = env.repository.join("desktop/home/.config/fish");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    std::fs::write(src.join("config.fish"), "same content").unwrap();
    std::fs::write(dst.join("config.fish"), "same content").unwrap();

    let cs = env.plan(&[source(".config/fish", &[])]);

    assert!(cs.is_empty());
}

#[test]
fn modified_files_appear_as_modifications() {
    let env = TestEnv::new();
    let src = env.home.join(".config/fish");
    let dst = env.repository.join("desktop/home/.config/fish");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    std::fs::write(src.join("config.fish"), "new content").unwrap();
    std::fs::write(dst.join("config.fish"), "old content").unwrap();

    let cs = env.plan(&[source(".config/fish", &[])]);

    assert!(cs.additions.is_empty());
    assert_eq!(cs.modifications.len(), 1);
    assert!(cs.deletions.is_empty());
}

#[test]
fn removed_source_files_appear_as_deletions() {
    let env = TestEnv::new();
    let src = env.home.join(".config/fish");
    let dst = env.repository.join("desktop/home/.config/fish");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    // Source has only config.fish, but dest also has old.fish
    std::fs::write(src.join("config.fish"), "content").unwrap();
    std::fs::write(dst.join("config.fish"), "content").unwrap();
    std::fs::write(dst.join("old.fish"), "old content").unwrap();

    let cs = env.plan(&[source(".config/fish", &[])]);

    assert!(cs.additions.is_empty());
    assert!(cs.modifications.is_empty());
    assert_eq!(cs.deletions.len(), 1);
}

// --- Ignore rules ---

#[test]
fn ignored_files_appear_as_exclusions() {
    let env = TestEnv::new();
    let src = env.home.join(".config/fish");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("config.fish"), "content").unwrap();
    std::fs::write(src.join("debug.log"), "log data").unwrap();

    let cs = env.plan(&[source(".config/fish", &["*.log"])]);

    assert_eq!(cs.additions.len(), 1);
    assert_eq!(cs.exclusions.len(), 1);
}

#[test]
fn newly_ignored_tracked_files_become_deletions_with_warning() {
    let env = TestEnv::new();
    let src = env.home.join(".config/fish");
    let dst = env.repository.join("desktop/home/.config/fish");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    // Source still has the file but it's now ignored
    std::fs::write(src.join("config.fish"), "content").unwrap();
    std::fs::write(src.join("secret.key"), "key data").unwrap();
    std::fs::write(dst.join("config.fish"), "content").unwrap();
    std::fs::write(dst.join("secret.key"), "key data").unwrap();

    let cs = env.plan(&[source(".config/fish", &["*.key"])]);

    // secret.key is now ignored → deleted from dest + warned
    assert_eq!(cs.deletions.len(), 1);
    assert!(cs.warnings.iter().any(|w| matches!(
        &w.kind,
        super::super::changeset::WarningKind::IgnoredButTracked
    )));
}

// --- Missing source root ---

#[test]
fn missing_source_root_emits_warning_without_deletions() {
    let env = TestEnv::new();
    let dst = env.repository.join("desktop/home/.config/gone");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(dst.join("preserved.txt"), "data").unwrap();

    // Source doesn't exist — backup should be preserved
    let cs = env.plan(&[source(".config/gone", &[])]);

    assert!(cs.additions.is_empty());
    assert!(cs.modifications.is_empty());
    assert!(cs.deletions.is_empty()); // No deletions!
    assert!(cs.warnings.iter().any(|w| matches!(
        &w.kind,
        super::super::changeset::WarningKind::MissingSourceRoot { .. }
    )));
}

// --- Secret warnings ---

#[test]
fn secret_files_produce_warnings() {
    let env = TestEnv::new();
    let src = env.home.join(".ssh");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("id_rsa"), "private key").unwrap();
    std::fs::write(src.join("id_rsa.pub"), "public key").unwrap();
    std::fs::write(src.join("config"), "Host *").unwrap();

    let cs = env.plan(&[source(".ssh", &[])]);

    // id_rsa should trigger a secret warning
    assert!(cs.warnings.iter().any(|w| matches!(
        &w.kind,
        super::super::changeset::WarningKind::PossibleSecret { .. }
    )));
}

// --- Multiple sources ---

#[test]
fn multiple_sources_combined_in_changeset() {
    let env = TestEnv::new();
    let fish = env.home.join(".config/fish");
    let waybar = env.home.join(".config/waybar");
    std::fs::create_dir_all(&fish).unwrap();
    std::fs::create_dir_all(&waybar).unwrap();
    std::fs::write(fish.join("config.fish"), "fish").unwrap();
    std::fs::write(waybar.join("config"), "waybar").unwrap();

    let cs = env.plan(&[source(".config/fish", &[]), source(".config/waybar", &[])]);

    assert_eq!(cs.additions.len(), 2);
}

// --- Deterministic output ---

#[test]
fn output_is_deterministic() {
    let env = TestEnv::new();
    let src = env.home.join(".config/test");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("z.txt"), "z").unwrap();
    std::fs::write(src.join("a.txt"), "a").unwrap();
    std::fs::write(src.join("m.txt"), "m").unwrap();

    let cs1 = env.plan(&[source(".config/test", &[])]);
    let cs2 = env.plan(&[source(".config/test", &[])]);

    // Same inputs → same output.
    assert_eq!(cs1.additions.len(), cs2.additions.len());
    for (a, b) in cs1.additions.iter().zip(cs2.additions.iter()) {
        assert_eq!(a.destination, b.destination);
    }
}

#[test]
fn additions_are_sorted_by_destination() {
    let env = TestEnv::new();
    let src = env.home.join(".config/test");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("z.txt"), "z").unwrap();
    std::fs::write(src.join("a.txt"), "a").unwrap();
    std::fs::write(src.join("m.txt"), "m").unwrap();

    let cs = env.plan(&[source(".config/test", &[])]);

    let dests: Vec<_> = cs.additions.iter().map(|a| &a.destination).collect();
    let mut sorted_dests = dests.clone();
    sorted_dests.sort();
    assert_eq!(dests, sorted_dests);
}

// --- Mixed operations ---

#[test]
fn full_scenario_with_additions_modifications_deletions() {
    let env = TestEnv::new();
    let src = env.home.join(".config/app");
    let dst = env.repository.join("desktop/home/.config/app");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    // Addition: new.txt exists in source only
    std::fs::write(src.join("new.txt"), "new").unwrap();
    // Modification: mod.txt has different content
    std::fs::write(src.join("mod.txt"), "modified").unwrap();
    std::fs::write(dst.join("mod.txt"), "original").unwrap();
    // Unchanged: same.txt is identical
    std::fs::write(src.join("same.txt"), "same").unwrap();
    std::fs::write(dst.join("same.txt"), "same").unwrap();
    // Deletion: old.txt exists only in destination
    std::fs::write(dst.join("old.txt"), "old").unwrap();

    let cs = env.plan(&[source(".config/app", &[])]);

    assert_eq!(cs.additions.len(), 1);
    assert_eq!(cs.modifications.len(), 1);
    assert_eq!(cs.deletions.len(), 1);
}

// --- Symlinks ---

#[test]
fn symlink_additions_detected() {
    let env = TestEnv::new();
    let src = env.home.join(".config/links");
    std::fs::create_dir_all(&src).unwrap();
    std::os::unix::fs::symlink("/some/target", src.join("my-link")).unwrap();

    let cs = env.plan(&[source(".config/links", &[])]);

    assert_eq!(cs.additions.len(), 1);
    assert_eq!(
        cs.additions[0].entry_type,
        super::super::changeset::EntryType::Symlink
    );
}

#[test]
fn executable_bit_change_detected() {
    let env = TestEnv::new();
    let src = env.home.join(".config/scripts");
    let dst = env.repository.join("desktop/home/.config/scripts");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();

    // Source is executable, dest is not
    std::fs::write(src.join("run.sh"), "#!/bin/bash").unwrap();
    std::fs::set_permissions(src.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dst.join("run.sh"), "#!/bin/bash").unwrap();
    // Dest is regular (0o644 by default)

    let cs = env.plan(&[source(".config/scripts", &[])]);

    assert_eq!(cs.modifications.len(), 1);
    assert!(matches!(
        &cs.modifications[0].change,
        super::super::changeset::ChangeKind::ExecutableBitChanged {
            now_executable: true
        }
    ));
}

// --- Single-file sources ---

#[test]
fn single_file_source_planned_correctly() {
    let env = TestEnv::new();
    std::fs::write(env.home.join(".bashrc"), "# bash config").unwrap();

    let cs = env.plan(&[source(".bashrc", &[])]);

    assert_eq!(cs.additions.len(), 1);
    assert!(
        cs.additions[0]
            .destination
            .to_string_lossy()
            .ends_with("home/.bashrc")
    );
}
