# Native Distribution Design

Approved by the user: lightweight packaging scripts, read-only shipped resources,
OS user-data directories, non-overwriting initial migration, tag-only publishing.

## Boundaries

- `src/app_paths.rs` resolves executable-relative Windows and macOS resources,
  with a development manifest fallback only for development builds. Bevy receives
  an absolute assets directory. No physics, contacts or terrain geometry changes.
- User data lives under APPDATA/MotorTrail on Windows, Library/Application
  Support/MotorTrail on macOS, and XDG_DATA_HOME/MotorTrail on Linux for CI/dev.
  A command-line data-directory override enables isolated tests and portable
  diagnostics, not writes into the installation by default.
- Initial migration copies recognized legacy profiles, selection marker and
  scenes before missing defaults, never replacing user files or deleting sources.
  A successful migration marker prevents deleted user files being resurrected.
- Packages contain assets, defaults/vehicles, defaults/terrains, README.txt and
  version.json. macOS uses Contents/MacOS and Contents/Resources, a stable
  com.motortrail.game bundle ID and a numeric bundle version plus full Git tag.
  With no existing icon, use the OS generic application icon for now.
- `--check-installation` validates shipped JSON/GLB resources without creating
  a window, renderer or user data; it must work from an unrelated directory.
- The release workflow has only push.tags.v*. Native Windows x64, macOS ARM
  (macos-15), macOS Intel (macos-15-intel) build with stable Rust and --locked.
  Builds have contents:read, publishing alone contents:write. All three archives
  must validate before a draft is published; alpha/beta/rc tags are prereleases.
- No tags, remote releases, secret changes, signing bypasses, updater or launcher
  are part of this implementation. macOS builds and first real publishing require
  future tag execution on GitHub, not an artificial manual-trigger exception.

## Validation

TDD for path selection, migration preservation and archive validation; full Rust
regressions; packaging tests with synthetic platform binaries; real local Windows
package and headless check from another working directory; fmt/release build and
workflow validation. Graphics and macOS Gatekeeper remain manual acceptance steps.

Runner labels verified against https://docs.github.com/en/actions/reference/runners/github-hosted-runners
and https://github.com/actions/runner-images on 2026-10-15.
