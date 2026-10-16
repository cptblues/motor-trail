# Nature Assets Integration

Approved scope: replace existing trees/rocks and expose all 23 provided models
in the Terrain Workshop, with preview, rotation and JSON persistence.

## Representation

- Preserve the supplied assets, generator and catalogue. Their actual vertical
  axis is Z; convert Z-up to Y-up once when reading the geometry.
- Decode the known static solid-colour GLBs with the existing gltf library,
  not a handwritten binary parser. Cache canonical CPU geometry and preserve
  faceted normals and material colours as vertex colours. This small fixed pack
  is embedded for deterministic, synchronous mesh/collision/preview generation;
  also ship the original GLBs and catalogue in native packages.
- Normalize geometry around its bounding-box centre; dimensions remain full
  metres, baked into workshop meshes and colliders. Default dimensions come
  from the measured model, not rounded catalogue values.
- Rocks/wood use static triangle surfaces; tree collisions use only wooden
  geometry, not foliage. Grass, ferns and bushes are decorative with no active
  physics collider; give them non-physical picking bounds for editor selection.
  Tree foliage is also selectable through these editor-only bounds.

## Workshop And Existing Scenes

- Extend ObstacleKind with a stable Nature asset identifier. Existing primitive
  JSON remains readable; the old Rock kind uses the new medium boulder geometry.
- All 23 assets are selectable by category. Selection supplies natural default
  dimensions. Existing Q/E/Shift rotation, confirmation, cancel and input capture
  remain intact. Previews use the exact final geometry and no active collider.
- Preserve IDs, transforms, dimensions, undo/redo and JSON round trips. Reject
  unknown model IDs. Keep the vehicle controller and force calculations intact.

## Decoration And Resource Lifetime

- Replace the historical valley's procedural trees/cubes with deterministic
  variants of the imported trees/rocks, retaining road clearance and density.
- Share meshes/materials within a scene; canonical CPU data stays cached.
  Scene disposal must not remove assets belonging to another scene or preview.
- Keep decoration grounded after terrain edits. No extra automatic forest is
  added to currently empty generated terrain; models are manually placeable.

## Distribution And Verification

- Whitelist all 23 GLBs and catalog.json in Windows/macOS packages. Exclude
  preview images, generator code and prompt files from player packages.
- Validate package contents and startup resource presence. Test real model
  bounds/orientation/colours, colliders, all-model JSON persistence, preview
  capture/cleanup, picking decorative plants, legacy decoration and reloads.
- Run fmt, debug/release Rust tests, Python packaging tests, release build and
  real-renderer smoke with isolated user data. Do not tag or publish anything.

Work stays in the existing checkout so the user's untracked assets and shared
Cargo cache remain available. No commits or remote changes are needed.
