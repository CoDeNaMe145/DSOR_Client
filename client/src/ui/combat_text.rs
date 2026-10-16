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
//! Release 261 (dro_client64 0x1401cbd13) adds the server's own type, a string
//!   after the hit (HealingSelf for a potion, DamageDonePoison... for damage
//!   over time, DamageImmunityOther), which every branch takes in place of its
//!   default; and the local player's heals of itself are shown.
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
    /// gui.window lootReceived ("Vous avez reçu : {0:s}") and lootReceivedAmount
    /// ("Vous avez reçu {0:i} {0:s}.").
    #[serde(default)]
    pub loot: String,
    #[serde(default)]
    pub loot_amount: String,
    #[serde(default)]
    pub sudden_death: String,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct CombatTextTable {
    pub types: HashMap<String, TextType>,
    pub texts: Texts,
    /// The overhead labels' own animations (tools/export_combat_text.py).
    #[serde(default)]
    pub labels: HashMap<String, LabelAnim>,
    /// A loot message's currency type -> its name (tools/export_combat_text.py).
    #[serde(default)]
    pub currencies: HashMap<String, String>,
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

pub fn type_name(t: u8) -> &'static str {
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
    /// Already worded (loot received, sudden death).
    Text(String),
}

/// Texts to show, queued by crate::actors::monsters: (over this actor, its
/// `_Template_CombatText` type name, what).
#[derive(Resource, Default)]
pub struct CombatTexts(pub Vec<(Entity, String, Say)>);

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
            .init_resource::<OfflineMessages>()
            .add_systems(Update, offline_demo)
            .add_systems(PostUpdate, (message_texts, spawn_texts, move_texts).chain().after(TransformSystems::Propagate));
    }
}

/// MessageCommand (137) texts over the local player: what it picked up (0x19) and
/// sudden death (0x1d).
/// EVIDENCE (release261 dro_client64, ClientGameWorld::HandleMessageCommand
///   0x1401d0848): case 0x19 (0x1401d09ac) takes four params -- currency type,
///   amount, item, template -- and picks the text type by the currency: 0
///   VirtualCurrency, 1 RealCurrency, 3 and 5 GrindCurrency, 6 and 7 AutoPickUpLoot
///   (the item's own name, from the template), any other none; the text is
///   lootReceivedAmount when the amount is over 1, else lootReceived. Case 0x1d
///   (0x1401d1a23): SuddenDeath, text suddenDeath. Both only with a local player
///   (ClientGameWorld +0x678).
/// UNVERIFIED: the text's anchor, taken as the local player (the message's actor
///   is the player's own in every loot packet the server sends).
#[allow(clippy::too_many_arguments)]
fn message_texts(
    mut net: Option<NonSendMut<crate::net::Net>>,
    mut queue: ResMut<CombatTexts>,
    table: Res<Table>,
    tables: Res<Assets<CombatTextTable>>,
    locals: Query<Entity, With<crate::net::LocalPlayer>>,
    assets: Res<AssetServer>,
    catalogs: Res<Assets<crate::ui::Catalog>>,
    mut catalog: Local<Option<Handle<crate::ui::Catalog>>>,
    mut offline: ResMut<OfflineMessages>,
) {
    let mut pending: Vec<_> = net.as_deref_mut().map(|n| std::mem::take(&mut n.messages)).unwrap_or_default();
    pending.extend(offline.0.drain(..).map(|m| (0, m)));
    if pending.is_empty() {
        return;
    }
    let Some(table) = tables.get(&table.0) else { return };
    let catalog = catalog.get_or_insert_with(|| assets.load("interface/inventory.items.json")).clone();
    let Some(player) = locals.iter().next() else { return };
    for (_, m) in pending {
        match m.message_type {
            0x19 if m.params.len() == 4 => {
                let (Some(currency), Some(amount)) = (m.params[0].int(), m.params[1].int()) else { continue };
                let (kind, name) = match currency {
                    0 => ("VirtualCurrency", table.currencies.get("0").cloned()),
                    1 => ("RealCurrency", table.currencies.get("1").cloned()),
                    3 | 5 => ("GrindCurrency", table.currencies.get(&currency.to_string()).cloned()),
                    6 | 7 => {
                        let template = m.params[3].str().unwrap_or_default();
                        let name = catalogs.get(&catalog).and_then(|c| c.items.get(template)).map(|d| d.name.clone());
                        ("AutoPickUpLoot", Some(name.unwrap_or_else(|| template.to_owned())))
                    }
                    _ => continue,
                };
                let name = name.unwrap_or_default();
                let text = if amount > 1 { &table.texts.loot_amount } else { &table.texts.loot };
                let text = text.replace("{0:i}", &amount.to_string()).replace("{0:s}", &name);
                queue.0.push((player, kind.to_owned(), Say::Text(text)));
            }
            0x1d => queue.0.push((player, "SuddenDeath".to_owned(), Say::Text(table.texts.sudden_death.clone()))),
            _ => {}
        }
    }
}

/// MessageCommands made offline (offline_demo), read as the server's.
#[derive(Resource, Default)]
struct OfflineMessages(Vec<dsor_proto::commands::social::Message>);

/// Offline: DSOR_TEST_TEXTS=<seconds> shows every kind of text over the local
/// player in turn, one each that many seconds: damage taken, critical, block,
/// immunity, a heal, poison and fire damage over time, coins, Andermant, a single
/// coin, sudden death.
fn offline_demo(
    net: Option<NonSend<crate::net::Net>>,
    time: Res<Time>,
    locals: Query<Entity, With<crate::net::LocalPlayer>>,
    mut queue: ResMut<CombatTexts>,
    mut offline: ResMut<OfflineMessages>,
    mut state: Local<(f32, usize)>,
) {
    use dsor_proto::commands::social::{Message, MessageParam};
    // Offline play keeps a Net that connects nowhere.
    if net.is_some_and(|n| !n.offline) {
        return;
    }
    let Some(every) = std::env::var("DSOR_TEST_TEXTS").ok().and_then(|v| v.parse::<f32>().ok()) else { return };
    let Some(player) = locals.iter().next() else { return };
    state.0 -= time.delta_secs();
    if state.0 > 0.0 {
        return;
    }
    state.0 = every;
    let loot = |currency: u32, amount: u32, template: &str| Message {
        message_type: 0x19,
        params: vec![MessageParam::Int(currency), MessageParam::Int(amount), MessageParam::Item(1), MessageParam::Str(template.into())],
    };
    let step = state.1 % 11;
    state.1 += 1;
    let mut text = |kind: &str, say: Say| queue.0.push((player, kind.to_owned(), say));
    match step {
        0 => text("DamageReceived", Say::Damage(87)),
        1 => text("CriticalReceived", Say::Critical(254)),
        2 => text("BlockSelf", Say::Block),
        3 => text("DamageImmunitySelf", Say::Immunity),
        4 => text("HealingSelf", Say::Healing(120)),
        5 => text("DamageDonePoison", Say::Damage(23)),
        6 => text("DamageDoneFire", Say::Damage(31)),
        7 => offline.0.push(loot(0, 57, "money_vc")),
        8 => offline.0.push(loot(1, 3, "money_rc")),
        9 => offline.0.push(loot(0, 1, "money_vc")),
        _ => offline.0.push(Message { message_type: 0x1d, params: vec![] }),
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
        let Some(t) = table.types.get(&kind) else { continue };
        let Ok(at) = actors.get(actor) else { continue };
        let text = match say {
            Say::Damage(d) | Say::Healing(d) => d.to_string(),
            Say::Critical(d) => table.texts.critical.replace("{0:i}", &d.to_string()),
            Say::Block => table.texts.block.clone(),
            Say::Immunity => table.texts.immunity.clone(),
            Say::LowHealth => table.texts.low_health.clone(),
            Say::Text(t) => t,
        };
        // The velocity: the WHOLE vector is negated half the time (0x58AB4B:
        // `xorps xmm1; subps xmm1, velocity` when the first random bit is 0), then
        // the variation is added to or taken from its x (0x58AB62..0x58AB87, the
        // second bit); the dynamic x offset flips on its own (0x5880FC).
        let flip = if heads(&mut seed) { -1.0 } else { 1.0 };
        let mut vx = t.velocity[0] * flip;
        let vy = t.velocity[1] * flip;
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
        // Texts without a label clip (NameLabel: coins, Andermant, wisdom,
        // experience) move by their velocity alone and last their table duration.
        // EVIDENCE: the user's view of the game: "Coins received" rises a little
        // and is gone in ~1.5 s. Read as world units per second over the table's
        // 3.5 s it rose 3.5 units ("monte beaucoup trop et devrait disparaitre
        // plus rapidement").
        // UNVERIFIED: the velocity's unit and the lifetime the client uses
        //   (0x58A8F8 hands both to the element; their consumer was not traced).
        let (scale, lifetime) = if anim.is_none() && t.label == "NameLabel" { (0.25, 1.5) } else { (1.0, t.duration.max(0.1)) };
        let (vx, vy) = (vx * scale, vy * scale);
        let vh = t.size * 96.0 / 72.0 / LAYOUT_HEIGHT * 100.0;
        commands.spawn((
            Floating {
                // From the actor's feet: the table's OffsetYBase (1.0 for damage)
                // puts it on the body. EVIDENCE: the user's view of the 2018 client
                // ("les degats ... devraient etre sur mon perso pas au dessus");
                // 2.25 over the feet put them over the head.
                from: at.translation(),
                offset: Vec2::new(ox, t.offset_y[1]),
                // Moved by it whether or not its label has a clip: the element is
                // given position, velocity and a start time (0x84FBC4: +0x1B0,
                // +0x1C0, +0x1D0), the clip plays on top (its local hop, pop and
                // fade). UNVERIFIED: the velocity's unit, world units per second
                // as the offsets.
                velocity: Vec2::new(vx, vy),
                age: 0.0,
                duration: anim.as_ref().map(|a| a.duration()).unwrap_or(lifetime),
                fade: t.fade.min(lifetime),
                color: t.color,
                anim,
            },
            Node { position_type: PositionType::Absolute, width: Val::Px(BOX), justify_content: JustifyContent::Center, ..default() },
            UiTransform::default(),
            Text::new(text),
            TextFont { font: font.clone().into(), font_size: bevy::text::FontSize::Vh(vh), ..default() },
            TextColor(Color::srgb(t.color[0], t.color[1], t.color[2])),
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
        // CONTRACT: the table colour's alpha is not the text's opacity.
        // EVIDENCE: _Template_CombatText gives VirtualCurrency, RealCurrency,
        //   GrindCurrency, HonorCurrency and Experience an alpha of 0, the
        //   elemental DamageDone* types 2.0; the 2018 client shows the coins it
        //   picks up (recorded). FAILURE (2026-10-16): multiplied in, picked-up
        //   coins were drawn fully transparent ("rien ne s'affiche").
        let alpha = match &f.anim {
            // The clip: its translation y in screen heights, y UP (Nebula's
            // node translation): a small hop (+0.07) then a fall below its start
            // (-0.117). Read y down, every number rose off the screen ("ca monte
            // en l'air"). Its scale pops 0.8 -> 1.8 -> 1.06, its alpha fades.
            // UNVERIFIED: the unit, from the bxml rects (0..1).
            Some(a) => {
                let (t, dt) = (f.age, a.dt);
                node.top = Val::Px(screen.y - LabelAnim::at(&a.y, dt, t, 0.0) * height);
                ui.scale = Vec2::splat(LabelAnim::at(&a.scale, dt, t, 1.0));
                LabelAnim::at(&a.alpha, dt, t, 1.0).clamp(0.0, 1.0)
            }
            // The last `fade` seconds fade it out.
            None => {
                node.top = Val::Px(screen.y);
                let left = f.duration - f.age;
                if f.fade > 0.0 && left < f.fade { left / f.fade } else { 1.0 }
            }
        };
        color.0 = Color::srgba(f.color[0], f.color[1], f.color[2], alpha);
        shadow.color = Color::srgba(0.0, 0.0, 0.0, alpha);
    }
}
