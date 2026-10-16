//! Floating combat texts over actors: damage numbers, criticals, "Parade !"...
//!
//! What and when: HitCommand's handler 0x516453 (2018 client), for a blow whose
//! victim or attacker is the local player (or the attacker one of its own, not
//! traced here), victim and attacker different:
//!   blocked              BlockEnemy (2) / BlockSelf (3, the local player hit),
//!                        text floatingBlock;
//!   then, unless immune: DamageDone (13) / DamageReceived (14) with the damage, or
//!                        on a critical CriticalDone (4) / CriticalReceived (5),
//!                        text floatingCritical ("{0:i} !") with the damage;
//!   immune:              DamageImmunityOther (26) / DamageImmunitySelf (25);
//!   kind 10:             LowHealthHit (20) / LowHealthHitReceived (21), lowhealthhit;
//!   combat value > 0 for the local player's own blow: HealingSelf (6) /
//!                        HealingOther (7), the damage times the combat value.
//! How: `_Template_CombatText` per type (tools/export_combat_text.py): colour,
//!   duration with its fade-out, base offsets, velocity, size. The overhead text
//!   window's adder 0x58A8F8 picks the side at random: the velocity's sign
//!   (and the variation's) flips half the time; with DynamicXOffsetEnabled the x
//!   offset too (0x5880FC).
//! UNVERIFIED: offsets and velocities are taken as world units (x along the
//!   screen, y up) from the actor's feet, as the names; the size
//!   as points at the 1040-pixel layout height, as NameLabel's.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct TextType {
    pub color: [f32; 4],
    pub duration: f32,
    pub fade: f32,
    pub offset_y: [f32; 3],
    pub offset_x: [f32; 3],
    pub offset_x_var: f32,
    pub dynamic_x: bool,
    pub velocity: [f32; 3],
    pub velocity_var: f32,
    pub size: f32,
    /// Its overhead label (DamageLabel, CriticalDamageLabel...).
    #[serde(default)]
    pub label: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct Texts {
    pub block: String,
    pub critical: String,
    pub immunity: String,
    pub low_health: String,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct CombatTextTable {
    pub types: HashMap<String, TextType>,
    pub texts: Texts,
    /// The overhead labels' own animations (tools/export_combat_text.py).
    #[serde(default)]
    pub labels: HashMap<String, LabelAnim>,
}

/// A label's UI animation: one key per `dt` seconds.
/// EVIDENCE: ui/overheadtext.bxml: DamageLabel AnimRes anims:ui/overheadtext.nax3,
///   AnimGroup 3, AnimDuration 880 (CriticalDamageLabel 2/760, BlockLabel 1/1040,
///   HealingLabel 0/1640, ImmunityLabel 4/760, InterceptLabel 5/1040); the clip
///   moves it (translation y), pops it (scale 0.8 -> 1.8 -> 1.06) and fades it
///   (the scale curve's w, ~1 -> 0 over the clip).
#[derive(Deserialize, Debug, Clone)]
pub struct LabelAnim {
    pub dt: f32,
    pub y: Vec<f32>,
    pub scale: Vec<f32>,
    pub alpha: Vec<f32>,
}

impl LabelAnim {
    fn duration(&self) -> f32 {
        self.dt * self.alpha.len().max(self.y.len()).max(1) as f32
    }
    fn at(keys: &[f32], dt: f32, t: f32, default: f32) -> f32 {
        if keys.is_empty() {
            return default;
        }
        let k = (t / dt.max(1e-4)).max(0.0);
        let i = (k.floor() as usize).min(keys.len() - 1);
        let j = (i + 1).min(keys.len() - 1);
        keys[i] + (keys[j] - keys[i]) * (k - i as f32).min(1.0)
    }
}

#[derive(Default, TypePath)]
pub struct CombatTextLoader;

impl AssetLoader for CombatTextLoader {
    type Asset = CombatTextTable;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, r: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<CombatTextTable, Self::Error> {
        let mut b = Vec::new();
        r.read_to_end(&mut b).await?;
        serde_json::from_slice(&b).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] {
        &["combat_text.json"]
    }
}

/// CombatTextType, as the client numbers it (TemplateManager's name lookup 0x5102C9).
pub const BLOCK_ENEMY: u8 = 2;
pub const BLOCK_SELF: u8 = 3;
pub const CRITICAL_DONE: u8 = 4;
pub const CRITICAL_RECEIVED: u8 = 5;
pub const HEALING_SELF: u8 = 6;
pub const HEALING_OTHER: u8 = 7;
pub const DAMAGE_DONE: u8 = 13;
pub const DAMAGE_RECEIVED: u8 = 14;
pub const LOW_HEALTH_HIT: u8 = 20;
pub const LOW_HEALTH_HIT_RECEIVED: u8 = 21;
pub const DAMAGE_IMMUNITY_SELF: u8 = 25;
pub const DAMAGE_IMMUNITY_OTHER: u8 = 26;

fn type_name(t: u8) -> &'static str {
    match t {
        BLOCK_ENEMY => "BlockEnemy",
        BLOCK_SELF => "BlockSelf",
        CRITICAL_DONE => "CriticalDone",
        CRITICAL_RECEIVED => "CriticalReceived",
        HEALING_SELF => "HealingSelf",
        HEALING_OTHER => "HealingOther",
        DAMAGE_DONE => "DamageDone",
        DAMAGE_RECEIVED => "DamageReceived",
        LOW_HEALTH_HIT => "LowHealthHit",
        LOW_HEALTH_HIT_RECEIVED => "LowHealthHitReceived",
        DAMAGE_IMMUNITY_SELF => "DamageImmunitySelf",
        DAMAGE_IMMUNITY_OTHER => "DamageImmunityOther",
        _ => "",
    }
}

/// What the text says.
pub enum Say {
    Damage(i32),
    Critical(i32),
    Block,
    Immunity,
    LowHealth,
    Healing(i32),
}

/// Texts to show, queued by crate::actors::monsters: (over this actor, type, what).
#[derive(Resource, Default)]
pub struct CombatTexts(pub Vec<(Entity, u8, Say)>);

#[derive(Resource)]
struct Table(Handle<CombatTextTable>);

/// One text in flight.
#[derive(Component)]
struct Floating {
    /// World point it started from, and its motion.
    from: Vec3,
    offset: Vec2,
    velocity: Vec2,
    age: f32,
    duration: f32,
    fade: f32,
    color: [f32; 4],
    /// Its label's animation, when it has one: then it drives the motion, the
    /// size and the fade.
    anim: Option<LabelAnim>,
}

const LAYOUT_HEIGHT: f32 = 1040.0;

const BOX: f32 = 300.0;

pub struct CombatTextPlugin;

impl Plugin for CombatTextPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<CombatTextTable>()
            .register_asset_loader(CombatTextLoader)
            .init_resource::<CombatTexts>()
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Table(assets.load("interface/combat_text.json")));
            })
            .add_systems(PostUpdate, (spawn_texts, move_texts).chain().after(TransformSystems::Propagate));
    }
}

/// A coin toss, as the adder's `rand() * (1 / RAND_MAX) + ...` rounded to 0 or 1.
fn heads(seed: &mut u32) -> bool {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed & 1 == 0
}

#[allow(clippy::too_many_arguments)]
fn spawn_texts(
    mut commands: Commands,
    mut queue: ResMut<CombatTexts>,
    table: Res<Table>,
    tables: Res<Assets<CombatTextTable>>,
    actors: Query<&GlobalTransform>,
    assets: Res<AssetServer>,
    mut font: Local<Option<Handle<Font>>>,
    mut seed: Local<u32>,
) {
    if queue.0.is_empty() {
        return;
    }
    let Some(table) = tables.get(&table.0) else { return };
    if *seed == 0 {
        *seed = 0x9E37_79B9;
    }
    // EVIDENCE: ui/overheadtext.bxml: every combat label is font "tahoma" (its
    //   "bold" flag set); the 2018 client draws them thin (recorded: names, map
    //   name, money), so the regular face: Wine's Tahoma (assets/fonts/tahoma.ttf,
    //   copied by hand, not shipped with the game data). FAILURE: Tahoma Bold
    //   was "trop grasse".
    let font = font.get_or_insert_with(|| assets.load("fonts/tahoma.ttf")).clone();
    for (actor, kind, say) in std::mem::take(&mut queue.0) {
        let Some(t) = table.types.get(type_name(kind)) else { continue };
        let Ok(at) = actors.get(actor) else { continue };
        let text = match say {
            Say::Damage(d) | Say::Healing(d) => d.to_string(),
            Say::Critical(d) => table.texts.critical.replace("{0:i}", &d.to_string()),
            Say::Block => table.texts.block.clone(),
            Say::Immunity => table.texts.immunity.clone(),
            Say::LowHealth => table.texts.low_health.clone(),
        };
        // The side: velocity and its variation each flip at random (0x58A8F8), and
        // the dynamic x offset too (0x5880FC).
        let mut vx = if heads(&mut seed) { -t.velocity[0] } else { t.velocity[0] };
        if t.velocity_var != 0.0 {
            vx += if heads(&mut seed) { -t.velocity_var } else { t.velocity_var };
        }
        let mut ox = t.offset_x[0];
        if t.dynamic_x && heads(&mut seed) {
            ox = -ox;
        }
        if t.offset_x_var != 0.0 {
            ox += if heads(&mut seed) { -t.offset_x_var } else { t.offset_x_var };
        }
        let anim = table.labels.get(&t.label).cloned();
        let vh = t.size * 96.0 / 72.0 / LAYOUT_HEIGHT * 100.0;
        commands.spawn((
            Floating {
                // From the actor's feet: the table's OffsetYBase (1.0 for damage)
                // puts it on the body. EVIDENCE: the user's view of the 2018 client
                // ("les degats ... devraient etre sur mon perso pas au dessus");
                // 2.25 over the feet put them over the head.
                from: at.translation(),
                offset: Vec2::new(ox, t.offset_y[1]),
                // UNVERIFIED: with a label animation the table's velocity is not
                //   applied (read as world units it slid the damage sideways, which
                //   the 2018 client does not).
                velocity: if anim.is_some() { Vec2::ZERO } else { Vec2::new(vx, t.velocity[1]) },
                age: 0.0,
                duration: anim.as_ref().map(|a| a.duration()).unwrap_or(t.duration.max(0.1)),
                fade: t.fade,
                color: t.color,
                anim,
            },
            Node { position_type: PositionType::Absolute, width: Val::Px(BOX), justify_content: JustifyContent::Center, ..default() },
            UiTransform::default(),
            Text::new(text),
            TextFont { font: font.clone().into(), font_size: bevy::text::FontSize::Vh(vh), ..default() },
            TextColor(Color::srgba(t.color[0], t.color[1], t.color[2], t.color[3])),
            TextLayout::justify(Justify::Center),
            TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
            Visibility::Hidden,
        ));
    }
}

fn move_texts(
    mut commands: Commands,
    time: Res<Time>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    mut texts: Query<(Entity, &mut Floating, &mut Node, &mut Visibility, &mut TextColor, &mut TextShadow, &mut UiTransform)>,
) {
    let Ok((camera, cam_at)) = cameras.single() else { return };
    let right = cam_at.right();
    let height = camera.logical_viewport_size().map(|s| s.y).unwrap_or(1080.0);
    for (e, mut f, mut node, mut vis, mut color, mut shadow, mut ui) in &mut texts {
        f.age += time.delta_secs();
        if f.age >= f.duration {
            commands.entity(e).despawn();
            continue;
        }
        let o = f.offset + f.velocity * f.age;
        let p = f.from + *right * o.x + Vec3::Y * o.y;
        let Ok(screen) = camera.world_to_viewport(cam_at, p) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        node.left = Val::Px(screen.x - BOX / 2.0);
        let alpha = match &f.anim {
            // The clip: y in the UI's units (the screen's height, y down), its
            // scale, its alpha. UNVERIFIED: the unit, from the bxml rects (0..1).
            Some(a) => {
                let (t, dt) = (f.age, a.dt);
                node.top = Val::Px(screen.y + LabelAnim::at(&a.y, dt, t, 0.0) * height);
                ui.scale = Vec2::splat(LabelAnim::at(&a.scale, dt, t, 1.0));
                LabelAnim::at(&a.alpha, dt, t, 1.0).clamp(0.0, 1.0) * f.color[3]
            }
            // The last `fade` seconds fade it out.
            None => {
                node.top = Val::Px(screen.y);
                let left = f.duration - f.age;
                (if f.fade > 0.0 && left < f.fade { left / f.fade } else { 1.0 }) * f.color[3]
            }
        };
        color.0 = Color::srgba(f.color[0], f.color[1], f.color[2], alpha);
        shadow.color = Color::srgba(0.0, 0.0, 0.0, alpha);
    }
}
