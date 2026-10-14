# Vehicle Physics Workshop

The workshop keeps the valley, model, camera and driving controls. The key left
of `1` (`KeyCode::Backquote`, logical `\u{00b2}` on French keyboards) toggles a
movable egui window. No function key is required.

## Implementation

1. Centralize validated, serializable physical settings in `vehicle/config.rs`.
   Keep physical units and preserve the original controller/profile for comparison.
2. Separate vehicle simulation, wheel visuals and telemetry from the world in
   `main.rs`. Run physics at 120 Hz, between Rapier synchronization and stepping.
   Apply each tire force once. Live settings update mass properties, damping,
   suspension tuning, tires, throttle response, ratios and brakes.
3. Add five egui tabs in `ui/physics_tuning.rs`, including wheel diagnostics,
   preset selection, modified values and an A/B reference. Capture mouse input
   only when interacting with the panel and keyboard input when editing values.
4. Add JSON profile persistence with validated filenames, last-profile restore
   and a visible fallback error for invalid files. Commit reference presets.
5. Run profile/configuration tests and headless Rapier driving regressions,
   build the release executable, then prepare the requested branch and PR.

## Checks

- Correct rolling sign for the glTF instance rotated 180 degrees.
- Four contact positions match the displayed hubs; suspension travel is shared
  by rendering and physics.
- Idle steering, reverse starts, braking priority, slopes and the actual terrain.
- Mass and spring/load relationships, invalid values, distinct saved profiles,
  startup fallback and keyboard toggle recognition.
- Native normal load is measured; legacy spring loads and slip are estimates.
- Vehicle feel remains a manual test; compilation is not a handling verdict.

## Physics References

- https://rapier.rs/docs/user_guides/python/vehicle_controller/
- https://rapier.rs/docs/user_guides/bevy_plugin/integration_parameters/
- https://docs.rs/crate/bevy_egui/0.42.0

The native controller's Coulomb limit is a combined grip coefficient; lateral
stiffness is a separate dimensionless response multiplier. Neither is presented
as an independently measured longitudinal tire-friction coefficient.
