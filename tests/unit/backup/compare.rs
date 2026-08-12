use super::*;
use std::path::PathBuf;

fn source_meta(path: &Path, relative: &str, entry_type: EntryType, size: u64) -> EntryMeta {
    EntryMeta {
        source_path: path.to_path_buf(),
        relative_path: PathBuf::from(relative),
        entry_type,
        size,
        mtime_secs: 1000,
        symlink_target: None,
    }
}

fn dest_meta(path: &Path, relative: &str, entry_type: EntryType, size: u64) -> DestinationMeta {
    DestinationMeta {
        destination_path: path.to_path_buf(),
        relative_path: PathBuf::from(relative),
        entry_type,
        size,
        mtime_secs: 1000,
        symlink_target: None,
    }
}

#[test]
fn identical_files_report_no_change() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src.txt");
    let dst = tmp.path().join("dst.txt");
    fs::write(&src, "hello world").unwrap();
    fs::write(&dst, "hello world").unwrap();

    let source = source_meta(&src, "file.txt", EntryType::RegularFile, 11);
    let dest = dest_meta(&dst, "file.txt", EntryType::RegularFile, 11);

    assert_eq!(compare_entries(&source, &dest), None);
}

#[test]
fn different_content_detected() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src.txt");
    let dst = tmp.path().join("dst.txt");
    fs::write(&src, "new content").unwrap();
    fs::write(&dst, "old content").unwrap();

    let source = source_meta(&src, "file.txt", EntryType::RegularFile, 11);
    let dest = dest_meta(&dst, "file.txt", EntryType::RegularFile, 11);

    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ContentChanged)
    );
}

#[test]
fn different_size_detected_without_reading_content() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src.txt");
    let dst = tmp.path().join("dst.txt");
    fs::write(&src, "short").unwrap();
    fs::write(&dst, "much longer content here").unwrap();

    let source = source_meta(&src, "file.txt", EntryType::RegularFile, 5);
    let dest = dest_meta(&dst, "file.txt", EntryType::RegularFile, 24);

    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ContentChanged)
    );
}

#[test]
fn executable_bit_gained() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("script.sh");
    let dst = tmp.path().join("script_dst.sh");
    fs::write(&src, "#!/bin/bash").unwrap();
    fs::write(&dst, "#!/bin/bash").unwrap();

    let source = source_meta(&src, "script.sh", EntryType::ExecutableFile, 11);
    let dest = dest_meta(&dst, "script.sh", EntryType::RegularFile, 11);

    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ExecutableBitChanged {
            now_executable: true
        })
    );
}

#[test]
fn executable_bit_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("file.txt");
    let dst = tmp.path().join("file_dst.txt");
    fs::write(&src, "data").unwrap();
    fs::write(&dst, "data").unwrap();

    let source = source_meta(&src, "file.txt", EntryType::RegularFile, 4);
    let dest = dest_meta(&dst, "file.txt", EntryType::ExecutableFile, 4);

    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ExecutableBitChanged {
            now_executable: false
        })
    );
}

#[test]
fn content_and_executable_bit_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("script.sh");
    let dst = tmp.path().join("script_dst.sh");
    fs::write(&src, "new script").unwrap();
    fs::write(&dst, "old script").unwrap();

    let source = source_meta(&src, "script.sh", EntryType::ExecutableFile, 10);
    let dest = dest_meta(&dst, "script.sh", EntryType::RegularFile, 10);

    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ContentAndExecutableBitChanged {
            now_executable: true
        })
    );
}

#[test]
fn file_became_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("link");
    let dst = tmp.path().join("file.txt");
    std::os::unix::fs::symlink("/target", &src).unwrap();
    fs::write(&dst, "data").unwrap();

    let mut source = source_meta(&src, "entry", EntryType::Symlink, 0);
    source.symlink_target = Some(PathBuf::from("/target"));
    let dest = dest_meta(&dst, "entry", EntryType::RegularFile, 4);

    let result = compare_entries(&source, &dest);
    assert_eq!(
        result,
        Some(ChangeKind::TypeChanged {
            old_type: EntryType::RegularFile,
            new_type: EntryType::Symlink,
        })
    );
}

#[test]
fn symlink_became_file() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("file.txt");
    let dst = tmp.path().join("link");
    fs::write(&src, "data").unwrap();
    std::os::unix::fs::symlink("/target", &dst).unwrap();

    let source = source_meta(&src, "entry", EntryType::RegularFile, 4);
    let mut dest = dest_meta(&dst, "entry", EntryType::Symlink, 0);
    dest.symlink_target = Some(PathBuf::from("/target"));

    let result = compare_entries(&source, &dest);
    assert_eq!(
        result,
        Some(ChangeKind::TypeChanged {
            old_type: EntryType::Symlink,
            new_type: EntryType::RegularFile,
        })
    );
}

#[test]
fn symlink_target_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("link_src");
    let dst = tmp.path().join("link_dst");
    std::os::unix::fs::symlink("/new/target", &src).unwrap();
    std::os::unix::fs::symlink("/old/target", &dst).unwrap();

    let mut source = source_meta(&src, "link", EntryType::Symlink, 0);
    source.symlink_target = Some(PathBuf::from("/new/target"));
    let mut dest = dest_meta(&dst, "link", EntryType::Symlink, 0);
    dest.symlink_target = Some(PathBuf::from("/old/target"));

    let result = compare_entries(&source, &dest);
    assert_eq!(
        result,
        Some(ChangeKind::SymlinkTargetChanged {
            old_target: PathBuf::from("/old/target"),
            new_target: PathBuf::from("/new/target"),
        })
    );
}

#[test]
fn identical_symlinks_report_no_change() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("link_src");
    let dst = tmp.path().join("link_dst");
    std::os::unix::fs::symlink("/same/target", &src).unwrap();
    std::os::unix::fs::symlink("/same/target", &dst).unwrap();

    let mut source = source_meta(&src, "link", EntryType::Symlink, 0);
    source.symlink_target = Some(PathBuf::from("/same/target"));
    let mut dest = dest_meta(&dst, "link", EntryType::Symlink, 0);
    dest.symlink_target = Some(PathBuf::from("/same/target"));

    assert_eq!(compare_entries(&source, &dest), None);
}

#[test]
fn unreadable_source_treated_as_different() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("gone.txt"); // Does not exist
    let dst = tmp.path().join("dst.txt");
    fs::write(&dst, "data").unwrap();

    let source = source_meta(&src, "file.txt", EntryType::RegularFile, 4);
    let dest = dest_meta(&dst, "file.txt", EntryType::RegularFile, 4);

    // Unreadable source → treated as changed (will trigger re-copy attempt)
    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ContentChanged)
    );
}

#[test]
fn make_addition_creates_correct_struct() {
    let source = EntryMeta {
        source_path: PathBuf::from("/home/user/.bashrc"),
        relative_path: PathBuf::from(".bashrc"),
        entry_type: EntryType::RegularFile,
        size: 100,
        mtime_secs: 1000,
        symlink_target: None,
    };

    let addition = make_addition(&source, PathBuf::from("/repo/home/.bashrc"));

    assert_eq!(addition.source, PathBuf::from("/home/user/.bashrc"));
    assert_eq!(addition.destination, PathBuf::from("/repo/home/.bashrc"));
    assert_eq!(addition.entry_type, EntryType::RegularFile);
}

#[test]
fn make_modification_creates_correct_struct() {
    let source = EntryMeta {
        source_path: PathBuf::from("/home/user/.bashrc"),
        relative_path: PathBuf::from(".bashrc"),
        entry_type: EntryType::RegularFile,
        size: 100,
        mtime_secs: 1000,
        symlink_target: None,
    };

    let modification = make_modification(
        &source,
        PathBuf::from("/repo/home/.bashrc"),
        ChangeKind::ContentChanged,
    );

    assert_eq!(modification.source, PathBuf::from("/home/user/.bashrc"));
    assert_eq!(
        modification.destination,
        PathBuf::from("/repo/home/.bashrc")
    );
    assert_eq!(modification.change, ChangeKind::ContentChanged);
}

#[test]
fn large_identical_files_detected_as_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("large_src.bin");
    let dst = tmp.path().join("large_dst.bin");

    // Create files larger than the buffer size (8192)
    let data: Vec<u8> = (0..20000).map(|i| (i % 256) as u8).collect();
    fs::write(&src, &data).unwrap();
    fs::write(&dst, &data).unwrap();

    let source = source_meta(&src, "large.bin", EntryType::RegularFile, 20000);
    let dest = dest_meta(&dst, "large.bin", EntryType::RegularFile, 20000);

    assert_eq!(compare_entries(&source, &dest), None);
}

#[test]
fn large_files_differ_at_end() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("large_src.bin");
    let dst = tmp.path().join("large_dst.bin");

    let mut data: Vec<u8> = (0..20000).map(|i| (i % 256) as u8).collect();
    fs::write(&src, &data).unwrap();
    // Flip the last byte
    *data.last_mut().unwrap() ^= 0xFF;
    fs::write(&dst, &data).unwrap();

    let source = source_meta(&src, "large.bin", EntryType::RegularFile, 20000);
    let dest = dest_meta(&dst, "large.bin", EntryType::RegularFile, 20000);

    assert_eq!(
        compare_entries(&source, &dest),
        Some(ChangeKind::ContentChanged)
    );
}

#[test]
fn empty_files_are_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("empty_src");
    let dst = tmp.path().join("empty_dst");
    fs::write(&src, "").unwrap();
    fs::write(&dst, "").unwrap();

    let source = source_meta(&src, "empty", EntryType::RegularFile, 0);
    let dest = dest_meta(&dst, "empty", EntryType::RegularFile, 0);

    assert_eq!(compare_entries(&source, &dest), None);
}
