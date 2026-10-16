use super::*;
use bevy::mesh::VertexAttributeValues;
use gltf::binary::Glb;
use std::borrow::Cow;

const MEASURED_SIZES: [[f32; 3]; 23] = [
    [4.560812, 5.706191, 3.529605],
    [6.033190, 5.376265, 3.897906],
    [3.518035, 8.597967, 2.525369],
    [2.467117, 3.380416, 1.848173],
    [2.381479, 4.268191, 2.362179],
    [2.916398, 6.336209, 2.892465],
    [3.353767, 8.415145, 3.326334],
    [1.274346, 0.757548, 1.147425],
    [2.169720, 1.450860, 1.719673],
    [3.264810, 2.117613, 2.537853],
    [2.424754, 0.452894, 1.921423],
    [2.363751, 1.258722, 2.215565],
    [1.153800, 0.742618, 0.930495],
    [1.365671, 1.278302, 1.171495],
    [1.946760, 1.305422, 1.597778],
    [0.532126, 0.452250, 0.502203],
    [0.885922, 0.869696, 0.875919],
    [0.753640, 0.746322, 0.651288],
    [0.921449, 0.482784, 0.880417],
    [1.618318, 0.849111, 1.605020],
    [1.771410, 0.453311, 0.446727],
    [3.023172, 0.728775, 0.720959],
    [0.755558, 0.718000, 0.755558],
];

fn positions(mesh: &Mesh) -> Vec<Vec3> {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap()
        .iter()
        .copied()
        .map(Vec3::from_array)
        .collect()
}

fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().copied().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(min, max), point| (min.min(point), max.max(point)),
    )
}

fn modified_source(
    asset: NatureAsset,
    edit: impl FnOnce(&mut serde_json::Value, &mut Vec<u8>),
) -> Vec<u8> {
    let glb = Glb::from_slice(asset.source_bytes()).unwrap();
    let mut json = serde_json::from_slice(&glb.json).unwrap();
    let mut bin = glb.bin.unwrap().into_owned();
    edit(&mut json, &mut bin);
    Glb {
        header: glb.header,
        json: Cow::Owned(serde_json::to_vec(&json).unwrap()),
        bin: Some(Cow::Owned(bin)),
    }
    .to_vec()
    .unwrap()
}

#[test]
fn all_23_real_assets_have_measured_y_up_defaults_and_centered_baked_bounds() {
    for (index, asset) in NatureAsset::ALL.into_iter().enumerate() {
        assert_eq!(asset.index(), index);
        assert!((asset.default_size() - Vec3::from_array(MEASURED_SIZES[index])).length() < 2e-5);
        for size in [
            asset.default_size(),
            Vec3::new(0.1, 32.0, 0.3),
            Vec3::new(32.0, 0.1, 7.0),
        ] {
            let points = positions(&asset.build_mesh(size).unwrap());
            assert!(points.iter().all(|point| point.is_finite()), "{asset:?}");
            let (min, max) = bounds(&points);
            assert!((max - min - size).length() < 3e-5, "{asset:?}");
            assert!((max + min).length() < 3e-5, "{asset:?}");
        }
    }
}

#[test]
fn tree_trunks_start_at_the_bottom_and_foliage_is_above_wood() {
    for asset in NatureAsset::TREES {
        let mesh = asset.build_mesh(asset.default_size()).unwrap();
        let points = positions(&mesh);
        let wood = asset.collider(asset.default_size()).unwrap().unwrap();
        let wood_points: Vec<_> = wood.as_trimesh().unwrap().vertices().collect();
        let (min, max) = bounds(&points);
        let (wood_min, wood_max) = bounds(&wood_points);
        assert!((min.y - wood_min.y).abs() < 1e-5, "{asset:?}");
        assert!(max.y > wood_max.y + 0.1, "{asset:?}");
        assert!((max.y - min.y - MEASURED_SIZES[asset.index()][1]).abs() < 1e-5);
    }
}

#[test]
fn meshes_keep_all_material_surface_colors_flat_normals_and_valid_indices() {
    for asset in NatureAsset::ALL {
        let mesh = asset.build_mesh(Vec3::new(2.0, 3.0, 4.0)).unwrap();
        let points = positions(&mesh);
        let normals = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .unwrap()
            .as_float3()
            .unwrap();
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("missing colors: {asset:?}");
        };
        let indices: Vec<_> = mesh.indices().unwrap().iter().collect();
        assert_eq!(points.len(), normals.len());
        assert_eq!(points.len(), colors.len());
        assert_eq!(indices.len() % 3, 0);
        let source = gltf::Gltf::from_slice(asset.source_bytes()).unwrap();
        let palette: Vec<_> = source
            .materials()
            .map(|m| m.pbr_metallic_roughness().base_color_factor())
            .collect();
        assert!(
            palette.iter().all(|color| colors.contains(color)),
            "{asset:?}"
        );
        assert!(
            colors.iter().all(|color| palette.contains(color)),
            "{asset:?}"
        );
        for triangle in indices.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
            assert!(a < points.len() && b < points.len() && c < points.len());
            let normal = (points[b] - points[a])
                .cross(points[c] - points[a])
                .normalize();
            for index in [a, b, c] {
                let actual = Vec3::from_array(normals[index]);
                assert!(actual.is_finite() && (actual.length() - 1.0).abs() < 1e-5);
                assert!(actual.dot(normal) > 0.9999, "{asset:?}");
                assert_eq!(colors[index], colors[a]);
            }
        }
    }
}

#[test]
fn rocks_and_fallen_wood_use_exact_render_triangles() {
    for asset in NatureAsset::ROCKS.into_iter().chain([
        NatureAsset::LogFallenSmall,
        NatureAsset::LogFallenLarge,
        NatureAsset::StumpOld,
    ]) {
        let size = Vec3::new(3.0, 0.7, 2.0);
        let mesh = asset.build_mesh(size).unwrap();
        let collider = asset.collider(size).unwrap().unwrap();
        let shape = collider
            .as_trimesh()
            .expect("static exact triangle surfaces");
        assert_eq!(positions(&mesh), shape.vertices().collect::<Vec<_>>());
        assert_eq!(
            mesh.indices().unwrap().iter().collect::<Vec<_>>(),
            shape
                .indices()
                .iter()
                .flatten()
                .map(|i| *i as usize)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn bushes_grasses_and_ferns_have_no_physical_collider() {
    for asset in &NatureAsset::ALL[12..20] {
        assert!(
            asset.collider(asset.default_size()).unwrap().is_none(),
            "{asset:?}"
        );
    }
}

#[test]
fn tree_contacts_include_wood_and_exclude_foliage_surfaces() {
    use bevy_rapier3d::{parry::query::intersection_test, rapier::math::Pose};
    for asset in NatureAsset::TREES {
        let collider = asset.collider(asset.default_size()).unwrap().unwrap();
        let geometry = asset.geometry().unwrap();
        let ball = Collider::ball(0.002);
        let touches = |point: Vec3| {
            intersection_test(
                &Pose::IDENTITY,
                collider.raw.as_ref(),
                &Pose::from_translation(point),
                ball.raw.as_ref(),
            )
            .unwrap()
        };
        let wood = geometry
            .solid_triangles
            .first()
            .unwrap()
            .map(|i| geometry.positions[i as usize]);
        assert!(
            touches((wood[0] + wood[1] + wood[2]) / 3.0),
            "no wood contact: {asset:?}"
        );
        let leaf = geometry
            .positions
            .chunks_exact(3)
            .enumerate()
            .find(|(index, _)| {
                !geometry
                    .solid_triangles
                    .iter()
                    .any(|t| t[0] as usize == index * 3)
            })
            .unwrap()
            .1;
        assert!(
            !touches((leaf[0] + leaf[1] + leaf[2]) / 3.0),
            "foliage blocks contacts: {asset:?}"
        );
        assert!(collider.as_trimesh().unwrap().indices().len() < geometry.positions.len() / 3);
    }
}

#[test]
fn identifiers_json_and_original_asset_paths_are_stable() {
    let catalogue: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/3D/catalog.json")).unwrap();
    for asset in NatureAsset::ALL {
        let json = serde_json::to_string(&asset).unwrap();
        assert_eq!(json, format!("\"{}\"", asset.id()));
        assert_eq!(serde_json::from_str::<NatureAsset>(&json).unwrap(), asset);
        assert_eq!(asset.asset_path(), format!("3D/assets/{}.glb", asset.id()));
        assert_eq!(
            catalogue["assets"][asset.index()]["name"].as_str(),
            Some(asset.id())
        );
        assert!(!asset.name().is_empty() && asset.name().is_ascii());
        assert!(!asset.category().is_empty() && asset.category().is_ascii());
        assert!(asset.source_bytes().starts_with(b"glTF"));
        assert!(std::ptr::eq(
            asset.geometry().unwrap(),
            asset.geometry().unwrap()
        ));
    }
    for json in [
        "\"unknown_tree\"",
        "\"TreeOakRound\"",
        "\"tree-oak-round\"",
        "null",
        "42",
    ] {
        assert!(serde_json::from_str::<NatureAsset>(json).is_err(), "{json}");
    }
}

#[test]
fn invalid_dimensions_return_errors_even_for_decorative_plants() {
    for asset in NatureAsset::ALL {
        for bad in [
            0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::MAX,
            f32::MIN_POSITIVE,
        ] {
            for axis in 0..3 {
                let mut size = Vec3::ONE;
                size[axis] = bad;
                assert!(asset.build_mesh(size).is_err(), "{asset:?} {size:?}");
                assert!(asset.collider(size).is_err(), "{asset:?} {size:?}");
            }
        }
    }
}

#[test]
fn decoding_applies_nested_node_transforms_before_the_axis_conversion() {
    let asset = NatureAsset::RockBoulderMedium;
    let bytes = modified_source(asset, |json, _| {
        json["nodes"][0]["translation"] = serde_json::json!([7.0, -11.0, 13.0]);
        json["nodes"][0]["scale"] = serde_json::json!([2.0, 3.0, 4.0]);
        let q = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        json["nodes"][0]["rotation"] = serde_json::json!(q.to_array());
        for node in json["nodes"].as_array_mut().unwrap().iter_mut().skip(1) {
            node["translation"] = serde_json::json!([1.0, 2.0, 3.0]);
        }
    });
    let decoded = decode(asset, &bytes).unwrap();
    let original = asset.default_size();
    let expected = Vec3::new(original.z * 3.0, original.y * 4.0, original.x * 2.0);
    assert!((decoded.size - expected).length() < 3e-5);
    let (min, max) = bounds(&decoded.positions);
    assert!((min + max).length() < 3e-5);
}

#[test]
fn malformed_containers_and_accessors_are_errors_without_panics() {
    let asset = NatureAsset::TreeOakRound;
    for length in [0, 4, 12, 19, 20, asset.source_bytes().len() - 1] {
        assert!(decode(asset, &asset.source_bytes()[..length]).is_err());
    }
    let mut bad_header = asset.source_bytes().to_vec();
    bad_header[8..12].copy_from_slice(&0_u32.to_le_bytes());
    assert!(decode(asset, &bad_header).is_err());
    for edit in 0..8 {
        let bytes = modified_source(asset, |json, bin| match edit {
            0 => json["accessors"][1]["count"] = serde_json::json!(0),
            1 => json["accessors"][1]["byteOffset"] = serde_json::json!(4294967295_u64),
            2 => json["bufferViews"][1]["byteStride"] = serde_json::json!(4),
            3 => bin[0..4].copy_from_slice(&u32::MAX.to_le_bytes()),
            4 => {
                let offset = json["bufferViews"][1]["byteOffset"].as_u64().unwrap() as usize;
                bin[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
            }
            5 => json["nodes"][0]["children"] = serde_json::json!([0]),
            6 => json["nodes"][0]["scale"] = serde_json::json!([0.0, 1.0, 1.0]),
            _ => json["bufferViews"][1]["byteLength"] = serde_json::json!(4),
        });
        assert!(
            decode(asset, &bytes).is_err(),
            "accepted malformed case {edit}"
        );
    }
}

#[test]
fn unsupported_external_buffers_textures_animations_and_nontriangle_modes_are_rejected() {
    let asset = NatureAsset::TreeOakRound;
    for edit in 0..7 {
        let bytes = modified_source(asset, |json, _| match edit {
            0 => json["buffers"][0]["uri"] = serde_json::json!("external.bin"),
            1 => {
                json["images"] = serde_json::json!([{"uri": "leaf.png"}]);
                json["textures"] = serde_json::json!([{"source": 0}]);
            }
            2 => json["animations"] = serde_json::json!([{"samplers": [], "channels": []}]),
            3 => json["meshes"][0]["primitives"][0]["mode"] = serde_json::json!(1),
            4 => json["materials"][0]["alphaMode"] = serde_json::json!("BLEND"),
            5 => {
                json["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"] =
                    serde_json::json!(0.5)
            }
            _ => {
                json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] =
                    serde_json::json!(1)
            }
        });
        assert!(
            decode(asset, &bytes).is_err(),
            "accepted unsupported case {edit}"
        );
    }
}
