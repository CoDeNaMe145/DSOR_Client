//! One click-through hover layer for navigation, skills and populated HUD slots.
use super::*;

#[derive(Component)]
pub(in crate::ui) struct Tooltip;

fn title(id: &str) -> String {
    let mut out = String::new();
    for (i,c) in id.chars().enumerate() {
        if c == '_' { out.push(' '); }
        else { if i > 0 && c.is_uppercase() { out.push(' '); } out.push(c); }
    }
    out.trim().to_owned()
}

fn window_title(name: &str) -> &str {
    match name { "skilltalentswindow" => "Skills", "charactersheet" => "Character", "collectorsalbum" => "Collector's album",
        "questlog" => "Quests", "locationmap" => "Map overlay", "travelmap" => "World map", "socialscreen" => "Social",
        "shopwindow" => "Shop", "matchmakingwindow" => "Battlegrounds", "gamemenu" => "Game menu", "leaderboards" => "Rankings",
        "eventwindow" => "Events", "inventory" => "Inventory", _ => name }
}

/// Icon buttons need names; text buttons already explain their destination.
fn repeats_label(window: &str, button: &Button, layout: Option<&Layout>) -> bool {
    if window == "gamemenu" { return true; }
    match &button.action {
        Action::Skill(_) | Action::Spell | Action::Item(_) => false,
        Action::NpcChoice(_) | Action::Edit(_) | Action::Choose(_, _) | Action::ChatChannel(_) => true,
        Action::Settings(event) => event.starts_with("row") || matches!(event.as_str(), "previous" | "next"),
        _ => layout.is_some_and(|layout| layout.widgets.iter().any(|w|
            !w.hidden && (w.id == button.id || w.id.starts_with(&format!("{}/", button.id)))
                && w.text.as_ref().is_some_and(|t| t.color[3] > 0.0 && t.text != "Text"
                    && t.text.chars().filter(|c| c.is_alphabetic()).count() > 1))),
    }
}

pub(super) fn clean_text(raw: &str) -> String {
    // Localized descriptions carry the old renderer's colour/format tags.
    let mut out = String::new(); let mut tag = false;
    for c in raw.replace("\\n", "\n").chars() {
        if c == '<' { tag = true; } else if c == '>' { tag = false; }
        else if !tag { out.push(c); }
    }
    // Formula placeholders require a stat/talent evaluator. Never display them
    // as if they were a computed damage figure.
    out.lines().filter(|s| !s.contains('{') && !s.contains('$')).collect::<Vec<_>>().join("\n")
}

#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn overlay(mut commands: Commands, windows: Query<(&Window2018, &WindowControls, &Visibility, &Transform), Without<Tooltip>>,
    primary: Query<&Window, With<PrimaryWindow>>, screen: Res<Screen>, settings: Res<settings::Settings>, pointer: Res<Pointer>,
    net: Option<NonSend<crate::network::net::Net>>, hotbar: Res<hotbar::Hotbar>,
    (data, catalogs, dialogue): (Res<items::Data>, Res<Assets<items::Catalog>>, Res<dialogue::Presentation>), layouts: Res<Assets<Layout>>, loaded: Res<Loaded>,
    skill_tables: Res<Assets<crate::gameplay::skills::SkillTable>>, assets: Res<AssetServer>,
    (old, mut transforms): (Query<Entity, With<Tooltip>>, Query<&mut Transform, With<Tooltip>>),
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<UiMaterial>>),
    mut last: Local<Option<(String, String, Option<String>, u32)>>) {
    let point = primary.single().ok().filter(|w| w.focused).and_then(|w| w.cursor_position().map(|p| p*(BASE_H/w.height().max(1.0))));
    let enabled = settings.current.tooltips && pointer.down.is_none() && pointer.drag.is_none() && pointer.spell_drag.is_none();
    let catalog = catalogs.get(&data.catalog);
    let mut content: Option<(String,String,Option<String>)> = None;
    let mut anchor = None;
    if enabled && point.is_some() {
        let hovered = windows.iter().filter(|(_,_,v,_)| **v != Visibility::Hidden)
            .find_map(|(w,c,_,tf)| c.hovered.filter(|i| !matches!(c.buttons[*i].action, Action::Item(_))).map(|i| (w,c,&c.buttons[i],tf)));
        if let Some((w,c,b,tf)) = hovered.filter(|(w,_,b,_)| !repeats_label(&w.0,b,
            loaded.layouts.iter().filter_map(|(h,_,_)| layouts.get(h)).find(|l| l.window == w.0))) {
            let [l,t,r,bottom] = b.rect;
            let offset = window_offset(tf) + Vec2::new(b.k*(screen.width-BASE_W),0.0);
            anchor = Some([l+offset.x,t+offset.y,r+offset.x,bottom+offset.y]);
            let mut heading = String::new(); let mut body = String::new(); let mut icon = None;
            let mut spell = b.spell.as_ref();
            match &b.action {
                Action::Skill(index) => {
                    heading = match *index { 17 => "Primary essence".into(), 18 => "Backup essence".into(),
                        _ => settings::shortcuts().iter().find(|s| s.0 == format!("slot{index}")).map_or_else(|| "Action slot".into(), |s| s.1.clone()) };
                    if *index < 8 {
                        let id = c.spells.bar.get(*index).and_then(|s| s.as_ref());
                        spell = windows.iter().flat_map(|(_,c,_,_)| &c.buttons).filter_map(|b| b.spell.as_ref())
                            .find(|s| Some(&s.id) == id && s.class_id == c.spells.class);
                    }
                    if let Some(net) = &net {
                        if *index >= hotbar::PRIMARY_ESSENCE {
                            if let Some(item) = hotbar::essence(&net.inventory,*index) {
                                if let Some(catalog) = catalog {
                                    let name = catalog.items.get(&item.template).map_or(item.template.as_str(), |d| d.name.as_str());
                                    body = format!("{name}\nQuantity: {}",item.third);
                                    icon = catalog.icon(&item.template,Some(item)).0.map(str::to_owned);
                                }
                            } else { body = "Empty. Equip an essence from your inventory.".into(); }
                            body.push_str(&format!("\nSwitch essences: {}",settings.current.label("essence")));
                        } else if let Some(slot) = hotbar::slot(net, hotbar.active,*index) {
                            if slot.kind == 0 {
                                spell = windows.iter().flat_map(|(_,c,_,_)| &c.buttons).filter_map(|b| b.spell.as_ref())
                                    .find(|s| slot.id.as_ref() == Some(&s.id));
                            } else if let (Some(id),Some(catalog)) = (&slot.id,catalog) {
                                let item = hotbar::item(slot,&net.inventory);
                                let template = item.map_or(id.as_str(),|i| i.template.as_str());
                                if let Some(meta) = catalog.items.get(template) {
                                    heading = meta.name.clone(); body = clean_text(&meta.description);
                                    icon = catalog.icon(template,item).0.map(str::to_owned);
                                    if slot.kind == 1 { body.push_str(&format!("\nQuantity: {}",item.map_or(0,|i| i.third))); }
                                }
                            } else { body = "Empty slot".into(); }
                        }
                    }
                    if spell.is_none() && *index < 17 { body.push_str(&format!("\nPress {}", settings.current.slot_label(*index))); }
                }
                Action::Spell => {}
                Action::NpcChoice(index) => { heading = dialogue.choice_label(*index).unwrap_or_default().into(); }
                Action::Item(_) => return, // Inventory owns its detailed item tooltip.
                Action::Window(name) => { heading = window_title(name).into(); let key = if *name == "gamemenu" { "Esc".into() } else { settings.current.label(name) }; if key != "Unbound" { body = format!("(Press {key})"); } }
                Action::Settings(event) => {
                    heading = match event.as_str() { "open" => "Game Settings", "controls" => "Hotkeys", "general" => "General", "graphics" => "Graphics", "audio" => "Audio", "cursor" => "Mouse", "combatSettings" => "Combat", "apply" => "Apply and save", "cancel" => "Discard changes", "revert" => "Restore defaults", _ => "Settings" }.into();
                    if let Some(row) = event.strip_prefix("row").and_then(|s| s.parse::<usize>().ok()) {
                        if let Some((_,label,value)) = settings.rows().get(settings.page*settings::ROWS+row) { heading = label.clone(); body = value.clone(); }
                    }
                    if event == "revert" { body = "Restore default preferences and hotkeys. Apply saves them; Cancel keeps your current settings.".into(); }
                }
                Action::Lock => { heading = if c.locked { "Unlock action bar" } else { "Lock action bar" }.into(); body = "Lock prevents dragging skills out of their slots.".into(); }
                Action::Essence => { heading = "Switch primary and backup essence".into(); body = format!("Press {}",settings.current.label("essence")); }
                Action::Bar(delta) => { heading = if *delta<0 { "Previous action bar" } else { "Next action bar" }.into(); body = format!("Press {}",settings.current.label(if *delta<0 { "previous_bar" } else { "next_bar" })); }
                Action::Fullscreen => heading = "Toggle fullscreen".into(),
                Action::Membership => { heading = "Membership".into(); body = if net.as_ref().is_some_and(|n| n.inventory.premium) { "Membership active. Open the membership page for details." } else { "Open the membership page." }.into(); }
                Action::Close => { heading = "Close".into(); body = "Press Esc".into(); }
                _ => {
                    // Prefer the source's localized button text, then a readable ID.
                    heading = if w.0 == "tradingscreen" {
                        match b.id.rsplit('/').next().unwrap_or("") {
                            "BuyTabBtn" => "Buy", "RestoreTabBtn" => "Buy back",
                            "PrevPageBtn" => "Previous page", "NextPageBtn" => "Next page", _ => "",
                        }.into()
                    } else { loaded.layouts.iter().filter_map(|(h,_,_)| layouts.get(h)).find(|l| l.window == w.0)
                        .and_then(|l| l.widgets.iter().find(|x| x.id.starts_with(&format!("{}/",b.id)) && x.text.as_ref().is_some_and(|t| !t.text.is_empty() && t.text != "Text")))
                        .and_then(|w| w.text.as_ref()).map_or_else(|| title(b.id.rsplit('/').next().unwrap_or("")), |t| t.text.clone()) };
                    if !c.enabled(b) { body = "Currently unavailable".into(); }
                }
            }
            if let Some(s) = spell {
                heading = s.name.clone(); icon = Some(s.icon.clone()); body = clean_text(&s.description);
                if !s.target.is_empty() { body.push_str(&format!("\nTarget: {}",s.target)); }
                if let Some(def) = skill_tables.iter().find_map(|(_,t)| t.0.values().find(|d| d.id == s.id)) {
                    let resource = ["Rage", "Mana", "Concentration", "Steam"].get(s.class_id as usize).unwrap_or(&"Resource");
                    // Release261 stores costs as fractions of maximum resource;
                    // talent modifiers are applied by the server when casting.
                    body.push_str(&format!("\nBase cost: {}% maximum {resource}\nEssence costs: {}\nCooldown: {} s\nRange: {} m",(def.resource_cost*1000.0).round()/10.0,def.ammo_cost,def.cooldown,def.range));
                }
                if !c.spells.available(s) { body.push_str(&format!("\nRequired level: {}",s.unlock)); }
                if let Action::Skill(index) = b.action { body.push_str(&format!("\nPress {}",settings.current.slot_label(index))); }
                else if s.wire.is_some() { body.push_str("\nDrag to the action bar"); }
                else { body.push_str("\nPassive talent"); }
            }
            if !heading.is_empty() { content = Some((heading,body.trim().to_owned(),icon)); }
        } else if let Some(p) = point {
            // Non-button gauges and currency amounts also explain themselves.
            if let Some((w,_,_,tf)) = windows.iter().filter(|(_,c,v,tf)| **v != Visibility::Hidden && c.covers(p-window_offset(tf),screen.width))
                .max_by(|a,b|a.3.translation.z.total_cmp(&b.3.translation.z)) {
                if let Some(layout) = loaded.layouts.iter().filter_map(|(h,_,_)| layouts.get(h)).find(|l|l.window==w.0) {
                    for widget in layout.widgets.iter().rev().filter(|x| !x.hidden && contains(x.k,x.rect,p-window_offset(tf),screen.width)) {
                        let tip = if widget.id.contains("andermant") { Some("Andermant") } else if widget.id.contains("VirtualCurrency") { Some("Coins: 100 copper = 1 silver; 100 silver = 1 gold") }
                        else if widget.id.contains("bottombar_xp_bar") { Some("Experience: earn experience to level up") } else if widget.id.contains("bottombar_wisdom_bar") { Some("Ancient Wisdom") }
                        else { match gauge_of(&widget.id) { Some(Gauge::Health | Gauge::HealthBar) => Some("Health"), Some(Gauge::Resource | Gauge::ResourceBar) => Some("Skill resource"), _ => None } };
                        if let Some(tip) = tip { content=Some((tip.into(),String::new(),None)); break; }
                    }
                }
            }
        }
    }
    let key = content.as_ref().map(|(h,b,i)| (h.clone(),b.clone(),i.clone(),screen.scale.to_bits()));
    let compact = content.as_ref().is_some_and(|(_,_,icon)| icon.is_none());
    let mut lines = Vec::new();
    let (width,height) = if let Some((heading,body,_)) = content.as_ref().filter(|_| compact) {
        // Small icon hints use the same Tahoma metrics as chat, with just enough
        // padding for the original border. A two-line hint is about 60 x 48.
        let measure = |text: &str| crate::network::chat::text_width(text,12.0*TEXT_SCALE);
        let width = (heading.lines().chain(body.lines()).map(measure).fold(0.0,f32::max).ceil()+24.0).clamp(60.0,220.0);
        for line in heading.lines().chain(body.lines()) {
            lines.extend(crate::network::chat::wrap_with(line,&|row| measure(row) <= width-24.0));
        }
        (width,24.0+lines.len() as f32*12.0)
    } else {
        (340.0,content.as_ref().map_or(50.0, |(_,b,_)| 65.0+b.lines().map(|l| l.chars().count().div_ceil(45).max(1)).sum::<usize>() as f32*16.0).min(BASE_H-16.0))
    };
    let at = point.map(|p| {
        let (x,y) = if compact {
            let [l,t,r,b] = anchor.unwrap_or([p.x,p.y,p.x,p.y]);
            ((l+r-width)*0.5,if b+10.0+height <= BASE_H-8.0 { b+10.0 } else { t-height-10.0 })
        } else { (p.x+18.0,p.y-height-12.0) };
        Vec2::new(x.clamp(8.0,(screen.width-width-8.0).max(8.0)),y.clamp(8.0,(BASE_H-height-8.0).max(8.0)))
    });
    if *last == key { if let Some(at)=at { for mut tf in &mut transforms { tf.translation=screen.world(0.0,at.x,at.y).extend(850.0); } } return; }
    *last=key; for e in &old { commands.entity(e).despawn(); }
    let (Some((heading,body,icon)),Some(at))=(content,at) else{return;};
    let root=commands.spawn((Tooltip,Transform::from_translation(screen.world(0.0,at.x,at.y).extend(850.0)),Visibility::Inherited)).id();
    commands.spawn((Sprite::from_color(Color::srgba(0.025,0.02,0.015,0.97),Vec2::new(width,height)),Anchor::TOP_LEFT,RenderLayers::layer(LAYER),ChildOf(root)));
    if let Some(layout)=layouts.get(&data.tooltip) { items::frame(&mut commands,&assets,&mut meshes,&mut materials,root,layout,width,height); }
    if compact {
        for (i,line) in lines.into_iter().enumerate() {
            let color = if line.starts_with("(Press ") || line.starts_with("Press ") { Color::srgb(1.0,0.9,0.0) } else { Color::srgb(0.93,0.9,0.83) };
            commands.spawn((Text2d::new(line),TextFont { font:assets.load("fonts/tahoma.ttf").into(),font_size:bevy::text::FontSize::Px(12.0*TEXT_SCALE*screen.scale),..default() },
                TextColor(color),TextLayout::justify(Justify::Center),Anchor::TOP_LEFT,bevy::text::TextBounds::new_horizontal((width-24.0)*screen.scale),
                Transform::from_xyz(12.0,-12.0-i as f32*12.0,1.0).with_scale(Vec3::splat(1.0/screen.scale)),RenderLayers::layer(LAYER),ChildOf(root)));
        }
        return;
    }
    let title_x = if let Some(icon)=icon { commands.spawn((Sprite{image:assets.load(icon),custom_size:Some(Vec2::splat(28.0)),..default()},Anchor::TOP_LEFT,Transform::from_xyz(14.0,-12.0,1.0),RenderLayers::layer(LAYER),ChildOf(root))); 51.0 } else {16.0};
    items::text(&mut commands,&assets,&screen,root,heading,Vec3::new(title_x,-15.0,1.0),17.0,Color::srgb(1.0,0.88,0.62),width-title_x-14.0);
    items::text(&mut commands,&assets,&screen,root,body,Vec3::new(16.0,-49.0,1.0),15.0,Color::srgb(0.93,0.9,0.83),width-32.0);
}
