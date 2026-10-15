//! Bounded snapshot documents, published by a same-directory atomic rename.
use std::ffi::OsStr;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::scene::{ReferenceScene, TerrainScene, validate_name};

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;
static WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct TerrainStore {
    pub root: PathBuf,
}

impl Default for TerrainStore {
    fn default() -> Self {
        Self::new("configs/terrains")
    }
}

impl TerrainStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Lists saved user documents; reference scenes are available through ReferenceScene::ALL.
    pub fn list(&self) -> Result<Vec<String>, String> {
        if !check_directory(&self.root, false)? {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.root)
            .map_err(|e| format!("Cannot list terrains in {}: {e}", self.root.display()))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("Cannot read terrain directory entry: {e}"))?;
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)
                .map_err(|e| format!("Cannot inspect terrain {}: {e}", path.display()))?;
            if is_link(&metadata) {
                return Err(format!(
                    "Terrain {} must not be a symbolic link or reparse point",
                    path.display()
                ));
            }
            if !metadata.is_file() {
                continue;
            }
            if let Some(name) = path.file_stem().and_then(|v| v.to_str()) {
                if validate_name(name).is_ok() && reference_named(name).is_none() {
                    names.push(name.to_owned());
                }
            }
        }
        names.sort_by_key(|name| (name.to_ascii_lowercase(), name.clone()));
        Ok(names)
    }

    pub fn load(&self, name: &str) -> Result<TerrainScene, String> {
        validate_name(name)?;
        if let Some(reference) = reference_named(name) {
            return TerrainScene::reference(reference);
        }
        if !check_directory(&self.root, false)? {
            return Err(format!(
                "Terrain directory {} does not exist",
                self.root.display()
            ));
        }
        let path = self.root.join(format!("{name}.json"));
        if !regular_file(&path)? {
            return Err(format!("Terrain {name:?} does not exist"));
        }
        let file = File::open(&path).map_err(|e| format!("Cannot read terrain {name:?}: {e}"))?;
        let metadata = file
            .metadata()
            .map_err(|e| format!("Cannot inspect terrain {name:?}: {e}"))?;
        if !metadata.is_file() || metadata.len() > MAX_JSON_BYTES {
            return Err(format!(
                "Terrain {name:?} must be a regular JSON file no larger than 16 MiB"
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_JSON_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Cannot read terrain {name:?}: {e}"))?;
        if bytes.len() as u64 > MAX_JSON_BYTES {
            return Err(format!("Terrain {name:?} exceeds 16 MiB"));
        }
        let scene: TerrainScene = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid terrain JSON {name:?}: {e}"))?;
        scene
            .validate()
            .map_err(|e| format!("Invalid terrain {name:?}: {e}"))?;
        if scene.name != name {
            return Err(format!(
                "Terrain document name {:?} does not match filename {name:?}",
                scene.name
            ));
        }
        Ok(scene)
    }

    /// Existing user files may be replaced; confirmation belongs to the runtime UI.
    pub fn save(&self, name: &str, scene: &TerrainScene) -> Result<(), String> {
        validate_name(name)?;
        if reference_named(name).is_some() {
            return Err(format!(
                "Reference terrain {name:?} is protected; save under a different name"
            ));
        }
        scene.validate()?;
        let mut document = scene.clone();
        document.name = name.to_owned();
        document.validate()?;
        let mut bytes = serde_json::to_vec_pretty(&document)
            .map_err(|e| format!("Cannot serialize terrain {name:?}: {e}"))?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_JSON_BYTES {
            return Err("Terrain JSON exceeds 16 MiB".into());
        }
        check_directory(&self.root, true)?;
        let path = self.root.join(format!("{name}.json"));
        regular_file(&path)?;
        let (temporary_path, mut temporary_file) = create_temporary(&self.root)?;
        let result = (|| {
            temporary_file
                .write_all(&bytes)
                .and_then(|_| temporary_file.sync_all())
                .map_err(|e| format!("Cannot write terrain {name:?}: {e}"))?;
            drop(temporary_file);
            // Recheck root and destination immediately before publication.
            check_directory(&self.root, false)?;
            regular_file(&path)?;
            fs::rename(&temporary_path, &path)
                .map_err(|e| format!("Cannot replace terrain {name:?}: {e}"))?;
            #[cfg(unix)]
            File::open(&self.root)
                .and_then(|directory| directory.sync_all())
                .map_err(|e| format!("Terrain {name:?} saved, but directory sync failed: {e}"))?;
            Ok(())
        })();
        if let Err(e) = fs::remove_file(&temporary_path) {
            if e.kind() != std::io::ErrorKind::NotFound && result.is_ok() {
                return Err(format!("Terrain saved, but temporary cleanup failed: {e}"));
            }
        }
        result
    }
}

fn reference_named(name: &str) -> Option<ReferenceScene> {
    ReferenceScene::ALL
        .into_iter()
        .find(|reference| reference.name().eq_ignore_ascii_case(name))
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions, which are not always reported by is_symlink().
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn check_directory(root: &Path, create: bool) -> Result<bool, String> {
    // Check the literal text too, whether classified as ParentDir or Normal.
    if root.as_os_str().is_empty()
        || root.components().any(|component| {
            matches!(component, Component::ParentDir)
                || matches!(component, Component::Normal(name) if name == OsStr::new(".."))
        })
    {
        return Err("Terrain root must not be empty or contain parent traversal".into());
    }
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Cannot resolve terrain root: {e}"))?
            .join(root)
    };
    let mut path = PathBuf::new();
    for component in absolute.components() {
        path.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) => require_real_directory(&path, &metadata)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(false);
                }
                match fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => {
                        return Err(format!(
                            "Cannot create terrain directory {}: {e}",
                            path.display()
                        ));
                    }
                }
                let metadata = fs::symlink_metadata(&path).map_err(|e| {
                    format!("Cannot inspect terrain directory {}: {e}", path.display())
                })?;
                require_real_directory(&path, &metadata)?;
            }
            Err(e) => {
                return Err(format!(
                    "Cannot inspect terrain directory {}: {e}",
                    path.display()
                ));
            }
        }
    }
    Ok(true)
}

fn require_real_directory(path: &Path, metadata: &Metadata) -> Result<(), String> {
    if !metadata.is_dir() || is_link(metadata) {
        return Err(format!(
            "Terrain directory {} must be a directory without symbolic links or reparse points",
            path.display()
        ));
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => Ok(true),
        Ok(_) => Err(format!(
            "Terrain {} must be a regular file without symbolic links or reparse points",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Cannot inspect terrain {}: {e}", path.display())),
    }
}

fn create_temporary(root: &Path) -> Result<(PathBuf, File), String> {
    for _ in 0..64 {
        let sequence = WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            ".motor-trail-terrain-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create temporary terrain file: {e}")),
        }
    }
    Err("Cannot allocate temporary terrain file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap();
            loop {
                let path = root.join(format!(
                    "motor-trail-terrains-test-{}-{}",
                    std::process::id(),
                    WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("{e}"),
                }
            }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let owned = self.0.parent() == std::env::temp_dir().canonicalize().ok().as_deref()
                && self
                    .0
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("motor-trail-terrains-test-")
                && self.0.canonicalize().ok().as_ref() == Some(&self.0)
                && fs::symlink_metadata(&self.0)
                    .map(|m| m.is_dir() && !is_link(&m))
                    .unwrap_or(false);
            if owned {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn save_as_roundtrips_edited_heights_objects_spawn_and_replaces_file() {
        let dir = TestDir::new();
        let store = TerrainStore::new(&dir.0);
        let mut scene = TerrainScene::reference(ReferenceScene::Rocks).unwrap();
        scene.terrain.heights[10] += 0.75;
        scene.spawn.yaw_degrees = 35.0;
        store.save("Essai", &scene).unwrap();
        let mut expected = scene.clone();
        expected.name = "Essai".into();
        assert_eq!(store.load("Essai").unwrap(), expected);
        scene.obstacles[0].size = [1.2, 0.8, 2.3];
        store.save("Essai", &scene).unwrap();
        expected.obstacles = scene.obstacles.clone();
        assert_eq!(store.load("Essai").unwrap(), expected);
        assert_eq!(store.list().unwrap(), vec!["Essai"]);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn protected_references_cannot_be_shadowed_case_insensitively() {
        let dir = TestDir::new();
        let store = TerrainStore::new(&dir.0);
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        for reference in ReferenceScene::ALL {
            assert!(
                store
                    .save(&reference.name().to_ascii_lowercase(), &scene)
                    .is_err()
            );
        }
        fs::write(
            dir.0.join(format!("{}.json", ReferenceScene::Flat.name())),
            b"bad",
        )
        .unwrap();
        assert_eq!(store.load(ReferenceScene::Flat.name()).unwrap(), scene);
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn invalid_json_versions_unknown_fields_and_oversize_are_rejected() {
        let dir = TestDir::new();
        let store = TerrainStore::new(&dir.0);
        let path = dir.0.join("Bad.json");
        fs::write(&path, b"{bad}").unwrap();
        assert!(store.load("Bad").is_err());
        let mut value =
            serde_json::to_value(TerrainScene::reference(ReferenceScene::Flat).unwrap()).unwrap();
        value["name"] = "Bad".into();
        value["format_version"] = 999.into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(store.load("Bad").is_err());
        value["format_version"] = 1.into();
        value["spawn"]["clearance"] = 1.into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(store.load("Bad").is_err());
        File::create(&path)
            .unwrap()
            .set_len(MAX_JSON_BYTES + 1)
            .unwrap();
        assert!(store.load("Bad").is_err());
    }

    #[test]
    fn unsafe_names_and_nonregular_targets_fail_without_touching_existing_file() {
        let dir = TestDir::new();
        let store = TerrainStore::new(&dir.0);
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        for name in [
            "../outside",
            "CON",
            "NUL",
            "com1",
            " a",
            "a ",
            "a.json",
            "a/b",
        ] {
            assert!(store.save(name, &scene).is_err());
            assert!(store.load(name).is_err());
        }
        fs::create_dir(dir.0.join("Directory.json")).unwrap();
        assert!(store.save("Directory", &scene).is_err());
        assert!(store.load("Directory").is_err());
        store.save("Kept", &scene).unwrap();
        let bytes = fs::read(dir.0.join("Kept.json")).unwrap();
        let mut invalid = scene.clone();
        invalid.terrain.heights[0] = f32::NAN;
        assert!(store.save("Kept", &invalid).is_err());
        assert_eq!(fs::read(dir.0.join("Kept.json")).unwrap(), bytes);
    }

    #[test]
    fn missing_store_reads_have_no_filesystem_side_effect() {
        let dir = TestDir::new();
        let root = dir.0.join("missing");
        let store = TerrainStore::new(&root);
        assert!(store.list().unwrap().is_empty());
        assert!(store.load("Missing").is_err());
        assert!(!root.exists());
    }

    #[test]
    fn checked_in_example_is_a_valid_snapshot() {
        let scene: TerrainScene =
            serde_json::from_str(include_str!("../../configs/terrains/Exemple Plat.json")).unwrap();
        scene.validate().unwrap();
        assert_eq!(scene.terrain.heights.len(), 9);
        assert_eq!(scene.obstacles.len(), 2);
    }

    #[test]
    fn parent_traversal_in_the_store_root_is_rejected() {
        let dir = TestDir::new();
        // PathBuf::join normalizes '..' away for Windows verbatim prefixes.
        let mut raw = dir.0.as_os_str().to_os_string();
        raw.push(std::path::MAIN_SEPARATOR_STR);
        raw.push("..");
        let root = PathBuf::from(raw);
        assert!(
            root.components()
                .any(|component| component.as_os_str() == "..")
        );
        assert_parent_root_rejected(&root);
    }

    fn assert_parent_root_rejected(root: &Path) {
        let store = TerrainStore::new(root);
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        assert!(store.list().is_err(), "list accepted {root:?}");
        assert!(store.load("Essai").is_err(), "load accepted {root:?}");
        assert!(
            store.save("Essai", &scene).is_err(),
            "save accepted {root:?}"
        );
    }

    #[test]
    fn relative_store_roots_reject_parent_traversal_before_directory_access() {
        for root in ["..", "configs/../terrains", "missing/../../terrains"] {
            assert_parent_root_rejected(Path::new(root));
        }
    }

    #[cfg(windows)]
    #[test]
    fn canonical_windows_roots_reject_unmodified_parent_components() {
        let dir = TestDir::new();
        let canonical = dir.0.canonicalize().unwrap();
        assert!(matches!(canonical.components().next(),
            Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim()));
        for suffix in [r"\..", r"\missing\..\terrains"] {
            let mut raw = canonical.as_os_str().to_os_string();
            raw.push(suffix);
            let root = PathBuf::from(raw);
            assert!(
                root.components()
                    .any(|component| component.as_os_str() == "..")
            );
            assert_parent_root_rejected(&root);
        }
    }

    #[cfg(windows)]
    #[test]
    fn valid_canonical_windows_roots_remain_usable() {
        let dir = TestDir::new();
        let canonical = dir.0.canonicalize().unwrap();
        assert!(matches!(canonical.components().next(),
            Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim()));
        let store = TerrainStore::new(canonical);
        assert!(store.list().unwrap().is_empty());
        let mut scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        scene.name = "Canonical".into();
        store.save("Canonical", &scene).unwrap();
        assert_eq!(store.load("Canonical").unwrap(), scene);
        assert_eq!(store.list().unwrap(), vec!["Canonical"]);
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_drive_and_unc_parent_roots_are_rejected_lexically() {
        for root in [r"\\?\C:\terrains\..", r"\\?\UNC\server\share\terrains\.."] {
            assert_parent_root_rejected(Path::new(root));
        }
    }

    #[test]
    fn unknown_fields_in_objects_config_and_terrain_are_rejected() {
        let dir = TestDir::new();
        let store = TerrainStore::new(&dir.0);
        let scene = TerrainScene::reference(ReferenceScene::Rocks).unwrap();
        for pointer in ["/terrain/config", "/terrain", "/obstacles/0"] {
            let mut value = serde_json::to_value(&scene).unwrap();
            value["name"] = "Bad".into();
            value.pointer_mut(pointer).unwrap()["unexpected"] = 42.into();
            fs::write(dir.0.join("Bad.json"), serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(store.load("Bad").is_err(), "{pointer}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_files_roots_and_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;
        let dir = TestDir::new();
        let real = dir.0.join("real");
        let store = TerrainStore::new(&real);
        let scene = TerrainScene::reference(ReferenceScene::Flat).unwrap();
        store.save("Original", &scene).unwrap();
        symlink(real.join("Original.json"), real.join("Linked.json")).unwrap();
        assert!(store.load("Linked").is_err());
        assert!(store.save("Linked", &scene).is_err());
        assert!(store.list().is_err());
        let link = dir.0.join("link");
        symlink(&real, &link).unwrap();
        for root in [&link, &link.join("child")] {
            let store = TerrainStore::new(root);
            assert!(store.list().is_err());
            assert!(store.load("Original").is_err());
            assert!(store.save("Other", &scene).is_err());
        }
        assert!(!real.join("Other.json").exists());
    }
}
