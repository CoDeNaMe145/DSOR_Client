use super::*;
pub(super) fn draw(
    ui: &mut egui::Ui,
    state: &mut State,
    previews: &mut previews::Previews,
    textures: &mut EguiUserTextures,
    images: &mut Assets<Image>,
    actions: &mut Vec<Action>,
) {
    ui.heading("Details");
    let Some(key) = state.selected.clone() else {
        ui.add_space(30.);
        ui.label("Select an object to inspect it");
        ui.small("Edit scene objects, lighting, cameras, and DB4 records.");
        return;
    };
    let Some((kind, id)) = key.split_once('\t') else {
        return;
    };
    if let Some(row) = state.rows.iter().find(|r| r.key == key) {
        ui.horizontal(|ui| {
            state.thumbs.image(
                ui,
                &row.model,
                &row.appearance,
                Some(&key),
                88.,
                previews,
                textures,
                images,
            );
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&row.label).strong());
                ui.small(kind.trim_start_matches("_Instance_"));
                if !row.active {
                    ui.colored_label(
                        egui::Color32::from_rgb(229, 185, 107),
                        "Inactive in this map event state",
                    );
                }
            });
        });
    }
    ui.horizontal(|ui| {
        if ui.button("Apply").clicked() {
            actions.push(Action::Apply);
        }
        if ui.button("Revert override").clicked() {
            actions.push(Action::Revert(key.clone()));
        }
        if state.draft_dirty() {
            ui.small("● pending");
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui|{
        if state.draft.is_null(){ui.label("Record deleted");return;}
        let editable=state.session.as_ref().is_some_and(|s|s.records[kind].get(id).is_some());
        ui.add_enabled_ui(editable,|ui|{
            egui::CollapsingHeader::new("Transform").default_open(true).show(ui,|ui|{if let Some(mut p)=scene::position(kind,&state.draft){if vector(ui,"Position",&mut p,0.1){if let Ok(v)=scene::moved(kind,&state.draft,p){state.draft=v;}}if let Ok(m)=scene::matrix(kind,&state.draft){let(scale,rotation,_)=Mat4::from_cols_array(&m).to_scale_rotation_translation();let(x,y,z)=rotation.to_euler(EulerRot::XYZ);let mut angles=[x.to_degrees(),y.to_degrees(),z.to_degrees()];if vector(ui,"Rotation °",&mut angles,0.5){let q=Quat::from_euler(EulerRot::XYZ,angles[0].to_radians(),angles[1].to_radians(),angles[2].to_radians());if kind==data::GEOMETRY{state.draft["r"]=json!(q.to_array());}else{state.draft["Transform"]=data::float_blob(Mat4::from_scale_rotation_translation(scale,q,Vec3::from(p)).to_cols_array());}}
                if !scene::is_camera(kind){let mut s=scale.to_array();if vector(ui,"Scale",&mut s,0.01){if kind==data::GEOMETRY{state.draft["s"]=json!(s);}else{state.draft["Transform"]=data::float_blob(Mat4::from_scale_rotation_translation(Vec3::from(s),rotation,Vec3::from(p)).to_cols_array());}}}
            }}else{ui.small("This DB4 record has no spatial transform.");}});
            if scene::is_camera(kind){egui::CollapsingHeader::new("Game camera").default_open(true).show(ui,|ui|{ui.small("The monitor shows the in-game lens independently of the editor view.");for k in if kind==data::CAMERA{vec!["FieldOfView","PreviewAspect","NearClip","FarClip"]}else{vec!["CameraVertFieldOfView","CameraAspectRatio"]}{field(ui,k,&mut state.draft);}});}
            if kind==data::LIGHT{egui::CollapsingHeader::new("Light").default_open(true).show(ui,|ui|{for k in ["LightType","LightColor","LightIntensity","LightRange","LightConeAngle","LightCastShadows","LightFlickerEnable","LightFlickerFrequency","LightFlickerIntensity","EventSetRow"]{field(ui,k,&mut state.draft);}ui.small("Cone angle is the full cone in degrees.");});}
            if kind==data::AMBIENCE{for(title,keys)in [("Sun & ambient",vec!["LightColor","LightOppositeColor","LightIntensity","BackLightFactor","LightAmbient","LightShadowIntensity"]),("Fog",vec!["FogColor","HeightFogColor","FogIntensity","FogNearHeight","FogFarHeight","FogNearDist","FogFarDist"]),("Color & bloom",vec!["Saturation","Balance","BloomScale","BloomColor","BrightPassThreshold","VignetteIntensity","VignetteSize","VignetteColor"]),("Volume",vec!["AmbienceBubbleEnabled","AmbienceBubblePriority","PEShapeType","PEFadeTime","EventSetRow"]) ]{egui::CollapsingHeader::new(title).default_open(true).show(ui,|ui|{for key in keys{field(ui,key,&mut state.draft);}});}}
            egui::CollapsingHeader::new("Object properties").default_open(!matches!(kind,data::LIGHT|data::AMBIENCE|data::CAMERA)).show(ui,|ui|{let keys:Vec<_>=data::object(&state.draft).filter(|(k,v)|!matches!(k.as_str(),"Guid"|"Transform"|"p"|"r"|"s")&&!v.is_object()&&!v.is_array()).map(|(k,_)|k.clone()).collect();for k in keys{let stable=state.session.as_ref().is_some_and(|s|data::array(&s.snapshot["schemas"][kind]["keys"]).any(|v|v==&k));ui.add_enabled_ui(!stable,|ui|{field(ui,&k,&mut state.draft);});}});
            egui::CollapsingHeader::new("DB4 / JSON").show(ui,|ui|{if !state.raw_changed{state.raw=serde_json::to_string_pretty(&scene::friendly(&state.draft)).unwrap_or_default();}ui.small("Float32 blobs are editable arrays. Other binary fields retain their original bytes.");if ui.add(egui::TextEdit::multiline(&mut state.raw).code_editor().desired_width(f32::INFINITY).desired_rows(24)).changed(){state.raw_changed=true;}if ui.button("Apply fields").clicked(){actions.push(Action::Apply);}});
        });
        ui.separator();ui.small(id);if let Some(s)=&state.session{ui.small(format!("Source: {}",data::text(&s.sources[data::source_key(kind)])));}
    });
}
fn vector(ui: &mut egui::Ui, label: &str, values: &mut [f32; 3], speed: f64) -> bool {
    ui.label(label);
    let mut changed = false;
    ui.horizontal(|ui| {
        let width = ((ui.available_width() - 30.) / 3.).max(40.);
        for (i, c) in [
            egui::Color32::LIGHT_RED,
            egui::Color32::LIGHT_GREEN,
            egui::Color32::LIGHT_BLUE,
        ]
        .into_iter()
        .enumerate()
        {
            ui.colored_label(c, ["X", "Y", "Z"][i]);
            changed |= ui
                .add_sized(
                    [width - 18., 22.],
                    egui::DragValue::new(&mut values[i]).speed(speed),
                )
                .changed();
        }
    });
    changed
}
fn field(ui: &mut egui::Ui, key: &str, row: &mut Value) {
    let Some(value) = row.get_mut(key) else {
        return;
    };
    ui.push_id(key, |ui| {
        if value.is_object() {
            if let Ok(mut f) = data::floats(value) {
                if f.len() == 4 {
                    ui.label(key);
                    let mut rgb = [f[0], f[1], f[2]];
                    let mut changed = ui.color_edit_button_rgb(&mut rgb).changed();
                    if changed {
                        f[..3].copy_from_slice(&rgb);
                    }
                    ui.horizontal(|ui| {
                        for v in &mut f[..3] {
                            changed |= ui
                                .add(egui::DragValue::new(v).speed(0.01).range(0. ..=32.))
                                .changed();
                        }
                    });
                    if changed {
                        *value = data::float_blob(f);
                    }
                }
            }
            return;
        }
        if matches!(
            key,
            "LightCastShadows"
                | "LightFlickerEnable"
                | "AmbienceBubbleEnabled"
                | "SequenceIsPlaying"
                | "LoopStartAnimation"
        ) || value.is_boolean()
        {
            let mut b = data::flag(value);
            if ui.checkbox(&mut b, key).changed() {
                *value = if value.is_boolean() {
                    json!(b)
                } else {
                    json!(b as i32)
                };
            }
            return;
        }
        if key == "LightType" {
            let mut n = value.as_i64().unwrap_or(0);
            egui::ComboBox::from_id_salt(key)
                .selected_text(if n == 0 {
                    "Point light"
                } else {
                    "Spot / projected light"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut n, 0, "Point light");
                    ui.selectable_value(&mut n, 1, "Spot / projected light");
                });
            *value = json!(n);
            return;
        }
        ui.label(egui::RichText::new(key).small().weak());
        match value {
            Value::String(s) => {
                ui.add(egui::TextEdit::singleline(s).desired_width(f32::INFINITY));
            }
            Value::Number(n) => {
                if let Some(mut i) = n.as_i64() {
                    if ui.add(egui::DragValue::new(&mut i)).changed() {
                        *value = json!(i);
                    }
                } else {
                    let mut f = n.as_f64().unwrap_or(0.);
                    if ui.add(egui::DragValue::new(&mut f).speed(0.05)).changed() {
                        *value = json!(f);
                    }
                }
            }
            Value::Null => {
                ui.small("null");
            }
            _ => {}
        }
    });
}
