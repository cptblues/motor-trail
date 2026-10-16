use std::cell::Cell;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use crate::terrain::brush::{Brush, BrushKind};
use crate::terrain::config::{TerrainConfig, TerrainPreset};
use crate::terrain::obstacles::{Obstacle, ObstacleKind};
use crate::terrain::runtime::{
    ActiveTerrain, EditorTool, GizmoMode, PlacementMode, TerrainWorkshopState, WorkshopAction, WorkshopMode,
};
use crate::terrain::scene::{ReferenceScene, VehicleSpawn};

/// The parent installs egui, initializes terrain resources and owns input capture.
pub struct TerrainWorkshopPlugin;

impl Plugin for TerrainWorkshopPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, terrain_workshop_ui);
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WorkshopTab {
    #[default]
    Generation,
    Sculpture,
    Objects,
    Scenes,
    Diagnostic,
}

impl WorkshopTab {
    const ALL: [Self; 5] = [
        Self::Generation,
        Self::Sculpture,
        Self::Objects,
        Self::Scenes,
        Self::Diagnostic,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Generation => "Generation",
            Self::Sculpture => "Sculpture",
            Self::Objects => "Objets",
            Self::Scenes => "Scenes",
            Self::Diagnostic => "Diagnostic",
        }
    }
}

#[derive(Clone)]
enum Confirmation {
    Generate(TerrainConfig),
    New,
    Reference(ReferenceScene),
    Load(String),
    Save(String),
    SaveAs(String),
}

impl Confirmation {
    fn description(&self) -> String {
        match self {
            Self::Generate(config) => format!("Remplacer le relief sculpte, les objets et le depart actuel ? Preset : {} · Graine : {} · Dimensions : {:.0} x {:.0} m. Enregistrer votre map avant de confirmer.", preset_name(config.preset), config.seed, config.width, config.depth),
            Self::New => "Creer une scene et remplacer le terrain courant ?".to_owned(),
            Self::Reference(reference) => format!(
                "Charger {} et remplacer le terrain courant ?",
                reference.name()
            ),
            Self::Load(name) => format!("Charger \"{name}\" et remplacer le terrain courant ?"),
            Self::Save(name) => format!("Remplacer le fichier \"{name}\" ?"),
            Self::SaveAs(name) => format!("Remplacer le fichier \"{name}\" ?"),
        }
    }

    fn into_action(self) -> WorkshopAction {
        match self {
            Self::Generate(config) => WorkshopAction::Generate(config),
            Self::New => WorkshopAction::New,
            Self::Reference(reference) => WorkshopAction::LoadReference(reference),
            Self::Load(name) => WorkshopAction::Load(name),
            Self::Save(_) => WorkshopAction::Save,
            Self::SaveAs(name) => WorkshopAction::SaveAs(name),
        }
    }
}

#[derive(Clone, PartialEq)]
struct SpawnDraft {
    position: [f32; 3],
    yaw_degrees: f32,
}

impl SpawnDraft {
    fn read(spawn: &VehicleSpawn) -> Self {
        Self {
            position: spawn.position,
            yaw_degrees: spawn.yaw_degrees,
        }
    }

    fn valid(&self) -> bool {
        self.position.iter().all(|value| value.is_finite()) && self.yaw_degrees.is_finite()
    }

    fn into_spawn(self) -> VehicleSpawn {
        VehicleSpawn {
            position: self.position,
            yaw_degrees: self.yaw_degrees,
        }
    }
}

#[derive(Clone, Default)]
struct UiDraft {
    tab: WorkshopTab,
    confirmation: Option<Confirmation>,
    object: Option<Obstacle>,
    object_baseline: Option<Obstacle>,
    spawn: Option<SpawnDraft>,
    spawn_baseline: Option<SpawnDraft>,
    seed_widget: Option<egui::Id>,
    asset_query: String,
    asset_category: String,
    quick_relief: Option<u8>,
}

/// Only editable controls are copied. Files, heightmaps and object lists stay borrowed.
#[derive(Clone)]
struct Controls {
    open: bool,
    mode: WorkshopMode,
    config: TerrainConfig,
    brush: Brush,
    tool: EditorTool,
    object_kind: ObstacleKind,
    object_size: [f32; 3],
    placement_mode: PlacementMode,
    placement_follow_ground: bool,
    placement_offset_y: f32,
    gizmo_mode: GizmoMode,
    snap_enabled: bool,
    snap_objects: bool,
    snap_grid: f32,
    snap_angle: f32,
    uniform_scale: bool,
    placement_yaws: Vec<(ObstacleKind, f32)>,
    selected: Option<u64>,
    scene_name: String,
    file_selection: String,
    cursor: Option<Vec3>,
}

impl Controls {
    fn choose_kind(&mut self, kind: ObstacleKind) {
        if self.object_kind != kind {
            self.object_size = match kind {
                ObstacleKind::Nature(asset) => asset.default_size().to_array(),
                _ => [2.0, 2.0, 2.0],
            };
        }
        self.object_kind = kind;
        self.tool = EditorTool::Place;
    }

    fn read(state: &TerrainWorkshopState) -> Self {
        Self {
            open: state.open,
            mode: state.mode,
            config: state.draft.clone(),
            brush: state.brush.clone(),
            tool: state.tool,
            object_kind: state.object_kind,
            object_size: state.object_size,
            placement_mode: state.placement_mode,
            placement_follow_ground: state.placement_follow_ground,
            placement_offset_y: state.placement_offset_y,
            gizmo_mode: state.gizmo_mode,
            snap_enabled: state.snap_enabled,
            snap_objects: state.snap_objects,
            snap_grid: state.snap_grid,
            snap_angle: state.snap_angle,
            uniform_scale: state.uniform_scale,
            placement_yaws: ObstacleKind::catalogue()
                .map(|kind| (kind, state.placement_yaw_degrees(kind)))
                .collect(),
            selected: state.selected,
            scene_name: state.scene_name.clone(),
            file_selection: state.file_selection.clone(),
            cursor: state.cursor,
        }
    }
}

#[derive(Clone)]
struct FrameSnapshot {
    frame: u64,
    controls: Controls,
    draft: UiDraft,
    text_states: Vec<(egui::Id, egui::widgets::text_edit::TextEditState)>,
}

#[derive(Default)]
struct WorkshopUi {
    draft: UiDraft,
    snapshot: Option<FrameSnapshot>,
    committed_frame: Option<u64>,
}

impl WorkshopUi {
    fn sync_selection(&mut self, state: &TerrainWorkshopState, view: &WorkshopView<'_>) {
        let original = state.selected.and_then(|id| 
            view.obstacles.iter().find(|object| object.id == id));
        if let Some(candidate) = view.preview_object {
            self.draft.object = Some(candidate.clone());
            self.draft.object_baseline = original.cloned();
        } else if self.draft.object_baseline.as_ref() != original {
            self.draft.object = original.cloned();
            self.draft.object_baseline = original.cloned();
        }
        let spawn = SpawnDraft::read(view.spawn);
        if self.draft.spawn_baseline.as_ref() != Some(&spawn) {
            self.draft.spawn = Some(spawn.clone());
            self.draft.spawn_baseline = Some(spawn);
        }
    }

    fn begin_frame(
        &mut self,
        ctx: &egui::Context,
        state: &TerrainWorkshopState,
    ) -> (Controls, UiDraft) {
        let frame = ctx.cumulative_frame_nr();
        if self
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.frame != frame)
        {
            let mut ids = vec![scene_name_id()];
            if let Some(id) = self.draft.seed_widget {
                ids.push(id);
            }
            let text_states = ids
                .into_iter()
                .map(|id| (id, egui::TextEdit::load_state(ctx, id).unwrap_or_default()))
                .collect();
            self.snapshot = Some(FrameSnapshot {
                frame,
                controls: Controls::read(state),
                draft: self.draft.clone(),
                text_states,
            });
        }
        let snapshot = self.snapshot.as_ref().unwrap();
        for (id, text) in &snapshot.text_states {
            egui::TextEdit::store_state(ctx, *id, text.clone());
        }
        (snapshot.controls.clone(), snapshot.draft.clone())
    }

    fn commit(
        &mut self,
        ctx: &egui::Context,
        state: &mut TerrainWorkshopState,
        controls: &mut Controls,
        draft: &mut UiDraft,
        action: Option<WorkshopAction>,
    ) {
        let frame = ctx.cumulative_frame_nr();
        if self.committed_frame == Some(frame) {
            return;
        }
        self.committed_frame = Some(frame);
        self.draft = draft.clone();
        state.open = controls.open;
        state.draft = controls.config.clone();
        if controls.brush.validate().is_ok() {
            state.brush = controls.brush.clone();
        }
        state.tool = controls.tool;
        state.object_kind = controls.object_kind;
        state.object_size = controls.object_size;
        state.placement_mode = controls.placement_mode;
        state.placement_follow_ground = controls.placement_follow_ground;
        state.placement_offset_y = controls.placement_offset_y.clamp(-32.0, 32.0);
        state.gizmo_mode = controls.gizmo_mode;
        state.snap_enabled = controls.snap_enabled;
        state.snap_objects = controls.snap_objects;
        state.snap_grid = controls.snap_grid.clamp(0.05, 5.0);
        state.snap_angle = controls.snap_angle.clamp(1.0, 90.0);
        state.uniform_scale = controls.uniform_scale;
        state.selected = controls.selected;
        state.scene_name.clone_from(&controls.scene_name);
        state.file_selection.clone_from(&controls.file_selection);
        if let Some(action) = action {
            state.actions.push(action);
        }
    }
}

struct WorkshopView<'a> {
    scene_name: &'a str,
    config: &'a TerrainConfig,
    obstacles: &'a [Obstacle],
    spawn: &'a VehicleSpawn,
    file: Option<&'a str>,
    revision: u64,
    applied_revision: u64,
    generation_ms: f64,
    undo: usize,
    redo: usize,
    history_bytes: usize,
    vertices: usize,
    triangles: usize,
    collider_count: usize,
    terrain_bytes: usize,
    cells: (usize, usize),
    ground_height: &'a dyn Fn(f32, f32) -> Option<f32>,
    preview_object: Option<&'a Obstacle>,
}

fn terrain_workshop_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<TerrainWorkshopState>,
    active: Res<ActiveTerrain>,
    manipulation: Res<crate::terrain::runtime::GizmoGesture>,
    mut local: Local<WorkshopUi>,
) -> Result {
    if !state.open {
        return Ok(());
    }
    let terrain = &active.scene.terrain;
    let ground_height = |x, z| terrain.height(x, z);
    let view = WorkshopView {
        scene_name: &active.scene.name,
        config: &terrain.config,
        obstacles: &active.scene.obstacles,
        spawn: &active.scene.spawn,
        file: active.file.as_deref(),
        revision: active.revision,
        applied_revision: active.applied_revision,
        generation_ms: active.generation_ms,
        undo: active.history.undo_len(),
        redo: active.history.redo_len(),
        history_bytes: active.history.memory_bytes(),
        vertices: terrain.vertex_count(),
        triangles: terrain.triangle_count(),
        collider_count: active.collider_count,
        terrain_bytes: terrain.memory_bytes(),
        cells: (terrain.cells_x, terrain.cells_z),
        ground_height: &ground_height,
        preview_object: manipulation.candidate.as_ref(),
    };
    show_workshop(contexts.ctx_mut()?, &mut state, &view, &mut local);
    Ok(())
}

fn show_workshop(
    ctx: &egui::Context,
    state: &mut TerrainWorkshopState,
    view: &WorkshopView<'_>,
    local: &mut WorkshopUi,
) -> Option<egui::Rect> {
    if !state.open {
        return None;
    }
    if local
        .snapshot
        .as_ref()
        .is_none_or(|snapshot| snapshot.frame != ctx.cumulative_frame_nr())
    {
        local.sync_selection(state, view);
    }
    let (mut controls, mut draft) = local.begin_frame(ctx, state);
    if let Some(Confirmation::Save(name)) = &draft.confirmation {
        if view.file != Some(name.as_str()) {
            draft.confirmation = None;
        }
    }
    let mut action = None;
    let mut open = controls.open;
    let screen = ctx.content_rect();
    let maximum = (screen.size() - egui::vec2(16.0, 16.0)).max(egui::vec2(1.0, 1.0));
    let width = 560.0_f32.min(maximum.x);
    let result = egui::Window::new("Motor Trail | Terrain [T]")
        .id(egui::Id::new("motor_trail_terrain_workshop"))
        .open(&mut open)
        .movable(true)
        .resizable(true)
        .collapsible(false)
        .constrain_to(screen)
        .default_pos(egui::pos2(
            (screen.right() - width - 24.0).max(screen.left()),
            screen.top() + 76.0,
        ))
        .default_size(egui::vec2(width, 650.0_f32.min(maximum.y)))
        .min_size(egui::vec2(
            280.0_f32.min(maximum.x),
            180.0_f32.min(maximum.y),
        ))
        .max_size(maximum)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 5.0);
            let _ = toolbar(ui, &controls, view.undo, view.redo, &mut action);
            if !state.status.is_empty() {
                ui.label(&state.status);
            }
            if let Some(error) = &state.last_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal_wrapped(|ui| {
                for tab in WorkshopTab::ALL {
                    ui.selectable_value(&mut draft.tab, tab, tab.label());
                }
            });
            ui.separator();
            egui::ScrollArea::both()
                .id_salt(("terrain_workshop_body", draft.tab as u8))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match draft.tab {
                        WorkshopTab::Generation => generation(ui, &mut controls, &mut draft),
                        WorkshopTab::Sculpture => sculpture(ui, &mut controls),
                        WorkshopTab::Objects => {
                            objects(ui, &mut controls, view.obstacles, &mut draft, &mut action)
                        }
                        WorkshopTab::Scenes => {
                            scene_files(
                                ui,
                                &mut controls,
                                &state.files,
                                view.file,
                                &mut draft,
                                &mut action,
                            );
                            ui.separator();
                            spawn_controls(
                                ui,
                                &controls,
                                &mut draft,
                                view.ground_height,
                                &mut action,
                            );
                        }
                        WorkshopTab::Diagnostic => diagnostics(ui, view),
                    }
                    if draft.confirmation.is_some() {
                        ui.separator();
                        confirmation_controls(ui, &mut draft, &mut action);
                    }
                });
        });
    controls.open = open;
    local.commit(ctx, state, &mut controls, &mut draft, action);
    result.map(|result| result.response.rect)
}

fn toolbar(
    ui: &mut egui::Ui,
    controls: &Controls,
    undo: usize,
    redo: usize,
    action: &mut Option<WorkshopAction>,
) -> [egui::Response; 2] {
    ui.horizontal_wrapped(|ui| -> [egui::Response; 2] {
        for (mode, label) in [
            (WorkshopMode::Edit, "Edition"),
            (WorkshopMode::Drive, "Conduite"),
        ] {
            if ui.selectable_label(controls.mode == mode, label).clicked() && controls.mode != mode
            {
                enqueue(action, WorkshopAction::SetMode(mode));
            }
        }
        let undo_button = history_icon_button(ui, undo > 0, false)
            .on_hover_text(format!("Annuler ({undo})"))
            .on_disabled_hover_text("Annuler : historique vide");
        if undo_button.clicked() {
            enqueue(action, WorkshopAction::Undo);
        }
        let redo_button = history_icon_button(ui, redo > 0, true)
            .on_hover_text(format!("Retablir ({redo})"))
            .on_disabled_hover_text("Retablir : historique vide");
        if redo_button.clicked() {
            enqueue(action, WorkshopAction::Redo);
        }
        [undo_button, redo_button]
    })
    .inner
}

fn history_icon_button(ui: &mut egui::Ui, enabled: bool, redo: bool) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                if redo { "Retablir" } else { "Annuler" },
            )
        });
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            ui.painter().rect(
                rect,
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let point = |x: f32, y: f32| rect.min + egui::vec2(if redo { 22.0 - x } else { x }, y);
            let stroke = egui::Stroke::new(1.5, visuals.fg_stroke.color);
            ui.painter().line(
                vec![
                    point(16.0, 16.0),
                    point(16.0, 12.0),
                    point(15.0, 9.0),
                    point(12.0, 7.0),
                    point(5.0, 7.0),
                ],
                stroke,
            );
            ui.painter().line(
                vec![point(8.0, 4.0), point(5.0, 7.0), point(8.0, 10.0)],
                stroke,
            );
        }
        response
    })
    .inner
}

fn enqueue(slot: &mut Option<WorkshopAction>, action: WorkshopAction) {
    if slot.is_none() {
        *slot = Some(action);
    }
}

fn quick_config(preset: TerrainPreset, relief: u8, width: f32, depth: f32, seed: u64) -> TerrainConfig {
    let mut config = TerrainConfig::preset(preset);
    config.width = width;
    config.depth = depth;
    config.seed = seed;
    config.amplitude =
        (config.amplitude * (relief.min(100) as f32 / 50.0)).clamp(0.0, 256.0);
    config
}

/// SplitMix64 yields reproducible variants without dropping high bits of a u64 seed.
fn next_variation(seed: u64) -> u64 {
    let mut x = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let result = x ^ (x >> 31);
    if result == seed { result.wrapping_add(1) } else { result }
}

fn generation(ui: &mut egui::Ui, controls: &mut Controls, draft: &mut UiDraft) {
    ui.heading("Creation rapide");
    ui.label("Choisissez un paysage et creez une variante en un clic.");
    ui.horizontal_wrapped(|ui| {
        for (preset, name) in [
            (TerrainPreset::Flat, "Plaine"),
            (TerrainPreset::Valley, "Vallee"),
            (TerrainPreset::Mountain, "Montagne"),
            (TerrainPreset::Trial, "Trial"),
        ] {
            if ui.selectable_label(controls.config.preset == preset, name).clicked() {
                let current = &controls.config;
                controls.config = quick_config(
                    preset, draft.quick_relief.unwrap_or(50),
                    current.width, current.depth, current.seed,
                );
            }
        }
    });
    if controls.config.preset == TerrainPreset::Legacy {
        ui.small("La vallee historique est figee : choisissez un autre paysage pour la generation aleatoire.");
    }
    let mut relief = draft.quick_relief.unwrap_or(50);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Intensite du relief : {relief}%"));
        if ui.add(egui::Slider::new(&mut relief, 0..=100).show_value(false)).changed() {
            draft.quick_relief = Some(relief);
            if controls.config.preset != TerrainPreset::Legacy {
                let base = TerrainConfig::preset(controls.config.preset);
                controls.config.amplitude =
                    (base.amplitude * (relief as f32 / 50.0)).clamp(0.0, 256.0);
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Largeur [m]");
        ui.add(egui::DragValue::new(&mut controls.config.width).speed(1.0));
        ui.label("Profondeur [m]");
        ui.add(egui::DragValue::new(&mut controls.config.depth).speed(1.0));
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Graine");
        draft.seed_widget = Some(seed_editor(ui, &mut controls.config.seed).id);
        if ui.button("Copier").on_hover_text("Copier la graine exacte").clicked() {
            ui.ctx().copy_text(controls.config.seed.to_string());
        }
        if ui.button("Autre variante").clicked() {
            controls.config.seed = next_variation(controls.config.seed);
        }
    });
    let validation = controls.config.validate();
    if let Err(message) = &validation {
        ui.colored_label(ui.visuals().error_fg_color, message);
    } else {
        let (x, z) = controls.config.cells();
        ui.small(format!(
            "{:.0} x {:.0} m · {} x {} cellules · {} sommets · pas effectif {:.2} x {:.2} m",
            controls.config.width, controls.config.depth, x, z,
            (x + 1) * (z + 1),
            controls.config.width / x as f32, controls.config.depth / z as f32,
        ));
        ui.small(format!(
            "Seed {} : le meme preset et les memes reglages reproduisent le meme terrain.",
            controls.config.seed,
        ));
    }
    if ui.add_enabled(validation.is_ok(), egui::Button::new("Generer")).clicked() {
        draft.confirmation = Some(Confirmation::Generate(controls.config.clone()));
    }
    ui.small("La generation est appliquee seulement apres confirmation. Enregistrez la map actuelle pour la conserver.");
    ui.separator();
    egui::CollapsingHeader::new("Avance : tous les reglages du terrain")
        .default_open(false)
        .show(ui, |ui| {

    const HISTORICAL_HELP: &str = "Vallee historique : grille fixe de 120 x 165 m, pas de 1.25 m. La graine et les parametres de bruit sont ignores.";
    ui.horizontal_wrapped(|ui| {
        ui.label("Preset");
        egui::ComboBox::from_id_salt("terrain_generation_preset")
            .selected_text(preset_name(controls.config.preset))
            .show_ui(ui, |ui| {
                for preset in TerrainPreset::ALL {
                    let response =
                        ui.selectable_label(controls.config.preset == preset, preset_name(preset));
                    let response = if preset == TerrainPreset::Legacy {
                        response.on_hover_text(HISTORICAL_HELP)
                    } else {
                        response
                    };
                    if response.clicked() {
                        controls.config = TerrainConfig::preset(preset);
                    }
                }
            });
    });
    ui.add_enabled_ui(controls.config.preset != TerrainPreset::Legacy, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Graine");
            let response = seed_editor(ui, &mut controls.config.seed);
            draft.seed_widget = Some(response.id);
        });
        for (label, value, range, logarithmic) in [
            (
                "Largeur [m]",
                &mut controls.config.width,
                8.0..=256.0,
                false,
            ),
            (
                "Profondeur [m]",
                &mut controls.config.depth,
                8.0..=256.0,
                false,
            ),
            (
                "Resolution [m/cellule]",
                &mut controls.config.resolution,
                0.25..=4.0,
                false,
            ),
            (
                "Amplitude [m]",
                &mut controls.config.amplitude,
                0.0..=256.0,
                false,
            ),
            (
                "Frequence [1/m]",
                &mut controls.config.frequency,
                0.0001..=4.0,
                true,
            ),
            (
                "Persistance",
                &mut controls.config.persistence,
                0.0..=1.0,
                false,
            ),
            (
                "Lacunarite",
                &mut controls.config.lacunarity,
                1.0..=4.0,
                false,
            ),
            ("Rugosite", &mut controls.config.roughness, 0.0..=2.0, false),
        ] {
            slider_control(ui, label, value, range, logarithmic);
        }
        ui.label("Octaves");
        ui.add(
            egui::Slider::new(&mut controls.config.octaves, 1..=8)
                .clamping(egui::SliderClamping::Edits),
        );
    })
    .response
    .on_disabled_hover_text(HISTORICAL_HELP);
    let validation = controls.config.validate();
    if let Err(error) = &validation {
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    } else {
        let (x, z) = controls.config.cells();
        ui.label(format!(
            "{x} x {z} cellules | {} sommets",
            (x + 1) * (z + 1)
        ));
    }
    if ui
        .add_enabled(validation.is_ok(), egui::Button::new("Regenerer"))
        .clicked()
    {
        draft.confirmation = Some(Confirmation::Generate(controls.config.clone()));
    }

    });
}

fn preset_name(preset: TerrainPreset) -> &'static str {
    match preset {
        TerrainPreset::Flat => "Plat",
        TerrainPreset::Valley => "Vallee",
        TerrainPreset::Mountain => "Montagne",
        TerrainPreset::Trial => "Trial",
        TerrainPreset::Legacy => "Historique",
    }
}

fn float_control(ui: &mut egui::Ui, label: &str, value: &mut f32, speed: f64) {
    ui.push_id(label, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(value).speed(speed).max_decimals(5));
        });
    });
}

fn slider_control(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    logarithmic: bool,
) {
    ui.push_id(label, |ui| {
        ui.label(label);
        ui.spacing_mut().slider_width = (ui.available_width() - 110.0).clamp(70.0, 320.0);
        ui.add(
            egui::Slider::new(value, range)
                .clamping(egui::SliderClamping::Edits)
                .logarithmic(logarithmic)
                .show_value(true)
                .max_decimals(5)
                .custom_formatter(|value, _| (value as f32).to_string()),
        );
    });
}

fn offset_seed(seed: u64, delta: f64) -> u64 {
    if !delta.is_finite() {
        return seed;
    }
    i128::from(seed)
        .saturating_add(delta.round() as i128)
        .clamp(0, i128::from(u64::MAX)) as u64
}

fn seed_editor(ui: &mut egui::Ui, seed: &mut u64) -> egui::Response {
    let exact = Cell::new(*seed);
    let parsed = Cell::new(None);
    // DragValue transports f64. Transport only the low word; parse text as u64
    // and apply drag deltas with integer arithmetic to retain every seed bit.
    let mut response = ui.add(
        egui::DragValue::from_get_set(|candidate| {
            if let Some(candidate) = candidate {
                exact.set(parsed.take().unwrap_or_else(|| {
                    offset_seed(exact.get(), candidate - f64::from(exact.get() as u32))
                }));
            }
            f64::from(exact.get() as u32)
        })
        .speed(1.0)
        .max_decimals(0)
        .custom_formatter(|_, _| exact.get().to_string())
        .custom_parser(|text| {
            let value = text.trim().parse::<u64>().ok()?;
            parsed.set(Some(value));
            Some(f64::from(value as u32))
        }),
    );
    if exact.get() != *seed {
        response.mark_changed();
        if exact.get() >> 32 != *seed >> 32 {
            // Reset egui's accumulated drag offset after crossing a word boundary.
            ui.data_mut(|data| data.remove::<f64>(response.id));
        }
        *seed = exact.get();
    }
    response
}

fn sculpture(ui: &mut egui::Ui, controls: &mut Controls) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut controls.tool, EditorTool::Sculpt, "Sculpter");
        for (kind, label) in [
            (BrushKind::Raise, "Elever"),
            (BrushKind::Lower, "Abaisser"),
            (BrushKind::Smooth, "Lisser"),
            (BrushKind::Flatten, "Aplanir"),
        ] {
            if ui
                .selectable_label(controls.brush.kind == kind, label)
                .clicked()
            {
                controls.brush.kind = kind;
                controls.tool = EditorTool::Sculpt;
            }
        }
    });
    slider_control(
        ui,
        "Rayon [m]",
        &mut controls.brush.radius,
        0.25..=32.0,
        true,
    );
    slider_control(
        ui,
        if matches!(controls.brush.kind, BrushKind::Smooth | BrushKind::Flatten) {
            "Vitesse [1/s]"
        } else {
            "Force [m/s]"
        },
        &mut controls.brush.strength,
        0.01..=20.0,
        true,
    );
    slider_control(
        ui,
        "Exposant de retombee",
        &mut controls.brush.falloff,
        0.1..=4.0,
        false,
    );
    if controls.brush.kind == BrushKind::Flatten {
        float_control(
            ui,
            "Altitude cible [m]",
            &mut controls.brush.flatten_height,
            0.1,
        );
        if ui
            .add_enabled(
                controls.cursor.is_some(),
                egui::Button::new("Altitude au curseur"),
            )
            .clicked()
        {
            if let Some(cursor) = controls.cursor {
                controls.brush.flatten_height = cursor.y;
            }
        }
    }
    if let Err(error) = controls.brush.validate() {
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
    cursor_label(ui, controls.cursor);
}

fn cursor_label(ui: &mut egui::Ui, cursor: Option<Vec3>) {
    if let Some(cursor) = cursor {
        ui.label(format!(
            "Curseur [m] : X {:.2} | Y {:.2} | Z {:.2}",
            cursor.x, cursor.y, cursor.z
        ));
    }
}

fn valid_size(size: [f32; 3]) -> bool {
    size.iter()
        .all(|value| value.is_finite() && (0.1..=32.0).contains(value))
}

fn vector_control(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], unit: &str) {
    ui.push_id(label, |ui| {
        ui.label(label);
        ui.horizontal_wrapped(|ui| {
            for (axis, value) in ["X", "Y", "Z"].into_iter().zip(values.iter_mut()) {
                ui.add(
                    egui::DragValue::new(value)
                        .prefix(format!("{axis} "))
                        .suffix(unit)
                        .speed(0.05)
                        .max_decimals(3),
                );
            }
        });
    });
}

fn objects(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    obstacles: &[Obstacle],
    draft: &mut UiDraft,
    action: &mut Option<WorkshopAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut controls.tool, EditorTool::Place, "Placer");
        ui.selectable_value(&mut controls.tool, EditorTool::Select, "Selectionner");
    });
    if let Some(kind) = super::terrain_catalogue::catalogue(
        ui, controls.object_kind, &mut draft.asset_query, &mut draft.asset_category,
    ) {
        controls.choose_kind(kind);
    }
    ui.horizontal_wrapped(|ui| {
        ui.label("Placement");
        ui.selectable_value(&mut controls.placement_mode, PlacementMode::Single, "Unique");
        ui.selectable_value(&mut controls.placement_mode, PlacementMode::Multiple, "Multiple");
    });
    ui.small("Choisissez un modele puis cliquez sur le terrain. Q/E tourne ; Echap annule.");
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut controls.placement_follow_ground, "Suivre le sol");
        if !controls.placement_follow_ground {
            ui.label("Libre : le modele peut traverser le sol");
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Decalage vertical [m]");
        ui.add(egui::DragValue::new(&mut controls.placement_offset_y)
            .speed(0.1).range(-32.0..=32.0));
    });
    ui.separator();
    ui.heading("Manipulation 3D de la selection");
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut controls.gizmo_mode, GizmoMode::Move, "G · Deplacer");
        ui.selectable_value(&mut controls.gizmo_mode, GizmoMode::Rotate, "R · Tourner");
        ui.selectable_value(&mut controls.gizmo_mode, GizmoMode::Scale, "S · Taille");
    });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut controls.snap_enabled, "Aimantation");
        if controls.snap_enabled {
            ui.checkbox(&mut controls.snap_objects, "Autres objets");
        }
        ui.checkbox(&mut controls.uniform_scale, "Taille uniforme");
    });
    if controls.snap_enabled {
        ui.horizontal_wrapped(|ui| {
            ui.label("Pas grille [m]");
            ui.add(egui::DragValue::new(&mut controls.snap_grid)
                .speed(0.05).range(0.05..=5.0));
            ui.label("Angle [°]");
            ui.add(egui::DragValue::new(&mut controls.snap_angle)
                .speed(1.0).range(1.0..=90.0));
        });
    }
    ui.small("Sur une selection : G/R/S, glisser une poignee coloree, Shift = mouvement fin, Alt = aimantation temporairement desactivee, Echap = annuler.");
    ui.separator();
    vector_control(
        ui,
        "Dimensions de placement [m]",
        &mut controls.object_size,
        " m",
    );
    if !valid_size(controls.object_size) {
        ui.colored_label(ui.visuals().error_fg_color, "Dimensions : 0.1 a 32 m");
    }
    let yaw = controls
        .placement_yaws
        .iter()
        .find(|(kind, _)| *kind == controls.object_kind)
        .map_or(0.0, |(_, yaw)| *yaw);
    ui.label(format!("Lacet de placement : {yaw:.0} deg"));
    ui.separator();
    ui.strong(format!("Objets ({})", obstacles.len()));
    egui::ScrollArea::vertical()
        .id_salt("terrain_objects")
        .max_height(150.0)
        .show(ui, |ui| {
            for object in obstacles {
                if ui
                    .selectable_label(
                        controls.selected == Some(object.id),
                        format!("#{} {}", object.id, object.kind.name()),
                    )
                    .clicked()
                {
                    controls.selected = Some(object.id);
                    controls.tool = EditorTool::Select;
                    draft.object = Some(object.clone());
                    draft.object_baseline = Some(object.clone());
                }
            }
        });
    if let (Some(object), Some(baseline)) = (&mut draft.object, &draft.object_baseline) {
        ui.separator();
        if draft.object_baseline.as_ref() != draft.object.as_ref() && 
            controls.selected == Some(object.id) {
            // Numerical edits remain explicitly validated. During a drag the
            // displayed draft tracks the transient candidate.
        }
        object_transform(ui, object, baseline, action);
    }
    cursor_label(ui, controls.cursor);
}

fn object_transform(
    ui: &mut egui::Ui,
    object: &mut Obstacle,
    baseline: &Obstacle,
    action: &mut Option<WorkshopAction>,
) {
    ui.strong(format!("#{} {}", object.id, object.kind.name()));
    vector_control(ui, "Position [m]", &mut object.position, " m");
    vector_control(ui, "Rotation [deg]", &mut object.rotation_degrees, " deg");
    vector_control(ui, "Dimensions [m]", &mut object.size, " m");
    let validation = object.validate();
    if let Err(error) = &validation {
        ui.colored_label(ui.visuals().error_fg_color, error.to_string());
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                validation.is_ok() && *object != *baseline,
                egui::Button::new("Appliquer la transformation"),
            )
            .clicked()
        {
            enqueue(action, WorkshopAction::TransformObject(object.clone()));
        }
        if ui.button("Retablir les valeurs").clicked() {
            *object = baseline.clone();
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Aligner au sol").clicked() {
            enqueue(action, WorkshopAction::AlignObject(object.id));
        }
        if ui.button("Dupliquer").clicked() {
            enqueue(action, WorkshopAction::DuplicateObject(object.id));
        }
        if ui.button("Supprimer").clicked() {
            enqueue(action, WorkshopAction::DeleteObject(object.id));
        }
    });
}

fn scene_name_id() -> egui::Id {
    egui::Id::new("terrain_workshop_scene_name")
}

fn same_scene_file(left: &str, right: &str) -> bool {
    fn stem(value: &str) -> &str {
        let value = value.trim();
        match value.rsplit_once('.') {
            Some((stem, extension)) if extension.eq_ignore_ascii_case("json") => stem,
            _ => value,
        }
    }
    stem(left).to_lowercase() == stem(right).to_lowercase()
}

fn scene_files(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    files: &[String],
    file: Option<&str>,
    draft: &mut UiDraft,
    action: &mut Option<WorkshopAction>,
) {
    ui.strong("References");
    ui.horizontal_wrapped(|ui| {
        for reference in ReferenceScene::ALL {
            if ui.button(reference.name()).clicked() {
                draft.confirmation = Some(Confirmation::Reference(reference));
            }
        }
        if ui.button("Nouvelle scene").clicked() {
            draft.confirmation = Some(Confirmation::New);
        }
    });
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label("Fichier");
        egui::ComboBox::from_id_salt("terrain_workshop_files")
            .selected_text(if controls.file_selection.is_empty() {
                "Aucun"
            } else {
                &controls.file_selection
            })
            .width(170.0_f32.min(ui.available_width().max(40.0)))
            .truncate()
            .show_ui(ui, |ui| {
                for name in files {
                    ui.selectable_value(&mut controls.file_selection, name.clone(), name);
                }
            });
        if ui
            .button("\u{21bb}")
            .on_hover_text("Actualiser les fichiers")
            .clicked()
        {
            enqueue(action, WorkshopAction::RefreshFiles);
        }
        if ui
            .add_enabled(
                files.contains(&controls.file_selection),
                egui::Button::new("Charger"),
            )
            .clicked()
        {
            draft.confirmation = Some(Confirmation::Load(controls.file_selection.clone()));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Nom");
        ui.add(
            egui::TextEdit::singleline(&mut controls.scene_name)
                .id(scene_name_id())
                .desired_width(180.0)
                .char_limit(64),
        );
    });
    ui.horizontal_wrapped(|ui| {
        let name = controls.scene_name.trim();
        if ui
            .add_enabled(!name.is_empty(), egui::Button::new("Enregistrer sous"))
            .clicked()
        {
            if files.iter().any(|file| same_scene_file(name, file))
                || file.is_some_and(|file| same_scene_file(name, file))
            {
                draft.confirmation = Some(Confirmation::SaveAs(name.to_owned()));
            } else {
                enqueue(action, WorkshopAction::SaveAs(name.to_owned()));
            }
        }
        if ui
            .add_enabled(file.is_some(), egui::Button::new("Enregistrer"))
            .clicked()
        {
            if let Some(file) = file {
                draft.confirmation = Some(Confirmation::Save(file.to_owned()));
            }
        }
    });
    if let Some(file) = file {
        ui.label(format!("Scene courante : {file}"));
    }
}

fn confirmation_controls(
    ui: &mut egui::Ui,
    draft: &mut UiDraft,
    action: &mut Option<WorkshopAction>,
) {
    let Some(confirmation) = &draft.confirmation else {
        return;
    };
    ui.label(confirmation.description());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Confirmer").clicked() {
            if let Some(confirmation) = draft.confirmation.take() {
                enqueue(action, confirmation.into_action());
            }
        }
        if ui.button("Annuler").clicked() {
            draft.confirmation = None;
        }
    });
}

fn spawn_controls(
    ui: &mut egui::Ui,
    controls: &Controls,
    draft: &mut UiDraft,
    ground_height: &dyn Fn(f32, f32) -> Option<f32>,
    action: &mut Option<WorkshopAction>,
) {
    let Some(spawn) = &mut draft.spawn else {
        return;
    };
    ui.strong("Depart du vehicule");
    float_control(ui, "Depart X [m]", &mut spawn.position[0], 0.1);
    float_control(ui, "Depart Z [m]", &mut spawn.position[2], 0.1);
    float_control(ui, "Cap [deg]", &mut spawn.yaw_degrees, 1.0);
    if let Some(height) = ground_height(spawn.position[0], spawn.position[2]) {
        spawn.position[1] = height;
        ui.label(format!("Sol Y [m] : {height:.3}"))
            .on_hover_text("Altitude du sol ; la garde au sol est ajoutee par le runtime.");
    } else {
        ui.colored_label(ui.visuals().error_fg_color, "Depart hors du terrain");
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(controls.cursor.is_some(), egui::Button::new("Au curseur"))
            .clicked()
        {
            if let Some(cursor) = controls.cursor {
                spawn.position = cursor.to_array();
            }
        }
        if ui
            .add_enabled(
                spawn.valid()
                    && ground_height(spawn.position[0], spawn.position[2]).is_some()
                    && draft.spawn_baseline.as_ref() != Some(spawn),
                egui::Button::new("Appliquer le depart"),
            )
            .clicked()
        {
            enqueue(action, WorkshopAction::SetSpawn(spawn.clone().into_spawn()));
        }
        if ui.button("Replacer le vehicule").clicked() {
            enqueue(action, WorkshopAction::MoveVehicleToSpawn);
        }
    });
}

fn diagnostics(ui: &mut egui::Ui, view: &WorkshopView<'_>) {
    let synchronized = view.revision == view.applied_revision;
    ui.colored_label(
        if synchronized {
            egui::Color32::from_rgb(105, 197, 143)
        } else {
            ui.visuals().warn_fg_color
        },
        if synchronized {
            "Collision synchronisee"
        } else {
            "Collision en attente"
        },
    );
    for (label, value) in [
        ("Scene", view.scene_name.to_owned()),
        (
            "Revision terrain / collision",
            format!("{} / {}", view.revision, view.applied_revision),
        ),
        (
            "Resolution configuree",
            format!("{} m/cellule", view.config.resolution),
        ),
        (
            "Pas effectif",
            format!(
                "X {:.4} m | Z {:.4} m",
                view.config.width / view.cells.0 as f32,
                view.config.depth / view.cells.1 as f32,
            ),
        ),
        ("Cellules", format!("{} x {}", view.cells.0, view.cells.1)),
        ("Sommets terrain", view.vertices.to_string()),
        ("Triangles terrain", view.triangles.to_string()),
        ("Objets places", view.obstacles.len().to_string()),
        ("Colliders Rapier (scene)", view.collider_count.to_string()),
        (
            "Terrain canonique (est.)",
            format!("{:.2} MiB", view.terrain_bytes as f64 / 1_048_576.0),
        ),
        ("Generation", format!("{:.2} ms", view.generation_ms)),
        (
            "Historique annuler / retablir",
            format!("{} / {}", view.undo, view.redo),
        ),
        (
            "Memoire historique",
            format!("{:.2} MiB", view.history_bytes as f64 / 1_048_576.0),
        ),
    ] {
        ui.horizontal_wrapped(|ui| {
            ui.label(label);
            ui.monospace(value);
        });
    }
    if view.config.preset == TerrainPreset::Legacy {
        ui.small("Decor historique exclu du compte des objets places.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn quick_relief_preserves_noise_and_reproducible_seed() {
        for preset in [TerrainPreset::Flat, TerrainPreset::Valley, TerrainPreset::Mountain, TerrainPreset::Trial] {
            let base = TerrainConfig::preset(preset);
            let soft = quick_config(preset, 0, 80.0, 72.0, u64::MAX - 8);
            let normal = quick_config(preset, 50, 80.0, 72.0, u64::MAX - 8);
            let strong = quick_config(preset, 100, 80.0, 72.0, u64::MAX - 8);
            assert!(soft.validate().is_ok() && normal.validate().is_ok() && strong.validate().is_ok());
            assert_eq!(soft.amplitude, 0.0);
            assert_eq!(normal.amplitude, base.amplitude);
            assert!(strong.amplitude >= normal.amplitude);
            assert_eq!(normal.seed, u64::MAX - 8);
            assert_eq!(normal.width, 80.0);
            assert_eq!(normal.depth, 72.0);
            assert_eq!(normal.frequency, base.frequency);
            assert_eq!(normal.octaves, base.octaves);
        }
    }

    #[test]
    fn variant_is_stable_and_different_without_mutating_the_active_scene() {
        let seed = (1u64 << 60) + 43;
        let alternate = next_variation(seed);
        assert_eq!(alternate, next_variation(seed));
        assert_ne!(seed, alternate);
        let config = quick_config(TerrainPreset::Valley, 50, 32.0, 32.0, seed);
        let scene = crate::terrain::scene::TerrainScene::generate("Test variante", config.clone()).unwrap();
        let exact = crate::terrain::scene::TerrainScene::generate("Test variante", config.clone()).unwrap();
        assert_eq!(scene.terrain.heights, exact.terrain.heights);
        let mut alternate_config = config;
        alternate_config.seed = alternate;
        let other = crate::terrain::scene::TerrainScene::generate("Test variante", alternate_config).unwrap();
        assert_ne!(scene.terrain.heights, other.terrain.heights);
        assert_eq!(scene.obstacles, Vec::new());
    }
    #[test]
    fn nature_catalogue_choices_use_natural_sizes_and_preserve_repeated_custom_sizes() {
        let mut controls = Controls::read(&TerrainWorkshopState::default());
        for asset in crate::terrain::nature::NatureAsset::ALL {
            let kind = ObstacleKind::Nature(asset);
            controls.choose_kind(kind);
            assert_eq!(controls.object_size, asset.default_size().to_array());
            assert_eq!(controls.tool, EditorTool::Place);
            controls.object_size = [1.5, 2.0, 2.5];
            controls.choose_kind(kind);
            assert_eq!(controls.object_size, [1.5, 2.0, 2.5]);
        }
    }

    fn input(events: Vec<egui::Event>, size: egui::Vec2) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }

    fn click(position: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing rendered control: {label}"))
    }

    fn discard_output(mut output: egui::FullOutput) {
        output.textures_delta.clear();
    }

    fn click_control(ctx: &egui::Context, label: &str, mut draw: impl FnMut(&mut egui::Ui)) {
        let size = egui::vec2(1000.0, 1000.0);
        let mut position = egui::Pos2::ZERO;
        for _ in 0..2 {
            let output = ctx.run_ui(input(vec![], size), &mut draw);
            position = text_position(&output, label);
            discard_output(output);
        }
        discard_output(ctx.run_ui(input(click(position), size), draw));
    }

    #[test]
    fn native_seed_editor_preserves_all_bits_and_accepts_exact_typed_u64() {
        let ctx = egui::Context::default();
        let size = egui::vec2(700.0, 300.0);
        let mut seed = (1_u64 << 60) + 37;
        let mut seed_id = None;
        let mut position = egui::Pos2::ZERO;
        for _ in 0..2 {
            discard_output(ctx.run_ui(input(vec![], size), |ui| {
                let response = seed_editor(ui, &mut seed);
                seed_id = Some(response.id);
                position = response.rect.center();
            }));
        }
        assert_eq!(seed, (1_u64 << 60) + 37);
        discard_output(ctx.run_ui(input(click(position), size), |ui| {
            seed_editor(ui, &mut seed);
        }));
        // Give DragValue its focused text-edit pass before replacing the selection.
        discard_output(ctx.run_ui(input(vec![], size), |ui| {
            seed_editor(ui, &mut seed);
        }));
        let select_all = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        let raw = input(
            vec![select_all, egui::Event::Text(u64::MAX.to_string())],
            size,
        );
        discard_output(ctx.run_ui(raw, |ui| {
            seed_editor(ui, &mut seed);
        }));
        assert_eq!(seed, u64::MAX);
        assert!(ctx.memory(|memory| memory.has_focus(seed_id.unwrap())));
    }

    #[test]
    fn seed_deltas_cross_word_boundaries_without_float_rounding() {
        assert_eq!(offset_seed((1_u64 << 60) + 37, 1.0), (1_u64 << 60) + 38);
        assert_eq!(offset_seed((1_u64 << 32) - 1, 1.0), 1_u64 << 32);
        assert_eq!(offset_seed(1_u64 << 32, -1.0), (1_u64 << 32) - 1);
        assert_eq!(offset_seed(u64::MAX, 100.0), u64::MAX);
        assert_eq!(offset_seed(0, -100.0), 0);
    }

    #[test]
    fn placement_invalid_dimension_draft_reaches_preview_without_committing_objects() {
        let ctx = egui::Context::default();
        let mut state = TerrainWorkshopState::default();
        let mut local = WorkshopUi::default();
        discard_output(ctx.run_ui(input(vec![], egui::vec2(700.0, 300.0)), |ui| {
            let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
            controls.tool = EditorTool::Place;
            controls.object_size = [0.0, 1.0, 2.0];
            local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, None);
        }));
        assert_eq!(
            state.object_size,
            [0.0, 1.0, 2.0],
            "invalid drafts must not leave the old green preview active"
        );
        assert!(state.actions.is_empty());
    }

    #[test]
    fn mode_click_enqueues_one_action_even_if_the_frame_is_replayed() {
        let ctx = egui::Context::default();
        let mut state = TerrainWorkshopState::default();
        state.mode = WorkshopMode::Drive;
        let mut local = WorkshopUi::default();
        let size = egui::vec2(700.0, 300.0);
        let mut position = egui::Pos2::ZERO;
        for _ in 0..2 {
            let output = ctx.run_ui(input(vec![], size), |ui| {
                let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
                let mut action = None;
                let _ = toolbar(ui, &controls, 0, 0, &mut action);
                local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, action);
            });
            position = text_position(&output, "Edition");
            discard_output(output);
        }
        let mut passes = 0;
        discard_output(ctx.run_ui(input(click(position), size), |ui| {
            passes += 1;
            let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
            let mut action = None;
            let _ = toolbar(ui, &controls, 0, 0, &mut action);
            local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, action);
            if ui.ctx().current_pass_index() == 0 {
                ui.ctx().request_discard("Test terrain action replay");
            }
        }));
        assert!(passes >= 2);
        assert_eq!(state.actions.len(), 1);
        assert!(matches!(
            state.actions[0],
            WorkshopAction::SetMode(WorkshopMode::Edit)
        ));
        assert!(state.mode == WorkshopMode::Drive);
    }

    #[test]
    fn painted_history_buttons_dispatch_once_and_keep_disabled_square_hit_areas() {
        for enabled in [false, true] {
            for index in 0..2 {
                let ctx = egui::Context::default();
                let mut state = TerrainWorkshopState::default();
                let mut local = WorkshopUi::default();
                let size = egui::vec2(700.0, 300.0);
                let count = usize::from(enabled);
                let mut position = egui::Pos2::ZERO;
                for _ in 0..2 {
                    let output = ctx.run_ui(input(vec![], size), |ui| {
                        let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
                        let mut action = None;
                        let buttons: [egui::Response; 2] =
                            toolbar(ui, &controls, count, count, &mut action);
                        for button in &buttons {
                            assert_eq!(button.rect.size(), egui::vec2(22.0, 22.0));
                            assert_eq!(button.enabled(), enabled);
                        }
                        position = buttons[index].rect.center();
                        local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, action);
                    });
                    let arrow_paths = output
                        .shapes
                        .iter()
                        .filter(|shape| {
                            matches!(&shape.shape, egui::epaint::Shape::Path(path)
                            if matches!(path.points.len(), 3 | 5) && path.stroke.width > 0.0)
                        })
                        .count();
                    assert!(arrow_paths >= 4, "Both arrows must paint a bend and a head");
                    discard_output(output);
                }
                let mut passes = 0;
                discard_output(ctx.run_ui(input(click(position), size), |ui| {
                    passes += 1;
                    let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
                    let mut action = None;
                    let buttons: [egui::Response; 2] =
                        toolbar(ui, &controls, count, count, &mut action);
                    assert_eq!(buttons[index].rect.center(), position);
                    assert_eq!(buttons[index].rect.size(), egui::vec2(22.0, 22.0));
                    local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, action);
                    if ui.ctx().current_pass_index() == 0 {
                        ui.ctx()
                            .request_discard("Test painted terrain history buttons");
                    }
                }));
                assert!(passes >= 2);
                assert_eq!(state.actions.len(), usize::from(enabled));
                if enabled {
                    match index {
                        0 => assert!(matches!(state.actions[0], WorkshopAction::Undo)),
                        _ => assert!(matches!(state.actions[0], WorkshopAction::Redo)),
                    }
                }
            }
        }
    }

    #[test]
    fn scene_text_input_commits_once_in_a_replayed_frame() {
        let ctx = egui::Context::default();
        let mut state = TerrainWorkshopState::default();
        state.scene_name.clear();
        let mut local = WorkshopUi::default();
        let size = egui::vec2(700.0, 300.0);
        discard_output(ctx.run_ui(input(vec![], size), |ui| {
            let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
            ui.add(egui::TextEdit::singleline(&mut controls.scene_name).id(scene_name_id()));
            local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, None);
        }));
        ctx.memory_mut(|memory| memory.request_focus(scene_name_id()));
        let mut passes = 0;
        discard_output(ctx.run_ui(
            input(vec![egui::Event::Text("Terrain 7".into())], size),
            |ui| {
                passes += 1;
                let (mut controls, mut draft) = local.begin_frame(ui.ctx(), &state);
                ui.add(egui::TextEdit::singleline(&mut controls.scene_name).id(scene_name_id()));
                local.commit(ui.ctx(), &mut state, &mut controls, &mut draft, None);
                if ui.ctx().current_pass_index() == 0 {
                    ui.ctx().request_discard("Test terrain scene-name replay");
                }
            },
        ));
        assert!(passes >= 2);
        assert_eq!(state.scene_name, "Terrain 7");
        assert!(state.actions.is_empty());
    }

    #[test]
    fn generation_rejects_invalid_config_and_confirms_valid_replacement() {
        let mut controls = Controls::read(&TerrainWorkshopState::default());
        let mut draft = UiDraft::default();
        controls.config.resolution = 0.0;
        click_control(&egui::Context::default(), "Generer", |ui| {
            generation(ui, &mut controls, &mut draft);
        });
        assert!(draft.confirmation.is_none());
        controls.config = TerrainConfig::default();
        click_control(&egui::Context::default(), "Generer", |ui| {
            generation(ui, &mut controls, &mut draft);
        });
        assert!(matches!(draft.confirmation, Some(Confirmation::Generate(_))));
        let mut action = None;
        click_control(&egui::Context::default(), "Confirmer", |ui| {
            confirmation_controls(ui, &mut draft, &mut action);
        });
        assert!(matches!(action, Some(WorkshopAction::Generate(_))));
    }

    #[test]
    fn every_reference_button_selects_its_actual_scene_before_confirmation() {
        for reference in ReferenceScene::ALL {
            let ctx = egui::Context::default();
            let mut controls = Controls::read(&TerrainWorkshopState::default());
            let mut draft = UiDraft::default();
            let mut action = None;
            click_control(&ctx, reference.name(), |ui| {
                scene_files(ui, &mut controls, &[], None, &mut draft, &mut action);
            });
            assert!(action.is_none());
            assert!(
                matches!(draft.confirmation, Some(Confirmation::Reference(selected)) if selected == reference)
            );
            click_control(&ctx, "Confirmer", |ui| {
                confirmation_controls(ui, &mut draft, &mut action);
            });
            assert!(
                matches!(action, Some(WorkshopAction::LoadReference(selected)) if selected == reference)
            );
        }
    }

    #[test]
    fn refresh_and_save_as_new_name_queue_real_actions() {
        for label in ["\u{21bb}", "Enregistrer sous"] {
            let mut controls = Controls::read(&TerrainWorkshopState::default());
            controls.scene_name = "New course".to_owned();
            let mut draft = UiDraft::default();
            let mut action = None;
            click_control(&egui::Context::default(), label, |ui| {
                scene_files(ui, &mut controls, &[], None, &mut draft, &mut action);
            });
            match label {
                "Enregistrer sous" => assert!(
                    matches!(action, Some(WorkshopAction::SaveAs(name)) if name == "New course")
                ),
                _ => assert!(matches!(action, Some(WorkshopAction::RefreshFiles))),
            }
            assert!(draft.confirmation.is_none());
        }
    }

    #[test]
    fn scene_overwrite_requires_explicit_confirmation_and_keeps_target() {
        let ctx = egui::Context::default();
        let mut state = TerrainWorkshopState::default();
        state.scene_name = "Existing".to_owned();
        state.files = vec!["Existing.json".to_owned()];
        let mut controls = Controls::read(&state);
        let mut draft = UiDraft::default();
        let mut action = None;
        let size = egui::vec2(900.0, 900.0);
        let mut position = egui::Pos2::ZERO;
        for _ in 0..2 {
            let output = ctx.run_ui(input(vec![], size), |ui| {
                scene_files(
                    ui,
                    &mut controls,
                    &state.files,
                    None,
                    &mut draft,
                    &mut action,
                );
            });
            position = text_position(&output, "Enregistrer sous");
            discard_output(output);
        }
        discard_output(ctx.run_ui(input(click(position), size), |ui| {
            scene_files(
                ui,
                &mut controls,
                &state.files,
                None,
                &mut draft,
                &mut action,
            );
        }));
        assert!(action.is_none());
        assert!(
            matches!(&draft.confirmation, Some(Confirmation::SaveAs(name)) if name == "Existing")
        );
        controls.scene_name = "Other".to_owned();
        let output = ctx.run_ui(input(vec![], size), |ui| {
            confirmation_controls(ui, &mut draft, &mut action);
        });
        position = text_position(&output, "Confirmer");
        discard_output(output);
        discard_output(ctx.run_ui(input(click(position), size), |ui| {
            confirmation_controls(ui, &mut draft, &mut action);
        }));
        assert!(matches!(action, Some(WorkshopAction::SaveAs(name)) if name == "Existing"));
        assert!(draft.confirmation.is_none());
    }

    #[test]
    fn object_changes_stay_in_draft_until_explicit_apply() {
        let ctx = egui::Context::default();
        let before = Obstacle {
            id: 7,
            kind: ObstacleKind::Cube,
            position: [0.0, 1.0, 0.0],
            rotation_degrees: [0.0; 3],
            size: [2.0; 3],
        };
        let mut edited = before.clone();
        edited.position[0] = 8.25;
        let mut action = None;
        let size = egui::vec2(900.0, 900.0);
        let mut position = egui::Pos2::ZERO;
        for _ in 0..2 {
            let output = ctx.run_ui(input(vec![], size), |ui| {
                object_transform(ui, &mut edited, &before, &mut action);
            });
            position = text_position(&output, "Appliquer la transformation");
            discard_output(output);
            assert!(action.is_none());
        }
        discard_output(ctx.run_ui(input(click(position), size), |ui| {
            object_transform(ui, &mut edited, &before, &mut action);
        }));
        assert!(
            matches!(action, Some(WorkshopAction::TransformObject(object)) if object.position[0] == 8.25 && object.id == 7)
        );
        assert_eq!(before.position[0], 0.0);
    }

    #[test]
    fn selected_object_commands_keep_the_stable_object_id() {
        for label in ["Aligner au sol", "Dupliquer", "Supprimer"] {
            let baseline = Obstacle {
                id: 91,
                kind: ObstacleKind::Ramp,
                position: [0.0, 1.0, 0.0],
                rotation_degrees: [0.0; 3],
                size: [2.0; 3],
            };
            let mut object = baseline.clone();
            let mut action = None;
            click_control(&egui::Context::default(), label, |ui| {
                object_transform(ui, &mut object, &baseline, &mut action);
            });
            match label {
                "Aligner au sol" => {
                    assert!(matches!(action, Some(WorkshopAction::AlignObject(91))))
                }
                "Dupliquer" => assert!(matches!(action, Some(WorkshopAction::DuplicateObject(91)))),
                _ => assert!(matches!(action, Some(WorkshopAction::DeleteObject(91)))),
            }
            assert_eq!(object, baseline);
        }
    }

    #[test]
    fn applying_spawn_samples_ground_without_adding_vehicle_clearance() {
        use crate::terrain::data::TerrainData;

        let config = TerrainConfig {
            width: 8.0,
            depth: 8.0,
            resolution: 4.0,
            ..Default::default()
        };
        let terrain = TerrainData::from_heights(config, vec![3.25; 9]).unwrap();
        let controls = Controls::read(&TerrainWorkshopState::default());
        let mut draft = UiDraft::default();
        draft.spawn_baseline = Some(SpawnDraft {
            position: [0.0; 3],
            yaw_degrees: 0.0,
        });
        draft.spawn = Some(SpawnDraft {
            position: [1.0, 100.0, 2.0],
            yaw_degrees: 45.0,
        });
        let mut action = None;
        click_control(&egui::Context::default(), "Appliquer le depart", |ui| {
            spawn_controls(
                ui,
                &controls,
                &mut draft,
                &|x, z| terrain.height(x, z),
                &mut action,
            );
        });
        assert!(
            matches!(action, Some(WorkshopAction::SetSpawn(spawn)) if spawn.position == [1.0, 3.25, 2.0] && spawn.yaw_degrees == 45.0)
        );
    }

    #[test]
    fn all_tabs_render_at_desktop_and_narrow_sizes_without_mutating_live_values() {
        for size in [egui::vec2(1440.0, 900.0), egui::vec2(360.0, 740.0)] {
            for tab in WorkshopTab::ALL {
                let ctx = egui::Context::default();
                let mut state = TerrainWorkshopState::default();
                state.open = true;
                state.draft.seed = u64::MAX - 9;
                let expected = state.draft.clone();
                let spawn = VehicleSpawn {
                    position: [0.0; 3],
                    yaw_degrees: 0.0,
                };
                let view = WorkshopView {
                    scene_name: "Terrain headless",
                    config: &expected,
                    obstacles: &[],
                    spawn: &spawn,
                    file: None,
                    revision: 4,
                    applied_revision: 4,
                    generation_ms: 2.5,
                    undo: 0,
                    redo: 0,
                    history_bytes: 0,
                    vertices: 81,
                    triangles: 128,
                    collider_count: 913,
                    terrain_bytes: 324,
                    cells: (8, 8),
                    ground_height: &|_, _| Some(0.0),
                    preview_object: None,
                };
                let mut local = WorkshopUi::default();
                local.draft.tab = tab;
                let mut passes = 0;
                let mut rect = None;
                let output = ctx.run_ui(input(vec![], size), |ui| {
                    passes += 1;
                    rect = show_workshop(ui.ctx(), &mut state, &view, &mut local);
                    if ui.ctx().current_pass_index() == 0 {
                        ui.ctx().request_discard("Test responsive terrain tabs");
                    }
                });
                if tab == WorkshopTab::Diagnostic {
                    text_position(&output, "Terrain headless");
                    text_position(&output, "913");
                    text_position(&output, "0.5 m/cellule");
                    text_position(&output, "X 8.0000 m | Z 8.0000 m");
                }
                discard_output(output);
                assert!(passes >= 2);
                assert!(rect.is_some());
                assert_eq!(state.draft, expected);
                assert!(state.actions.is_empty());
            }
        }
    }
}
