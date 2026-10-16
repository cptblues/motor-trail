use gltf::{accessor::Dimensions, material::AlphaMode};

pub(super) fn document(bytes: &[u8]) -> Result<gltf::Gltf, String> {
    // The pinned GLB reader subtracts 12 from the declared length unchecked.
    if bytes.len() < 20
        || bytes.len() > 1024 * 1024
        || !bytes.starts_with(b"glTF")
        || bytes[4..8] != 2_u32.to_le_bytes()
        || bytes[8..12] != (bytes.len() as u32).to_le_bytes()
    {
        return Err("Invalid nature GLB header or container length".into());
    }
    let source =
        gltf::Gltf::from_slice(bytes).map_err(|error| format!("Invalid nature GLB: {error}"))?;
    if source.buffers().len() != 1
        || source.as_json().buffers[0].uri.is_some()
        || source.textures().len() != 0
        || source.images().len() != 0
        || source.animations().len() != 0
        || source.skins().len() != 0
        || source.extensions_used().next().is_some()
        || source.extensions_required().next().is_some()
        || source.nodes().len() > 1024
    {
        return Err("Nature supports embedded static solid-color geometry without textures, skins, animations or extensions".into());
    }
    let blob = source
        .blob
        .as_deref()
        .ok_or("Nature GLB has no embedded binary buffer")?;
    let declared = source
        .buffers()
        .next()
        .ok_or("Nature GLB has no buffer")?
        .length();
    if declared > blob.len() || blob.len() - declared > 3 {
        return Err("Nature binary payload does not match its declared buffer".into());
    }
    for view in source.views() {
        if view
            .offset()
            .checked_add(view.length())
            .is_none_or(|end| end > declared)
        {
            return Err("Nature buffer view exceeds its binary payload".into());
        }
    }
    let mut total = 0_usize;
    for accessor in source.accessors() {
        if accessor.sparse().is_some()
            || !matches!(accessor.dimensions(), Dimensions::Scalar | Dimensions::Vec3)
        {
            return Err("Nature supports dense scalar/vector accessors only".into());
        }
        let view = accessor
            .view()
            .ok_or("Nature accessor has no buffer view")?;
        let size = accessor.size();
        let stride = view.stride().unwrap_or(size);
        total = accessor
            .count()
            .checked_add(total)
            .filter(|count| *count <= 100_000)
            .ok_or("Nature exceeds its accessor budget")?;
        if accessor.count() == 0
            || stride < size
            || (accessor.count() - 1)
                .checked_mul(stride)
                .and_then(|bytes| accessor.offset().checked_add(bytes))
                .and_then(|end| end.checked_add(size))
                .is_none_or(|end| end > view.length())
        {
            return Err(
                "Nature accessor exceeds its buffer view or has an invalid stride/count".into(),
            );
        }
    }
    for material in source.materials() {
        let pbr = material.pbr_metallic_roughness();
        let color = pbr.base_color_factor();
        if material.alpha_mode() != AlphaMode::Opaque
            || color[3] != 1.0
            || color
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || pbr.roughness_factor() != 1.0
            || pbr.metallic_factor() != 0.0
            || pbr.base_color_texture().is_some()
            || pbr.metallic_roughness_texture().is_some()
            || material.normal_texture().is_some()
            || material.occlusion_texture().is_some()
            || material.emissive_texture().is_some()
            || material.emissive_factor() != [0.0; 3]
        {
            return Err("Nature materials must be opaque, rough, nonmetallic solid colors".into());
        }
    }
    Ok(source)
}
