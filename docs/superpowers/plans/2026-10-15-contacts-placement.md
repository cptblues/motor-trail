# Wheel Contacts And Placement Preview Implementation Plan

**Goal:** Reduce wheel/obstacle penetration without changing tire dimensions or applying forces twice, and add a collider-free placement preview with rotation and cancellation.

**Architecture:** Keep canonical terrain and stored obstacles unchanged. A dedicated placement-preview module owns temporary assets/entities, while the terrain runtime remains the only path that commits obstacles. Diagnose wheel geometry against the actual GLB and reproduce the vertical-step failure before selecting a contact-query change. Retain the existing suspension, traction, braking and friction-limiting calculations.

**Tech Stack:** Existing pinned Bevy, Rapier, bevy_egui and serde. No dependency upgrade or vehicle-profile edit.

## Audit Evidence

- The GLB rubber radius is 0.53000003 m; the physical default and visual reference radius are 0.53 m. Tread reaches 0.55512388 m, with rubber width 0.342 m.
- The model root is translated by -1 m and rotated by PI around Y. Suspension Y = 0.78 - travel therefore matches physical mount Y = -0.22 minus travel. Front/rear and left/right pivots match MOUNTS after that rotation.
- Rapier 0.33-alpha uses one descending ray per wheel, length rest_length + radius. The ray misses a vertical face before the hub crosses its edge. RayCastInfo is read-only and contact injection is not exposed.
- Workshop cubes use exact cuboids and unit scale. Other primitives bake the same dimensions into meshes/colliders.
- Options: radius/visual offsets would hide the cause; additional physical wheel colliders risk duplicate support/friction. Selected a narrowly derived native-controller adapter that replaces contact detection while retaining one force path, without a dependency upgrade or new wheel colliders.

## Work Areas

- Vehicle work: controller.rs, visuals.rs, vehicle plugin, focused contact-query/diagnostic modules and GLB/step/flat regression tests. Parent owns this work.
- Preview work: terrain runtime and a placement-preview module, narrowly necessary terrain UI changes, tests and shortcut documentation. Independent worker owns this work; no vehicle/main edits.
- Shared verification: serialize Cargo builds because the Windows linker is memory-heavy. Preserve the already staged Terrain Workshop implementation.

## Steps

- [x] Measure actual GLB vertices and pivots; document the radius, tread and suspension comparison.
- [x] Add diagnostic overlays for physical centers/radii, native contacts, ray directions and obstacle collider wireframes. Keep diagnostics optional and out of persisted profiles.
- [x] Add a vertical-step regression showing excessive penetration with the existing central ray. Keep a flat-ground contact/ride-height regression and GLB geometry parity checks.
- [x] Derive a contact-injectable adapter from the pinned Rapier implementation with source/license attribution. Use native Parry/Rapier shape casting for a finite-width wheel envelope. Preserve force equations, mass normalization, friction limits, braking, steering and the 120 Hz schedule; never run native and adapted force solvers together.
- [x] Verify the step/rock envelope detects obstacles before the central ray while flat-ground support stays equivalent. Test reverse, steering, no contact, invalid inputs and negative maximum-compression limits. Review raised-cast overlap and add a combined ceiling/step regression.
- [x] Add preview tests first: geometry/scale/orientation parity, no rigid body or collider, stable handles, valid/invalid colors, UI ownership, Q/E increments, repeated placements and cleanup on cancellation/tool/mode/scene changes.
- [x] Implement preview as a real transparent mesh owned separately from persisted scene objects. Compute one candidate pose for preview and confirmation. Keep rotation per catalog kind; Q/E are 15 degrees, Shift+Q/E are 5 degrees. RMB/Escape cancel and consume placement input without exiting the game or driving.
- [x] Confirm only through the existing transactional commit_objects path after a fresh valid pick, with no UI-captured mouse. Remove temporary assets and entities when the preview is no longer applicable.
- [x] Review both changes and run focused tests before full normal/release suites. Run cargo fmt --all -- --check, cargo test, cargo test --release, cargo build --release, git diff --check and index whitespace checks.
- [x] Execute the native renderer probe for preview rotation/cancellation, obstacle contact diagnostics, driving and the two workshops. Inspect screenshots; distinguish automated driving from a manual feel assessment.
- [x] Update README and terrain documentation with controls, diagnosed cause, adapter provenance, actual checks and remaining ray/finite-wheel limitations. Do not commit, push or merge unrelated staged work.

## Final Evidence

- 206/206 tests pass in normal and release profiles, none ignored. Formatting, release build and both Git whitespace checks pass.
- The original central ray fails the step acceptance test at 0.2531546 m. The selected envelope path passes at 0.037604123 m forward and 0.03319609 m reverse; native flat-ground force parity is exact.
- Final native wheel probe: four wheels contact the step, 13.312 m travelled, maximum rubber penetration 0.01868 m, four captures inspected, exit 0.
- Final terrain probe: six captures inspected, yaw 15 -> 10, exact preview/committed pose, cancellation cleanup, JSON and Undo/Redo, 9.86 m driving with the physics window, eight objects and nine scene colliders, exit 0. Windows denied one native cursor-warp request; injected inputs and all assertions still succeeded.
- No claim of manually validated driving feel or perfect rigid-wheel CCD. GLB, wheel dimensions, vehicle-profile JSON and Legacy force equations unchanged. Current corrections remain local; pre-existing staged changes were preserved.

## Verification Commands

```powershell
cargo test --release placement
cargo test --release wheel
cargo fmt --all -- --check
cargo test
cargo test --release
cargo build --release
git diff --check
git diff --cached --check
```

Cargo uses the existing target junction to D:\motor-trail-build, requiring the same execution permission as the previous verification. Native screenshots and probe JSON go to this chat's visualization directory, not user scene files.
