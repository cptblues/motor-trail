//! Stable, geometry-backed thumbnail catalogue for the Terrain Workshop.
//! Each preview is projected exactly once from the same meshes used in game;
//! no extra cameras, GPU render targets or packaged image files are required.
use std::sync::OnceLock;

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy_egui::egui;

use crate::terrain::obstacles::{Obstacle, ObstacleKind};

const CATEGORIES: [&str; 8] = [
    "Toutes", "Primitives", "Arbres", "Rochers",
    "Buissons", "Herbes", "Fougeres", "Bois",
];

#[derive(Clone)]
struct Face {
    projected: [[f32; 2]; 3],
    depth: f32,
    color: egui::Color32,
}

#[derive(Default)]
struct Thumbnail {
    faces: Vec<Face>,
    min: [f32; 2],
    max: [f32; 2],
}

static THUMBNAILS: OnceLock<Vec<Thumbnail>> = OnceLock::new();

fn category(kind: ObstacleKind) -> &'static str {
    match kind {
        ObstacleKind::Nature(asset) => asset.category(),
        _ => "Primitives",
    }
}

fn normalise(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|ch| match ch {
            'à' | 'â' | 'ä' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ÿ' => 'y',
            _ => ch,
        })
        .collect()
}

fn matches(kind: ObstacleKind, query: &str, filter: &str) -> bool {
    (filter == "Toutes" || category(kind) == filter)
        && (query.trim().is_empty()
            || normalise(&format!("{} {}", kind.name(), category(kind)))
                .contains(&normalise(query.trim())))
}

fn natural_size(kind: ObstacleKind) -> [f32; 3] {
    match kind {
        ObstacleKind::Nature(asset) => asset.default_size().to_array(),
        _ => [2.0, 2.0, 2.0],
    }
}

fn project(p: Vec3) -> [f32; 2] {
    [(p.x - p.z) * 0.82, -p.y * 0.96 + (p.x + p.z) * 0.33]
}

fn rgb(linear: [f32; 4], light: f32) -> egui::Color32 {
    let to_byte = |channel: f32| {
        (255.0 * (channel.clamp(0.0, 1.0) * light).clamp(0.0, 1.0)
            .powf(1.0 / 2.2))
            .round() as u8
    };
    egui::Color32::from_rgb(
        to_byte(linear[0]), to_byte(linear[1]), to_byte(linear[2]),
    )
}

fn build_thumbnail(kind: ObstacleKind) -> Thumbnail {
    let object = Obstacle {
        id: 1,
        kind,
        position: [0.0; 3],
        rotation_degrees: [0.0; 3],
        size: natural_size(kind),
    };
    let Ok(mesh) = object.build_mesh() else {
        return Thumbnail::default();
    };
    let Some(VertexAttributeValues::Float32x3(vertices)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Thumbnail::default();
    };
    let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(colors)) => Some(colors),
        _ => None,
    };
    let indices: Vec<usize> = match mesh.indices() {
        Some(indices) => indices.iter().collect(),
        None => (0..vertices.len()).collect(),
    };
    let mut result = Thumbnail {
        faces: Vec::new(),
        min: [f32::INFINITY; 2],
        max: [f32::NEG_INFINITY; 2],
    };
    for triangle in indices.chunks_exact(3) {
        let (Some(a), Some(b), Some(c)) = (
            vertices.get(triangle[0]),
            vertices.get(triangle[1]),
            vertices.get(triangle[2]),
        ) else {
            continue;
        };
        let positions = [Vec3::from_array(*a), Vec3::from_array(*b), Vec3::from_array(*c)];
        let normal = (positions[1] - positions[0])
            .cross(positions[2] - positions[0])
            .normalize_or_zero();
        if normal == Vec3::ZERO {
            continue;
        }
        let projected = positions.map(project);
        for xy in projected {
            for axis in 0..2 {
                result.min[axis] = result.min[axis].min(xy[axis]);
                result.max[axis] = result.max[axis].max(xy[axis]);
            }
        }
        let material = colors
            .and_then(|colors| colors.get(triangle[0]))
            .copied()
            .unwrap_or([0.37, 0.40, 0.39, 1.0]);
        let light = 0.65 + 0.35 * normal.dot(Vec3::new(0.4, 0.8, 0.4).normalize()).abs();
        result.faces.push(Face {
            projected,
            depth: positions.iter().map(|p| p.x + p.z + p.y * 0.25).sum::<f32>() / 3.0,
            color: rgb(material, light),
        });
    }
    result.faces.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    result
}

fn draw_thumbnail(painter: &egui::Painter, rect: egui::Rect, thumbnail: &Thumbnail) {
    if thumbnail.faces.is_empty() {
        return;
    }
    let width = (thumbnail.max[0] - thumbnail.min[0]).max(0.001);
    let height = (thumbnail.max[1] - thumbnail.min[1]).max(0.001);
    let scale = (rect.width() / width).min(rect.height() / height);
    let cx = (thumbnail.min[0] + thumbnail.max[0]) * 0.5;
    let cy = (thumbnail.min[1] + thumbnail.max[1]) * 0.5;
    for face in &thumbnail.faces {
        let points = face.projected.map(|xy| {
            egui::pos2(
                rect.center().x + (xy[0] - cx) * scale,
                rect.center().y + (xy[1] - cy) * scale,
            )
        });
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(), face.color, egui::Stroke::NONE,
        ));
    }
}

/// Draws all 28 existing assets. Returns the stable kind chosen by the user.
pub(super) fn catalogue(
    ui: &mut egui::Ui,
    selected: ObstacleKind,
    query: &mut String,
    filter: &mut String,
) -> Option<ObstacleKind> {
    ui.horizontal_wrapped(|ui| {
        ui.label("Rechercher");
        ui.add(egui::TextEdit::singleline(query).hint_text("pin, roche, fougere...")
            .desired_width(150.0));
        egui::ComboBox::from_id_salt("editor_asset_category")
            .selected_text(if filter.is_empty() { "Toutes" } else { filter.as_str() })
            .show_ui(ui, |ui| {
                for category in CATEGORIES {
                    ui.selectable_value(filter, category.to_owned(), category);
                }
            });
    });

    let assets: Vec<_> = ObstacleKind::catalogue().collect();
    let thumbnails = THUMBNAILS.get_or_init(|| {
        assets.iter().copied().map(build_thumbnail).collect()
    });
    let count = assets.iter().filter(|kind| matches(**kind, query, filter)).count();
    ui.small(format!("{count} resultat(s) sur {} objets", assets.len()));
    if count == 0 {
        ui.label("Aucun resultat. Essayez un autre mot ou une autre categorie.");
    }

    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(5.0, 6.0);
        for (index, kind) in assets.iter().copied().enumerate() {
            if !matches(kind, query, filter) {
                continue;
            }
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(112.0, 119.0), egui::Sense::click());
            let painter = ui.painter_at(rect);
            let active = kind == selected;
            let fill = if active {
                egui::Color32::from_rgb(44, 88, 86)
            } else {
                ui.visuals().faint_bg_color
            };
            painter.rect_filled(rect, 6.0, fill);
            painter.rect_stroke(
                rect, 6.0,
                egui::Stroke::new(if active { 2.0 } else { 1.0 },
                    if active { egui::Color32::LIGHT_GREEN } else { ui.visuals().widgets.inactive.bg_stroke.color }),
                egui::StrokeKind::Inside,
            );
            draw_thumbnail(&painter, egui::Rect::from_min_max(
                rect.min + egui::vec2(8.0, 5.0),
                egui::pos2(rect.max.x - 8.0, rect.max.y - 26.0),
            ), &thumbnails[index]);
            painter.text(
                egui::pos2(rect.center().x, rect.max.y - 13.0),
                egui::Align2::CENTER_CENTER, kind.name(),
                egui::FontId::proportional(10.0), ui.visuals().text_color(),
            );
            let size = natural_size(kind);
            let response = response.on_hover_text(format!(
                "{} · {}\nTaille initiale : {:.1} × {:.1} × {:.1} m\nCliquer pour placer",
                category(kind), kind.name(), size[0], size[1], size[2],
            ));
            if response.clicked() {
                chosen = Some(kind);
            }
        }
    });
    chosen
}

/// Compact horizontal asset shelf, designed for the bottom dock. Uses the
/// same source meshes, cached projections, IDs and filters as the side gallery.
pub(super) fn catalogue_strip(
    ui: &mut egui::Ui,
    selected: ObstacleKind,
    query: &mut String,
    filter: &mut String,
) -> Option<ObstacleKind> {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("BIBLIOTHÈQUE D'OBJETS").strong());
        ui.add(egui::TextEdit::singleline(query)
            .hint_text("Rechercher un arbre, une roche…")
            .desired_width(215.0));
        for label in ["Toutes", "Arbres", "Rochers", "Bois"] {
            if ui.selectable_label(
                (filter.is_empty() && label == "Toutes") || filter == label,
                label,
            ).clicked() {
                *filter = label.to_owned();
            }
        }
        egui::ComboBox::from_id_salt("asset_shelf_category")
            .selected_text(if filter.is_empty() { "Toutes" } else { filter.as_str() })
            .show_ui(ui, |ui| {
                for label in CATEGORIES {
                    ui.selectable_value(filter, label.to_owned(), label);
                }
            });
    });
    let assets: Vec<_> = ObstacleKind::catalogue().collect();
    let thumbnails = THUMBNAILS.get_or_init(|| assets.iter().copied().map(build_thumbnail).collect());
    let mut chosen = None;
    egui::ScrollArea::horizontal()
        .id_salt("motor_trail_asset_shelf")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (index, kind) in assets.iter().copied().enumerate() {
                    if !matches(kind, query, filter) { continue; }
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(112.0, 121.0), egui::Sense::click());
                    let painter = ui.painter_at(rect);
                    let active = kind == selected;
                    let background = if active {
                        egui::Color32::from_rgb(209, 240, 223)
                    } else {
                        egui::Color32::from_rgb(238, 241, 237)
                    };
                    painter.rect_filled(rect, 7.0, background);
                    painter.rect_stroke(
                        rect, 7.0,
                        egui::Stroke::new(
                            if active { 2.0 } else { 1.0 },
                            if active {
                                egui::Color32::from_rgb(39, 169, 108)
                            } else {
                                egui::Color32::from_rgb(185, 194, 187)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    draw_thumbnail(&painter, egui::Rect::from_min_max(
                        rect.min + egui::vec2(8.0, 5.0),
                        egui::pos2(rect.max.x - 8.0, rect.max.y - 26.0),
                    ), &thumbnails[index]);
                    painter.text(
                        egui::pos2(rect.center().x, rect.max.y - 13.0),
                        egui::Align2::CENTER_CENTER,
                        kind.name(),
                        egui::FontId::proportional(10.0),
                        egui::Color32::from_rgb(32, 50, 42),
                    );
                    let dimensions = natural_size(kind);
                    if response.on_hover_text(format!(
                        "{}\n{:.1} × {:.1} × {:.1} m\nCliquer pour placer",
                        kind.name(), dimensions[0], dimensions[1], dimensions[2],
                    )).clicked() {
                        chosen = Some(kind);
                    }
                }
            });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_accent_and_case_insensitive() {
        let pine = ObstacleKind::Nature(crate::terrain::nature::NatureAsset::TreePineTall);
        let fern = ObstacleKind::Nature(crate::terrain::nature::NatureAsset::FernSmall);
        assert!(matches(pine, "PIN", "Toutes"));
        assert!(matches(fern, "FOUGÈRE", "Toutes"));
        assert!(matches(fern, "fougere", "Fougeres"));
        assert!(!matches(pine, "fougere", "Toutes"));
        assert!(!matches(ObstacleKind::Cube, "", "Arbres"));
    }

    #[test]
    fn thumbnails_cover_every_stable_kind() {
        let assets: Vec<_> = ObstacleKind::catalogue().collect();
        assert_eq!(assets.len(), 28);
        for kind in assets {
            assert!(!build_thumbnail(kind).faces.is_empty(), "missing thumbnail: {kind:?}");
        }
    }
}
