use super::{ResourceLayout, validation::read_bounded};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const MARKER: &str = ".migration-v1.json";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn initialize(layout: &ResourceLayout, root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("User-data path must be absolute".into());
    }
    directory(root)?;
    for name in ["vehicles", "terrains", "settings", "community_maps"] {
        directory(&root.join(name))?;
    }
    let marker = root.join(MARKER);
    if regular(&marker)? {
        let value: serde_json::Value = serde_json::from_slice(&read_bounded(&marker, 1024)?)
            .map_err(|e| format!("Invalid migration marker: {e}"))?;
        if value.get("schema_version").and_then(|value| value.as_u64()) != Some(1) {
            return Err("Unsupported user-data migration marker".into());
        }
        return Ok(());
    }
    if let Some(legacy) = &layout.legacy_configs {
        copy_group(&legacy.join("vehicles"), &root.join("vehicles"), true, true)?;
        copy_group(
            &legacy.join("terrains"),
            &root.join("terrains"),
            false,
            false,
        )?;
    }
    copy_group(
        &layout.defaults.join("vehicles"),
        &root.join("vehicles"),
        true,
        false,
    )?;
    copy_group(
        &layout.defaults.join("terrains"),
        &root.join("terrains"),
        false,
        false,
    )?;
    publish_new(&marker, b"{\"schema_version\":1}\n")
}

fn copy_group(
    source: &Path,
    destination: &Path,
    profiles: bool,
    marker: bool,
) -> Result<(), String> {
    match fs::symlink_metadata(source) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(format!(
                "Cannot inspect migration source {}: {e}",
                source.display()
            ));
        }
    }
    if !source.is_absolute() {
        return Err("Migration source must be absolute".into());
    }
    for ancestor in source.ancestors() {
        require_directory(ancestor)?;
    }
    let entries =
        fs::read_dir(source).map_err(|e| format!("Cannot read {}: {e}", source.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let is_marker = marker && name == ".last-profile.json";
        let valid = path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path
                .file_stem()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    if profiles {
                        crate::persistence::profiles::validate_name(name).is_ok()
                    } else {
                        crate::terrain::scene::validate_name(name).is_ok()
                    }
                });
        if !valid && !is_marker {
            continue;
        }
        let target = destination.join(name);
        if regular(&target)? {
            continue;
        }
        let bytes = read_bounded(
            &path,
            if profiles {
                1_048_576
            } else {
                16 * 1024 * 1024
            },
        )?;
        publish_new(&target, &bytes)?;
    }
    Ok(())
}

fn directory(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|parent| *parent != path) {
        if !parent.as_os_str().is_empty() {
            directory(parent)?;
        }
    }
    match fs::symlink_metadata(path) {
        Ok(_) => return require_directory(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "Cannot inspect user-data directory {}: {e}",
                path.display()
            ));
        }
    }
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(format!(
                "Cannot create user-data directory {}: {e}",
                path.display()
            ));
        }
    }
    require_directory(path)
}

fn require_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect {}: {e}", path.display()))?;
    if metadata.is_dir() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(format!(
            "{} must be a directory, not a link",
            path.display()
        ))
    }
}

pub(super) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn regular(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => Ok(true),
        Ok(_) => Err(format!("{} must be a regular file", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Cannot inspect {}: {e}", path.display())),
    }
}

fn publish_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if regular(path)? {
        return Ok(());
    }
    let parent = path.parent().ok_or("Migration path has no parent")?;
    let (temporary, mut file) = (0..64)
        .find_map(|_| {
            let temporary = parent.join(format!(
                ".migration-{}-{}.tmp",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => Some(Ok((temporary, file))),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(format!("Cannot create migration file: {e}"))),
            }
        })
        .ok_or("Cannot allocate a migration temporary file")??;
    let result = (|| {
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        // A hard-link publish is atomic and cannot replace a concurrent user save.
        match fs::hard_link(&temporary, path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                regular(path)?;
            }
            Err(e) => return Err(format!("Cannot publish {}: {e}", path.display())),
        }
        #[cfg(unix)]
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    let cleanup = fs::remove_file(&temporary)
        .map_err(|e| format!("Cannot clean migration temporary file: {e}"));
    result.and(cleanup)
}
