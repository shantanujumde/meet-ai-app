//! The config writer's tests for where the file lands (TUR-155): through a
//! symlinked `config.jsonc`, and with a schema that cannot be written.

use std::fs;

use super::FILE;
use super::app_section::parse_app;
use super::file::{SCHEMA, SCHEMA_FILE, read_in, with_section, write_in};

fn save_dock_on(dir: &std::path::Path) -> Result<(), super::ConfigError> {
    write_in(dir, |raw| {
        with_section(raw, "app", vec![("show_in_dock_when_closed", true.into())])
    })
}

#[test]
fn a_symlinked_config_stays_a_link_and_its_target_gets_the_save() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("app-dir");
    let dotfiles = temp.path().join("dotfiles");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(&dotfiles).unwrap();
    let target = dotfiles.join("meet-ai.jsonc");
    fs::write(&target, "{\n  // mine\n}\n").unwrap();
    let link = dir.join(FILE);
    if let Err(error) = crate::meetings::symlink(&target, &link, false) {
        // Windows without Developer Mode cannot make one at all.
        eprintln!("skipped: cannot make a symlink here: {error}");
        return;
    }

    save_dock_on(&dir).unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "config.jsonc is still a link"
    );
    let saved = fs::read_to_string(&target).unwrap();
    assert!(saved.contains("// mine"), "{saved}");
    assert!(parse_app(&saved).show_in_dock_when_closed, "{saved}");
    assert_eq!(read_in(&dir).unwrap(), saved);
}

#[test]
fn a_relative_link_resolves_from_its_own_folder() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("app-dir");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("real.jsonc"), "{}").unwrap();
    let link = dir.join(FILE);
    if let Err(error) = crate::meetings::symlink(std::path::Path::new("real.jsonc"), &link, false) {
        eprintln!("skipped: cannot make a symlink here: {error}");
        return;
    }

    save_dock_on(&dir).unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let saved = fs::read_to_string(dir.join("real.jsonc")).unwrap();
    assert!(parse_app(&saved).show_in_dock_when_closed, "{saved}");
}

#[test]
fn a_schema_that_cannot_be_written_does_not_fail_a_save_that_landed() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("app-dir");
    // A folder where the schema goes: nothing can be renamed over it.
    fs::create_dir_all(dir.join(SCHEMA_FILE)).unwrap();

    save_dock_on(&dir).unwrap();

    assert!(parse_app(&read_in(&dir).unwrap()).show_in_dock_when_closed);
    assert!(dir.join(SCHEMA_FILE).is_dir());
}

#[test]
fn the_schema_is_written_beside_a_normal_save() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("app-dir");
    save_dock_on(&dir).unwrap();
    assert_eq!(fs::read_to_string(dir.join(SCHEMA_FILE)).unwrap(), SCHEMA);
}
