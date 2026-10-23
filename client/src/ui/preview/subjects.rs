//! What the stages show: an album sticker, through what its item shows once
//! used (crate::gameplay::systems::StickerLook, tools/systems_catalog.py), and
//! the player's Shadow Soul.
//!
//! A sticker shows the look of its item for the player's class (the original's
//! StickerbookWidget::FilterStickerbookItemsByPlayerClass): a costume on the
//! player, an emote played by the player, the player riding a mount
//! (characters/mounts.json), a pet (characters/pets.json), a Shadow Soul's
//! monster (characters/monster_templates.json). A decoration (a trail, a
//! ground marker, an aura, wings) only starts effects, whose particles are
//! drawn outside the actors' hierarchy: the album shows its icon.
//! The player's Shadow Soul is the one whose effect runs on the player
//! (minion_mercenary_*, Net::stat_effects): the server summons it from that
//! effect (DSOR_Server services/customization.py `_activate_mercenary`).

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Deserialize;

use super::{Look, Model, Subject};
use crate::actors::appearance::Equipment;
use crate::actors::monsters::{json_loader, MonsterTemplates};
use crate::actors::mounts::MountTable;
use crate::gameplay::systems::{StickerLook, SystemsCatalog};

/// A costume dresses the player as a worn item does, on a slot no item uses:
/// the base clothes and items its skin part covers are culled by its mask
/// (crate::actors::character's cull_covered_parts), as the original's
/// gfxskintags hide them; its `_female` variant is chosen for a woman.
pub(super) const COSTUME_SLOT: u8 = 0xFE;

/// No dye (a model's colours as authored).
const UNDYED: [[f32; 4]; 2] = [[1.0, 1.0, 1.0, 0.0], [1.0, 1.0, 1.0, 0.0]];

/// The pets' models (characters/pets.json): only what a preview needs.
#[derive(Asset, TypePath, Deserialize, Default)]
pub struct PetModels {
    #[serde(default)]
    pets: HashMap<String, PetModel>,
}

#[derive(Deserialize, Clone)]
struct PetModel {
    graphics: String,
    set: String,
    anim_set: String,
    #[serde(default = "undyed")]
    dye: [[f32; 4]; 2],
}

fn undyed() -> [[f32; 4]; 2] { UNDYED }

json_loader!(PetModelsLoader, PetModels, "pets.json");

/// The tables a subject's model comes from (the same handles as the actors'
/// plugins: loaded once).
#[derive(Resource)]
pub(crate) struct Data {
    mounts: Handle<MountTable>,
    pets: Handle<PetModels>,
    monsters: Handle<MonsterTemplates>,
}

pub(super) fn load(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Data {
        mounts: assets.load("characters/mounts.json"),
        pets: assets.load("characters/pets.json"),
        monsters: assets.load("characters/monster_templates.json"),
    });
}

/// What the subjects are resolved from.
#[derive(SystemParam)]
pub(crate) struct Tables<'w> {
    data: Option<Res<'w, Data>>,
    state: Res<'w, crate::ui::controls::systems::State>,
    catalogs: Res<'w, Assets<SystemsCatalog>>,
    mounts: Res<'w, Assets<MountTable>>,
    pets: Res<'w, Assets<PetModels>>,
    monsters: Res<'w, Assets<MonsterTemplates>>,
}

impl Tables<'_> {
    /// Which tables have loaded, a bit each: a subject waiting for one is
    /// resolved again when it arrives.
    pub(super) fn ready(&self) -> u8 {
        let Some(data) = &self.data else { return 0 };
        u8::from(self.catalogs.contains(&self.state.catalog)) | u8::from(self.mounts.contains(&data.mounts)) << 1
            | u8::from(self.pets.contains(&data.pets)) << 2 | u8::from(self.monsters.contains(&data.monsters)) << 3
    }

    fn model(&self, look: &StickerLook) -> Option<Model> {
        let data = self.data.as_ref()?;
        if !look.mount.is_empty() {
            let m = self.mounts.get(&data.mounts)?.0.get(&look.mount)?;
            return Some(Model { graphics: m.graphics.clone(), set: m.set.clone(), anim_set: m.anim_set.clone(), dye: m.dye });
        }
        if !look.pet.is_empty() {
            let p = self.pets.get(&data.pets)?.pets.get(&look.pet)?;
            return Some(Model { graphics: p.graphics.clone(), set: p.set.clone(), anim_set: p.anim_set.clone(), dye: p.dye });
        }
        if !look.monster.is_empty() {
            let t = self.monsters.get(&data.monsters)?.0.get(&look.monster)?;
            return Some(Model { graphics: t.graphics.clone(), set: t.set.clone(), anim_set: t.anim_set.clone(), dye: t.dye });
        }
        None
    }

    /// The album's sticker, as the player would wear, play, ride or keep it.
    pub(super) fn sticker(&self, id: &str, player: Option<&Look>) -> Option<Subject> {
        let def = self.catalogs.get(&self.state.catalog)?.stickers.get(id)?;
        let look = for_class(&def.looks, player.map(|p| p.class))?;
        resolve(look, player, self.model(look))
    }

    /// The player's Shadow Soul: the one whose effect runs on the player.
    pub(super) fn shadow_soul(&self, net: &crate::network::net::Net) -> Option<Subject> {
        let catalog = self.catalogs.get(&self.state.catalog)?;
        let look = catalog.stickers.values().flat_map(|s| s.looks.iter())
            .filter(|l| !l.monster.is_empty())
            .find(|l| l.status.is_some_and(|status| net.stat_effects.values().any(|e| u32::from(e.index) == status)))?;
        resolve(look, None, self.model(look))
    }
}

/// A sticker's look for a class: the item for it, else one for every class.
pub(super) fn for_class(looks: &[StickerLook], class: Option<u8>) -> Option<&StickerLook> {
    let name = class.and_then(|c| crate::actors::character::CLASSES.get(c as usize).copied());
    looks.iter().find(|l| !l.class.is_empty() && Some(l.class.as_str()) == name)
        .or_else(|| looks.iter().find(|l| l.class.is_empty()))
}

/// What a look shows: the player in a costume or playing an emote, the player
/// riding a mount (its model), a pet or a Shadow Soul (its model). None for a
/// decoration, or when the player or the model it needs is missing.
pub(super) fn resolve(look: &StickerLook, player: Option<&Look>, model: Option<Model>) -> Option<Subject> {
    if !look.skin.is_empty() {
        let mut dressed = player?.clone();
        dressed.equipment.push(Equipment { slot: COSTUME_SLOT, skin_parts: vec![look.skin.clone()], colour: None });
        return Some(Subject::Character { look: dressed, state: None, emote: false, mount: None });
    }
    if !look.animation.is_empty() {
        return Some(Subject::Character { look: player?.clone(), state: Some(look.animation.clone()), emote: true, mount: None });
    }
    if !look.mount.is_empty() {
        return Some(Subject::Character { look: player?.clone(), state: None, emote: false, mount: Some(model?) });
    }
    if !look.pet.is_empty() {
        return Some(Subject::Model { model: model?, state: None });
    }
    // A Shadow Soul at rest: its Idle is its battle stance
    // (retinue_ancient_watcher_01_idle_bat); a set without Idle_Hub plays Idle.
    if !look.monster.is_empty() {
        return Some(Subject::Model { model: model?, state: Some("Idle_Hub".into()) });
    }
    None
}
