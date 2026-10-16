use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::vehicle::config::VehiclePhysicsConfig;

const LAST_PROFILE: &str = ".last-profile.json";
const MAX_JSON_BYTES: u64 = 1_048_576;
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Resource)]
pub struct ProfileStore {
    pub root: PathBuf,
    pub selected: String,
    pub names: Vec<String>,
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LastProfile {
    selected: String,
}

impl ProfileStore {
    pub fn bootstrap(root: impl Into<PathBuf>) -> (VehiclePhysicsConfig, Self) {
        let mut store = Self {
            root: root.into(),
            selected: "Baseline".into(),
            names: Vec::new(),
            status: String::new(),
            last_error: None,
        };
        if let Err(error) = ensure_directory(&store.root) {
            store.startup_error(error);
            return (VehiclePhysicsConfig::baseline(), store);
        }

        let mut errors = Vec::new();
        for name in VehiclePhysicsConfig::preset_names() {
            let path = store.root.join(format!("{name}.json"));
            let result = match regular_file_exists(&path) {
                Ok(true) => Ok(()),
                Ok(false) => match VehiclePhysicsConfig::preset(name) {
                    Some(config) => write_json(&path, &config, false),
                    None => Err(format!("Unknown built-in preset {name:?}")),
                },
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                errors.push(error);
            }
        }
        if let Err(error) = store.refresh() {
            errors.push(error);
        }

        let restored = (|| {
            let marker = store.root.join(LAST_PROFILE);
            let name = if regular_file_exists(&marker)? {
                let data = read_json_bytes(&marker)?;
                let marker: LastProfile = serde_json::from_slice(&data)
                    .map_err(|error| format!("Invalid last-profile marker: {error}"))?;
                validate_name(&marker.selected)
                    .map_err(|error| format!("Invalid last-profile marker: {error}"))?;
                marker.selected
            } else {
                "Baseline".to_string()
            };
            let config = store.compare(&name)?;
            Ok::<_, String>((config, name))
        })();

        let config = match restored {
            Ok((config, name)) => {
                store.selected = name;
                store.status = format!("Loaded profile {:?}", store.selected);
                config
            }
            Err(error) => {
                errors.push(error);
                VehiclePhysicsConfig::baseline()
            }
        };
        if !errors.is_empty() {
            let error = errors.join("; ");
            store.status =
                if store.selected == "Baseline" && config == VehiclePhysicsConfig::baseline() {
                    format!("Using built-in Baseline fallback: {error}")
                } else {
                    format!(
                        "Loaded profile {:?}; startup warning: {error}",
                        store.selected
                    )
                };
            store.last_error = Some(error);
        }
        (config, store)
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        match list_profiles(&self.root) {
            Ok(names) => {
                self.names = names;
                self.status = format!("{} profiles available", self.names.len());
                self.last_error = None;
                Ok(())
            }
            Err(error) => self.fail(error),
        }
    }

    pub fn load(&mut self, name: &str) -> Result<VehiclePhysicsConfig, String> {
        let config = match self.compare(name) {
            Ok(config) => config,
            Err(error) => return self.fail(error),
        };
        if let Err(error) = self.select(name) {
            return self.fail(format!("Could not load profile {name:?}: {error}"));
        }
        self.status = format!("Loaded profile {name:?}");
        self.last_error = None;
        Ok(config)
    }

    pub fn save(&mut self, name: &str, config: &VehiclePhysicsConfig) -> Result<(), String> {
        if let Err(error) = validate_name(name) {
            return self.fail(error);
        }
        if let Err(error) = config.validate() {
            return self.fail(format!("Cannot save profile {name:?}: {error}"));
        }
        if let Err(error) = ensure_directory(&self.root) {
            return self.fail(error);
        }
        let path = self.root.join(format!("{name}.json"));
        if let Err(error) = write_json(&path, config, true) {
            return self.fail(error);
        }
        if let Err(error) = self.refresh() {
            return self.fail(format!(
                "Profile {name:?} saved, but profile refresh failed: {error}"
            ));
        }
        if let Err(error) = self.select(name) {
            return self.fail(format!(
                "Profile {name:?} saved, but its last-profile marker was not saved; selection unchanged: {error}"
            ));
        }
        self.status = format!("Saved profile {name:?}");
        self.last_error = None;
        Ok(())
    }

    pub fn compare(&self, name: &str) -> Result<VehiclePhysicsConfig, String> {
        validate_name(name)?;
        require_directory(&self.root)?;
        let path = self.root.join(format!("{name}.json"));
        let bytes = read_json_bytes(&path)?;
        let config: VehiclePhysicsConfig = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid profile {name:?} ({}): {error}", path.display()))?;
        config
            .validate()
            .map_err(|error| format!("Invalid profile {name:?}: {error}"))?;
        Ok(config)
    }

    fn select(&mut self, name: &str) -> Result<(), String> {
        write_json(
            &self.root.join(LAST_PROFILE),
            &LastProfile {
                selected: name.to_string(),
            },
            true,
        )
        .map_err(|error| format!("Could not write last-profile marker: {error}"))?;
        // Commit the visible selection only after its restoration marker is saved.
        self.selected = name.to_string();
        Ok(())
    }

    fn fail<T>(&mut self, error: String) -> Result<T, String> {
        self.status = error.clone();
        self.last_error = Some(error.clone());
        Err(error)
    }

    fn startup_error(&mut self, error: String) {
        self.status = format!("Using built-in Baseline fallback: {error}");
        self.last_error = Some(error);
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 || name.trim() != name {
        return Err(
            "Profile name must contain 1..=64 ASCII characters without leading or trailing spaces"
                .into(),
        );
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_'))
    {
        return Err(format!(
            "Unsafe profile name {name:?}: use letters, digits, spaces, '-' or '_', without a path or extension"
        ));
    }
    let upper = name.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Err(format!(
            "Unsafe profile name {name:?}: reserved Windows filename"
        ));
    }
    Ok(())
}

fn ensure_directory(root: &Path) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| {
        format!(
            "Cannot create profile directory {}: {error}",
            root.display()
        )
    })?;
    require_directory(root)
}

fn require_directory(root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        format!(
            "Cannot access profile directory {}: {error}",
            root.display()
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "Profile root {} must be a directory, not a symbolic link",
            root.display()
        ));
    }
    Ok(())
}

fn regular_file_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(format!(
            "{} must be a regular file, not a directory or symbolic link",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot inspect {}: {error}", path.display())),
    }
}

fn read_json_bytes(path: &Path) -> Result<Vec<u8>, String> {
    if !regular_file_exists(path)? {
        return Err(format!("Profile file {} does not exist", path.display()));
    }
    let file =
        File::open(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(format!("{} exceeds the 1 MiB JSON limit", path.display()));
    }
    Ok(bytes)
}

fn list_profiles(root: &Path) -> Result<Vec<String>, String> {
    require_directory(root)?;
    let entries = fs::read_dir(root)
        .map_err(|error| format!("Cannot list profile directory {}: {error}", root.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Cannot read profile directory entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Cannot inspect profile entry: {error}"))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) {
            if validate_name(name).is_ok() {
                names.push(name.to_string());
            }
        }
    }
    let presets = VehiclePhysicsConfig::preset_names();
    names.sort_by_key(|name| {
        (
            presets
                .iter()
                .position(|preset| *preset == name)
                .unwrap_or(presets.len()),
            name.to_ascii_lowercase(),
            name.clone(),
        )
    });
    Ok(names)
}

fn write_json<T: Serialize>(path: &Path, value: &T, replace: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    require_directory(parent)?;
    let exists = regular_file_exists(path)?;
    if exists && !replace {
        return Ok(());
    }
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Cannot serialize {}: {error}", path.display()))?;
    bytes.push(b'\n');

    let (temporary_path, mut temporary_file) = (0..64)
        .find_map(|_| {
            let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let temporary_path = parent.join(format!(
                ".motor-trail-write-{}-{sequence}.tmp",
                std::process::id()
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary_path)
            {
                Ok(file) => Some(Ok((temporary_path, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(format!(
                    "Cannot create temporary file for {}: {error}",
                    path.display()
                ))),
            }
        })
        .unwrap_or_else(|| {
            Err(format!(
                "Cannot allocate a temporary file for {}",
                path.display()
            ))
        })?;

    let written = temporary_file
        .write_all(&bytes)
        .and_then(|_| temporary_file.sync_all());
    drop(temporary_file);
    let result = written
        .map_err(|error| format!("Cannot write {}: {error}", path.display()))
        .and_then(|_| {
            regular_file_exists(path)?;
            if replace {
                fs::rename(&temporary_path, path)
                    .map_err(|error| format!("Cannot replace {}: {error}", path.display()))
            } else {
                // Publishing a missing preset must not overwrite a concurrent user save.
                match fs::hard_link(&temporary_path, path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        regular_file_exists(path).and_then(|exists| {
                            if exists {
                                Ok(())
                            } else {
                                Err(format!("{} disappeared while saving", path.display()))
                            }
                        })
                    }
                    Err(error) => Err(format!("Cannot create {}: {error}", path.display())),
                }
            }
        });
    if let Err(error) = fs::remove_file(&temporary_path) {
        if error.kind() != std::io::ErrorKind::NotFound && result.is_ok() {
            return Err(format!(
                "Saved {}, but could not remove temporary file {}: {error}",
                path.display(),
                temporary_path.display()
            ));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
        parent: PathBuf,
        owner: String,
    }

    impl TestDir {
        fn new() -> Self {
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let owner = format!(
                "motor-trail-profiles-test-{}-{timestamp}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            );
            let path = parent.join(&owner);
            fs::create_dir(&path).unwrap();
            fs::write(path.join(".test-owner"), &owner).unwrap();
            Self {
                path,
                parent,
                owner,
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let owned = self.path.parent() == Some(self.parent.as_path())
                && self.path.file_name().and_then(|name| name.to_str()) == Some(&self.owner)
                && self.owner.starts_with("motor-trail-profiles-test-")
                && fs::symlink_metadata(&self.path)
                    .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                    .unwrap_or(false)
                && self.path.canonicalize().ok().as_ref() == Some(&self.path)
                && fs::read_to_string(self.path.join(".test-owner"))
                    .ok()
                    .as_ref()
                    == Some(&self.owner);
            if owned {
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }

    #[test]
    fn bootstrap_populates_valid_presets_without_global_settings() {
        let directory = TestDir::new();
        let root = directory.path.join("profiles");
        let (config, store) = ProfileStore::bootstrap(&root);
        assert_eq!(config, VehiclePhysicsConfig::baseline());
        assert_eq!(store.selected, "Baseline");
        assert_eq!(store.names, VehiclePhysicsConfig::preset_names());
        assert!(store.last_error.is_none(), "{:?}", store.last_error);
        for name in VehiclePhysicsConfig::preset_names() {
            assert_eq!(
                store.compare(name).unwrap(),
                VehiclePhysicsConfig::preset(name).unwrap()
            );
            assert!(root.join(format!("{name}.json")).is_file());
        }
    }

    #[test]
    fn saves_and_loads_two_distinct_profiles() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        let first = VehiclePhysicsConfig::preset("Confort").unwrap();
        let second = VehiclePhysicsConfig::preset("Dynamique").unwrap();
        store.save("Trail A", &first).unwrap();
        store.save("Trail B", &second).unwrap();
        assert!(store.names.contains(&"Trail A".to_string()));
        assert!(store.names.contains(&"Trail B".to_string()));
        assert_eq!(store.load("Trail A").unwrap(), first);
        assert_eq!(store.selected, "Trail A");
        assert_eq!(store.load("Trail B").unwrap(), second);
        assert_eq!(store.selected, "Trail B");
        assert!(store.last_error.is_none());
    }

    #[test]
    fn replacing_a_profile_preserves_valid_json_and_leaves_no_temporary_files() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store
            .save("Trial", &VehiclePhysicsConfig::baseline())
            .unwrap();
        let replacement = VehiclePhysicsConfig::original();
        store.save("Trial", &replacement).unwrap();
        assert_eq!(store.compare("Trial").unwrap(), replacement);
        assert!(fs::read_dir(&directory.path).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
    }

    #[test]
    fn restart_restores_the_last_selected_profile() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        let expected = VehiclePhysicsConfig::preset("Franchissement").unwrap();
        store.save("Rock trail", &expected).unwrap();
        let (restored, store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(restored, expected);
        assert_eq!(store.selected, "Rock trail");
        assert!(store.last_error.is_none());
    }

    #[test]
    fn loading_a_builtin_also_updates_the_restoration_marker() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store.load("Confort").unwrap();
        let (restored, store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(restored, VehiclePhysicsConfig::preset("Confort").unwrap());
        assert_eq!(store.selected, "Confort");
    }

    #[test]
    fn compare_does_not_change_selection_or_marker() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store.load("Confort").unwrap();
        let before = fs::read(directory.path.join(".last-profile.json")).unwrap();
        assert_eq!(
            store.compare("Dynamique").unwrap(),
            VehiclePhysicsConfig::preset("Dynamique").unwrap()
        );
        assert_eq!(store.selected, "Confort");
        assert_eq!(
            fs::read(directory.path.join(".last-profile.json")).unwrap(),
            before
        );
    }

    #[test]
    fn invalid_selected_data_falls_back_with_a_clear_error() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store.load("Confort").unwrap();
        fs::write(directory.path.join("Confort.json"), "{broken json").unwrap();
        let (restored, mut store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(restored, VehiclePhysicsConfig::baseline());
        assert_eq!(store.selected, "Baseline");
        assert!(store.last_error.as_ref().unwrap().contains("Confort"));
        assert!(store.status.contains("Baseline"));
        assert_eq!(
            fs::read_to_string(directory.path.join("Confort.json")).unwrap(),
            "{broken json"
        );
        store.load("Baseline").unwrap();
        assert!(store.last_error.is_none());
        let (_, restored_store) = ProfileStore::bootstrap(&directory.path);
        assert!(restored_store.last_error.is_none());
        assert_eq!(restored_store.selected, "Baseline");
    }

    #[test]
    fn invalid_baseline_file_is_not_used_or_overwritten_on_startup() {
        let directory = TestDir::new();
        fs::write(
            directory.path.join("Baseline.json"),
            r#"{"chassis":{"mass_kg":-1}}"#,
        )
        .unwrap();
        let (config, store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(config, VehiclePhysicsConfig::baseline());
        assert_eq!(store.selected, "Baseline");
        assert!(store.last_error.as_ref().unwrap().contains("mass_kg"));
        assert!(store.compare("Baseline").is_err());
    }

    #[test]
    fn invalid_or_missing_last_profile_falls_back() {
        for marker in [
            "{bad",
            r#"{"selected":"Missing"}"#,
            r#"{"selected":"../outside"}"#,
        ] {
            let directory = TestDir::new();
            ProfileStore::bootstrap(&directory.path);
            fs::write(directory.path.join(".last-profile.json"), marker).unwrap();
            let (config, store) = ProfileStore::bootstrap(&directory.path);
            assert_eq!(config, VehiclePhysicsConfig::baseline());
            assert_eq!(store.selected, "Baseline");
            assert!(store.last_error.is_some(), "{marker}");
        }
    }

    #[test]
    fn load_rejects_invalid_data_without_changing_the_selection() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store.load("Confort").unwrap();
        fs::write(
            directory.path.join("Broken.json"),
            r#"{"tires":{"grip_coefficient":99}}"#,
        )
        .unwrap();
        assert!(
            store
                .load("Broken")
                .unwrap_err()
                .contains("grip_coefficient")
        );
        assert_eq!(store.selected, "Confort");
        assert!(store.last_error.is_some());
        let (_, restored_store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(restored_store.selected, "Confort");
    }

    #[test]
    fn invalid_save_does_not_replace_an_existing_profile() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        let good = VehiclePhysicsConfig::baseline();
        store.save("Trial", &good).unwrap();
        let mut invalid = good.clone();
        invalid.chassis.mass_kg = f32::NAN;
        assert!(store.save("Trial", &invalid).is_err());
        assert_eq!(store.compare("Trial").unwrap(), good);
        assert!(store.last_error.is_some());
    }

    #[test]
    fn traversal_and_unsafe_windows_names_are_rejected() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "..\\escape",
            "C:\\escape",
            "/escape",
            "\\\\server\\share",
            "CON",
            "con",
            "PRN",
            "AUX",
            "NUL",
            "COM1",
            "com9",
            "LPT1",
            "lpt9",
            "CON.txt",
            "Bad.",
            " Bad",
            "Bad ",
            "Bad:name",
            "Bad?name",
            "Bad*name",
            "Bad|name",
            "Bad<name",
            "Bad>name",
            "Bad\"name",
            "Bad\nname",
            ".last-profile",
            "Trial.json",
        ] {
            assert!(
                store.save(name, &VehiclePhysicsConfig::baseline()).is_err(),
                "{name:?}"
            );
            assert!(store.load(name).is_err(), "{name:?}");
            assert!(store.compare(name).is_err(), "{name:?}");
        }
        assert!(
            store
                .save(&"A".repeat(65), &VehiclePhysicsConfig::baseline())
                .is_err()
        );
        assert_eq!(store.selected, "Baseline");
    }

    #[test]
    fn refresh_lists_only_safe_profile_files_in_a_stable_order() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store
            .save("Zulu", &VehiclePhysicsConfig::baseline())
            .unwrap();
        store
            .save("Alpha", &VehiclePhysicsConfig::baseline())
            .unwrap();
        fs::write(directory.path.join("notes.txt"), "notes").unwrap();
        fs::write(directory.path.join("hidden.option.json"), "{}").unwrap();
        fs::create_dir(directory.path.join("Directory.json")).unwrap();
        store.refresh().unwrap();
        let mut expected: Vec<String> = VehiclePhysicsConfig::preset_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        expected.extend(["Alpha".into(), "Zulu".into()]);
        assert_eq!(store.names, expected);
    }

    #[test]
    fn marker_write_failure_is_reported_and_does_not_select_an_unrestorable_profile() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        let marker = directory.path.join(".last-profile.json");
        if marker.is_file() {
            fs::remove_file(&marker).unwrap();
        }
        fs::create_dir(&marker).unwrap();
        let error = store.load("Confort").unwrap_err();
        assert!(error.contains("last-profile"), "{error}");
        assert_eq!(store.selected, "Baseline");
        assert_eq!(store.last_error, Some(error));
        let error = store
            .save("Saved", &VehiclePhysicsConfig::original())
            .unwrap_err();
        assert!(error.contains("saved"), "{error}");
        assert_eq!(store.selected, "Baseline");
        assert_eq!(
            store.compare("Saved").unwrap(),
            VehiclePhysicsConfig::original()
        );
    }

    #[test]
    fn unusable_profile_root_does_not_panic() {
        let directory = TestDir::new();
        let root = directory.path.join("regular-file");
        fs::write(&root, "not a directory").unwrap();
        let (config, mut store) = ProfileStore::bootstrap(&root);
        assert_eq!(config, VehiclePhysicsConfig::baseline());
        assert_eq!(store.selected, "Baseline");
        assert!(store.last_error.is_some());
        assert!(store.refresh().is_err());
    }

    #[test]
    fn existing_builtin_profiles_are_preserved_on_bootstrap() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        let customized = VehiclePhysicsConfig::preset("Dynamique").unwrap();
        store.save("Baseline", &customized).unwrap();
        let before = fs::read(directory.path.join("Baseline.json")).unwrap();
        let (restored, store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(restored, customized);
        assert_eq!(store.selected, "Baseline");
        assert_eq!(
            fs::read(directory.path.join("Baseline.json")).unwrap(),
            before
        );
    }

    #[test]
    fn save_rejects_a_directory_in_place_of_a_profile_file() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        let occupied = directory.path.join("Occupied.json");
        fs::create_dir(&occupied).unwrap();
        let error = store
            .save("Occupied", &VehiclePhysicsConfig::baseline())
            .unwrap_err();
        assert!(error.contains("regular file"), "{error}");
        assert!(occupied.is_dir());
        assert_eq!(store.selected, "Baseline");
    }

    #[test]
    fn oversized_selected_json_falls_back_before_deserialization() {
        let directory = TestDir::new();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        store
            .save("Oversized", &VehiclePhysicsConfig::baseline())
            .unwrap();
        fs::write(
            directory.path.join("Oversized.json"),
            vec![b' '; MAX_JSON_BYTES as usize + 1],
        )
        .unwrap();
        let (config, store) = ProfileStore::bootstrap(&directory.path);
        assert_eq!(config, VehiclePhysicsConfig::baseline());
        assert!(store.last_error.unwrap().contains("1 MiB"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_profile_files_are_not_read_written_or_listed() {
        let directory = TestDir::new();
        let outside = TestDir::new();
        let target = outside.path.join("outside.json");
        fs::write(&target, "{}").unwrap();
        let (_, mut store) = ProfileStore::bootstrap(&directory.path);
        std::os::unix::fs::symlink(&target, directory.path.join("Linked.json")).unwrap();
        assert!(store.load("Linked").is_err());
        assert!(
            store
                .save("Linked", &VehiclePhysicsConfig::original())
                .is_err()
        );
        store.refresh().unwrap();
        assert!(!store.names.contains(&"Linked".to_string()));
        assert_eq!(fs::read_to_string(target).unwrap(), "{}");
    }
}
