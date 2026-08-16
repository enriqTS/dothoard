use super::*;

#[test]
fn setup_starts_with_existing_repository_choice() {
    let setup = SetupState::new();
    assert_eq!(setup.step, SetupStep::Repository);
    assert_eq!(setup.repository_mode, RepositorySetupMode::Choose);
    assert_eq!(setup.repository_method, RepositoryMethod::Existing);
    assert_eq!(setup.clone_field, CloneField::Url);
    assert!(!setup.cloning());
    assert!(setup.clone_error().is_none());
}

#[test]
fn repository_method_toggle_is_reversible() {
    assert_eq!(RepositoryMethod::Existing.toggle(), RepositoryMethod::Clone);
    assert_eq!(RepositoryMethod::Clone.toggle(), RepositoryMethod::Existing);
}
