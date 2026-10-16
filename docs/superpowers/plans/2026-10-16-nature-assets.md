# Nature Assets Implementation Plan

> Execute using superpowers:executing-plans and isolated parallel tasks. Preserve
> the supplied pack and unrelated files. Serialize Cargo commands.

**Goal:** Replace primitive scenery and make the 23 nature models usable in the
Terrain Workshop, including previews, collisions, persistence and packaging.

**Architecture:** A small immutable nature geometry catalogue feeds the existing
obstacle factory and decoration. Runtime publication accepts optional colliders;
editor picking supports decorative-only objects. Existing primitive JSON remains
valid. The fixed solid-colour GLBs are parsed by the pinned gltf library.

**Tech Stack:** Rust, Bevy, Rapier, serde, existing gltf reader, Python packaging.

## 1. Regression And Geometry Catalogue

Files: `src/terrain/nature.rs`, `src/terrain/nature/`, `src/terrain/obstacles.rs`.

- [x] Add a failing test decoding a Nature/tree_oak_round obstacle JSON and
  expecting round-trip preservation rather than unknown-variant failure.
- [x] Run `cargo test --release nature_assets_json -- --nocapture`, observe RED.
- [x] Add typed stable identifiers, all 23 original model bytes and metadata;
  read glTF scenes/positions/indices/material factors with the existing library.
- [x] Convert Z to Y, preserve facets/colours, measure natural dimensions and
  cache canonical geometry. Expose build_mesh(size) and collider(size).
- [x] Test finite centred bounds, all 23 models, upright trees, exact rock
  surfaces, trunk-only tree contacts and no decorative colliders.

## 2. Workshop And Runtime

Files: `src/terrain/obstacles.rs`, `src/terrain/runtime.rs`,
`src/terrain/runtime/placement_preview.rs`, `src/ui/terrain_workshop.rs`,
`src/terrain/scene.rs`, focused tests beside these modules.

- [x] Extend kinds with Nature IDs while accepting existing strings.
- [x] Keep build() for physical primitive callers; add a factory returning
  (Mesh, Option<Collider>) for runtime/preview validation of decorative models.
- [x] Add categorized catalogue choices and natural default dimensions.
- [x] Preserve per-kind yaw, capture, history and exact JSON round trips.
- [x] Use actual meshes with white base material for vertex colours; strip
  colours only in tinted previews, keeping their geometry unchanged.
- [x] Add non-physical picking bounds for decorative objects and tree foliage.
- [x] Test plants are selectable, preview/final geometry match, all models save
  and load, repeated reloads clean meshes/materials/entities and inputs remain safe.

## 3. Historical Decoration

Files: `src/terrain/legacy.rs`, runtime decoration integration.

- [x] Replace trunks/cones/cubes with deterministic tree and rock variants.
- [x] Share per-kind scene meshes and one vertex-colour material; sample the
  current canonical terrain and update grounded decoration after sculpting.
- [x] Test track clearance, model variants, physical alignment and reload cleanup.

## 4. Packaging And Documentation

Files: `scripts/package_release.py`, `scripts/tests/test_package_release.py`,
`src/app_paths/validation.rs`, `README.md`, `docs/nature-assets.md`.

- [x] Add failing tests that all 23 GLBs and catalogue ship, no authoring files
  ship, and missing/corrupt nature resources fail packaging.
- [x] Update the explicit whitelist and runtime resource checks.
- [x] Document model IDs, axis correction, collisions, natural dimensions,
  persistence and prototype limitations.

## 5. Acceptance

- [x] Run targeted tests then full `cargo test --quiet` and
  `cargo test --release --quiet`, `cargo fmt --all -- --check`, Python tests,
  `cargo build --release --locked`, and `git diff --check`.
- [x] Extend/render an isolated nature smoke showing all models and scenery;
  inspect captures and verify preview, placement, selection, reload and driving.
- [x] Review data compatibility, collision consistency and resource ownership.
- [x] Record exact results and limits; no commit, tag, push or release.

Validation detail and limitations are recorded in `docs/nature-assets.md`.
Witnessed RED/GREEN cases included unknown Nature JSON, small-map orphan meshes,
post-sculpt Rapier pose restoration, existing material-count cleanup and tree
selection through noncolliding foliage. Final suites: 340 Rust tests in each
mode; 39 Python tests; Nature and existing Terrain GPU probes successful.
