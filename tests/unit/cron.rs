use std::cell::{Cell, RefCell};

use super::*;

fn params(interval_minutes: u32) -> CronParams {
    CronParams {
        binary_path: PathBuf::from("/home/alice/.local/bin/dothoard"),
        runtime_dir: PathBuf::from("/run/user/1000"),
        interval_minutes,
    }
}

#[derive(Debug)]
struct FakeCrontab {
    current: RefCell<Option<String>>,
    list_calls: Cell<usize>,
    replacements: RefCell<Vec<String>>,
}

impl FakeCrontab {
    fn new(current: Option<&str>) -> Self {
        Self {
            current: RefCell::new(current.map(str::to_string)),
            list_calls: Cell::new(0),
            replacements: RefCell::new(Vec::new()),
        }
    }
}

impl CrontabRunner for FakeCrontab {
    fn list(&self) -> Result<Option<String>, CronError> {
        self.list_calls.set(self.list_calls.get() + 1);
        Ok(self.current.borrow().clone())
    }

    fn replace(&self, content: &str) -> Result<(), CronError> {
        self.replacements.borrow_mut().push(content.to_string());
        *self.current.borrow_mut() = Some(content.to_string());
        Ok(())
    }
}

#[test]
fn generated_block_is_deterministic_and_direct() {
    let expected = "# BEGIN dothoard managed automation v1\n\
# Managed by dothoard; do not edit this block\n\
# backend=cron interval_minutes=5\n\
*/5 * * * * XDG_RUNTIME_DIR=/run/user/1000 /home/alice/.local/bin/dothoard backup\n\
# END dothoard managed automation v1\n";

    assert_eq!(generate_managed_block(&params(5)).unwrap(), expected);
    assert_eq!(generate_managed_block(&params(5)).unwrap(), expected);
}

#[test]
fn generation_rejects_unportable_interval_and_shell_sensitive_paths() {
    assert!(matches!(
        generate_managed_block(&params(60)),
        Err(CronError::UnsupportedInterval(60))
    ));

    let mut unsafe_params = params(5);
    unsafe_params.binary_path = PathBuf::from("/home/alice/My Bin/dothoard");
    assert!(matches!(
        generate_managed_block(&unsafe_params),
        Err(CronError::UnsafePath {
            name: "executable",
            ..
        })
    ));
}

#[test]
fn install_appends_owned_block_and_preserves_unrelated_content() {
    let original = "MAILTO=alice@example.test\n0 4 * * * /usr/bin/existing\n";
    let runner = FakeCrontab::new(Some(original));

    install_with(&params(5), &runner).unwrap();

    let replacements = runner.replacements.borrow();
    assert_eq!(replacements.len(), 1);
    assert!(replacements[0].starts_with(original));
    assert!(replacements[0].contains("*/5 * * * *"));
}

#[test]
fn reinstall_is_idempotent_and_refresh_replaces_only_owned_range() {
    let block = generate_managed_block(&params(5)).unwrap();
    let current = format!("SHELL=/bin/sh\n{block}0 3 * * * /usr/bin/unrelated\n");
    let runner = FakeCrontab::new(Some(&current));

    install_with(&params(5), &runner).unwrap();
    assert!(runner.replacements.borrow().is_empty());

    install_with(&params(10), &runner).unwrap();
    let replacements = runner.replacements.borrow();
    assert_eq!(replacements.len(), 1);
    assert!(replacements[0].starts_with("SHELL=/bin/sh\n"));
    assert!(replacements[0].contains("*/10 * * * *"));
    assert!(replacements[0].ends_with("0 3 * * * /usr/bin/unrelated\n"));
    assert!(!replacements[0].contains("*/5 * * * *"));
}

#[test]
fn remove_deletes_only_owned_block_and_is_idempotent() {
    let block = generate_managed_block(&params(5)).unwrap();
    let unrelated = "0 3 * * * /usr/bin/unrelated\n";
    let runner = FakeCrontab::new(Some(&format!("{block}{unrelated}")));

    remove_with(&runner).unwrap();
    assert_eq!(runner.current.borrow().as_deref(), Some(unrelated));

    remove_with(&runner).unwrap();
    assert_eq!(runner.replacements.borrow().len(), 1);
}

#[test]
fn malformed_duplicate_or_unowned_markers_are_refused_without_writing() {
    let cases = [
        format!("{BEGIN_MARKER}\n"),
        format!("{BEGIN_MARKER}\n{END_MARKER}\n"),
        format!(
            "{BEGIN_MARKER}\n{OWNER_MARKER}\n{END_MARKER}\n{BEGIN_MARKER}\n{OWNER_MARKER}\n{END_MARKER}\n"
        ),
    ];

    for current in cases {
        let runner = FakeCrontab::new(Some(&current));
        assert!(matches!(
            install_with(&params(5), &runner),
            Err(CronError::AmbiguousManagedBlock(_))
        ));
        assert!(runner.replacements.borrow().is_empty());
    }
}

#[cfg(unix)]
#[test]
fn command_runner_uses_literal_crontab_arguments_and_stdin() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let program = temp.path().join("controlled-crontab");
    std::fs::write(
        &program,
        "#!/bin/sh\n\
base=$(dirname \"$0\")\n\
printf '%s\\n' \"$1\" >> \"$base/calls\"\n\
case \"$1\" in\n\
  -l)\n\
    if test -f \"$base/current\"; then cat \"$base/current\"; else echo 'no crontab for test' >&2; exit 1; fi\n\
    ;;\n\
  -) cat > \"$base/replacement\" ;;\n\
  *) exit 9 ;;\n\
esac\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&program).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&program, permissions).unwrap();

    let runner = CommandCrontab {
        program: program.clone(),
    };
    assert_eq!(runner.list().unwrap(), None);

    runner.replace("MAILTO=alice@example.test\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(temp.path().join("replacement")).unwrap(),
        "MAILTO=alice@example.test\n"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("calls")).unwrap(),
        "-l\n-\n"
    );

    std::fs::copy(temp.path().join("replacement"), temp.path().join("current")).unwrap();
    assert_eq!(
        runner.list().unwrap().as_deref(),
        Some("MAILTO=alice@example.test\n")
    );
}

#[test]
fn status_reports_absent_current_and_stale_without_writing() {
    let absent = FakeCrontab::new(None);
    assert_eq!(
        status_with(&params(5), &absent).unwrap(),
        CronStatus::NotInstalled
    );

    let current_block = generate_managed_block(&params(5)).unwrap();
    let current = FakeCrontab::new(Some(&current_block));
    assert_eq!(
        status_with(&params(5), &current).unwrap(),
        CronStatus::Installed { stale: false }
    );
    assert!(current.replacements.borrow().is_empty());

    assert_eq!(
        status_with(&params(10), &current).unwrap(),
        CronStatus::Installed { stale: true }
    );
    assert!(current.replacements.borrow().is_empty());
}
