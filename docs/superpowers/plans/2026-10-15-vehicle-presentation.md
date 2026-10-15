# Vehicle presentation synchronization

Approved scope: preserve the 120 Hz physics, existing tire support, traction and
single force pass. Interpolate graphics and follow camera together. Characterize
a wheel wedged between two obstacles before considering contact changes.

## Diagnosis

The chassis GLB and wheel nodes currently display the last fixed-step pose,
while the camera smooths every render frame. At render rates not dividing 120 Hz,
this repeats and skips visible poses. Rapier's TransformInterpolation does not
interpolate in the fixed timestep mode used here.

## Tasks

- [x] Add a failing render-frame regression above 120 FPS.
- [x] Capture fixed-step endpoints and interpolate graphics only, including
  wheel travel/steering/wrapped spin and equipment. Camera uses the displayed pose.
- [x] Test resets, missing/recreated vehicles, equipment collider separation,
  and native/experimental presentation without changing physical trajectories.
- [x] Characterize repeated wedged-wheel and flat TriMesh contacts. Keep contact
  behavior unchanged unless a reproduced failure establishes a separate cause.
- [x] Run format, focused/full debug and release tests, release build, review.
- [x] Document measurements and remaining limitations.

Implementation stays in the current checkout with the existing Mission A work.

Debug and release tests: 299 passed in each profile, none ignored. Format check,
release build and git diff check passed. The four pre-existing dead_code warnings
remain. Independent presentation review found no actionable
issues; added the requested production-path reset checks. Native GPU probes for
experimental step crossing, body/equipment and Terrain Workshop all exited 0.
Constrained wedge tracing establishes support switching, not a stable full-car
equilibrium; production contact selection remains unchanged.
