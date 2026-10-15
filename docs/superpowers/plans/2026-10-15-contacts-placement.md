# Wheel Diagnostic And Placement Preview Delivery

**Scope decision:** The user chose to retain the original Rapier wheel controller
and deliver diagnosis plus placement preview first. The local adapted controller
was removed. This decision supersedes the earlier integrated-envelope experiment
and its 206-test results. Tire penetration is reproduced, not fixed.

## Delivered

- [x] Measure actual GLB pivots, rubber/tread radius and suspension offsets.
- [x] Preserve controller.rs, visuals.rs, vehicle-profile JSON and the GLB.
- [x] Add optional V diagnostics for physical wheel centers, radius, rays,
  native contacts and actual obstacle colliders.
- [x] Keep finite-cylinder shape queries read-only in contacts.rs. No force
  solver, physical wheel collider or contact replacement is connected.
- [x] Characterize native 25 cm step penetration forward and reverse, with
  flat-ground/static-load and GLB parity checks. These are diagnosis tests,
  not claims that the future low-penetration acceptance criterion passes.
- [x] Add real transparent green/red placement geometry without a collider.
- [x] Implement Q/E yaw, Shift fine steps, physical AZERTY alias, repeated
  placements, fresh picks and UI-owned input protection.
- [x] Clean temporary meshes, materials and entities on cancellation, tool,
  mode or scene changes. Escape cancels without exiting the game.
- [x] Retain sculpting, selection, Undo/Redo, exact JSON and both workshops.
- [x] Remove the experimental copied Rapier controller and its license/tests.
- [x] Document that physical tire improvement is deferred.

## Verification

The retained-controller snapshot passes 201 normal tests and 201 release
tests, with no ignored tests. This replaces the previous 206-test experimental
snapshot. controller.rs and visuals.rs have no diff from their original
implementation. Native tire probes measure the current penetration defect,
rather than certify a correction.

The placement renderer scenario was verified with six captures, yaw 15 -> 10,
exact displayed/committed pose, cancellation cleanup, selection, JSON,
Undo/Redo and approximately 10 m of driving with the physics window.
No automated test is presented as manual driving-feel validation.

## Deferred Work

A future physical contact adapter requires an explicit design decision and a
single force path. Do not silently reconnect the geometric diagnostic to the
suspension solver. Mission A begins with the separate diagnostic/design in
`docs/vehicle-collision-design.md`; its body collision and equipment work is
not already implemented by this delivery.

## Commands

```powershell
cargo fmt --all -- --check
cargo test
cargo test --release
cargo build --release
git diff --check
```

Cargo uses the existing target junction to D:\motor-trail-build. Probe images
and JSON belong in the task visualization directory, not user scene files.
