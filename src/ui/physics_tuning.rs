use std::collections::HashSet;

pub use super::input_capture::UiInputCapture;
use super::input_capture::{UiInputPlugin, UiInputSet};
use bevy::input::keyboard::{Key, KeyboardFocusLost, KeyboardInput};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPreUpdateSet, EguiPrimaryContextPass, egui};

use crate::persistence::profiles::ProfileStore;
use crate::vehicle::config::{PhysicsModel, VehiclePhysicsConfig};
use crate::vehicle::telemetry::VehicleTelemetry;
use crate::vehicle::wheel_settings::WheelContactSettings;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum TuningTab {
    #[default]
    Chassis,
    Suspensions,
    Tires,
    Drivetrain,
    Diagnostic,
}

#[derive(Clone, Default)]
struct UiDraft {
    tab: TuningTab,
    profile_name: String,
    profile_selection: String,
    reference_selection: String,
}

#[derive(Clone)]
struct FrameSnapshot {
    frame: u64,
    draft: UiDraft,
    config: VehiclePhysicsConfig,
    experimental_tires: Option<bool>,
    profile_name_state: egui::widgets::text_edit::TextEditState,
}

#[derive(Resource, Default)]
pub struct PhysicsTuningState {
    pub open: bool,
    draft: UiDraft,
    reference: Option<VehiclePhysicsConfig>,
    reference_name: String,
    edit_error: Option<String>,
    initialized: bool,
    frame_snapshot: Option<FrameSnapshot>,
    committed_frame: Option<u64>,
}

/// The parent installs `EguiPlugin::default()`, config, profiles and telemetry.
pub struct PhysicsTuningPlugin;

impl Plugin for PhysicsTuningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiInputCapture>()
            .init_resource::<PhysicsTuningState>()
            .add_plugins(UiInputPlugin)
            .add_systems(
                PreUpdate,
                toggle_tuning_window
                    .before(UiInputSet)
                    .after(InputSystems)
                    .after(EguiPreUpdateSet::InitContexts)
                    .before(EguiPreUpdateSet::ProcessInput),
            )
            .add_systems(EguiPrimaryContextPass, physics_tuning_ui);
    }
}

fn is_toggle_key(physical: KeyCode, logical: &Key) -> bool {
    physical == KeyCode::Backquote
        || matches!(logical, Key::Character(text) if text.as_str() == "\u{00b2}")
}

#[derive(Default)]
struct ToggleKeys {
    held: HashSet<KeyCode>,
}

impl ToggleKeys {
    fn update(
        &mut self,
        physical: KeyCode,
        logical: &Key,
        state: ButtonState,
        repeat: bool,
    ) -> bool {
        if state == ButtonState::Released {
            self.held.remove(&physical);
            return false;
        }
        is_toggle_key(physical, logical) && !repeat && self.held.insert(physical)
    }
}

fn toggle_tuning_window(
    mut messages: MessageReader<KeyboardInput>,
    mut focus_lost: MessageReader<KeyboardFocusLost>,
    mut keys: Local<ToggleKeys>,
    mut state: ResMut<PhysicsTuningState>,
) {
    if focus_lost.read().count() > 0 {
        keys.held.clear();
    }
    // Read raw messages: egui capture must never disable the global shortcut.
    for message in messages.read() {
        if keys.update(
            message.key_code,
            &message.logical_key,
            message.state,
            message.repeat,
        ) {
            state.open = !state.open;
        }
    }
}

#[derive(Clone)]
enum ProfileAction {
    Refresh,
    Load(String),
    Save(String),
    Compare(String),
    ApplyReference,
    CaptureReference,
}

fn physics_tuning_ui(
    mut contexts: EguiContexts,
    mut state: ResMut<PhysicsTuningState>,
    mut config: ResMut<VehiclePhysicsConfig>,
    mut profiles: ResMut<ProfileStore>,
    telemetry: Option<Res<VehicleTelemetry>>,
    mut wheel_settings: Option<ResMut<WheelContactSettings>>,
    physics: Option<Res<crate::vehicle::VehiclePhysics>>,
    equipment: Option<Res<crate::vehicle::equipment_runtime::EquipmentRuntime>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !state.open {
        return Ok(());
    }
    if !state.initialized {
        state.draft.profile_name = profiles.selected.clone();
        let selection = if profiles.names.contains(&profiles.selected) {
            profiles.selected.clone()
        } else {
            profiles.names.first().cloned().unwrap_or_default()
        };
        state.draft.profile_selection = selection.clone();
        state.draft.reference_selection = selection;
        state.initialized = true;
    }

    let frame = ctx.cumulative_frame_nr();
    if state
        .frame_snapshot
        .as_ref()
        .is_none_or(|snapshot| snapshot.frame != frame)
    {
        state.frame_snapshot = Some(FrameSnapshot {
            frame,
            draft: state.draft.clone(),
            config: config.clone(),
            experimental_tires: wheel_settings
                .as_ref()
                .map(|settings| settings.experimental),
            profile_name_state: egui::TextEdit::load_state(ctx, profile_name_id())
                .unwrap_or_default(),
        });
    }
    let snapshot = state.frame_snapshot.as_ref().unwrap().clone();
    // Replayed passes start from the same strings and values. Commit text,
    // configuration and filesystem actions only once per egui frame.
    let mut draft = snapshot.draft;
    let mut candidate = snapshot.config;
    let mut experimental_tires = snapshot.experimental_tires;
    egui::TextEdit::store_state(ctx, profile_name_id(), snapshot.profile_name_state);
    let mut open = state.open;
    let mut action = None;
    let baseline = VehiclePhysicsConfig::baseline();
    let screen = ctx.content_rect().size();

    egui::Window::new("Motor Trail | Reglages physiques [\u{00b2} / `]")
        .id(egui::Id::new("motor_trail_physics_tuning"))
        .open(&mut open)
        .movable(true)
        .resizable(true)
        .collapsible(false)
        .default_pos(egui::pos2(24.0, 76.0))
        .default_size(egui::vec2(780.0_f32.min(screen.x), 650.0_f32.min(screen.y)))
        .min_size(egui::vec2(420.0_f32.min(screen.x), 260.0_f32.min(screen.y)))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 5.0);
            ui.spacing_mut().slider_width = (ui.available_width() - 275.0).clamp(100.0, 320.0);
            ui.horizontal_wrapped(|ui| {
                preset_picker(ui, &mut candidate);
                if ui
                    .small_button("\u{21ba}")
                    .on_hover_text("Retablir tous les parametres du preset Baseline.")
                    .clicked()
                {
                    candidate = baseline.clone();
                }
                ui.label(format!(
                    "{} parametres modifies",
                    modified_count(&candidate, &baseline)
                ))
                .on_hover_text("Les valeurs marquees * different de Baseline.");
                ui.menu_button("?", |ui| {
                    ui.label("Raccourci : \u{00b2} / Backquote (touche a gauche de 1).");
                    ui.label("Les reglages valides sont appliques en temps reel.");
                    ui.label("La valeur numerique de chaque curseur est editable.");
                    ui.label("* : valeur differente de Baseline. Fleche : retour a Baseline.");
                    ui.label("A/B : courant moins reference, dans l'unite affichee.");
                    ui.label("Appliquer B ne modifie aucun profil enregistre.");
                })
                .response
                .on_hover_text("Aide et raccourci du panneau.");
            });
            profile_controls(ui, &mut draft, &profiles, &mut action);
            if !profiles.status.is_empty() {
                ui.small(&profiles.status);
            }
            if let Some(error) = &profiles.last_error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if let Some(error) = &state.edit_error {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Reglage refuse : {error}"),
                );
            }
            for error in [physics.as_ref().and_then(|value|value.error.as_ref()),
                equipment.as_ref().and_then(|value|value.error.as_ref())].into_iter().flatten() {
                ui.colored_label(ui.visuals().error_fg_color,error);
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for (tab, label) in [
                    (TuningTab::Chassis, "Chassis"),
                    (TuningTab::Suspensions, "Suspensions"),
                    (TuningTab::Tires, "Pneus"),
                    (TuningTab::Drivetrain, "Moteur"),
                    (TuningTab::Diagnostic, "Diagnostic"),
                ] {
                    ui.selectable_value(&mut draft.tab, tab, label);
                }
            });
            ui.separator();
            egui::ScrollArea::both()
                .id_salt(("physics_tuning_body", draft.tab as u8))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    match draft.tab {
                        TuningTab::Diagnostic => diagnostics(ui, telemetry.as_deref()),
                        tab => {
                            parameter_editor(ui, tab, &mut candidate, &baseline);
                            if tab == TuningTab::Tires {
                                if let Some(experimental) = &mut experimental_tires {
                                    ui.separator();
                                    ui.checkbox(experimental,"Contacts volumetriques (experimental)")
                                        .on_hover_text("Option de session non enregistree dans les profils. Le controleur natif reste la reference.");
                                }
                            }
                        },
                    }
                    ui.separator();
                    comparison_controls(ui, &mut draft, &candidate, &state, &profiles, &mut action);
                });
        });

    if state.committed_frame != Some(frame) {
        if let (Some(settings), Some(experimental)) = (&mut wheel_settings, experimental_tires) {
            settings.experimental = experimental;
        }
        state.committed_frame = Some(frame);
        state.draft = draft;
        state.open = open;
        let changed = candidate != *config;
        match commit_candidate(&mut config, candidate) {
            Ok(_) => {
                if changed {
                    state.edit_error = None;
                }
                if let Some(action) = action {
                    apply_profile_action(action, &mut state, &mut config, &mut profiles);
                }
            }
            Err(error) => state.edit_error = Some(error),
        }
    }
    Ok(())
}

fn commit_candidate(
    current: &mut VehiclePhysicsConfig,
    candidate: VehiclePhysicsConfig,
) -> std::result::Result<bool, String> {
    candidate.validate()?;
    if *current == candidate {
        return Ok(false);
    }
    *current = candidate;
    Ok(true)
}

fn preset_picker(ui: &mut egui::Ui, candidate: &mut VehiclePhysicsConfig) {
    let selected = VehiclePhysicsConfig::preset_names()
        .iter()
        .copied()
        .find(|name| VehiclePhysicsConfig::preset(name).is_some_and(|preset| preset == *candidate))
        .unwrap_or("Personnalise");
    egui::ComboBox::from_id_salt("physics_preset")
        .selected_text(selected)
        .width(150.0)
        .show_ui(ui, |ui| {
            for &name in VehiclePhysicsConfig::preset_names() {
                if ui.selectable_label(selected == name, name).clicked() {
                    if let Some(preset) = VehiclePhysicsConfig::preset(name) {
                        *candidate = preset;
                    }
                }
            }
        })
        .response
        .on_hover_text(
            "Preset physique : Baseline, Confort, Franchissement, Dynamique ou Origine.",
        );
}

fn profile_controls(
    ui: &mut egui::Ui,
    draft: &mut UiDraft,
    profiles: &ProfileStore,
    action: &mut Option<ProfileAction>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Profil");
        profile_picker(
            ui,
            "physics_profile",
            &mut draft.profile_selection,
            &profiles.names,
        );
        if ui
            .add_enabled(
                !draft.profile_selection.is_empty(),
                egui::Button::new("Charger"),
            )
            .on_hover_text("Remplacer les reglages courants par le profil selectionne.")
            .clicked()
        {
            *action = Some(ProfileAction::Load(draft.profile_selection.clone()));
        }
        if ui
            .small_button("\u{21bb}")
            .on_hover_text("Actualiser la liste des profils sur disque.")
            .clicked()
        {
            *action = Some(ProfileAction::Refresh);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Nom");
        ui.add(
            egui::TextEdit::singleline(&mut draft.profile_name)
                .id(profile_name_id())
                .desired_width(180.0)
                .char_limit(64),
        )
        .on_hover_text("Nom du profil a enregistrer ; un nom existant remplace ce profil.");
        if ui
            .add_enabled(
                !draft.profile_name.trim().is_empty(),
                egui::Button::new("Enregistrer"),
            )
            .on_hover_text("Enregistrer les reglages courants sous ce nom.")
            .clicked()
        {
            *action = Some(ProfileAction::Save(draft.profile_name.trim().to_owned()));
        }
    });
}

fn profile_name_id() -> egui::Id {
    egui::Id::new("physics_profile_name")
}

fn profile_picker(ui: &mut egui::Ui, id: &str, selection: &mut String, names: &[String]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(if selection.is_empty() {
            "Aucun"
        } else {
            selection.as_str()
        })
        .width(180.0)
        .truncate()
        .show_ui(ui, |ui| {
            for name in names {
                ui.selectable_value(selection, name.clone(), name);
            }
        });
}

fn apply_profile_action(
    action: ProfileAction,
    state: &mut PhysicsTuningState,
    config: &mut VehiclePhysicsConfig,
    profiles: &mut ProfileStore,
) {
    let result = match action {
        ProfileAction::Refresh => profiles.refresh(),
        ProfileAction::Save(name) => {
            let result = profiles.save(&name, config);
            if result.is_ok() {
                profiles.selected = name.clone();
                state.draft.profile_selection = name.clone();
                state.draft.profile_name = name;
            }
            result
        }
        ProfileAction::Load(name) => profiles.load(&name).and_then(|loaded| {
            commit_candidate(config, loaded)?;
            profiles.selected = name.clone();
            state.draft.profile_name = name;
            state.edit_error = None;
            Ok(())
        }),
        ProfileAction::Compare(name) => profiles.compare(&name).and_then(|reference| {
            reference.validate()?;
            state.reference = Some(reference);
            state.reference_name = name.clone();
            profiles.status = format!("Reference B : {name}");
            Ok(())
        }),
        ProfileAction::ApplyReference => {
            if let Some(reference) = state.reference.clone() {
                match commit_candidate(config, reference) {
                    Ok(_) => state.edit_error = None,
                    Err(error) => state.edit_error = Some(error),
                }
            }
            // Applying B never calls persistence or changes the selected saved profile.
            return;
        }
        ProfileAction::CaptureReference => {
            state.reference = Some(config.clone());
            state.reference_name = "Capture du courant".to_owned();
            return;
        }
    };
    if let Err(error) = result {
        profiles.status = "Operation profil refusee".to_owned();
        profiles.last_error = Some(error);
    } else {
        profiles.last_error = None;
    }
}

struct Parameter {
    tab: TuningTab,
    model: Option<PhysicsModel>,
    section: &'static str,
    label: &'static str,
    unit: &'static str,
    min: f32,
    max: f32,
    decimals: usize,
    help: &'static str,
    read: fn(&VehiclePhysicsConfig) -> f32,
    edit: fn(&mut VehiclePhysicsConfig) -> &mut f32,
}

impl Parameter {
    fn editable_in(&self, model: PhysicsModel) -> bool {
        self.model.is_none_or(|required| required == model)
    }
}

macro_rules! parameter {
    ($tab:ident, $section:literal, $label:literal, $unit:literal, $min:expr, $max:expr, $decimals:expr, $help:literal, $($field:tt)+) => {
        Parameter {
            tab: TuningTab::$tab,
            model: None,
            section: $section,
            label: $label,
            unit: $unit,
            min: $min,
            max: $max,
            decimals: $decimals,
            help: $help,
            read: |config| config.$($field)+,
            edit: |config| &mut config.$($field)+,
        }
    };
}

const PARAMETERS: &[Parameter] = &[
    parameter!(
        Chassis,
        "Chassis",
        "Masse",
        "kg",
        300.0,
        5000.0,
        1,
        "Masse totale du vehicule ; augmente l'inertie et la charge des ressorts.",
        chassis.mass_kg
    ),
    parameter!(
        Chassis,
        "Centre de masse local",
        "Lateral X",
        "m",
        -1.0,
        1.0,
        3,
        "Decalage lateral du centre de masse, dans le repere local du chassis.",
        chassis.center_of_mass[0]
    ),
    parameter!(
        Chassis,
        "Centre de masse local",
        "Vertical Y",
        "m",
        -1.0,
        1.0,
        3,
        "Un centre de masse plus bas reduit le transfert de charge et le risque de retournement.",
        chassis.center_of_mass[1]
    ),
    parameter!(
        Chassis,
        "Centre de masse local",
        "Longitudinal Z",
        "m",
        -1.0,
        1.0,
        3,
        "Decalage vers l'avant (+Z) ou l'arriere (-Z), qui modifie la repartition des charges.",
        chassis.center_of_mass[2]
    ),
    parameter!(
        Chassis,
        "Stabilisation",
        "Amortissement lineaire",
        "1/s",
        0.0,
        5.0,
        3,
        "Dissipation globale de la vitesse du chassis, independante de la resistance des pneus.",
        chassis.linear_damping
    ),
    parameter!(
        Chassis,
        "Stabilisation",
        "Amortissement angulaire",
        "1/s",
        0.0,
        10.0,
        3,
        "Dissipation globale des rotations : roulis, tangage et lacet.",
        chassis.angular_damping
    ),
    parameter!(
        Chassis,
        "Stabilisation",
        "Maintien en pente",
        "",
        0.0,
        1.0,
        3,
        "Intensite de compensation de la gravite a l'arret : 0 libre, 1 maintien maximal.",
        chassis.hill_hold_strength
    ),
    parameter!(
        Suspensions,
        "Essieu avant",
        "Raideur du ressort",
        "N/m",
        1000.0,
        200000.0,
        1,
        "Force par metre de compression, par roue. Plus elevee : moins d'affaissement.",
        front.stiffness_n_per_m
    ),
    parameter!(
        Suspensions,
        "Essieu avant",
        "Amortissement compression",
        "N.s/m",
        0.0,
        30000.0,
        1,
        "Freine la compression du ressort, par roue, proportionnellement a sa vitesse.",
        front.compression_damping_ns_per_m
    ),
    parameter!(
        Suspensions,
        "Essieu avant",
        "Amortissement detente",
        "N.s/m",
        0.0,
        30000.0,
        1,
        "Freine l'extension du ressort et limite les rebonds, par roue.",
        front.rebound_damping_ns_per_m
    ),
    parameter!(
        Suspensions,
        "Essieu avant",
        "Longueur au repos",
        "m",
        0.05,
        1.2,
        3,
        "Longueur de suspension sans charge ; influe sur la hauteur du chassis.",
        front.rest_length_m
    ),
    parameter!(
        Suspensions,
        "Essieu avant",
        "Debattement maximal",
        "m",
        0.01,
        1.2,
        3,
        "Course autorisee de la suspension autour de sa longueur au repos.",
        front.max_travel_m
    ),
    parameter!(
        Suspensions,
        "Essieu avant",
        "Force maximale",
        "N",
        500.0,
        100000.0,
        1,
        "Plafond de force verticale du ressort et de l'amortisseur, par roue.",
        front.max_force_n
    ),
    parameter!(
        Suspensions,
        "Essieu arriere",
        "Raideur du ressort",
        "N/m",
        1000.0,
        200000.0,
        1,
        "Force par metre de compression, par roue. Equilibre l'assiette avec l'avant.",
        rear.stiffness_n_per_m
    ),
    parameter!(
        Suspensions,
        "Essieu arriere",
        "Amortissement compression",
        "N.s/m",
        0.0,
        30000.0,
        1,
        "Freine l'enfoncement des suspensions arriere, par roue.",
        rear.compression_damping_ns_per_m
    ),
    parameter!(
        Suspensions,
        "Essieu arriere",
        "Amortissement detente",
        "N.s/m",
        0.0,
        30000.0,
        1,
        "Freine l'extension des suspensions arriere et limite les rebonds, par roue.",
        rear.rebound_damping_ns_per_m
    ),
    parameter!(
        Suspensions,
        "Essieu arriere",
        "Longueur au repos",
        "m",
        0.05,
        1.2,
        3,
        "Longueur sans charge des suspensions arriere ; influe sur l'assiette.",
        rear.rest_length_m
    ),
    parameter!(
        Suspensions,
        "Essieu arriere",
        "Debattement maximal",
        "m",
        0.01,
        1.2,
        3,
        "Course autorisee de la suspension autour de sa longueur au repos.",
        rear.max_travel_m
    ),
    parameter!(
        Suspensions,
        "Essieu arriere",
        "Force maximale",
        "N",
        500.0,
        100000.0,
        1,
        "Plafond de force verticale du ressort et de l'amortisseur, par roue.",
        rear.max_force_n
    ),
    parameter!(
        Tires,
        "Pneus",
        "Rayon",
        "m",
        0.1,
        1.0,
        3,
        "Rayon de roulement : modifie le contact au sol et la rotation visuelle des roues.",
        tires.radius_m
    ),
    parameter!(
        Tires,
        "Pneus",
        "Adherence combinee",
        "",
        0.05,
        3.0,
        3,
        "Coefficient de limite de friction combinee, multiplie par la charge normale.",
        tires.grip_coefficient
    ),
    parameter!(
        Tires,
        "Pneus",
        "Reponse laterale",
        "",
        0.0,
        1.0,
        3,
        "Gain lateral sans dimension ; renforce la correction de vitesse laterale, de 0 a 1.",
        tires.lateral_stiffness
    ),
    parameter!(
        Tires,
        "Resistance au roulement",
        "Resistance constante totale",
        "N",
        0.0,
        4000.0,
        1,
        "Resistance au roulement totale pour les quatre roues, opposee au mouvement.",
        tires.rolling_resistance_n
    ),
    parameter!(
        Tires,
        "Resistance au roulement",
        "Trainee proportionnelle",
        "N.s/m",
        0.0,
        1000.0,
        2,
        "Coefficient total de resistance des quatre roues, multiplie par la vitesse longitudinale.",
        tires.rolling_drag_ns_per_m
    ),
    parameter!(
        Drivetrain,
        "Traction",
        "Force moteur totale",
        "N",
        0.0,
        60000.0,
        1,
        "Force motrice de base totale pour les quatre roues, avant multiplication par le rapport.",
        drivetrain.engine_force_n
    ),
    parameter!(
        Drivetrain,
        "Traction",
        "Reponse accelerateur",
        "Hz",
        0.1,
        1000.0,
        2,
        "Rapidite de convergence vers la consigne ; 1000 Hz conserve la reponse quasi directe du profil Origine.",
        drivetrain.throttle_response_hz
    ),
    parameter!(
        Drivetrain,
        "Rapports de gamme",
        "Rapport normal",
        "",
        0.1,
        6.0,
        3,
        "Multiplicateur de force motrice en gamme normale.",
        drivetrain.normal_ratio
    ),
    parameter!(
        Drivetrain,
        "Rapports de gamme",
        "Rapport court",
        "",
        0.1,
        6.0,
        3,
        "Multiplicateur de force motrice en gamme courte pour les obstacles et les pentes.",
        drivetrain.low_ratio
    ),
    parameter!(
        Drivetrain,
        "Vitesses limites",
        "Gamme normale",
        "m/s",
        1.0,
        60.0,
        2,
        "Vitesse limite en marche avant, gamme normale ; 1 m/s = 3.6 km/h.",
        drivetrain.normal_max_speed_mps
    ),
    parameter!(
        Drivetrain,
        "Vitesses limites",
        "Gamme courte",
        "m/s",
        0.2,
        25.0,
        2,
        "Vitesse limite en marche avant, gamme courte ; 1 m/s = 3.6 km/h.",
        drivetrain.low_max_speed_mps
    ),
    parameter!(
        Drivetrain,
        "Vitesses limites",
        "Marche arriere",
        "m/s",
        0.2,
        20.0,
        2,
        "Valeur positive de la vitesse limite en marche arriere.",
        drivetrain.reverse_max_speed_mps
    ),
    parameter!(
        Drivetrain,
        "Freinage",
        "Force de freinage totale",
        "N",
        0.0,
        100000.0,
        1,
        "Force constante totale de freinage en newtons, dans les deux modeles ; distincte du coefficient Legacy proportionnel a la vitesse.",
        drivetrain.brake_force_n
    ),
    Parameter {
        model: Some(PhysicsModel::Legacy),
        ..parameter!(
            Drivetrain,
            "Freinage",
            "Amortissement frein Legacy",
            "N*s/m",
            0.0,
            50000.0,
            1,
            "Legacy uniquement : coefficient de freinage proportionnel a la vitesse (force = coefficient x vitesse). Inactif en Rapier.",
            drivetrain.legacy_brake_damping_ns_per_m
        )
    },
    parameter!(
        Drivetrain,
        "Freinage",
        "Frein moteur total",
        "N",
        0.0,
        30000.0,
        1,
        "Force resistante totale appliquee quand l'accelerateur est relache.",
        drivetrain.engine_brake_force_n
    ),
];

const MODIFIED_COLOR: egui::Color32 = egui::Color32::from_rgb(231, 171, 60);

fn modified_count(config: &VehiclePhysicsConfig, baseline: &VehiclePhysicsConfig) -> usize {
    PARAMETERS
        .iter()
        .filter(|parameter| (parameter.read)(config) != (parameter.read)(baseline))
        .count()
        + usize::from(config.model != baseline.model)
}

fn parameter_editor(
    ui: &mut egui::Ui,
    tab: TuningTab,
    candidate: &mut VehiclePhysicsConfig,
    baseline: &VehiclePhysicsConfig,
) {
    if tab == TuningTab::Chassis {
        ui.horizontal_wrapped(|ui| {
            let label = if candidate.model != baseline.model {
                "Modele *"
            } else {
                "Modele"
            };
            ui.label(label).on_hover_text(
                "Rapier : controleur natif. Legacy : modele d'origine pour comparaison.",
            );
            ui.selectable_value(&mut candidate.model, PhysicsModel::Rapier, "Rapier");
            ui.selectable_value(&mut candidate.model, PhysicsModel::Legacy, "Legacy");
        });
    }
    let mut last_section = "";
    for parameter in PARAMETERS.iter().filter(|parameter| parameter.tab == tab) {
        if parameter.section != last_section {
            if !last_section.is_empty() {
                ui.add_space(6.0);
            }
            ui.strong(parameter.section);
            last_section = parameter.section;
        }
        let baseline_value = (parameter.read)(baseline);
        let editable = parameter.editable_in(candidate.model);
        ui.push_id((parameter.section, parameter.label), |ui| {
            ui.horizontal(|ui| {
                let modified = (parameter.read)(candidate) != baseline_value;
                let label = format!("{}{}", parameter.label, if modified { " *" } else { "" });
                let text = if modified {
                    egui::RichText::new(label).color(MODIFIED_COLOR)
                } else {
                    egui::RichText::new(label)
                };
                ui.add_sized([210.0, 22.0], egui::Label::new(text).wrap())
                    .on_hover_text(parameter.help);
                let suffix = if parameter.unit.is_empty() {
                    String::new()
                } else {
                    format!(" {}", parameter.unit)
                };
                // Rendering must preserve preset values, including fractional ratios.
                ui.add_enabled(
                    editable,
                    egui::Slider::new((parameter.edit)(candidate), parameter.min..=parameter.max)
                        .clamping(egui::SliderClamping::Edits)
                        .logarithmic(parameter.min > 0.0 && parameter.max / parameter.min >= 100.0)
                        .show_value(true)
                        .max_decimals(parameter.decimals)
                        .custom_formatter(|value, _| (value as f32).to_string())
                        .suffix(suffix),
                )
                .on_hover_text(format!(
                    "{}\nBaseline : {} {}",
                    parameter.help, baseline_value, parameter.unit
                ))
                .on_disabled_hover_text(parameter.help);
                if ui
                    .add_enabled(editable && modified, egui::Button::new("\u{21ba}").small())
                    .on_hover_text(format!("Retablir {} {}", baseline_value, parameter.unit))
                    .clicked()
                {
                    *(parameter.edit)(candidate) = baseline_value;
                }
            });
        });
    }
}

fn comparison_controls(
    ui: &mut egui::Ui,
    draft: &mut UiDraft,
    candidate: &VehiclePhysicsConfig,
    state: &PhysicsTuningState,
    profiles: &ProfileStore,
    action: &mut Option<ProfileAction>,
) {
    ui.collapsing("Comparaison A/B", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Reference B");
            profile_picker(ui, "physics_reference", &mut draft.reference_selection, &profiles.names);
            if ui.add_enabled(!draft.reference_selection.is_empty(), egui::Button::new("Comparer"))
                .on_hover_text("Lire ce profil comme reference B sans le charger dans la physique.")
                .clicked()
            {
                *action = Some(ProfileAction::Compare(draft.reference_selection.clone()));
            }
            if ui.button("Capturer B").on_hover_text("Conserver une copie des reglages courants comme reference, en memoire.").clicked() {
                *action = Some(ProfileAction::CaptureReference);
            }
        });
        if let Some(reference) = &state.reference {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("A : courant | B : {}", state.reference_name));
                if ui.button("Appliquer B")
                    .on_hover_text("Appliquer la copie de reference aux reglages courants sans modifier les profils sur disque.")
                    .clicked()
                {
                    *action = Some(ProfileAction::ApplyReference);
                }
            });
            egui::Grid::new("physics_comparison_table")
                .num_columns(5)
                .striped(true)
                .spacing(egui::vec2(14.0, 4.0))
                .show(ui, |ui| {
                    for heading in ["Parametre", "A courant", "B reference", "A - B", "Unite"] {
                        ui.strong(heading);
                    }
                    ui.end_row();
                    ui.label("Modele");
                    ui.label(candidate.model.label());
                    ui.label(reference.model.label());
                    ui.label(if candidate.model == reference.model { "=" } else { "Different" });
                    ui.label("");
                    ui.end_row();
                    for parameter in PARAMETERS {
                        let current = (parameter.read)(candidate);
                        let other = (parameter.read)(reference);
                        let difference = current - other;
                        let label = format!("{} / {}", parameter.section, parameter.label);
                        let text = if current != other { egui::RichText::new(label).color(MODIFIED_COLOR) } else { egui::RichText::new(label) };
                        ui.label(text).on_hover_text(parameter.help);
                        ui.monospace(current.to_string());
                        ui.monospace(other.to_string());
                        ui.monospace(if difference == 0.0 { "0".to_owned() } else { format!("{difference:+}") });
                        ui.label(parameter.unit);
                        ui.end_row();
                    }
                });
        }
    });
}

fn diagnostics(ui: &mut egui::Ui, telemetry: Option<&VehicleTelemetry>) {
    let Some(telemetry) = telemetry else {
        ui.label("Telemetrie indisponible");
        return;
    };
    ui.strong(format!("Modele actif : {}", telemetry.model.label()));
    if telemetry.model == PhysicsModel::Rapier {
        ui.label(if telemetry.experimental_tires {
            "Contacts pneus : volumetriques experimentaux"
        } else {
            "Contacts pneus : Rapier natif"
        });
    }
    ui.label(format!(
        "Masse effective : {:.1} kg | Centre de masse local : ({:.3}, {:.3}, {:.3}) m",
        telemetry.effective_mass_kg,
        telemetry.effective_center_of_mass.x,
        telemetry.effective_center_of_mass.y,
        telemetry.effective_center_of_mass.z
    ));
    egui::Grid::new("physics_telemetry_summary")
        .num_columns(4)
        .striped(true)
        .spacing(egui::vec2(16.0, 4.0))
        .show(ui, |ui| {
            for (left, left_value, right, right_value) in [
                (
                    "FPS",
                    format_number(telemetry.fps, 1),
                    "Temps de trame",
                    format!("{} ms", format_number(telemetry.frame_ms, 2)),
                ),
                (
                    "Vitesse",
                    format!("{} km/h", format_number(telemetry.speed_kmh, 2)),
                    "Acceleration long. estimee",
                    format!(
                        "{} m/s2",
                        format_number(telemetry.longitudinal_acceleration_mps2, 3)
                    ),
                ),
                (
                    "Vitesse de lacet",
                    format!("{} rad/s", format_number(telemetry.yaw_rate_rad_s, 3)),
                    "Direction",
                    format!("{} rad", format_number(telemetry.steering_rad, 3)),
                ),
                (
                    "Tangage",
                    format!("{} rad", format_number(telemetry.pitch_rad, 3)),
                    "Roulis",
                    format!("{} rad", format_number(telemetry.roll_rad, 3)),
                ),
                (
                    "Moteur demande / max.",
                    format!("{} N", format_number(telemetry.engine_force_n, 1)),
                    "Freinage demande / max.",
                    format!("{} N", format_number(telemetry.brake_force_n, 1)),
                ),
            ] {
                ui.label(left);
                ui.monospace(left_value);
                ui.label(right);
                ui.monospace(right_value);
                ui.end_row();
            }
        });
    if telemetry.model == PhysicsModel::Rapier {
        ui.small("Rapier : moteur/freinage [N] = demandes ou maxima disponibles.");
        ui.small("Les forces motrices et de freinage appliquees aux pneus ne sont pas mesurees.");
    }
    ui.add_space(8.0);
    ui.strong(format!(
        "Roues en contact : {} / 4",
        telemetry.grounded_wheels
    ));
    egui::Grid::new("physics_wheel_telemetry")
        .num_columns(8)
        .striped(true)
        .spacing(egui::vec2(12.0, 4.0))
        .show(ui, |ui| {
            for (label, help) in [
                ("Roue", "FR avant droite, FL avant gauche, RR arriere droite, RL arriere gauche ; indices physiques 0, 1, 2, 3."),
                ("Contact", "Roue en contact avec le terrain."),
                ("Compr. [m]", "Compression du ressort en metres."),
                ("Long. [m]", "Longueur actuelle de suspension en metres."),
                ("Charge [N]", "Rapier : echantillon reel de force de suspension native apres plafonnement. Legacy : charge estimee via les ressorts."),
                ("V long. [m/s]", "Vitesse longitudinale au contact de la roue."),
                ("V lat. [m/s]", "Vitesse laterale au contact de la roue."),
                ("Derive est. [m/s]", "Estimation de derive laterale (slip_estimate_mps). Le taux de glissement longitudinal est indisponible pour les roues raycast."),
            ] {
                ui.strong(label).on_hover_text(help);
            }
            ui.end_row();
            for (index, wheel) in telemetry.wheels.iter().enumerate() {
                ui.strong(["FR", "FL", "RR", "RL"][index]);
                ui.colored_label(if wheel.grounded { egui::Color32::from_rgb(105, 197, 143) } else { ui.visuals().weak_text_color() }, if wheel.grounded { "Sol" } else { "Air" });
                ui.monospace(format_number(wheel.compression_m, 3));
                ui.monospace(format_number(wheel.suspension_length_m, 3));
                ui.monospace(format_number(wheel.normal_load_n, 1));
                ui.monospace(format_number(wheel.longitudinal_speed_mps, 3));
                ui.monospace(format_number(wheel.lateral_speed_mps, 3));
                ui.monospace(format_number(wheel.slip_estimate_mps, 3));
                ui.end_row();
            }
        });
    ui.small(if telemetry.model == PhysicsModel::Legacy {
        "Charges Legacy [N] : estimations de force des ressorts."
    } else {
        "Charges Rapier [N] : echantillons reels de force de suspension native, plafonnes."
    });
    ui.small("Acceleration longitudinale [m/s2] : estimation par difference finie de la vitesse.");
    ui.small("Derive laterale [m/s] : estimation.");
    ui.small("Taux de glissement longitudinal indisponible (roues raycast).");
}

fn format_number(value: f32, decimals: usize) -> String {
    if value.is_finite() {
        format!("{value:.decimals$}")
    } else {
        "--".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_five_presets_fit_declared_slider_ranges() {
        assert_eq!(PARAMETERS.len(), 34);
        assert_eq!(
            VehiclePhysicsConfig::preset_names(),
            &[
                "Baseline",
                "Confort",
                "Franchissement",
                "Dynamique",
                "Origine"
            ]
        );
        for &name in VehiclePhysicsConfig::preset_names() {
            let config = VehiclePhysicsConfig::preset(name).unwrap();
            for parameter in PARAMETERS {
                let value = (parameter.read)(&config);
                assert!(
                    value.is_finite() && (parameter.min..=parameter.max).contains(&value),
                    "{name}: {} / {} = {value} outside {}..={}",
                    parameter.section,
                    parameter.label,
                    parameter.min,
                    parameter.max,
                );
            }
        }
    }

    #[test]
    fn idle_multipass_rendering_preserves_every_preset_value() {
        let baseline = VehiclePhysicsConfig::baseline();
        for &name in VehiclePhysicsConfig::preset_names() {
            let expected = VehiclePhysicsConfig::preset(name).unwrap();
            if name == "Origine" {
                assert_eq!(expected.drivetrain.throttle_response_hz, 1000.0);
                assert_eq!(expected.drivetrain.legacy_brake_damping_ns_per_m, 7800.0);
                assert_eq!(expected.drivetrain.brake_force_n, 0.0);
            }
            let mut candidate = expected.clone();
            let ctx = egui::Context::default();
            for tab in [
                TuningTab::Chassis,
                TuningTab::Suspensions,
                TuningTab::Tires,
                TuningTab::Drivetrain,
            ] {
                for _ in 0..3 {
                    let mut rendered_passes = 0;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::pos2(0.0, 0.0),
                                egui::vec2(1280.0, 1000.0),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            rendered_passes += 1;
                            parameter_editor(ui, tab, &mut candidate, &baseline);
                            if ctx.current_pass_index() == 0 {
                                ctx.request_discard("Exercise idle multipass preset rendering");
                            }
                        },
                    );
                    // Headless widget tests do not upload the font textures.
                    output.textures_delta.clear();
                    assert!(rendered_passes >= 2);
                    assert_eq!(candidate, expected, "Idle rendering changed preset {name}");
                }
            }
        }
    }

    #[test]
    fn legacy_braking_metadata_keeps_coefficient_and_force_units_distinct() {
        let baseline = VehiclePhysicsConfig::baseline();
        let mut original = VehiclePhysicsConfig::original();
        let coefficient = PARAMETERS
            .iter()
            .find(|parameter| parameter.model == Some(PhysicsModel::Legacy))
            .unwrap();
        assert_eq!(coefficient.unit, "N*s/m");
        assert_eq!((coefficient.min, coefficient.max), (0.0, 50000.0));
        assert!(coefficient.editable_in(PhysicsModel::Legacy));
        assert!(!coefficient.editable_in(PhysicsModel::Rapier));
        assert_eq!((coefficient.read)(&baseline), 0.0);
        assert_eq!((coefficient.read)(&original), 7800.0);
        *(coefficient.edit)(&mut original) = 9000.0;
        assert_eq!(original.drivetrain.legacy_brake_damping_ns_per_m, 9000.0);
        assert_eq!(original.drivetrain.brake_force_n, 0.0);

        let force = PARAMETERS
            .iter()
            .find(|parameter| parameter.label == "Force de freinage totale")
            .unwrap();
        assert_eq!(force.unit, "N");
        assert!(force.editable_in(PhysicsModel::Legacy));
        assert!(force.editable_in(PhysicsModel::Rapier));
        *(force.edit)(&mut original) = 1200.0;
        assert_eq!(original.drivetrain.brake_force_n, 1200.0);
        assert_eq!(original.drivetrain.legacy_brake_damping_ns_per_m, 9000.0);
    }

    #[test]
    fn idle_rapier_rendering_preserves_inactive_legacy_brake_coefficient() {
        let baseline = VehiclePhysicsConfig::baseline();
        let mut candidate = baseline.clone();
        candidate.drivetrain.legacy_brake_damping_ns_per_m = 7800.0;
        let expected = candidate.clone();
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                parameter_editor(ui, TuningTab::Drivetrain, &mut candidate, &baseline);
                if ctx.current_pass_index() == 0 {
                    ctx.request_discard("Exercise disabled Legacy coefficient rendering");
                }
            });
            output.textures_delta.clear();
            assert_eq!(candidate, expected);
        }
    }

    #[test]
    fn physical_backquote_is_recognized_independently_of_layout() {
        assert!(is_toggle_key(
            KeyCode::Backquote,
            &Key::Character("`".into())
        ));
        assert!(is_toggle_key(
            KeyCode::Backquote,
            &Key::Character("~".into())
        ));
    }

    #[test]
    fn logical_squared_is_recognized_independently_of_physical_position() {
        assert!(is_toggle_key(
            KeyCode::IntlBackslash,
            &Key::Character("\u{00b2}".into())
        ));
        assert!(!is_toggle_key(KeyCode::Digit2, &Key::Character("2".into())));
    }

    #[test]
    fn f2_is_not_a_tuning_shortcut() {
        assert!(!is_toggle_key(KeyCode::F2, &Key::F2));
        assert!(!is_toggle_key(KeyCode::KeyA, &Key::Character("a".into())));
    }

    #[test]
    fn toggle_fires_once_until_the_physical_key_is_released() {
        let mut keys = ToggleKeys::default();
        let logical = Key::Character("\u{00b2}".into());
        assert!(keys.update(KeyCode::Backquote, &logical, ButtonState::Pressed, false));
        assert!(!keys.update(KeyCode::Backquote, &logical, ButtonState::Pressed, true));
        assert!(!keys.update(KeyCode::Backquote, &logical, ButtonState::Pressed, false));
        assert!(!keys.update(
            KeyCode::Backquote,
            &Key::Character("~".into()),
            ButtonState::Released,
            false
        ));
        assert!(keys.update(KeyCode::Backquote, &logical, ButtonState::Pressed, false));
    }

    #[test]
    fn a_repeat_without_an_initial_press_never_toggles() {
        let mut keys = ToggleKeys::default();
        assert!(!keys.update(
            KeyCode::Backquote,
            &Key::Character("`".into()),
            ButtonState::Pressed,
            true
        ));
    }

    #[test]
    fn logical_squared_fires_once_on_another_physical_key() {
        let mut keys = ToggleKeys::default();
        let logical = Key::Character("\u{00b2}".into());
        assert!(keys.update(
            KeyCode::IntlBackslash,
            &logical,
            ButtonState::Pressed,
            false
        ));
        assert!(!keys.update(KeyCode::IntlBackslash, &logical, ButtonState::Pressed, true));
        assert!(!keys.update(
            KeyCode::IntlBackslash,
            &logical,
            ButtonState::Released,
            false
        ));
        assert!(keys.update(
            KeyCode::IntlBackslash,
            &logical,
            ButtonState::Pressed,
            false
        ));
    }

    #[test]
    fn invalid_candidate_leaves_live_physics_unchanged() {
        let mut current = VehiclePhysicsConfig::baseline();
        let before = current.clone();
        let mut candidate = before.clone();
        candidate.chassis.mass_kg = f32::NAN;
        assert!(commit_candidate(&mut current, candidate).is_err());
        assert_eq!(current, before);
    }

    #[test]
    fn invalid_suspension_load_leaves_live_physics_unchanged() {
        let mut current = VehiclePhysicsConfig::baseline();
        let before = current.clone();
        let mut candidate = before.clone();
        candidate.front.max_force_n = 100.0;
        assert!(commit_candidate(&mut current, candidate).is_err());
        assert_eq!(current, before);
    }

    #[test]
    fn valid_reference_is_applied_as_a_copy() {
        let mut current = VehiclePhysicsConfig::baseline();
        let reference = VehiclePhysicsConfig::original();
        assert!(commit_candidate(&mut current, reference.clone()).unwrap());
        assert_eq!(current, reference);
        current.chassis.mass_kg += 1.0;
        assert_ne!(current, reference);
    }

    #[test]
    fn applying_reference_keeps_profile_store_unchanged() {
        let mut state = PhysicsTuningState::default();
        state.reference = Some(VehiclePhysicsConfig::original());
        let mut config = VehiclePhysicsConfig::baseline();
        let mut profiles = ProfileStore {
            root: std::path::PathBuf::from("unused-by-reference-apply"),
            selected: "Saved A".to_owned(),
            names: vec!["Saved A".to_owned(), "Saved B".to_owned()],
            status: "Existing status".to_owned(),
            last_error: Some("Existing error".to_owned()),
        };
        apply_profile_action(
            ProfileAction::ApplyReference,
            &mut state,
            &mut config,
            &mut profiles,
        );
        assert_eq!(config, VehiclePhysicsConfig::original());
        assert_eq!(
            profiles.root,
            std::path::PathBuf::from("unused-by-reference-apply")
        );
        assert_eq!(profiles.selected, "Saved A");
        assert_eq!(profiles.names, vec!["Saved A", "Saved B"]);
        assert_eq!(profiles.status, "Existing status");
        assert_eq!(profiles.last_error.as_deref(), Some("Existing error"));
    }

    #[test]
    fn opening_a_panel_does_not_capture_devices_by_default() {
        let capture = UiInputCapture::default();
        assert!(!capture.pointer);
        assert!(!capture.keyboard);
        assert!(!PhysicsTuningState::default().open);
    }

    #[test]
    fn baseline_markers_cover_model_and_independent_axles() {
        let baseline = VehiclePhysicsConfig::baseline();
        let mut candidate = baseline.clone();
        assert_eq!(modified_count(&candidate, &baseline), 0);
        candidate.model = PhysicsModel::Legacy;
        candidate.front.stiffness_n_per_m += 100.0;
        candidate.rear.stiffness_n_per_m += 200.0;
        candidate.chassis.center_of_mass[0] += 0.1;
        assert_eq!(modified_count(&candidate, &baseline), 4);
    }
}
