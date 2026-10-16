use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "motor-trail-paths-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn resources(&self, relative: &str) -> PathBuf {
        let root = self.0.join(relative);
        fs::create_dir_all(root.join("assets/vehicles")).unwrap();
        fs::create_dir_all(root.join("defaults/vehicles")).unwrap();
        fs::create_dir_all(root.join("defaults/terrains")).unwrap();
        fs::write(root.join("version.json"), b"{}").unwrap();
        root
    }

    fn complete_installation(&self) -> (PathBuf, ResourceLayout) {
        let root = self.resources("Motor-Trail");
        let source = Path::new(env!("CARGO_MANIFEST_DIR"));
        for name in crate::vehicle::config::VehiclePhysicsConfig::preset_names()
            .into_iter()
            .map(|name| format!("{name}.json"))
            .chain(["motor_trail_4x4.mounts.json".into()])
        {
            fs::copy(
                source.join("configs/vehicles").join(&name),
                root.join("defaults/vehicles").join(name),
            )
            .unwrap();
        }
        for name in ["Exemple Plat.json", "Exemple Relief.json"] {
            fs::copy(
                source.join("configs/terrains").join(name),
                root.join("defaults/terrains").join(name),
            )
            .unwrap();
        }
        fs::copy(
            source.join("assets/vehicles/motor_trail_4x4.glb"),
            root.join("assets/vehicles/motor_trail_4x4.glb"),
        )
        .unwrap();
        fs::create_dir_all(root.join("assets/3D/assets")).unwrap();
        for asset in crate::terrain::nature::NatureAsset::ALL {
            let path = asset.asset_path();
            fs::copy(
                source.join("assets").join(&path),
                root.join("assets").join(&path),
            )
            .unwrap();
        }
        fs::copy(
            source.join("assets/3D/catalog.json"),
            root.join("assets/3D/catalog.json"),
        )
        .unwrap();
        let target = match (std::env::consts::ARCH, std::env::consts::OS) {
            ("x86_64", "windows") => "x86_64-pc-windows-msvc",
            ("aarch64", "macos") => "aarch64-apple-darwin",
            ("x86_64", "macos") => "x86_64-apple-darwin",
            _ => "development-host",
        };
        fs::write(root.join("version.json"), serde_json::to_vec(&serde_json::json!({
            "tag":"v0.1.0-alpha.1", "target":target, "commit":"0123456789012345678901234567890123456789"
        })).unwrap()).unwrap();
        let layout = ResourceLayout::resolve(&root.join("motor-trail.exe"), None, true).unwrap();
        (root, layout)
    }
}

#[test]
fn nature_pack_resources_must_match_the_compiled_geometry() {
    for name in [
        "3D/assets/tree_oak_round.glb",
        "3D/assets/grass_tall.glb",
        "3D/catalog.json",
    ] {
        let directory = TestDir::new();
        let (root, paths) = directory.complete_installation();
        fs::write(root.join("assets").join(name), b"corrupt nature data").unwrap();
        assert!(
            paths.check_installation().is_err(),
            "accepted corrupt {name}"
        );
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        if self.0.parent() == std::env::temp_dir().canonicalize().ok().as_deref()
            && self.0.canonicalize().ok().as_ref() == Some(&self.0)
            && self
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("motor-trail-paths-")
        {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn windows_package_resources_are_absolute_and_relative_to_executable() {
    let directory = TestDir::new();
    let root = directory.resources("Motor-Trail");
    let paths = ResourceLayout::resolve(&root.join("motor-trail.exe"), None, true).unwrap();
    assert_eq!(paths.assets, root.join("assets"));
    assert_eq!(paths.defaults, root.join("defaults"));
    assert_eq!(paths.version, Some(root.join("version.json")));
}

#[test]
fn macos_bundle_uses_resources_and_never_the_macos_executable_directory() {
    let directory = TestDir::new();
    let resources = directory.resources("Motor-Trail.app/Contents/Resources");
    let exe = directory
        .0
        .join("Motor-Trail.app/Contents/MacOS/motor-trail");
    let paths = ResourceLayout::resolve(&exe, None, true).unwrap();
    assert_eq!(paths.assets, resources.join("assets"));
    assert_eq!(paths.defaults, resources.join("defaults"));
}

#[test]
fn incomplete_distribution_never_falls_back_to_a_development_checkout() {
    let directory = TestDir::new();
    let development = directory.0.join("checkout");
    fs::create_dir_all(development.join("assets")).unwrap();
    fs::create_dir_all(development.join("configs")).unwrap();
    assert!(
        ResourceLayout::resolve(
            &directory.0.join("motor-trail.exe"),
            Some(&development),
            true
        )
        .is_err()
    );
}

#[test]
fn cargo_target_junction_can_fall_back_to_absolute_manifest_resources() {
    let directory = TestDir::new();
    let development = directory.0.join("checkout");
    fs::create_dir_all(development.join("assets")).unwrap();
    fs::create_dir_all(development.join("configs")).unwrap();
    let paths = ResourceLayout::resolve(
        &directory.0.join("external-cache/motor-trail.exe"),
        Some(&development),
        false,
    )
    .unwrap();
    assert_eq!(paths.assets, development.join("assets"));
    assert_eq!(paths.defaults, development.join("configs"));
    assert_eq!(paths.legacy_configs, Some(development.join("configs")));
    assert!(paths.version.is_none());
}

#[test]
fn user_data_roots_follow_native_os_conventions_without_using_cwd() {
    let directory = TestDir::new();
    let base = &directory.0;
    assert_eq!(
        user_root(Platform::Windows, Some(base), None, None).unwrap(),
        base.join("MotorTrail")
    );
    assert_eq!(
        user_root(Platform::MacOs, None, Some(base), None).unwrap(),
        base.join("Library/Application Support/MotorTrail")
    );
    assert_eq!(
        user_root(Platform::Linux, None, Some(base), None).unwrap(),
        base.join(".local/share/MotorTrail")
    );
    assert_eq!(
        user_root(Platform::Linux, None, None, Some(base)).unwrap(),
        base.join("MotorTrail")
    );
    assert!(user_root(Platform::Windows, None, None, None).is_err());
    assert!(
        user_root(
            Platform::Windows,
            Some(Path::new("relative")),
            Some(base),
            None
        )
        .is_err()
    );
}

#[test]
fn initial_migration_preserves_legacy_files_existing_saves_and_selection_marker() {
    let directory = TestDir::new();
    let shipped = directory.resources("installed");
    let legacy = directory.0.join("legacy");
    let user = directory.0.join("user");
    for root in [&legacy, &user] {
        fs::create_dir_all(root.join("vehicles")).unwrap();
        fs::create_dir_all(root.join("terrains")).unwrap();
    }
    fs::write(
        shipped.join("defaults/vehicles/Baseline.json"),
        b"new default",
    )
    .unwrap();
    fs::write(shipped.join("defaults/vehicles/Confort.json"), b"comfort").unwrap();
    fs::write(
        shipped.join("defaults/vehicles/motor_trail_4x4.mounts.json"),
        b"mounts",
    )
    .unwrap();
    fs::write(
        legacy.join("vehicles/Baseline.json"),
        b"old customized baseline",
    )
    .unwrap();
    fs::write(legacy.join("vehicles/Trial.json"), b"legacy trial").unwrap();
    fs::write(
        legacy.join("vehicles/.last-profile.json"),
        br#"{"selected":"Trial"}"#,
    )
    .unwrap();
    fs::write(legacy.join("terrains/My Trail.json"), b"legacy scene").unwrap();
    fs::write(user.join("vehicles/Trial.json"), b"existing user trial").unwrap();
    fs::write(user.join("terrains/My Trail.json"), b"existing user scene").unwrap();
    let mut paths = ResourceLayout::resolve(&shipped.join("motor-trail.exe"), None, true).unwrap();
    paths.legacy_configs = Some(legacy.clone());
    paths.initialize_user_data(&user).unwrap();
    assert_eq!(
        fs::read(user.join("vehicles/Baseline.json")).unwrap(),
        b"old customized baseline"
    );
    assert_eq!(
        fs::read(user.join("vehicles/Trial.json")).unwrap(),
        b"existing user trial"
    );
    assert_eq!(
        fs::read(user.join("terrains/My Trail.json")).unwrap(),
        b"existing user scene"
    );
    assert_eq!(
        fs::read(user.join("vehicles/Confort.json")).unwrap(),
        b"comfort"
    );
    assert_eq!(
        fs::read(user.join("vehicles/.last-profile.json")).unwrap(),
        br#"{"selected":"Trial"}"#
    );
    assert!(!user.join("vehicles/motor_trail_4x4.mounts.json").exists());
    assert_eq!(
        fs::read(legacy.join("vehicles/Baseline.json")).unwrap(),
        b"old customized baseline"
    );
    assert_eq!(
        fs::read(shipped.join("defaults/vehicles/Baseline.json")).unwrap(),
        b"new default"
    );
    fs::remove_file(user.join("terrains/My Trail.json")).unwrap();
    paths.initialize_user_data(&user).unwrap();
    assert!(
        !user.join("terrains/My Trail.json").exists(),
        "completed migration must not resurrect deleted scenes"
    );
}

#[test]
fn failed_migration_keeps_sources_and_can_retry_without_replacing_user_files() {
    let directory = TestDir::new();
    let shipped = directory.resources("installed");
    fs::write(shipped.join("defaults/vehicles/Baseline.json"), b"default").unwrap();
    let user = directory.0.join("user");
    fs::create_dir_all(user.join("vehicles/Baseline.json")).unwrap();
    let paths = ResourceLayout::resolve(&shipped.join("motor-trail.exe"), None, true).unwrap();
    assert!(paths.initialize_user_data(&user).is_err());
    fs::remove_dir(user.join("vehicles/Baseline.json")).unwrap();
    paths.initialize_user_data(&user).unwrap();
    assert_eq!(
        fs::read(user.join("vehicles/Baseline.json")).unwrap(),
        b"default"
    );
}

#[test]
fn invalid_version_metadata_and_missing_resources_fail_the_headless_check() {
    let directory = TestDir::new();
    let shipped = directory.resources("installed");
    let paths = ResourceLayout::resolve(&shipped.join("motor-trail.exe"), None, true).unwrap();
    assert!(paths.check_installation().is_err());
}

#[test]
fn headless_validation_loads_real_glb_profiles_mounts_and_scenes_without_writes() {
    let directory = TestDir::new();
    let (root, paths) = directory.complete_installation();
    let before = fs::read_dir(&root).unwrap().count();
    assert_eq!(paths.check_installation().unwrap(), "v0.1.0-alpha.1");
    assert_eq!(fs::read_dir(&root).unwrap().count(), before);
    assert!(!root.join("vehicles").exists());
    assert!(!root.join(".migration-v1.json").exists());
}

#[test]
fn headless_validation_rejects_corrupt_model_defaults_and_wrong_target() {
    for corrupted in [
        "assets/vehicles/motor_trail_4x4.glb",
        "defaults/vehicles/Confort.json",
        "defaults/terrains/Exemple Plat.json",
        "defaults/vehicles/motor_trail_4x4.mounts.json",
        "version.json",
    ] {
        let directory = TestDir::new();
        let (root, paths) = directory.complete_installation();
        fs::write(root.join(corrupted), if corrupted == "version.json" {
            br#"{"tag":"v0.1.0","target":"wrong-platform","commit":"0123456789012345678901234567890123456789"}"#.as_slice()
        } else if corrupted == "defaults/vehicles/Confort.json" {
            br#"{"tires":{"radius_m":-1.0}}"#.as_slice()
        } else { b"{}".as_slice() }).unwrap();
        assert!(
            paths.check_installation().is_err(),
            "accepted corrupt {corrupted}"
        );
    }
}

#[test]
fn existing_last_profile_marker_wins_over_legacy_selection() {
    let directory = TestDir::new();
    let (root, mut paths) = directory.complete_installation();
    paths.legacy_configs = Some(paths.defaults.clone());
    fs::write(
        paths.defaults.join("vehicles/.last-profile.json"),
        br#"{"selected":"Baseline"}"#,
    )
    .unwrap();
    let user = directory.0.join("user");
    fs::create_dir_all(user.join("vehicles")).unwrap();
    let marker = br#"{"selected":"Confort"}"#;
    fs::write(user.join("vehicles/.last-profile.json"), marker).unwrap();
    paths.initialize_user_data(&user).unwrap();
    let (config, store) = crate::persistence::ProfileStore::bootstrap(user.join("vehicles"));
    assert_eq!(store.selected, "Confort");
    assert_eq!(
        config,
        crate::vehicle::config::VehiclePhysicsConfig::preset("Confort").unwrap()
    );
    assert_eq!(
        fs::read(user.join("vehicles/.last-profile.json")).unwrap(),
        marker
    );
    assert!(!root.join(".migration-v1.json").exists());
}

#[test]
fn development_legacy_corruption_and_missing_presets_still_use_store_fallback() {
    let directory = TestDir::new();
    let (_, mut paths) = directory.complete_installation();
    paths.version = None;
    paths.legacy_configs = Some(paths.defaults.clone());
    fs::write(
        paths.defaults.join("vehicles/Confort.json"),
        br#"{"tires":{"radius_m":-1.0}}"#,
    )
    .unwrap();
    fs::remove_file(paths.defaults.join("vehicles/Baseline.json")).unwrap();
    fs::write(
        paths.defaults.join("vehicles/.last-profile.json"),
        br#"{"selected":"Confort"}"#,
    )
    .unwrap();
    assert!(
        paths.check_installation().is_ok(),
        "legacy user profiles are not immutable shipped defaults"
    );
    let user = directory.0.join("user");
    paths.initialize_user_data(&user).unwrap();
    let (config, store) = crate::persistence::ProfileStore::bootstrap(user.join("vehicles"));
    assert_eq!(
        config,
        crate::vehicle::config::VehiclePhysicsConfig::baseline()
    );
    assert!(store.last_error.is_some());
    assert!(user.join("vehicles/Baseline.json").is_file());
    assert_eq!(
        fs::read(user.join("vehicles/Confort.json")).unwrap(),
        br#"{"tires":{"radius_m":-1.0}}"#
    );
}

#[test]
fn headless_glb_validation_rejects_short_binary_payload_and_malformed_lengths_without_panicking() {
    let directory = TestDir::new();
    let (root, paths) = directory.complete_installation();
    let file = root.join("assets/vehicles/motor_trail_4x4.glb");
    let mut model = fs::read(&file).unwrap();
    let json_length = u32::from_le_bytes(model[12..16].try_into().unwrap()) as usize;
    let bin_header = 20 + json_length;
    model.truncate(bin_header + 8 + 4);
    let new_length = model.len() as u32;
    model[8..12].copy_from_slice(&new_length.to_le_bytes());
    model[bin_header..bin_header + 4].copy_from_slice(&4_u32.to_le_bytes());
    fs::write(&file, &model).unwrap();
    assert!(
        paths.check_installation().is_err(),
        "short BIN payload passed JSON-only validation"
    );
    for declared in [0_u32, 8, u32::MAX] {
        model[8..12].copy_from_slice(&declared.to_le_bytes());
        fs::write(&file, &model).unwrap();
        let checked = std::panic::catch_unwind(|| paths.check_installation());
        assert!(checked.is_ok(), "malformed length {declared} panicked");
        assert!(checked.unwrap().is_err());
    }
}

#[test]
fn factory_model_check_rejects_sparse_and_matrix_accessor_forms_not_used_by_the_shipped_model() {
    for accessor in [
        serde_json::json!({"type":"VEC3", "componentType":5126, "count":1_000_000_000,
            "sparse":{"count":1,"indices":{"bufferView":0,"componentType":5121},"values":{"bufferView":0}}}),
        serde_json::json!({"type":"MAT3", "componentType":5121,"count":2,"bufferView":0}),
    ] {
        let directory = TestDir::new();
        let (root, paths) = directory.complete_installation();
        let file = root.join("assets/vehicles/motor_trail_4x4.glb");
        rewrite_model_document(&file, |document| {
            document["accessors"].as_array_mut().unwrap().push(accessor);
        });
        assert!(
            paths.check_installation().is_err(),
            "unsupported factory accessor form accepted"
        );
    }
}

#[test]
fn factory_model_check_budgets_expanded_dense_elements_not_only_encoded_bytes() {
    let directory = TestDir::new();
    let (root, paths) = directory.complete_installation();
    let file = root.join("assets/vehicles/motor_trail_4x4.glb");
    rewrite_model_document(&file, |document| {
        let accessors = document["accessors"].as_array_mut().unwrap();
        let scalar = accessors
            .iter()
            .find(|accessor| accessor["type"] == "SCALAR" && accessor["componentType"] == 5123)
            .unwrap()
            .clone();
        let count = scalar["count"].as_u64().unwrap() as usize;
        // Reuse valid ranges without allocating a large BIN payload. A
        // conservative Float32x4 expansion must exceed the budget, while
        // counting the encoded U16 values alone remains well below it.
        let copies = (256 * 1024 * 1024) / (count * 16) + 1;
        assert!(copies * count * 2 < 256 * 1024 * 1024);
        accessors.extend(std::iter::repeat_n(scalar, copies));
    });
    let error = paths
        .check_installation()
        .expect_err("expanded geometry budget was not enforced");
    assert!(
        error.contains("budget"),
        "unexpected validation error: {error}"
    );
}

fn rewrite_model_document(file: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let model = fs::read(file).unwrap();
    let json_length = u32::from_le_bytes(model[12..16].try_into().unwrap()) as usize;
    let mut document: serde_json::Value =
        serde_json::from_slice(&model[20..20 + json_length]).unwrap();
    edit(&mut document);
    let mut json = serde_json::to_vec(&document).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let bin = &model[20 + json_length..];
    let mut changed = b"glTF".to_vec();
    changed.extend(2_u32.to_le_bytes());
    changed.extend(((20 + json.len() + bin.len()) as u32).to_le_bytes());
    changed.extend((json.len() as u32).to_le_bytes());
    changed.extend(0x4E4F534A_u32.to_le_bytes());
    changed.extend(json);
    changed.extend(bin);
    fs::write(file, changed).unwrap();
}

#[cfg(any(unix, windows))]
#[test]
fn migration_rejects_source_ancestor_links_and_checks_destinations_after_completion() {
    let directory = TestDir::new();
    let (_, mut paths) = directory.complete_installation();
    let alias = directory.0.join("legacy-alias");
    directory_link(&paths.defaults, &alias);
    paths.legacy_configs = Some(alias.clone());
    let user = directory.0.join("user");
    assert!(paths.initialize_user_data(&user).is_err());
    assert!(!user.join(".migration-v1.json").exists());
    paths.legacy_configs = None;
    paths.initialize_user_data(&user).unwrap();
    assert!(
        user.join("vehicles")
            .canonicalize()
            .unwrap()
            .starts_with(&directory.0)
    );
    fs::rename(user.join("vehicles"), user.join("original-vehicles")).unwrap();
    directory_link(&user.join("original-vehicles"), &user.join("vehicles"));
    assert!(paths.initialize_user_data(&user).is_err());
    remove_directory_link(&user.join("vehicles"));
    remove_directory_link(&alias);
}

#[cfg(unix)]
fn directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}
#[cfg(windows)]
fn directory_link(target: &Path, link: &Path) {
    let plain = |path: &Path| {
        path.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned()
    };
    let result = std::process::Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(plain(link))
        .arg(plain(target))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "junction creation failed: {:?}",
        result
    );
}
#[cfg(unix)]
fn remove_directory_link(link: &Path) {
    fs::remove_file(link).unwrap();
}
#[cfg(windows)]
fn remove_directory_link(link: &Path) {
    fs::remove_dir(link).unwrap();
}

#[cfg(unix)]
#[test]
fn migration_rejects_destination_file_links_without_touching_external_files() {
    let directory = TestDir::new();
    let (_, paths) = directory.complete_installation();
    let external = directory.0.join("external.json");
    fs::write(&external, b"do not modify").unwrap();
    let user = directory.0.join("user");
    fs::create_dir_all(user.join("vehicles")).unwrap();
    std::os::unix::fs::symlink(&external, user.join("vehicles/Baseline.json")).unwrap();
    assert!(paths.initialize_user_data(&user).is_err());
    assert_eq!(fs::read(external).unwrap(), b"do not modify");
    assert!(!user.join(".migration-v1.json").exists());
}
