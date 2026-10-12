//! Persistent lighting profiles and partial overrides.
//!
//! Original ambience assets remain the baseline. A missing override means
//! "keep the original value"; it does not mean zero or a default color.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SCHEMA_VERSION: u32 = 1;
const NATIVE_STORE_FILE: &str = "lighting-presets.json";
const WEB_STORAGE_KEY: &str = "dsor.lighting-presets.v1";

/// The editor's working overrides and named snapshots.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LightingStudioStore {
    pub schema_version: u32,
    pub active_preset_id: Option<String>,
    pub next_preset_id: u64,

    /// Unsaved or currently edited overrides, keyed by map asset name.
    pub working_map_overrides: BTreeMap<String, MapLightingOverrides>,

    /// Stable internal IDs keep presets usable even after being renamed.
    pub presets: BTreeMap<String, LightingPreset>,
}

impl Default for LightingStudioStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_preset_id: None,
            next_preset_id: 1,
            working_map_overrides: BTreeMap::new(),
            presets: BTreeMap::new(),
        }
    }
}

/// Snapshot of the existing global F2 controls.
/// Values match the defaults of `lighting::Lighting`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LightingControls {
    pub sun: f32,
    pub back: f32,
    pub ambient: f32,
    pub lights: f32,
    pub exposure: f32,
    pub fog: f32,
    pub bloom: f32,
    pub saturation: f32,
    pub vignette: f32,
    pub tonemapping: bool,
    pub shadows: bool,
}

impl Default for LightingControls {
    fn default() -> Self {
        Self {
            sun: 1.0,
            back: 1.0,
            ambient: 1.0,
            lights: 1.0,
            exposure: 0.0,
            fog: 1.0,
            bloom: 1.0,
            saturation: 1.0,
            vignette: 1.0,
            tonemapping: false,
            shadows: true,
        }
    }
}

/// A named snapshot of global controls and all custom map overrides.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LightingPreset {
    pub name: String,
    pub controls: LightingControls,
    pub map_overrides: BTreeMap<String, MapLightingOverrides>,
}

impl Default for LightingPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            controls: LightingControls::default(),
            map_overrides: BTreeMap::new(),
        }
    }
}

/// Partial changes for one map: its global ambience and optional local areas.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct MapLightingOverrides {
    pub global: BubbleOverrides,
    pub areas: Vec<AreaLightingOverride>,
}

/// Optional fields let untouched properties continue to come from the
/// original ambience data. Color arrays use the source's RGBA convention.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct BubbleOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_opposite_color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_intensity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub back_light_factor: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_ambient: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<[f32; 3]>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom_scale: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom_color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bright_pass_threshold: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_intensity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_near_dist: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_far_dist: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_light_color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_light_intensity: Option<f32>,

    /// The original ambience format has no player-light radius field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_light_range: Option<f32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette_intensity: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette_size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette_color: Option<[f32; 4]>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fade_time: Option<f32>,
}

/// Identifies a source ambience bubble as well as its array index. The extra
/// identity fields help detect mismatches if a different cache changes bubble
/// ordering. A missing value leaves that original property unchanged.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct AreaLightingOverride {
    pub bubble_index: usize,
    pub event: i32,
    pub priority: i32,
    pub shape: String,
    pub transform: Option<[f32; 16]>,
    pub values: BubbleOverrides,
}

/// Resolve original ambience values with map-wide overrides followed by a
/// matching local-area override. The source asset is never modified.
pub fn resolve_bubble(
    source: &crate::lighting::Bubble,
    map: Option<&MapLightingOverrides>,
    area: Option<(usize, &crate::lighting::LocalBubble)>,
) -> crate::lighting::Bubble {
    let mut resolved = source.clone();
    let Some(map) = map else {
        return resolved;
    };

    apply_bubble_overrides(&mut resolved, &map.global);

    if let Some((index, source_area)) = area {
        if let Some(area_override) = map
            .areas
            .iter()
            .find(|candidate| candidate.matches_source(index, source_area))
        {
            apply_bubble_overrides(&mut resolved, &area_override.values);
        }
    }

    resolved
}

fn apply_bubble_overrides(
    target: &mut crate::lighting::Bubble,
    values: &BubbleOverrides,
) {
    if let Some(v) = values.light_color { target.light_color = v; }
    if let Some(v) = values.light_opposite_color { target.light_opposite_color = v; }
    if let Some(v) = values.light_intensity { target.light_intensity = v; }
    if let Some(v) = values.back_light_factor { target.back_light_factor = v; }
    if let Some(v) = values.light_ambient { target.light_ambient = v; }
    if let Some(v) = values.dir { target.dir = Some(v); }

    if let Some(v) = values.saturation { target.saturation = v; }
    if let Some(v) = values.bloom_scale { target.bloom_scale = v; }
    if let Some(v) = values.bloom_color { target.bloom_color = v; }
    if let Some(v) = values.bright_pass_threshold { target.bright_pass_threshold = v; }

    if let Some(v) = values.fog_color { target.fog_color = v; }
    if let Some(v) = values.fog_intensity { target.fog_intensity = v; }
    if let Some(v) = values.fog_near_dist { target.fog_near_dist = v; }
    if let Some(v) = values.fog_far_dist { target.fog_far_dist = v; }

    if let Some(v) = values.local_light_color { target.local_light_color = v; }
    if let Some(v) = values.local_light_intensity { target.local_light_intensity = v; }
    if let Some(v) = values.local_light_range { target.local_light_range = v; }

    if let Some(v) = values.vignette_intensity { target.vignette_intensity = v; }
    if let Some(v) = values.vignette_size { target.vignette_size = v; }
    if let Some(v) = values.vignette_color { target.vignette_color = v; }
    if let Some(v) = values.fade_time { target.fade_time = v; }
}

impl AreaLightingOverride {
    /// Don't apply an area override to a different bubble merely because it
    /// occupies the same index in another cache.
    fn matches_source(
        &self,
        index: usize,
        source: &crate::lighting::LocalBubble,
    ) -> bool {
        self.bubble_index == index
            && self.event == source.event
            && self.priority == source.priority
            && self.shape == source.shape
            && self.transform == source.m
    }
}
impl LightingStudioStore {
    /// Load local presets. A bad or unsupported file is preserved on disk;
    /// the editor starts with defaults and reports the problem in the log.
    pub fn load() -> Self {
        let raw = match read_store_text() {
            Ok(Some(raw)) => raw,
            Ok(None) => return Self::default(),
            Err(error) => {
                log::warn!("lighting presets: could not read store: {error}");
                return Self::default();
            }
        };

        match serde_json::from_str::<Self>(&raw) {
            Ok(mut store) if store.schema_version <= SCHEMA_VERSION => {
                // Older files can gain newly added defaulted fields.
                store.schema_version = SCHEMA_VERSION;
                store
            }
            Ok(store) => {
                log::warn!(
                    "lighting presets: store version {} is newer than supported version {}; leaving the file untouched",
                    store.schema_version,
                    SCHEMA_VERSION
                );
                Self::default()
            }
            Err(error) => {
                log::warn!("lighting presets: invalid JSON; leaving the file untouched: {error}");
                Self::default()
            }
        }
    }

    /// Persist the working overrides and all named profiles.
    pub fn save(&mut self) -> Result<(), String> {
        self.schema_version = SCHEMA_VERSION;
        let raw = serde_json::to_string_pretty(self)
            .map_err(|error| format!("could not encode lighting presets: {error}"))?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::write(NATIVE_STORE_FILE, raw)
                .map_err(|error| format!("could not write {NATIVE_STORE_FILE}: {error}"))
        }

        #[cfg(target_arch = "wasm32")]
        {
            let window = web_sys::window()
                .ok_or_else(|| "browser window is unavailable".to_string())?;
            let storage = window
                .local_storage()
                .map_err(|error| format!("could not access local storage: {error:?}"))?
                .ok_or_else(|| "browser local storage is unavailable".to_string())?;
            storage
                .set_item(WEB_STORAGE_KEY, &raw)
                .map_err(|error| format!("could not save lighting presets: {error:?}"))
        }
    }

    /// Create a snapshot of the current controls and working map overrides.
    pub fn save_as(
        &mut self,
        name: &str,
        controls: &LightingControls,
    ) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("preset name cannot be empty".to_string());
        }

        while self
            .presets
            .contains_key(&format!("preset-{:04}", self.next_preset_id))
        {
            self.next_preset_id = self
                .next_preset_id
                .checked_add(1)
                .ok_or_else(|| "no preset IDs remain".to_string())?;
        }

        let id = format!("preset-{:04}", self.next_preset_id);
        self.next_preset_id = self.next_preset_id.saturating_add(1);

        self.presets.insert(
            id.clone(),
            LightingPreset {
                name: name.to_string(),
                controls: controls.clone(),
                map_overrides: self.working_map_overrides.clone(),
            },
        );
        self.active_preset_id = Some(id.clone());
        Ok(id)
    }

    /// Select a saved profile and replace the working map overrides.
    /// The caller applies the returned global controls to the live renderer.
    pub fn select_preset(&mut self, id: &str) -> Option<LightingPreset> {
        let preset = self.presets.get(id)?.clone();
        self.working_map_overrides = preset.map_overrides.clone();
        self.active_preset_id = Some(id.to_string());
        Some(preset)
    }

    pub fn overwrite_active(
        &mut self,
        controls: &LightingControls,
    ) -> Result<(), String> {
        let id = self
            .active_preset_id
            .clone()
            .ok_or_else(|| "no active preset is selected".to_string())?;
        let map_overrides = self.working_map_overrides.clone();
        let preset = self
            .presets
            .get_mut(&id)
            .ok_or_else(|| "the active preset no longer exists".to_string())?;
        preset.controls = controls.clone();
        preset.map_overrides = map_overrides;
        Ok(())
    }

    pub fn rename_preset(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("preset name cannot be empty".to_string());
        }
        let preset = self
            .presets
            .get_mut(id)
            .ok_or_else(|| "preset not found".to_string())?;
        preset.name = name.to_string();
        Ok(())
    }

    /// Delete a saved snapshot; working settings are deliberately preserved.
    pub fn delete_preset(&mut self, id: &str) -> bool {
        let removed = self.presets.remove(id).is_some();
        if removed && self.active_preset_id.as_deref() == Some(id) {
            self.active_preset_id = None;
        }
        removed
    }

    pub fn active_preset_is_modified(&self, controls: &LightingControls) -> bool {
        let Some(id) = self.active_preset_id.as_deref() else {
            return false;
        };
        let Some(preset) = self.presets.get(id) else {
            return true;
        };
        preset.controls != *controls
            || preset.map_overrides != self.working_map_overrides
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_store_text() -> Result<Option<String>, String> {
    match std::fs::read_to_string(NATIVE_STORE_FILE) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {NATIVE_STORE_FILE}: {error}")),
    }
}

#[cfg(target_arch = "wasm32")]
fn read_store_text() -> Result<Option<String>, String> {
    let Some(window) = web_sys::window() else {
        return Ok(None);
    };
    let storage = window
        .local_storage()
        .map_err(|error| format!("could not access local storage: {error:?}"))?;
    let Some(storage) = storage else {
        return Ok(None);
    };
    storage
        .get_item(WEB_STORAGE_KEY)
        .map_err(|error| format!("could not read lighting presets: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_store_round_trips_partial_map_overrides() {
        let mut store = LightingStudioStore::default();
        store.working_map_overrides.insert(
            "a0200_kingscity".to_string(),
            MapLightingOverrides {
                global: BubbleOverrides {
                    fog_far_dist: Some(72.0),
                    local_light_range: Some(11.5),
                    ..Default::default()
                },
                areas: vec![AreaLightingOverride {
                    bubble_index: 2,
                    event: -1,
                    priority: 100,
                    shape: "Box".to_string(),
                    transform: None,
                    values: BubbleOverrides {
                        vignette_intensity: Some(0.03),
                        ..Default::default()
                    },
                }],
            },
        );

        let mut controls = LightingControls::default();
        controls.vignette = 0.03;
        let id = store.save_as("Kingshill test", &controls).unwrap();

        let encoded = serde_json::to_string_pretty(&store).unwrap();
        let restored: LightingStudioStore = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored, store);

        let preset = restored.presets.get(&id).unwrap();
        assert_eq!(preset.controls.vignette, 0.03);
        assert_eq!(
            preset.map_overrides["a0200_kingscity"].global.fog_far_dist,
            Some(72.0)
        );
        assert_eq!(
            preset.map_overrides["a0200_kingscity"].areas[0]
                .values.vignette_intensity,
            Some(0.03)
        );
    }

    #[test]
    fn omitted_override_fields_stay_unset() {
        let override_values: BubbleOverrides = serde_json::from_str("{}").unwrap();
        assert_eq!(override_values.fog_far_dist, None);
        assert_eq!(override_values.light_color, None);
        assert_eq!(override_values.local_light_range, None);
    }

    #[test]
    fn map_and_area_overrides_resolve_in_order_without_mutating_assets() {
        use crate::lighting::{Bubble, LocalBubble};

        let mut global_source = Bubble::default();
        global_source.fog_far_dist = 50.0;
        let original_global = global_source.clone();

        let mut area_look = Bubble::default();
        area_look.fog_far_dist = 60.0;
        let transform = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            4.0, 5.0, 6.0, 1.0,
        ];
        let source_area = LocalBubble {
            look: area_look.clone(),
            m: Some(transform),
            priority: 100,
            event: -1,
            shape: "Box".to_string(),
        };

        let map = MapLightingOverrides {
            global: BubbleOverrides {
                fog_far_dist: Some(90.0),
                local_light_range: Some(12.0),
                light_color: Some([0.2, 0.3, 0.4, 1.0]),
                ..Default::default()
            },
            areas: vec![AreaLightingOverride {
                bubble_index: 0,
                event: -1,
                priority: 100,
                shape: "Box".to_string(),
                transform: Some(transform),
                values: BubbleOverrides {
                    fog_far_dist: Some(44.0),
                    vignette_intensity: Some(0.03),
                    ..Default::default()
                },
            }],
        };

        let global = resolve_bubble(&global_source, Some(&map), None);
        assert_eq!(global.fog_far_dist, 90.0);
        assert_eq!(global.local_light_range, 12.0);
        assert_eq!(global.light_color, [0.2, 0.3, 0.4, 1.0]);

        let resolved_area = resolve_bubble(
            &source_area.look,
            Some(&map),
            Some((0, &source_area)),
        );
        assert_eq!(resolved_area.fog_far_dist, 44.0);
        assert_eq!(resolved_area.vignette_intensity, 0.03);
        assert_eq!(resolved_area.local_light_range, 12.0);

        // Source ambience stays unchanged.
        assert_eq!(global_source.fog_far_dist, original_global.fog_far_dist);
        assert_eq!(source_area.look.fog_far_dist, 60.0);
    }

    #[test]
    fn selecting_a_preset_restores_its_working_overrides() {
        let mut store = LightingStudioStore::default();
        store.working_map_overrides.insert(
            "a0200_kingscity".to_string(),
            MapLightingOverrides {
                global: BubbleOverrides {
                    fog_near_dist: Some(12.0),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let id = store
            .save_as("Test", &LightingControls::default())
            .unwrap();

        store.working_map_overrides.clear();
        let selected = store.select_preset(&id).unwrap();

        assert_eq!(
            selected.map_overrides["a0200_kingscity"].global.fog_near_dist,
            Some(12.0)
        );
        assert_eq!(
            store.working_map_overrides["a0200_kingscity"].global.fog_near_dist,
            Some(12.0)
        );
        assert!(!store.active_preset_is_modified(&selected.controls));
    }
}