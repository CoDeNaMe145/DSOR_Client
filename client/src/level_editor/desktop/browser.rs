use super::*;
use std::{collections::BTreeSet, path::Path};
struct Thumb {
    path: PathBuf,
    model: String,
    key: Option<String>,
    texture: Option<egui::TextureId>,
    requested: bool,
    available: bool,
}
pub(super) struct Thumbnails {
    directory: PathBuf,
    assets: PathBuf,
    index: Value,
    items: HashMap<String, Thumb>,
}
impl Thumbnails {
    pub fn new(p: &Project) -> Self {
        let directory = p.cache.join("previews");
        let _ = std::fs::create_dir_all(&directory);
        let index = data::read_json(directory.join("index.json"))
            .ok()
            .flatten()
            .unwrap_or(json!({}));
        Self {
            directory,
            assets: p.assets.clone(),
            index,
            items: default(),
        }
    }
    pub fn image(
        &mut self,
        ui: &mut egui::Ui,
        model: &str,
        appearance: &Value,
        key: Option<&str>,
        size: f32,
        previews: &mut previews::Previews,
        textures: &mut EguiUserTextures,
        images: &mut Assets<Image>,
    ) {
        if model.is_empty() {
            icon(ui, key.unwrap_or(""), size);
            return;
        }
        let identity = data::digest(&json!({"model":model,"appearance":appearance}));
        let item = self.items.entry(identity.clone()).or_insert_with(|| {
            let reference = model.rsplit(':').next().unwrap();
            let source = self.assets.join(format!("{reference}.glb"));
            let sources = if matches!(reference, "characters/uniskel" | "characters/uniskel_dwarf")
            {
                [
                    "outfits.json",
                    "parts.json",
                    "variations.json",
                    "__animations.glb",
                ]
                .iter()
                .map(|n| self.assets.join(reference).join(n))
                .collect()
            } else {
                vec![source]
            };
            let stamps: Vec<_> = sources
                .iter()
                .filter_map(|p| {
                    let m = std::fs::metadata(p).ok()?;
                    Some(json!([
                        p,
                        m.len(),
                        m.modified()
                            .ok()?
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()?
                            .as_nanos()
                            .to_string()
                    ]))
                })
                .collect();
            let available = !stamps.is_empty();
            let signature = data::digest(&json!({"rust_renderer":1,"sources":stamps}));
            let mut path = self
                .directory
                .join(format!("{identity}-{}.png", &signature[..20]));
            if !available {
                if let Some(previous) = self.index[&identity]["file"].as_str() {
                    let cached = self.directory.join(previous);
                    if cached.is_file() {
                        path = cached;
                    }
                }
            }
            Thumb {
                path,
                model: model.into(),
                key: key.map(str::to_owned),
                texture: None,
                requested: false,
                available,
            }
        });
        if item.texture.is_none() && item.path.is_file() {
            if let Ok(raw) = std::fs::read(&item.path) {
                if let Ok(image) = Image::from_buffer(
                    &raw,
                    bevy::image::ImageType::Extension("png"),
                    bevy::image::CompressedImageFormats::NONE,
                    true,
                    bevy::image::ImageSampler::default(),
                    default(),
                ) {
                    item.texture =
                        Some(textures.add_image(EguiTextureHandle::Strong(images.add(image))));
                    self.index[&identity] = json!({"file":item.path.file_name().unwrap().to_string_lossy(),"model":model,"appearance":appearance});
                    let _ = data::atomic_json(self.directory.join("index.json"), &self.index);
                }
            }
        }
        if let Some(texture) = item.texture {
            ui.image((texture, egui::vec2(size, size)));
        } else {
            icon(ui, key.unwrap_or("geometry"), size);
            if !item.requested && item.available && !item.path.with_extension("failed").exists() {
                item.requested = true;
                previews.queue.insert(
                    0,
                    previews::Request {
                        model: item.model.clone(),
                        key: item.key.clone(),
                        path: item.path.clone(),
                    },
                );
            }
        }
    }
}
fn icon(ui: &mut egui::Ui, kind: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4., egui::Color32::from_rgb(20, 26, 34));
    let c = rect.center();
    let r = size * 0.26;
    let color = if kind.contains("Light") {
        egui::Color32::from_rgb(239, 194, 107)
    } else {
        egui::Color32::from_rgb(112, 155, 188)
    };
    let stroke = egui::Stroke::new(1.4, color);
    if kind.contains("Light") || kind.contains("Ambience") {
        p.circle_stroke(c, r * 0.5, stroke);
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::TAU / 8.;
            let v = egui::vec2(a.cos(), a.sin());
            p.line_segment([c + v * r * 0.7, c + v * r * 1.2], stroke);
        }
    } else if kind.contains("camera") || kind.contains("Camera") {
        p.rect_stroke(
            egui::Rect::from_center_size(c, egui::vec2(r * 1.5, r)),
            1.,
            stroke,
            egui::StrokeKind::Inside,
        );
        p.line_segment(
            [
                c + egui::vec2(r * 0.75, 0.),
                c + egui::vec2(r * 1.3, -r * 0.6),
            ],
            stroke,
        );
        p.line_segment(
            [
                c + egui::vec2(r * 0.75, 0.),
                c + egui::vec2(r * 1.3, r * 0.6),
            ],
            stroke,
        );
    } else {
        let a = c + egui::vec2(0., -r);
        let b = c + egui::vec2(r, -r * 0.4);
        let d = c + egui::vec2(-r, -r * 0.4);
        let e = c + egui::vec2(0., r * 0.2);
        let f = c + egui::vec2(0., r);
        for (x, y) in [
            (a, b),
            (a, d),
            (b, e),
            (d, e),
            (e, f),
            (b, b + egui::vec2(0., r * 0.8)),
            (d, d + egui::vec2(0., r * 0.8)),
            (f, b + egui::vec2(0., r * 0.8)),
            (f, d + egui::vec2(0., r * 0.8)),
        ] {
            p.line_segment([x, y], stroke);
        }
    }
}
pub fn scan_assets(project: &Project) -> Vec<String> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(ty) = entry.file_type() else { continue };
            let p = entry.path();
            if ty.is_dir() {
                visit(root, &p, out);
            } else if p.extension().is_some_and(|x| x == "glb")
                && !entry.file_name().to_string_lossy().starts_with("__")
            {
                if let Ok(relative) = p.strip_prefix(root) {
                    out.insert(
                        relative
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
    }
    let cache = project.cache.join("asset-library.json");
    let mut models = BTreeSet::new();
    visit(&project.assets, &project.assets, &mut models);
    if models.is_empty() {
        if let Ok(Some(cached)) = data::read_json(&cache) {
            models.extend(
                data::array(&cached)
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
        }
    } else {
        let _ = data::atomic_json(cache, &json!(models));
    }
    models.into_iter().collect()
}
pub(super) fn objects(
    ui: &mut egui::Ui,
    state: &mut State,
    previews: &mut previews::Previews,
    textures: &mut EguiUserTextures,
    images: &mut Assets<Image>,
    actions: &mut Vec<Action>,
) {
    if state.tab == 2 {
        ui.heading("Lighting");
        ui.horizontal(|ui| {
            if ui.button("+ Point").clicked() {
                actions.push(Action::AddLight(false));
            }
            if ui.button("+ Spot").clicked() {
                actions.push(Action::AddLight(true));
            }
        });
        if ui.button("Select environment").clicked() {
            if let Some(s) = &state.session {
                if let Some((id, _)) = data::object(&s.records[data::AMBIENCE])
                    .find(|(_, v)| data::flag(&v["PEDefaultEntity"]))
                {
                    actions.push(Action::Select(format!("{}\t{id}", data::AMBIENCE)));
                }
            }
        }
    } else {
        ui.heading("World outliner");
        egui::ComboBox::from_id_salt("object-kind")
            .selected_text(if state.table.is_empty() {
                "Spatial objects"
            } else {
                state.table.trim_start_matches("_Instance_")
            })
            .width(ui.available_width() - 16.)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.table, String::new(), "Spatial objects");
                if let Some(s) = &state.session {
                    for (kind, _) in data::object(&s.records) {
                        ui.selectable_value(
                            &mut state.table,
                            kind.clone(),
                            kind.trim_start_matches("_Instance_"),
                        );
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui.button("+ Add").clicked() {
                if state.table.is_empty() || state.table == data::GEOMETRY {
                    state.tab = 1;
                } else if let Some(s) = &state.session {
                    state.add_kind = state.table.clone();
                    match scene::new_record(s, &state.add_kind) {
                        Ok(v) => {
                            state.add_json =
                                serde_json::to_string_pretty(&scene::friendly(&v)).unwrap();
                            state.add_open = true;
                        }
                        Err(e) => {
                            state.message = e.to_string();
                            state.error = true;
                        }
                    }
                }
            }
            if ui.button("Duplicate").clicked() {
                actions.push(Action::Duplicate);
            }
            if ui.button("Delete").clicked() {
                actions.push(Action::Delete);
            }
        });
    }
    ui.add(
        egui::TextEdit::singleline(&mut state.search)
            .hint_text("Search name, model, GUID…")
            .desired_width(f32::INFINITY),
    );
    let search = state.search.to_lowercase();
    let rows: Vec<_> = state
        .rows
        .iter()
        .filter(|r| {
            r.search.contains(&search)
                && if state.tab == 2 {
                    matches!(r.kind.as_str(), data::LIGHT | data::AMBIENCE)
                } else if state.table.is_empty() {
                    state.session.as_ref().is_some_and(|s| {
                        scene::position(&r.kind, &s.records[&r.kind][&r.id]).is_some()
                    })
                } else {
                    r.kind == state.table
                }
        })
        .collect();
    ui.small(format!(
        "{} records · {} active",
        rows.len(),
        rows.iter().filter(|r| r.active).count()
    ));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, 48., rows.len(), |ui, range| {
            for i in range {
                let row = rows[i];
                ui.push_id(&row.key, |ui| {
                    ui.horizontal(|ui| {
                        state.thumbs.image(
                            ui,
                            &row.model,
                            &row.appearance,
                            Some(&row.key),
                            40.,
                            previews,
                            textures,
                            images,
                        );
                        ui.vertical(|ui| {
                            let label = if row.active {
                                egui::RichText::new(&row.label)
                            } else {
                                egui::RichText::new(&row.label).weak()
                            };
                            if ui
                                .selectable_label(state.selected.as_ref() == Some(&row.key), label)
                                .on_hover_text(&row.model)
                                .clicked()
                            {
                                actions.push(Action::Select(row.key.clone()));
                            }
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}{}",
                                    row.kind.trim_start_matches("_Instance_"),
                                    if row.active { "" } else { " · event inactive" }
                                ))
                                .size(10.)
                                .weak(),
                            );
                        });
                    });
                });
            }
        });
}
pub(super) fn assets(
    ui: &mut egui::Ui,
    state: &mut State,
    previews: &mut previews::Previews,
    textures: &mut EguiUserTextures,
    images: &mut Assets<Image>,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.heading("Project assets");
        if ui.small_button("Refresh").clicked() {
            state.assets = scan_assets(&state.project);
            state.thumbs = Thumbnails::new(&state.project);
        }
    });
    ui.add(
        egui::TextEdit::singleline(&mut state.asset_search)
            .hint_text("Search asset names and paths…")
            .desired_width(f32::INFINITY),
    );
    let folders: BTreeSet<_> = state
        .assets
        .iter()
        .filter_map(|a| a.split_once('/').map(|(a, _)| a))
        .collect();
    egui::ComboBox::from_id_salt("asset-folder")
        .selected_text(if state.asset_folder.is_empty() {
            "All folders"
        } else {
            &state.asset_folder
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut state.asset_folder, String::new(), "All folders");
            for folder in folders {
                ui.selectable_value(&mut state.asset_folder, folder.into(), folder);
            }
        });
    {
        ui.horizontal(|ui| {
            state.thumbs.image(
                ui,
                &state.asset_selected,
                &json!({}),
                None,
                96.,
                previews,
                textures,
                images,
            );
            ui.vertical(|ui| {
                ui.label(if state.asset_selected.is_empty() {
                    "Select an asset to preview"
                } else {
                    state.asset_selected.rsplit('/').next().unwrap_or("")
                });
                if ui
                    .add_enabled(
                        !state.asset_selected.is_empty(),
                        egui::Button::new("+ Add to level"),
                    )
                    .clicked()
                {
                    actions.push(Action::AddAsset(state.asset_selected.clone()));
                }
            });
        });
    }
    ui.add(
        egui::TextEdit::singleline(&mut state.asset_selected)
            .hint_text("Asset reference (also editable offline)")
            .desired_width(f32::INFINITY),
    );
    let search = state.asset_search.to_lowercase();
    let rows: Vec<_> = state
        .assets
        .iter()
        .filter(|a| {
            a.to_lowercase().contains(&search)
                && (state.asset_folder.is_empty()
                    || a.starts_with(&format!("{}/", state.asset_folder)))
        })
        .collect();
    ui.small(format!("{} assets", rows.len()));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, 48., rows.len(), |ui, range| {
            for i in range {
                let model = rows[i];
                ui.push_id(model, |ui| {
                    ui.horizontal(|ui| {
                        state.thumbs.image(
                            ui,
                            model,
                            &json!({}),
                            None,
                            40.,
                            previews,
                            textures,
                            images,
                        );
                        ui.vertical(|ui| {
                            let r = ui.selectable_label(
                                state.asset_selected == *model,
                                model.rsplit('/').next().unwrap(),
                            );
                            if r.clicked() {
                                state.asset_selected = model.clone();
                            }
                            if r.double_clicked() {
                                actions.push(Action::AddAsset(model.clone()));
                            }
                            ui.label(
                                egui::RichText::new(model.split('/').next().unwrap_or(""))
                                    .size(10.)
                                    .weak(),
                            );
                        });
                    });
                });
            }
        });
}
