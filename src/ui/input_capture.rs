//! One owner for input capture across all egui windows.
use super::physics_tuning::PhysicsTuningState;
use super::terrain_workshop::WorkshopDockBounds;
use crate::terrain::runtime::TerrainWorkshopState;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, EguiPreUpdateSet, egui};

#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct UiInputCapture {
    pub pointer: bool,
    pub keyboard: bool,
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct UiInputSet;

#[derive(Default)]
struct PointerOwner(Option<bool>);

impl PointerOwner {
    fn update(&mut self, focused: bool, pressed: bool, down: bool, over_ui: bool) -> bool {
        if !focused {
            self.0 = None;
            return true;
        }
        if pressed && self.0.is_none() {
            self.0 = Some(over_ui);
        }
        let captured = self.0.unwrap_or(over_ui);
        if !down {
            self.0 = None;
        }
        captured
    }
}

pub struct UiInputPlugin;
impl Plugin for UiInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiInputCapture>().add_systems(
            PreUpdate,
            refresh_input_capture
                .in_set(UiInputSet)
                .after(InputSystems)
                .after(EguiPreUpdateSet::InitContexts)
                .before(EguiPreUpdateSet::ProcessInput),
        );
    }
}

fn in_editor_dock(rect: Option<egui::Rect>, point: egui::Pos2) -> bool {
    rect.is_some_and(|bounds| bounds.contains(point))
}

fn refresh_input_capture(
    mut contexts: EguiContexts,
    physics: Res<PhysicsTuningState>,
    terrain: Option<Res<TerrainWorkshopState>>,
    dock: Option<Res<WorkshopDockBounds>>,
    mut capture: ResMut<UiInputCapture>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut owner: Local<PointerOwner>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let focused = windows.single().is_ok_and(|window| window.focused);
    let any_open = physics.open || terrain.as_ref().is_some_and(|state| state.open);
    if !any_open {
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }
    let over_ui = ctx.any_popup_open()
        || any_open
            && windows
                .single()
                .ok()
                .and_then(|window| {
                    window.cursor_position().map(|position| {
                        let scale = ctx.pixels_per_point() / window.scale_factor();
                        let pointer = egui::pos2(position.x / scale, position.y / scale);
                        // Panels are laid out on a background root Ui in egui
                        // 0.36, so layer_id_at may return Background even
                        // while the cursor is interacting with a dock widget.
                        let over_dock = terrain.as_ref().is_some_and(|state| state.open)
                            && dock.as_ref().is_some_and(|dock|
                                in_editor_dock(dock.rect, pointer));
                        over_dock || ctx.layer_id_at(pointer)
                            .is_some_and(|layer| layer.order != egui::Order::Background)
                    })
                })
                .unwrap_or(false);
    let buttons = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];
    capture.pointer = owner.update(
        focused,
        buttons.iter().any(|button| mouse.just_pressed(*button)),
        buttons.iter().any(|button| mouse.pressed(*button)),
        over_ui,
    );
    capture.keyboard =
        !focused || (any_open && (ctx.egui_wants_keyboard_input() || ctx.any_popup_open()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn docked_panel_captures_clicks_even_when_on_egui_background_layer() {
        let rect = egui::Rect::from_min_size(egui::pos2(900.0, 0.0), egui::vec2(400.0, 800.0));
        assert!(in_editor_dock(Some(rect), egui::pos2(910.0, 50.0)));
        assert!(in_editor_dock(Some(rect), egui::pos2(1200.0, 750.0)));
        assert!(!in_editor_dock(Some(rect), egui::pos2(400.0, 50.0)));
        assert!(!in_editor_dock(None, egui::pos2(910.0, 50.0)));
    }

    #[test]
    fn a_ui_drag_stays_owned_until_release() {
        let mut owner = PointerOwner::default();
        assert!(owner.update(true, true, true, true));
        assert!(owner.update(true, false, true, false));
        assert!(owner.update(true, false, false, false));
        assert!(!owner.update(true, false, false, false));
    }
    #[test]
    fn a_world_drag_cannot_become_a_widget_drag() {
        let mut owner = PointerOwner::default();
        assert!(!owner.update(true, true, true, false));
        assert!(!owner.update(true, false, true, true));
    }
    #[test]
    fn losing_focus_cancels_the_drag_and_blocks_controls() {
        let mut owner = PointerOwner::default();
        owner.update(true, true, true, true);
        assert!(owner.update(false, false, false, false));
        assert!(!owner.update(true, false, false, false));
    }
}
