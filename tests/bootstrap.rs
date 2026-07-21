mod support;

use support::TestEnvironment;

#[test]
fn test_environment_is_fully_isolated() {
    let environment = TestEnvironment::new().expect("test environment should be created");

    for directory in [
        &environment.home,
        &environment.config,
        &environment.state,
        &environment.runtime,
        &environment.repository,
        &environment.remote,
    ] {
        assert!(directory.is_dir());
        assert!(directory.starts_with(environment.root()));
    }
}
