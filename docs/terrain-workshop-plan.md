# Terrain Workshop Implementation Plan

> For agentic workers: use superpowers:subagent-driven-development for independent modules, then integrate and review the complete runtime.

**Goal:** Implement the complete Terrain Workshop specification (A through G), preserving the existing vehicle, profiles, 120 Hz simulation and historical valley.

**Architecture:** A validated, bounded heightmap is the canonical source for mesh vertices, triangle interpolation and Rapier collision. Scene documents contain exact height snapshots, procedural settings, stable object IDs and a vehicle spawn. Editing changes canonical data, records compact operations, and publishes geometry before Rapier SyncBackend; the car stays fixed in Edit mode until the new scene has been synchronized.

**Tech Stack:** Existing pinned Bevy 0.19, bevy_rapier3d 0.35, bevy_egui 0.42 and serde; add only pinned noise 0.9.0 for seeded Perlin layers. No vehicle profile changes.

## Audit And Decisions

- Baseline: 83 passing tests on 2026-10-15, clean source tree, based on vehicle physics branch commit e163dbd.
- Existing valley functions, forest, rocks, road and refuge move together to terrain/legacy.rs. Preserve their coordinates and geometry.
- A single input-capture system must cover both egui windows, retaining pointer-drag ownership and focus-loss cancellation. Egui actions commit once per frame even during multipass layout.
- T toggles terrain outside text entry, ² retains the physics shortcut. Both panels can coexist. Terrain mode defaults to Drive on the historical valley.
- Maximum 66,049 vertices, 512 placed objects, finite bounded dimensions/heights/transforms; reject invalid documents before modifying the active scene.
- Full bounded heightmap mesh/collider reconstruction is acceptable in v1, throttled while brushing; dirty vertex rectangles are retained for later chunks. Reuse mesh handles and shared materials. Rebuild objects only when changed.
- Obstacles use baked dimensions and unit Transform scale. Their meshes and colliders derive from identical geometry. Store u64 IDs, independent of Entity.
- Save complete edited heights plus generator/version metadata. Reject unknown format/generator versions explicitly. Save user variants atomically; built-in reference names cannot be overwritten.
- Brush history merges a complete drag into one sparse delta; store at least 20 operations within a bounded memory budget. Object edits store before/after object lists, not terrain copies.

## Increments

### A: Historical Scene And Ownership
- [x] Extract legacy geometry/decor and height queries from main.rs.
- [x] Give every scene entity one TerrainRoot owner, excluding car, camera and HUD.
- [x] Keep existing terrain/vehicle tests passing; add legacy mesh and ownership regressions.

### B: Canonical Generation
- [x] Test config bounds, deterministic seeds, finite values, dimensions, triangle winding, triangle-exact sampling and mesh/collider parity.
- [x] Implement config.rs, data.rs, generation.rs and mesh.rs with Flat/Valley/Mountain/Trial/Legacy presets.
- [x] Run focused terrain tests, then retain all previous physics tests.

### C: Runtime, Modes And Input
- [x] Add TerrainPlugin and terrain/runtime.rs resources/actions, with transactional scene replacement.
- [x] Generalize shared egui input capture, test drag ownership and two-panel coexistence.
- [x] Implement terrain UI tabs and Edit/Drive switching, editor orbit/pan/zoom and controlled fixed-body vehicle reset.
- [x] Add headless tests proving fixed-step backend sees replacements and Edit blocks driving.

### D: Sculpture
- [x] Test Raise/Lower/Smooth/Flatten, outside-radius invariance, source-snapshot smoothing and dt scaling.
- [x] Raycast only the synchronized terrain, show a surface-following brush ring, and preserve one gesture owner until release/focus loss.
- [x] Merge sparse height deltas per stroke; throttle global collision publication and flush before entering Drive.

### E: Objects And Reference Courses
- [x] Implement Cube/Cylinder/Sphere/Rock/Ramp, dimensions baked in meshes/colliders, and stable IDs.
- [x] Support click placement/selection, numeric transforms, align/duplicate/delete and preview/selection gizmos.
- [x] Add Historical/Flat/Hill/SideSlope/Articulation/Steps/Rocks/Ruts reproducible scenes and safe spawn orientations.

### F: Persistence And History
- [x] Test exact JSON roundtrip of sculpted data, objects/spawn, invalid versions/data/paths and failure preserving active scene.
- [x] Implement configs/terrains store, bounded reads, atomic saves and protected reference names.
- [x] Test undo/redo for complete strokes, placement, deletion and transforms, redo invalidation and memory/history limits.
- [x] Include usable example scene JSON documents.

### G: Integration And Verification
- [x] Review spec coverage and code quality with independent agents; address findings.
- [x] Add generated-terrain vehicle stabilization, replacement/removal backend collision queries and egui interaction tests.
- [x] Run cargo fmt --all -- --check, cargo test, cargo test --release, cargo build --release and git diff --check.
- [x] Launch the actual application and inspect logs/screenshots if available. Distinguish automated/runtime checks from manual driving claims.
- [x] Update README.md and docs/terrain-workshop.md with architecture, controls, JSON, costs, limitations and future chunks.

## Verification Commands

```powershell
cargo test terrain::
cargo fmt --all -- --check
cargo test
cargo test --release
cargo build --release
git diff --check
```

Cargo's target junction uses D:\motor-trail-build; commands need access to that existing cache. No new server or browser application is needed for this native game.

## Final Evidence: 2026-10-15

- Formatting, debug tests (178/178), release tests (178/178), release build and whitespace checks passed on Windows.
- The native Vulkan probe passed with real mouse raycasts, sculpting, placement/selection, exact JSON roundtrip, Undo/Redo and Edit/Drive transitions.
- The car travelled approximately 10 m during the automated driving interval; all four screenshots were inspected. This is not a manual assessment of driving feel.
- A failing backend regression reproduced new child obstacles being initialized at the origin. Publishing their global pose before first Rapier sync fixed all five shapes; the regression now passes.
- Implementation remains local on codex/terrain-workshop; no new pull request, push or merge was performed.
