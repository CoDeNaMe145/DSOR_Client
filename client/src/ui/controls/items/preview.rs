//! Item sections follow tooltip.bxml's text, gem, separator and currency
//! templates. Only fields whose wire meaning is known are shown as values.
use super::*;
use crate::ui::controls::tooltips::card::{Card, WHITE, MUTED, BLUE, GOLD, GREEN, plain};

fn number(value: f32, advanced: bool) -> String {
    let s = if advanced || value.abs()<1.0 { format!("{value:.3}") } else { format!("{value:.0}") };
    if s.contains('.') {s.trim_end_matches('0').trim_end_matches('.').to_owned()} else {s}
}

fn attribute(value: &values::Value, advanced: bool) -> String {
    format!("{} {}{} {}{}", if value.amount < 0.0 { "-" } else { "+" },
        number(value.amount.abs()*if value.relative {100.0} else {1.0},advanced),
        if value.relative {"%"} else {""}, value.name.to_lowercase(),
        if value.item_local {" on this item"} else {""})
}

fn readable(id: &str) -> String {
    let mut result = String::new();
    for (i,c) in id.chars().enumerate() {
        if c == '_' { result.push(' '); }
        else { if i > 0 && c.is_uppercase() { result.push(' '); } result.push(c); }
    }
    result.trim().to_owned()
}

/// The sticker card's text against an item card's (Card::scale_text): the
/// original's "Use, is not depleted when used" is 2.04 sticker cells long, ours
/// was 1.57 at 0.48 (the user's clip of the original's White Tiger card and their
/// shot of our Battle-boar's, 2026-10-30).
/// FAILURE (2026-10-29): the two shots of the Gray's card were taken the wrong
///   way round, so the text was made smaller (0.55 -> 0.48) and the card wider.
/// UNVERIFIED: measured from screenshots.
const STICKER_TEXT: f32 = 0.62;
/// The widest a sticker card grows (reference pixels) before its lines wrap,
/// its heading's size and its lines' height (in text sizes).
/// EVIDENCE: the user's recording 2026-10-30 (20261030-1640-20: ours at 2.0 s,
///   the original's Manticore card at 11.0 s, same video scale): the body lines
///   as wide as ours, but the original's heading 0.79 of ours, its lines 23
///   video pixels apart (ours 27), and its description wraps at 760 pixels
///   (278 reference pixels; card 301), where ours ran on to the 360 cap.
const STICKER_MAX_WIDTH: f32 = 312.0;
const STICKER_TITLE: f32 = 0.79;
const STICKER_LEADING: f32 = 1.1;

/// A Collector's Bag sticker's card, as the original's (the user's release261_192
/// shots of the Manticore and the armoured war horse): its name and kind in its
/// rarity's colour, then gui.window consumableUnderTitleInfinite, its description,
/// noSale and howToUseMount / howToUsePet.
/// FAILURE (2026-10-24): a bare "Sticker / Shows it in the preview" line.
pub(super) fn sticker(catalog: &Catalog, layout: Option<&Layout>, template: &str) -> Card {
    let meta = catalog.items.get(template);
    let mut card = Card::new(STICKER_MAX_WIDTH, layout);
    // The original writes it in a small hand, its name no larger than the rest.
    // FAILURE (2026-10-25): about 1.8 times the original's (the user's shots of the
    //   Battle-boar's card, measured against the sticker icon).
    card.scale_text(STICKER_TEXT);
    card.scale_title(STICKER_TITLE);
    card.set_leading(STICKER_LEADING);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    let (rarity, tint) = rarity(catalog, template, None);
    // A template has no instance colour; the original writes a Mythic one red.
    // UNVERIFIED: the exact red, matched by eye on the user's Manticore shot.
    let tint = if rarity == "Mythic" { Color::srgb(0.86, 0.12, 0.08) } else { tint };
    let name = format!("<i>{}</>", catalog.name(template, None));
    let kind = meta.filter(|m| !m.category.is_empty()).map(|m| format!("<i>{}</>", m.category));
    let description = meta.filter(|m| !m.description.is_empty())
        .map(|m| if m.description.contains('<') { m.description.clone() } else { format!("<i>{}</>", m.description) });
    let pet = meta.is_some_and(|m| matches!(m.item_category.as_str(), "Pet" | "Mercenary"));
    let how = if pet { "(Right click to call your Companion over)" } else { "(Right click to mount or equip)" };
    let used = "<i>Use, is not depleted when used</>";
    // As wide as its longest line, as the original's.
    // FAILURE (2026-10-30): a fixed wide card, its right half empty.
    let longest = |text: &str, size: f32| plain(text).lines().map(|l| crate::network::chat::text_width(l, size)).fold(0.0, f32::max);
    let size = card.text_size();
    let body = [kind.as_deref(), Some(used), description.as_deref(), Some("Not for sale"), Some(how)];
    let widest = body.into_iter().flatten().map(|t| longest(t, size)).fold(longest(&name, card.title_size()), f32::max);
    card.width = (widest.ceil() + 36.0).clamp(120.0, STICKER_MAX_WIDTH);
    // A little room before the description and more before "Not for sale"
    // (the original: 29 and 38 video pixels where its lines are 23 apart).
    card.heading(&name, tint);
    if let Some(kind) = &kind { card.text(kind, tint); }
    card.text(used, MUTED);
    card.gap(2.0);
    if let Some(description) = &description { card.text(description, WHITE); }
    card.gap(5.0);
    card.text("Not for sale", WHITE);
    card.text(how, Color::srgb(0.95, 0.85, 0.2));
    card
}

/// An item's rarity and the colour its name is written in.
pub(super) fn rarity<'a>(catalog: &'a Catalog, template: &str, item: Option<&ItemInfo>) -> (&'a str, Color) {
    let rarity = item.map(|i| ["Common", "Improved", "Magic", "Extraordinary", "Legendary", "Unique", "Set", "Mythic"].get(i.kind as usize).copied().unwrap_or("Item"))
        .unwrap_or_else(||catalog.items.get(template).map_or("Item",|m|m.rarity.as_str()));
    let tint = match rarity { "Unique" => Color::srgb(1.0,0.9,0.0), "Set" => GREEN, "Magic" => BLUE,
        "Extraordinary" => Color::srgb(0.75,0.4,1.0), "Legendary" => Color::srgb(1.0,0.65,0.15), _ => color(catalog,item) };
    (rarity, tint)
}

pub(super) fn card(d: &DisplayItem, catalog: &Catalog, layout: Option<&Layout>, advanced: bool) -> Card {
    let meta = catalog.items.get(d.template);
    let mut card = Card::new(if advanced { 370.0 } else { 285.0 },layout);
    card.background = Color::srgba(0.01, 0.01, 0.005, 0.985);
    let (rarity, tint) = rarity(catalog, d.template, d.item);
    card.title(catalog.name(d.template,d.item),None,tint);
    card.text(&format!("{rarity} Item"),tint);
    if let Some(m) = meta.filter(|m|!m.category.is_empty()) { card.text(&format!("<i>({})</>",m.category),MUTED); }
    if let Some(i) = d.item { card.text(&format!("Item level: {}",i.durability),Color::srgb(0.85,0.75,0.5)); }
    if let Some(m) = meta {
        if m.required_level.max(d.unlock)>1 { card.hint(&format!("Required level: {}",m.required_level.max(d.unlock))); }
        if m.max_stack>1 { card.text(&format!("Quantity: {} / {}",d.amount,m.max_stack),WHITE); }
    }

    if let Some(item) = d.item {
        if item.flag_0 { card.section("Identification"); card.text("Unidentified",GOLD); }
        else {
            let values=catalog.values.item(item);
            if !values.base.is_empty() {
                card.section("Base Values");
                if advanced {
                    for value in &values.base {
                        let quality=value.quality.map_or(String::new(),|q|format!(" ({:.0}%)",q*100.0));
                        card.text(&format!("{}{}",attribute(value,true),quality),WHITE);
                    }
                } else {
                    let mut names=std::collections::HashSet::new();
                    for value in &values.base {
                        if !value.item_local || value.relative {card.text(&attribute(value,false),WHITE);}
                        else if names.insert(&value.name) {card.text(&format!("+ <GreenStart>{}</>  {}",number(values.total(&value.name),false),value.name),WHITE);}
                    }
                }
            }
            if item.names.iter().any(|g|!g.is_empty()) || item.tenth>0 {
                card.section("Gems");
                if advanced {
                    let mut groups=std::collections::HashSet::new();
                    for value in &values.gems {
                        if !groups.insert((&value.name,value.relative,value.item_local)) {continue;}
                        let group:Vec<_>=values.gems.iter().filter(|v|v.name==value.name && v.relative==value.relative && v.item_local==value.item_local).collect();
                        let factor=if value.relative {100.0} else {1.0};
                        let details=if group.len()>1 {format!(" ({})",group.iter().map(|v|number(v.amount*factor,true)).collect::<Vec<_>>().join(", "))} else {String::new()};
                        let mut combined=value.clone(); combined.amount=group.iter().map(|v|v.amount).sum();
                        card.text(&format!("{}{}",attribute(&combined,true),details),BLUE);
                    }
                    card.gap(3.0);
                } else {
                    for value in values.gems.iter().filter(|v| !v.item_local || !values.base.iter().any(|b| b.item_local && !b.relative && b.name==v.name)) {
                        card.text(&attribute(value,false),BLUE);
                    }
                }
                // Every socket: its gem, or empty (names holds "" for an empty
                // one; tenth is the item's socket count).
                // FAILURE: "ça affiche pas dans la description d'un items ses
                //   slots et les gems qu'il a equipé".
                let sockets=(item.tenth as usize).max(item.names.len());
                for n in 0..sockets {
                    match item.names.get(n).filter(|g|!g.is_empty()) {
                        Some(gem)=>card.text(&format!("Socket {}: {}",n+1,catalog.items.get(gem).map_or_else(||readable(gem),|g|g.name.clone())),BLUE),
                        None=>card.hint(&format!("Socket {}: empty",n+1)),
                    }
                }
                card.icons(item.names.iter().filter(|g|!g.is_empty()).filter_map(|name|catalog.icon(name,None).0.map(str::to_owned)));
            }
            let enchantments:Vec<_>=values.enchantments.iter().filter(|v| advanced || !v.item_local
                || !values.base.iter().any(|b| b.item_local && !b.relative && b.name==v.name)).collect();
            if !enchantments.is_empty() {
                card.section("Enchantments");
                for value in enchantments {
                    let quality=value.quality.filter(|_|advanced).map_or(String::new(),|q|format!(" ({:.0}%)",q*100.0));
                    let contribution=values.contribution(value).filter(|_|advanced).map_or(String::new(),|v|
                        format!("  <GreenStart>({}{})</>",if v<0.0 {"-"} else {"+"},number(v.abs(),true)));
                    card.text(&format!("{}{}{}",attribute(value,advanced),quality,contribution),BLUE);
                }
            }
            // Unknown enchantments remain readable, but their normalized roll
            // must never masquerade as an absolute item stat.
            if advanced {
                for stat in &item.statistics {
                    if !catalog.values.knows(&stat.name) {
                        let name=catalog.enchantments.get(&stat.name).map_or_else(||readable(&stat.name),|e|e.name.clone());
                        card.hint(&format!("{} (roll: {:.0}%)",name,stat.value.clamp(0.0,1.0)*100.0));
                    }
                }
            }
            if !values.unique.is_empty() {
                card.section("Unique Values");
                for value in &values.unique {card.text(&attribute(value,true),GOLD);}
            } else if let Some(m)=meta.filter(|m| !m.unique_values.is_empty()) {
                card.section("Unique Values");
                for value in &m.unique_values {card.text(value,GOLD);}
            }
            if let Some(m)=meta.filter(|m|!m.unique_effects.is_empty()) {
                if values.unique.is_empty() && m.unique_values.is_empty() {card.section("Unique Values");}
                for effect in &m.unique_effects {card.text(effect,GOLD);}
            }
        }
        if meta.is_some_and(|m|!m.description.is_empty()) {
            card.section(if matches!(rarity,"Unique"|"Set") {"Item Effects"} else {"Description"});
            card.text(&meta.unwrap().description,if matches!(rarity,"Unique"|"Set") {GOLD} else {WHITE});
        }
        if meta.is_some_and(|m|!m.slots.is_empty() && m.max_stack==1) {
            card.section("Customization");
            if item.named.is_empty() || item.named == "none" {card.hint("Not yet dyed");}
            else {card.hint(&format!("Dye: {}",catalog.items.get(&item.named).map_or_else(||readable(&item.named),|m|m.name.clone())));}
            if !item.fourth_name.is_empty() {card.hint("Appearance customized");}
        }
        card.gap(4.0); card.rule();
        // release261: field_34 is per-instance glyph yield; field_30 is
        // the complete stack's selling value (server item_economy_values.py).
        if item.fifty_second>0 {card.melting(item.fifty_second);}
        card.price(item.level);
    } else if let Some(m)=meta.filter(|m|!m.description.is_empty()) {card.rule();card.text(&m.description,WHITE);}
    if d.waiting {card.hint("Free up a slot to collect this item automatically.");}
    if d.amount==0 {card.hint("Not yet collected");}
    if d.item.is_some() && d.amount>1 && !d.read_only {card.hint("Ctrl + click: split stack");}
    if !advanced && d.item.is_some() {card.gap(3.0);card.rule();card.hint("Hold Y for advanced view.");}
    card
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_bonuses_remain_visible_in_both_views_with_their_correct_scope() {
        let catalog:Catalog=serde_json::from_str(include_str!("../../../../tests/fixtures/item_tooltips.json")).unwrap();
        for advanced in [false,true] {
            let item=ItemInfo {template:"all_mythic_ring_death".into(),third:1,kind:7,durability:100,eighteenth:8477,..default()};
            let text=preview_card(&catalog,None,&item.template,Some(&item),1,advanced).text_content();
            for bonus in ["+ 5 attack speed", "+ 5 movement speed", "+ 66% damage against players"] {
                assert!(text.contains(bonus),"{text}");
                assert!(!text.contains(&format!("{bonus} on this item")),"character bonus must not be item-local: {text}");
            }
            // The character's +5 movement speed must not contaminate the ring's base total.
            let values=catalog.values.item(&item);
            assert!(values.total("Movement speed") < 5.0);
            assert!(values.unique.iter().all(|v|values.contribution(v).is_none()));
            for (template,bonus) in [("unique_ring_stego_yellow_01","+ 2% attack speed"),
                ("all_unique_ring_pw_death","+ 0.108 attack speed on this item"),
                ("all_unique_ring_pw_fire","- 33% burn duration")] {
                let item=ItemInfo {template:template.into(),..item.clone()};
                let text=preview_card(&catalog,None,template,Some(&item),1,advanced).text_content();
                assert!(text.contains(bonus),"{template}: {text}");
            }
        }
    }
}
