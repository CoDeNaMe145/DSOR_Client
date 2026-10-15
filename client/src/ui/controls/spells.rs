//! Database-ordered spell cards and pointer-owned hotbar edits. The server owns
//! availability; saving a slot always sends the complete release261 bar.
use super::*;
use dsor_proto::commands::{inventory::{QuickSlot as Slot, QuickSlots, SkillBookEntry}, ClientCommand};

const BAR_SIZE: usize = 17;
const VISIBLE_SLOTS: usize = 7;

#[derive(Clone, Deserialize, Debug)]
pub(in crate::ui) struct Spell {
    pub id: String,
    pub wire: Option<u32>,
    pub class_id: u8,
    pub unlock: u32,
    pub name: String,
    pub icon: String,
}

#[derive(Clone)]
pub(super) struct Model {
    pub class: u8,
    pub offline: bool,
    pub ready: bool,
    pub book: HashMap<u32, SkillBookEntry>,
    pub bar: Vec<Option<String>>,
}

impl Default for Model {
    fn default() -> Self {
        Self { class: 1, offline: true, ready: true, book: default(), bar: crate::net::default_bar(1) }
    }
}

impl Model {
    pub fn available(&self, spell: &Spell) -> bool {
        spell.class_id == self.class && spell.wire.is_some_and(|wire| self.offline ||
            self.book.get(&wire).is_some_and(|entry| entry.owned && !entry.unknown_flag_1))
    }
}

pub(in crate::ui) fn sync(
    net: Option<NonSend<crate::net::Net>>,
    players: Query<&crate::character::Character, With<crate::net::LocalPlayer>>,
    mut windows: Query<&mut WindowControls>,
) {
    let class = players.iter().next().map(|c| c.desc.class)
        .or_else(|| net.as_ref().and_then(|n| n.offline_pending.as_ref().map(|c| c.class))).unwrap_or(1);
    let model = if let Some(net) = net {
        Model { class, offline: net.offline,
            ready: net.offline || net.quickbars.bars.first().is_some_and(|b| b.len() == BAR_SIZE),
            book: net.skill_book.as_ref().map(|b| b.entries.iter().map(|e| (e.skill, *e)).collect()).unwrap_or_default(),
            bar: net.bar.clone() }
    } else { Model { class, ..default() } };
    for mut c in &mut windows { c.spells = model.clone(); }
}

pub(super) struct DragSpell {
    root: Entity,
    button: usize,
    origin: Vec2,
    pub active: bool,
    spell: Spell,
    source: Option<usize>,
}

pub(super) struct DropSpell {
    spell: Spell,
    source: Option<usize>,
    target: usize,
}

/// Returns true when release belongs to a drag, so it cannot also cast a spell.
pub(super) fn drag(pointer: &mut Pointer, windows: &Windows, mouse: &ButtonInput<MouseButton>,
    hit: Option<(Entity, usize)>, point: Option<Vec2>, allowed: bool) -> bool {
    let bar = windows.iter().find(|(_, n, _, v, _)| n.0 == "bottombar" && **v != Visibility::Hidden);
    let editable = allowed && bar.is_some_and(|(_, _, c, _, _)| !c.locked && c.spells.ready);
    if mouse.just_pressed(MouseButton::Left) {
        pointer.spell_drag = None;
        if editable {
            if let (Some((root, button)), Some(point)) = (hit, point) {
                let (_, _, c, _, _) = windows.get(root).unwrap();
                let b = &c.buttons[button];
                let (spell, source) = match b.action {
                    Action::Spell => (b.spell.clone().filter(|s| c.spells.available(s)), None),
                    Action::Skill(slot) => {
                        let id = c.spells.bar.get(slot).and_then(|id| id.as_ref());
                        let meta = windows.iter().flat_map(|(_, _, c, _, _)| c.buttons.iter())
                            .filter_map(|b| b.spell.as_ref()).find(|s| Some(&s.id) == id && s.class_id == c.spells.class);
                        (meta.cloned(), Some(slot))
                    }
                    _ => (None, None),
                };
                if let Some(spell) = spell {
                    pointer.spell_drag = Some(DragSpell { root, button, origin: point, active: false, spell, source });
                }
            }
        }
    }
    if pointer.spell_drag.as_ref().is_some_and(|d| !editable || !windows.get(d.root).is_ok_and(|(_, _, c, v, _)|
        *v != Visibility::Hidden && c.spells.class == d.spell.class_id && c.section_visible(c.buttons[d.button].section) && c.enabled(&c.buttons[d.button]))) {
        pointer.spell_drag = None;
        pointer.down = None;
    }
    if let Some(d) = &mut pointer.spell_drag {
        if point.is_some_and(|p| p.distance(d.origin) >= 4.0) { d.active = true; }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(d) = pointer.spell_drag.take() {
            if d.active {
                if let Some((root, button)) = hit {
                    let (_, name, c, _, _) = windows.get(root).unwrap();
                    if name.0 == "bottombar" && editable {
                        if let Action::Skill(target) = c.buttons[button].action {
                            pointer.spell_drop = Some(DropSpell { spell: d.spell, source: d.source, target });
                        }
                    }
                }
                pointer.down = None;
                return true;
            }
        }
    }
    if !mouse.pressed(MouseButton::Left) { pointer.spell_drag = None; }
    false
}

/// Refuse incomplete snapshots and stale source slots rather than replacing
/// unseen item/essence slots with guessed empty entries.
fn edited_bar(slots: &[Slot], drop: &DropSpell) -> Option<Vec<Slot>> {
    if slots.len() != BAR_SIZE || drop.target >= VISIBLE_SLOTS { return None; }
    let mut edited = slots.to_vec();
    if let Some(source) = drop.source {
        if source >= VISIBLE_SLOTS || slots[source].kind != 0 || slots[source].id.as_ref() != Some(&drop.spell.id) { return None; }
        edited.swap(source, drop.target);
    } else {
        edited[drop.target] = Slot { kind: 0, id: Some(drop.spell.id.clone()) };
    }
    (edited != slots).then_some(edited)
}

pub(in crate::ui) fn apply_drop(mut pointer: ResMut<Pointer>, net: Option<NonSendMut<crate::net::Net>>) {
    let Some(drop) = pointer.spell_drop.take() else { return };
    let Some(mut net) = net else { return };
    if drop.source.is_none() && !net.offline && !drop.spell.wire.is_some_and(|wire|
        net.skill_book.as_ref().is_some_and(|b| b.entries.iter().any(|e| e.skill == wire && e.owned && !e.unknown_flag_1))) { return; }
    if net.offline && net.quickbars.bars.is_empty() {
        let bar = (0..BAR_SIZE).map(|i| {
            let id = net.bar.get(i).cloned().flatten();
            Slot { kind: if id.is_some() { 0 } else { Slot::EMPTY }, id }
        }).collect();
        net.quickbars.bars.push(bar);
    }
    let Some(slots) = net.quickbars.bars.first().and_then(|bar| edited_bar(bar, &drop)) else { return };
    net.bar = slots.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
    net.quickbars.bars[0] = slots.clone();
    net.send(&ClientCommand::QuickSlots(QuickSlots { slots, bar: 0 }));
}

#[derive(Component)]
pub(in crate::ui) struct SpellOverlay;

/// This layer has no input region. It follows the cursor in reference pixels,
/// including while a window is moved or the viewport changes size.
pub(in crate::ui) fn overlay(
    mut commands: Commands, pointer: Res<Pointer>, windows: Query<(&WindowControls, &Visibility)>,
    primary: Query<&Window, With<PrimaryWindow>>, screen: Res<Screen>, assets: Res<AssetServer>,
    old: Query<Entity, With<SpellOverlay>>, mut last: Local<Option<(String, bool, u32)>>,
    mut transforms: Query<&mut Transform, With<SpellOverlay>>,
) {
    let window = primary.single().ok();
    let point = window.filter(|w| w.focused && w.height() > 0.0)
        .and_then(|w| w.cursor_position().map(|p| p * (BASE_H / w.height())));
    let dragged = pointer.spell_drag.as_ref().filter(|d| d.active).map(|d| &d.spell);
    let hovered = windows.iter().filter(|(_, v)| **v != Visibility::Hidden).find_map(|(c, _)|
        c.hovered.and_then(|i| c.buttons[i].spell.as_ref().map(|s| (s, c.spells.available(s)))));
    let description = dragged.map(|s| (s, true)).or(hovered);
    let key = description.map(|(s, available)| (format!("{}:{}", s.id, available), dragged.is_some(), screen.scale.to_bits()));
    let position = point.map(|p| if dragged.is_some() { p } else { Vec2::new((p.x + 20.0).min(screen.width - 265.0).max(0.0), (p.y + 20.0).min(BASE_H - 75.0)) });
    if *last != key || point.is_none() {
        for e in &old { commands.entity(e).despawn(); }
        *last = if point.is_some() { key } else { None };
        if let (Some((s, available)), Some(p)) = (description, position) {
            let tf = Transform::from_translation(screen.world(0.0, p.x, p.y).extend(800.0));
            if dragged.is_some() {
                commands.spawn((SpellOverlay, Sprite { image: assets.load(s.icon.clone()), custom_size: Some(Vec2::splat(34.0)), ..default() },
                    tf, RenderLayers::layer(LAYER)));
            } else {
                let hint = if s.wire.is_none() { "Passive talent".to_owned() } else if available { "Drag to the action bar".to_owned() }
                    else { format!("Level {} — unavailable", s.unlock) };
                let root = commands.spawn((SpellOverlay, tf, Visibility::Inherited)).id();
                commands.spawn((Sprite::from_color(Color::srgba(0.035, 0.025, 0.02, 0.96), Vec2::new(260.0, 66.0)),
                    Anchor::TOP_LEFT, RenderLayers::layer(LAYER), ChildOf(root)));
                commands.spawn((Text2d::new(format!("{}\n{}", s.name, hint)), TextFont { font: assets.load("fonts/tahoma.ttf").into(),
                    font_size: bevy::text::FontSize::Px(13.0 * TEXT_SCALE * screen.scale), ..default() }, TextColor(Color::srgb(1.0, 0.88, 0.62)),
                    bevy::text::TextBounds::new_horizontal(244.0 * screen.scale), Anchor::TOP_LEFT,
                    Transform::from_xyz(8.0, -7.0, 1.0).with_scale(Vec3::splat(1.0 / screen.scale)), RenderLayers::layer(LAYER), ChildOf(root)));
            }
        }
    } else if let Some(p) = position {
        for mut tf in &mut transforms { tf.translation = screen.world(0.0, p.x, p.y).extend(800.0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spell() -> Spell { Spell { id: "magicmissile".into(), wire: Some(42), class_id: 1, unlock: 1, name: "Magic missile".into(), icon: String::new() } }

    #[test]
    fn availability_uses_the_server_book_and_class_not_just_level() {
        let mut model = Model { offline: false, ..default() };
        assert!(!model.available(&spell()));
        model.book.insert(42, SkillBookEntry { skill: 42, owned: true, unknown_flag_1: false, unknown_flag_2: false });
        assert!(model.available(&spell()));
        model.book.get_mut(&42).unwrap().unknown_flag_1 = true;
        assert!(!model.available(&spell()));
        model.offline = true;
        model.class = 2;
        assert!(!model.available(&spell()));
    }

    #[test]
    fn replacing_and_swapping_preserve_unseen_slots_and_reject_stale_drags() {
        let mut slots = vec![Slot { kind: -1, id: None }; BAR_SIZE];
        slots[0] = Slot { kind: 0, id: Some("magicmissile".into()) };
        slots[2] = Slot { kind: 1, id: Some("healing_potion".into()) };
        slots[16] = Slot { kind: 2, id: Some("essence".into()) };
        let drop = DropSpell { spell: spell(), source: None, target: 3 };
        let edited = edited_bar(&slots, &drop).unwrap();
        assert_eq!(edited[3], slots[0]);
        for i in 0..BAR_SIZE { if i != 3 { assert_eq!(edited[i], slots[i]); } }
        let swap = DropSpell { source: Some(0), target: 2, ..drop };
        let edited = edited_bar(&slots, &swap).unwrap();
        assert_eq!(edited[0], slots[2]);
        assert_eq!(edited[2], slots[0]);
        assert_eq!(edited[16], slots[16]);
        assert!(edited_bar(&edited, &swap).is_none());
        assert!(edited_bar(&slots[..7], &swap).is_none());
    }

    #[test]
    fn applying_a_drop_updates_the_display_and_preserves_other_bars() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<Pointer>().add_systems(Update, apply_drop);
        crate::net::start_offline(app.world_mut(), 1, 0);
        let mut slots = vec![Slot { kind: Slot::EMPTY, id: None }; BAR_SIZE];
        slots[16] = Slot { kind: 2, id: Some("essence".into()) };
        let other = vec![Slot { kind: 1, id: Some("item".into()) }; BAR_SIZE];
        app.world_mut().non_send_resource_mut::<crate::net::Net>().quickbars.bars = vec![slots.clone(), other.clone()];
        app.world_mut().resource_mut::<Pointer>().spell_drop = Some(DropSpell { spell: spell(), source: None, target: 3 });
        app.update();
        let net = app.world().non_send_resource::<crate::net::Net>();
        assert_eq!(net.bar[3].as_deref(), Some("magicmissile"));
        assert_eq!(net.quickbars.bars[0].len(), BAR_SIZE);
        assert_eq!(net.quickbars.bars[0][16], slots[16]);
        assert_eq!(net.quickbars.bars[1], other);
        assert!(app.world().resource::<Pointer>().spell_drop.is_none());
    }
}
