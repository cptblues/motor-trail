use super::ResourceLayout;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

#[derive(Deserialize)]
struct Version {
    tag: String,
    target: String,
    commit: String,
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect {}: {e}", path.display()))?;
    if !metadata.is_file() || super::migration::is_link(&metadata) || metadata.len() > limit {
        return Err(format!(
            "{} must be a regular file no larger than {limit} bytes",
            path.display()
        ));
    }
    let file = File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} is oversized", path.display()));
    }
    Ok(bytes)
}

pub(super) fn check(layout: &ResourceLayout) -> Result<String, String> {
    let version = if let Some(path) = &layout.version {
        let version: Version = serde_json::from_slice(&read_bounded(path, 16 * 1024)?)
            .map_err(|e| format!("Invalid version.json: {e}"))?;
        let core = version
            .tag
            .strip_prefix('v')
            .unwrap_or("")
            .split(['-', '+'])
            .next()
            .unwrap_or("");
        if core.split('.').count() != 3
            || core.split('.').any(|part| part.parse::<u32>().is_err())
            || !version
                .tag
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
            || version.commit.len() != 40
            || !version.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Invalid release tag or commit in version.json".into());
        }
        let target = match (std::env::consts::ARCH, std::env::consts::OS) {
            ("x86_64", "windows") => "x86_64-pc-windows-msvc",
            ("aarch64", "macos") => "aarch64-apple-darwin",
            ("x86_64", "macos") => "x86_64-apple-darwin",
            _ => "development-host",
        };
        if version.target != target {
            return Err(format!(
                "Package target {} does not match {target}",
                version.target
            ));
        }
        version.tag
    } else {
        format!("development {}", env!("CARGO_PKG_VERSION"))
    };
    if layout.version.is_some() {
        for name in crate::vehicle::config::VehiclePhysicsConfig::preset_names() {
            let path = layout
                .defaults
                .join("vehicles")
                .join(format!("{name}.json"));
            let profile: crate::vehicle::config::VehiclePhysicsConfig =
                serde_json::from_slice(&read_bounded(&path, 1_048_576)?)
                    .map_err(|e| format!("Invalid default profile {}: {e}", path.display()))?;
            profile.validate()?;
        }
        let mounts = read_bounded(
            &layout.defaults.join("vehicles/motor_trail_4x4.mounts.json"),
            1_048_576,
        )?;
        crate::vehicle::mounting::VehicleMountDefinition::from_json(
            std::str::from_utf8(&mounts).map_err(|e| e.to_string())?,
        )?;
        for name in ["Exemple Plat", "Exemple Relief"] {
            let path = layout
                .defaults
                .join("terrains")
                .join(format!("{name}.json"));
            let scene: crate::terrain::scene::TerrainScene =
                serde_json::from_slice(&read_bounded(&path, 16 * 1024 * 1024)?)
                    .map_err(|e| format!("Invalid default scene {}: {e}", path.display()))?;
            scene.validate()?;
        }
    } else {
        // Development configs are legacy user data, not immutable installation
        // defaults. Preserve ProfileStore's missing/corrupt profile fallback.
        for name in crate::vehicle::config::VehiclePhysicsConfig::preset_names() {
            crate::vehicle::config::VehiclePhysicsConfig::preset(name)
                .ok_or("Unknown built-in profile")?
                .validate()?;
        }
        crate::vehicle::mounting::VehicleMountDefinition::motor_trail()?;
    }
    let model = read_bounded(
        &layout.assets.join("vehicles/motor_trail_4x4.glb"),
        256 * 1024 * 1024,
    )?;
    check_model(&model)?;
    for asset in crate::terrain::nature::NatureAsset::ALL {
        let path = layout.assets.join(asset.asset_path());
        if read_bounded(&path, 16 * 1024 * 1024)? != asset.source_bytes() {
            return Err(format!(
                "Nature resource {} differs from the compiled catalogue",
                path.display()
            ));
        }
        asset.build_mesh(bevy::prelude::Vec3::ONE)?;
    }
    let catalog = read_bounded(&layout.assets.join("3D/catalog.json"), 1_048_576)?;
    if catalog != include_bytes!("../../assets/3D/catalog.json") {
        return Err("Nature catalogue differs from the compiled model catalogue".into());
    }
    Ok(version)
}

fn check_model(model: &[u8]) -> Result<(), String> {
    // The pinned gltf binary reader subtracts the header size unchecked.
    // Check the container before invoking it, including in debug builds.
    if model.len() < 20
        || !model.starts_with(b"glTF")
        || u32::from_le_bytes(model[4..8].try_into().unwrap()) != 2
        || u32::from_le_bytes(model[8..12].try_into().unwrap()) as usize != model.len()
    {
        return Err("Invalid vehicle GLB header or container length".into());
    }
    let gltf = bevy::gltf::gltf::Gltf::from_slice(model)
        .map_err(|e| format!("Invalid vehicle GLB: {e}"))?;
    if gltf.blob.is_none() || gltf.scenes().len() == 0 || gltf.meshes().len() == 0 {
        return Err("The vehicle must be a GLB with meshes, a scene and embedded geometry".into());
    }
    let blob = gltf.blob.as_deref().unwrap();
    if gltf.buffers().len() != 1
        || gltf.as_json().buffers[0].uri.is_some()
        || gltf
            .as_json()
            .images
            .iter()
            .any(|image| image.uri.is_some())
    {
        return Err("Vehicle GLB must embed its geometry and images".into());
    }
    let declared = gltf.buffers().next().unwrap().length();
    if declared > blob.len() || blob.len() - declared > 3 {
        return Err("Vehicle GLB binary payload does not match its declared buffer".into());
    }
    for view in gltf.views() {
        if view
            .offset()
            .checked_add(view.length())
            .is_none_or(|end| end > declared)
        {
            return Err("Vehicle GLB buffer view exceeds its binary payload".into());
        }
    }
    let mut decoded_bytes = 0_usize;
    for accessor in gltf.accessors() {
        // The shipped model uses dense scalar/vector data only. Do not claim
        // support for sparse decoding or padded matrices without testing them.
        let shape = serde_json::to_value(accessor.dimensions()).map_err(|e| e.to_string())?;
        if accessor.sparse().is_some()
            || !matches!(shape.as_str(), Some("SCALAR" | "VEC2" | "VEC3" | "VEC4"))
        {
            return Err("Factory vehicle GLB requires dense scalar/vector accessors".into());
        }
        // Bevy can expand compact attributes, including RGB, to Float32x4.
        // Bound accessor values conservatively, not total renderer memory.
        decoded_bytes = accessor
            .size()
            .max(16)
            .checked_mul(accessor.count())
            .and_then(|bytes| decoded_bytes.checked_add(bytes))
            .filter(|bytes| *bytes <= 256 * 1024 * 1024)
            .ok_or("Vehicle GLB exceeds the conservative 256 MiB decoded-accessor budget")?;
        let view = accessor
            .view()
            .ok_or("Vehicle GLB accessor has no buffer view")?;
        if !contains_array(
            accessor.offset(),
            accessor.count(),
            view.stride().unwrap_or(accessor.size()),
            accessor.size(),
            view.length(),
        ) {
            return Err("Vehicle GLB accessor exceeds its buffer view".into());
        }
    }
    Ok(())
}

fn contains_array(offset: usize, count: usize, stride: usize, size: usize, limit: usize) -> bool {
    count > 0
        && stride >= size
        && (count - 1)
            .checked_mul(stride)
            .and_then(|length| offset.checked_add(length))
            .and_then(|end| end.checked_add(size))
            .is_some_and(|end| end <= limit)
}
