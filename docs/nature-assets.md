# Nature Assets

The Motor-Trail Nature Prototype is a fixed pack of 23 static, solid-colour GLBs.
The approved integration replaces the historical valley's trees and rocks and
exposes all models in the Terrain Workshop, with previews, rotation and JSON
persistence. It does not add an automatic forest to new, empty generated terrains;
models can be placed manually there. This prototype covers the supplied models,
not a general importer for arbitrary user GLBs, textures, skins or animations.

## Stable Asset IDs

Each ID is the catalogue's `name`. Its original model lives at
`assets/3D/assets/<id>.glb`; `assets/3D/catalog.json` records the pack metadata.
The catalogue's `file` paths are relative to `assets/3D/`.

| Category | Stable IDs | Collision |
| --- | --- | --- |
| Trees | `tree_oak_round`, `tree_oak_wide`, `tree_oak_tall`, `tree_young`, `tree_pine_small`, `tree_pine_medium`, `tree_pine_tall` | Wooden geometry only; foliage does not collide |
| Rocks | `rock_boulder_small`, `rock_boulder_medium`, `rock_boulder_large`, `rock_slab_flat`, `rock_cluster` | Static triangle mesh |
| Bushes | `bush_low`, `bush_round`, `bush_wild` | Decorative, no active physics collider |
| Plants | `grass_short`, `grass_tall`, `grass_wild`, `fern_small`, `fern_large` | Decorative, no active physics collider |
| Wood | `log_fallen_small`, `log_fallen_large`, `stump_old` | Static triangle mesh |

Decorative bushes and plants use non-physical picking bounds for editor selection.
Trees also expose these editor bounds so their noncolliding foliage can be
clicked; these bounds do not participate in vehicle physics.
Previews use the final geometry without active colliders.

## Geometry And Persistence

The supplied geometry is actually Z-up. Import converts it once to the game's
Y-up coordinates and centres it on its bounding-box centre. Natural dimensions
come from measured geometry in metres, rather than the rounded `size_m` catalogue
values. Requested full dimensions are baked into both mesh vertices and collider
geometry; they are not a second entity-scale transform. Faceted normals and the
supplied material colours are preserved.

This small fixed pack is embedded in the application. Cached canonical geometry
feeds rendering, placement previews and collision generation, keeping them
aligned; tree collision uses its wooden subset. Editing a shipped GLB does not
hot-reload or replace that embedded geometry.

Existing primitive terrain JSON remains readable. The legacy `Rock` kind uses
the medium boulder geometry. Nature objects persist a stable asset ID along with
their object IDs, transforms and dimensions; unknown asset IDs are rejected.
Saving and loading preserve these values, including rotation and resized models.

## Native Packages

Packaging explicitly whitelists the 23 GLBs above and `assets/3D/catalog.json`.
The files retain their source paths beneath the existing resource roots:

| Target | Nature resource root |
| --- | --- |
| Windows x64 | `Motor-Trail/assets/3D/` |
| macOS Apple Silicon and Intel | `Motor-Trail.app/Contents/Resources/assets/3D/` |

The supplied generator, preview images, prompts, style guide and pack README are
authoring files and are excluded. Arbitrary new files, including GLBs in the
same asset directory, are not picked up automatically. Adding another shipped
model requires a deliberate whitelist and test change.

The original GLBs and catalogue are shipped even though runtime geometry is
embedded. Archive creation and validation both check their integrity: every GLB
passes the existing GLB header/chunk/embedded-buffer checks, and the catalogue
passes strict JSON parsing with duplicate keys and non-finite constants rejected.
Its asset list must contain exactly the 23 unique known IDs, each paired with
`assets/<id>.glb`. Missing, unknown or duplicate IDs, unsafe paths and mismatching
file references are rejected. Catalogue order does not matter. ZIP validation
also checks the exact file set, CRCs and safe regular-file entries.

These structural checks do not establish visual quality or native application
startup. Python packaging tests exercise all three archive layouts with fixture
executables; native runtime and renderer verification belongs to the broader
nature integration.

## Local Verification: 2026-10-16

- Rust release and debug suites: 340 tests passed in each mode. New regressions
  cover all model bounds/colours, upright trees, wood-only colliders, decorative
  picking, tree-foliage picking, JSON identity, preview geometry, undo/redo,
  grounding after sculpting and scene-resource cleanup.
- Python packaging/workflow suite: 39 tests passed with YAML parsing enabled.
- `cargo check --tests --locked`, release builds, `cargo fmt --all -- --check`
  and `git diff --check` passed. Four existing vehicle dead-code warnings remain.
- Real-renderer Nature smoke exited 0 with five captures: all 23 models visible,
  preview rotation 15 to 10 degrees, exact confirmation of the displayed pose,
  24 objects saved/reloaded, decorative selection, 13.94 m of driving and 223
  imported historical-valley roots. Captures were inspected.
- Existing Terrain smoke also exited 0 with six captures: sculpt/JSON roundtrip,
  valid/invalid previews, primitive placement/selection, physics UI and driving.
- A native Windows MSVC static-CRT build produced a validated 32,679,226-byte
  `dist/Motor-Trail-Windows-x64.zip`. Its extracted resource check passed from
  an unrelated directory without Rust/Cargo in PATH and with an invalid external
  BEVY_ASSET_ROOT. The local metadata string is `v0.1.0-nature-local`, not a Git tag.
- No vehicle-controller code, existing workflows or supplied asset files were
  changed. The existing transitive `gltf` 1.4.1 is now an explicit dependency;
  no dependency versions were upgraded. No commit, tag, push or release was made.

Renderer pose comparisons allow 0.1 mm translation and 0.00001 quaternion
component error for Rapier floating-point conversion; saved JSON comparisons
remain exact. Source GLBs and catalogue must match the compiled copies at startup,
so changing a model requires rebuilding rather than hot reloading it.

Native macOS compilation and rendering were not run on this Windows machine.
The pack still has no LOD, wind animation or general-purpose arbitrary GLB import.

To repeat the optional graphics probe without touching normal saves:

```powershell
cargo run --release -- --nature-smoke "C:/Temp/MotorTrail-Nature/captures" --user-data-dir "C:/Temp/MotorTrail-Nature/user-data"
```

## Changed Files

- `src/terrain/nature.rs`, `src/terrain/nature/`: model catalogue, bounded glTF
  decoding, geometry cache and tests.
- `src/terrain/obstacles.rs`, `src/terrain/runtime.rs`, placement preview and
  `src/ui/terrain_workshop.rs`: factory, optional collisions, editor integration.
- `src/terrain/legacy.rs`: scenery variants, shared scene assets and grounding.
- `src/terrain/runtime/nature_smoke.rs`, `smoke.rs`: renderer regression probes.
- `src/app_paths/validation.rs`, `tests.rs`: installation checks and fixtures.
- Packaging script/tests, `Cargo.toml`, `Cargo.lock`, `src/terrain/mod.rs`, README,
  distribution documentation and the Nature design/implementation plan.

The supplied `assets/3D/` pack is consumed unchanged; it must be included when
committing this feature for future native builds.
