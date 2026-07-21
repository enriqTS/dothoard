use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

pub struct TestEnvironment {
    _temp_dir: TempDir,
    pub home: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub runtime: PathBuf,
    pub repository: PathBuf,
    pub remote: PathBuf,
}

impl TestEnvironment {
    pub fn new() -> std::io::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let root = temp_dir.path();
        let home = root.join("home");
        let config = root.join("config");
        let state = root.join("state");
        let runtime = root.join("runtime");
        let repository = root.join("repository");
        let remote = root.join("remote.git");

        for directory in [&home, &config, &state, &runtime, &repository, &remote] {
            fs::create_dir(directory)?;
        }

        Ok(Self {
            _temp_dir: temp_dir,
            home,
            config,
            state,
            runtime,
            repository,
            remote,
        })
    }

    pub fn root(&self) -> &Path {
        self._temp_dir.path()
    }
}
