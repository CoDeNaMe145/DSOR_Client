//! The transmog desk (transmogwindow, UI::CustomizationWidget).
//!
//! Its tabs shape (skins) or colour (dyes) the worn equipment. The preview
//! holds the eight slots that can be changed (helmet, cloak, hands, shoulders,
//! torso, gloves, boots), each with the worn item and, once it has a skin or
//! a dye, a reset; a click picks the slot (as a right click on the worn item
//! does). The grid lists what fits the picked slot for our class -- the skins
//! (with their effect, shop and owned flags; the one worn marked) or the dyes
//! (the carried ones free) -- the filters keep the owned or the shop's; the
//! bar counts the owned of them. A click chooses one: its icon shows in the
//! preview, its price (gold scaled by our level, Andermant) under it; Apply
//! asks the server.
//! SOURCE: DSOR_Server protocol/item_customization_protocol.py
//!   (apply_item_customizations: the skin must fit the item's slot type and our
//!   class; ApplyPriceRC, ApplyPriceVC and the old skin's RemovePrice scaled by
//!   _Template_DynamicLevelCosts; a dye is paid by a carried dye item or its
//!   Andermant, the old one's RemovePrice); the eye toggles have no command.
use super::super::template::{clip_x, clone_at, dim, has_part, icon_layers, image_art, is_template, leaf, make_button, rename, selected, set_text, without, Icon};
use super::Sources;
use crate::gameplay::inventory::Place;
use crate::gameplay::systems::texture_path;
use crate::ui::Layout;

const INLAY: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay";
const TABS: &str = "Canvas/Collectorsalbum/BackGroundLabel/TabBarLabel";
const PREVIEW: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/CharacterPreviewArea/background_image";
const SKIN_AREA: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/IconArea";
const DYE_AREA: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/IconArea_Color";
const BAR: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/IconArea/ProgressLabel/ProgressBarLabel/ProgressBarLabel2/ProgressBar";
const ENTRY: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/TemplateListEntry";
pub(super) const PRICE: &str = "Canvas/Collectorsalbum/BackGroundLabel/Inlay/PriceArea/VCDisplay/VCDisplayTemplates/TemplateGoldSilverCopper";
/// A cell spans 143..215 by 169..238; five columns; four rows of skins, six of dyes.
const CELL: (f32, f32) = (72.0, 69.0);
const COLUMNS: usize = 5;
const SKIN_ROWS: usize = 4;
const DYE_ROWS: usize = 6;
/// The chosen one's icon in the preview, between the slots.
const CHOSEN_ICON: [f32; 4] = [636.0, 250.0, 764.0, 378.0];
const CHECKED: &str = "textures/ui/icon_checkmark.png";
const SHAPING: u8 = 0;
const COLORING: u8 = 1;
/// The preview's slots: the layout's names, the equipment slots, the bar's icon.
pub(super) const SLOTS: [(&str, i8, &str); 8] = [("Helmet", 0, "HelmetSlot"), ("Back", 7, "CloakSlot"), ("RightHand", 5, "RightHandSlot"), ("LeftHand", 6, "LeftHandSlot"),
    ("Shoulder", 1, "ShoulderSlot"), ("Torso", 2, "TorsoSlot"), ("Gloves", 3, "GlovesSlot"), ("Boots", 4, "BootsSlot")];

/// A slot of the preview: what is worn there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Slot {
    pub slot: i8,
    pub icon: Icon,
    /// It has a skin, a dye (its reset shows).
    pub skinned: bool,
    pub dyed: bool,
}

/// A skin or a dye of the grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Choice {
    pub id: String,
    pub icon: Icon,
    pub effect: bool,
    pub shop: bool,
    pub owned: bool,
    /// The one the item has.
    pub worn: bool,
    pub chosen: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Transmog {
    pub tab: u8,
    pub slots: Vec<Slot>,
    pub slot: Option<i8>,
    /// The picked slot's item, by name.
    pub target: Option<String>,
    pub choices: Vec<Choice>,
    pub page: usize,
    pub pages: usize,
    pub owned_only: bool,
    pub shop_only: bool,
    /// The owned of those that fit, of how many.
    pub progress: (u32, u32),
    /// The chosen one's price: gold in copper, Andermant; whether it changes anything.
    pub price_vc: u64,
    pub price_rc: u32,
    pub ready: bool,
}

/// Per page: the skins' grid or the dyes'.
pub(super) fn per_page(tab: u8) -> usize { COLUMNS * if tab == COLORING { DYE_ROWS } else { SKIN_ROWS } }

pub(super) fn transmog(sources: &Sources) -> Transmog {
    let net = sources.net;
    let catalog = sources.catalog;
    let state = sources.state;
    let tab = state.transmog_tab;
    let worn = |slot: i8| net.inventory.at(Place::Equipment(slot));
    let slots: Vec<Slot> = SLOTS.iter().filter_map(|(_, slot, _)| {
        let item = worn(*slot)?;
        Some(Slot { slot: *slot, icon: sources.icon(&item.template, Some(item)), skinned: !item.fourth_name.is_empty(), dyed: !matches!(item.named.as_str(), "" | "none") })
    }).collect();
    let slot = state.skin_slot.filter(|s| worn(*s).is_some());
    let item = slot.and_then(worn);
    let class = sources.class_name();
    let owns = |template: &str| !template.is_empty() && net.inventory.items.values().any(|i| i.template == template);
    let mut all: Vec<(i32, Choice)> = Vec::new();
    if let (Some(slot), Some(item)) = (slot, item) {
        if tab == COLORING {
            for (id, dye) in &catalog.dyes {
                if dye.kind != "PvE" { continue; }
                all.push((dye.priority, Choice { id: id.clone(), icon: Icon { image: texture_path(&dye.icon), ..Default::default() }, owned: !sources.carried(&dye.item).is_empty(),
                    worn: item.named == *id, chosen: state.skin.as_deref() == Some(id.as_str()), ..Default::default() }));
            }
        } else {
            for (id, skin) in &catalog.skins {
                if skin.kind != "PvE" || !skin.slots.contains(&slot) || !(skin.classes.is_empty() || skin.classes.iter().any(|c| c == class)) { continue; }
                all.push((skin.priority, Choice { id: id.clone(), icon: Icon { image: texture_path(&skin.icon.replace('\\', "/")), ..Default::default() }, effect: skin.effect, shop: !skin.offer.is_empty(),
                    owned: owns(&skin.item), worn: item.fourth_name == *id, chosen: state.skin.as_deref() == Some(id.as_str()) }));
            }
        }
    }
    all.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    let progress = (all.iter().filter(|(_, c)| c.owned).count() as u32, all.len() as u32);
    let (owned_only, shop_only) = (state.transmog_owned, state.transmog_shop && tab == SHAPING);
    let choices: Vec<Choice> = all.into_iter().map(|(_, c)| c).filter(|c| (!owned_only || c.owned) && (!shop_only || c.shop)).collect();
    let pages = choices.len().div_ceil(per_page(tab)).max(1);
    // The price: as the server reckons it.
    let factor = net.level.and_then(|l| catalog.level_costs.get(l.max(1) as usize - 1)).copied().unwrap_or(1.0) as f64;
    let scaled = |vc: u32| (vc as f64 * factor + 0.5).floor() as u64;
    let chosen = state.skin.clone().filter(|id| choices.iter().any(|c| &c.id == id));
    let (price_vc, price_rc, ready) = match (&chosen, item) {
        (Some(id), Some(item)) if tab == COLORING => {
            let dye = &catalog.dyes[id];
            let old = catalog.dyes.get(&item.named).map_or(0, |d| d.remove_vc as u64);
            let carried = !sources.carried(&dye.item).is_empty();
            (old + dye.price_vc as u64, if carried { 0 } else { dye.price_rc }, item.named != *id)
        }
        (Some(id), Some(item)) => {
            let skin = &catalog.skins[id];
            let old = catalog.skins.get(&item.fourth_name).map_or(0, |s| scaled(s.remove_vc));
            (old + scaled(skin.price_vc), skin.price_rc, item.fourth_name != *id)
        }
        _ => (0, 0, false),
    };
    Transmog {
        tab, slots, slot, target: item.map(|i| sources.name(&i.template, Some(i))), choices,
        page: state.pages.get(super::TRANSMOG).copied().unwrap_or(0).min(pages - 1), pages,
        owned_only, shop_only, progress, price_vc, price_rc, ready,
    }
}

/// The desk: the tab, the preview's slots, the grid, the filters, the bar,
/// the price and Apply.
pub(super) fn prepare_transmog(source: &Layout, transmog: &Transmog) -> Layout {
    let coloring = transmog.tab == COLORING;
    let slot_part = |id: &str| SLOTS.iter().find(|(name, _, _)| has_part(id, name)).map(|(_, slot, _)| *slot);
    // A slot's reset sits beside it (its icon_slot_bg parent holds both).
    let reset_slot = |id: &str| -> Option<i8> {
        let at = id.find("/ButtonReset")?;
        let parent = &id[..at];
        source.widgets.iter().filter(|w| w.id.starts_with(&format!("{parent}/")) && w.kind == "Slot").find_map(|w| slot_part(&w.id))
    };
    let resettable = |slot: i8| transmog.slots.iter().any(|s| s.slot == slot && if coloring { s.dyed } else { s.skinned });
    let mut layout = without(source, |id| {
        is_template(id) || id.contains("/AnimUnlock") || id.contains("/AnimUse")
            || id.contains("/PvPEquipmentTab") || id.contains("/PvEEquipmentTab")
            || id.contains("/DragRotateAreaLabel") || id.contains("/ToggleButtonArea")
            || (coloring && (id.contains("/window_shaping") || id.starts_with(&format!("{SKIN_AREA}/")) || id == SKIN_AREA))
            || (!coloring && (id.contains("/window_coloring") || id.starts_with(DYE_AREA)))
            || id.contains("/FilterShopSkins") && coloring
            || (id.contains("/ButtonReset") && reset_slot(id).is_none_or(|s| !resettable(s)))
            || (id.contains("/pages/") && !id.ends_with("/PageNumText") && transmog.pages <= 1)
            || id.split('/').find_map(|p| p.strip_prefix("LabelProgressItem_")).is_some_and(|name| {
                let shown = transmog.slot.and_then(|s| SLOTS.iter().find(|(_, slot, _)| *slot == s)).map_or("All", |(_, _, label)| label);
                name != shown
            })
            // The preview's empty slot pictures, where an item is worn.
            || SLOTS.iter().any(|(_, slot, _)| transmog.slots.iter().any(|s| s.slot == *slot) && is_empty_picture(id, *slot))
    });
    let hint = match (&transmog.target, transmog.choices.is_empty()) {
        (None, _) => "Pick a worn item".to_owned(),
        (Some(name), true) if coloring => format!("No dye for {name}"),
        (Some(name), true) => format!("No skin fits {name}"),
        // The bar's caption: the item and the owned of what fits.
        (Some(name), false) => format!("{name} ({}/{})", transmog.progress.0, transmog.progress.1),
    };
    let mut extra = Vec::new();
    for w in &mut layout.widgets {
        let id = w.id.clone();
        // The tabs: the open one pressed.
        if id.starts_with(&format!("{TABS}/ShapingTab")) {
            if leaf(&id) == "ShapingTab" { w.event = format!("Sys:TransmogTab:{SHAPING}"); }
            if !coloring { selected(w); }
        }
        if id.starts_with(&format!("{TABS}/ColoringTab")) {
            if leaf(&id) == "ColoringTab" { w.event = format!("Sys:TransmogTab:{COLORING}"); }
            if coloring { selected(w); }
        }
        // The preview's slots: the worn item; a click picks it.
        if w.kind == "Slot" && id.starts_with(PREVIEW) {
            if let Some(slot) = slot_part(&id) {
                if let Some(worn) = transmog.slots.iter().find(|s| s.slot == slot) {
                    make_button(w, &format!("Sys:TransmogSlot:{slot}"));
                    let mut layers = icon_layers(w, &worn.icon);
                    if transmog.slot.is_some_and(|s| s != slot) { for l in &mut layers { dim(l, 0.5); } }
                    extra.extend(layers);
                }
            }
        }
        if leaf(&id) == "ButtonResetVisibility" {
            if let Some(slot) = reset_slot(&id) { w.event = format!("Sys:TransmogReset:{slot}"); }
        }
        // The filters: their check shows when on.
        let filter = if id.contains("/FilterOwnedItems") { Some(("Sys:TransmogFilter:0", transmog.owned_only)) } else if id.contains("/FilterShopSkins") { Some(("Sys:TransmogFilter:1", transmog.shop_only)) } else { None };
        if let Some((event, on)) = filter {
            if matches!(leaf(&id), "FilterOwnedItems" | "FilterShopSkins") { w.event = event.into(); }
            if on && w.state.is_some() { if let Some(art) = &mut w.art { art.tex = Some(CHECKED.into()); } }
        }
        match leaf(&id) {
            "PageNumText" => set_text(w, &format!("{} / {}", transmog.page + 1, transmog.pages)),
            "ProgressBarTextLabel" => set_text(w, &hint),
            "progresspoints" => set_text(w, ""),
            "progress" if id.starts_with(BAR) => clip_x(w, transmog.progress.0 as f32 / transmog.progress.1.max(1) as f32),
            "MoneyDisplayRC" => set_text(w, &transmog.price_rc.to_string()),
            _ => {}
        }
        // Nothing to apply: its disabled face (crate::ui::controls: a disabled
        // button takes no click).
        if leaf(&id) == "ApplyButton" && !transmog.ready { w.enabled = false; }
    }
    layout.widgets.extend(extra);
    for mut w in clone_at(source, PRICE, &format!("{INLAY}/PriceArea/VCDisplay/Price"), 0.0, 0.0) {
        let vc = transmog.price_vc;
        match leaf(&w.id) {
            "Gold" => set_text(&mut w, &(vc / 10_000).to_string()),
            "Silver" => set_text(&mut w, &(vc / 100 % 100).to_string()),
            "Copper" => set_text(&mut w, &(vc % 100).to_string()),
            _ => {}
        }
        w.hidden = false;
        layout.widgets.push(w);
    }
    // The grid.
    let area = if coloring { DYE_AREA } else { SKIN_AREA };
    let per_page = per_page(transmog.tab);
    let any_chosen = transmog.choices.iter().any(|c| c.chosen);
    for (i, choice) in transmog.choices.iter().skip(transmog.page * per_page).take(per_page).enumerate() {
        let (dx, dy) = ((i % COLUMNS) as f32 * CELL.0, (i / COLUMNS) as f32 * CELL.1);
        let replacement = format!("{area}/Cell{i}");
        for mut w in clone_at(source, ENTRY, &replacement, dx, dy) {
            rename(&mut w, "TemplateSlot", "Slot");
            rename(&mut w, "TemplateIcon", "Icon");
            let flag = |name: &str| has_part(&w.id, name);
            if (flag("Label_fx") && !choice.effect) || (flag("Label_shop") && !choice.shop) || (flag("Label_in_inventory") && !choice.owned)
                || (flag("Label_transmog") && !choice.worn) || (flag("highlight") && !choice.chosen) { continue; }
            if flag("highlight") { w.hidden = false; }
            if w.id == replacement { make_button(&mut w, &format!("Sys:Skin:{}", choice.id)); }
            if leaf(&w.id) == "Icon" {
                let mut layers = icon_layers(&mut w, &choice.icon);
                if any_chosen && !choice.chosen { for l in &mut layers { dim(l, 0.6); } }
                set_text(&mut w, "");
                layout.widgets.extend(layers);
            }
            layout.widgets.push(w);
        }
    }
    // The chosen one in the preview.
    let area = source.widgets.iter().find(|w| w.id == format!("{PREVIEW}/DragRotateAreaLabel"));
    if let (Some(choice), Some(area)) = (transmog.choices.iter().find(|c| c.chosen), area) {
        let mut w = area.clone();
        w.id = format!("{PREVIEW}/Chosen");
        w.kind = "Label".into();
        w.event.clear();
        w.text = None;
        w.hidden = false;
        w.rect = CHOSEN_ICON;
        w.art = Some(image_art(CHOSEN_ICON, choice.icon.image.clone()));
        layout.widgets.push(w);
    }
    layout
}

/// The preview's empty picture of a slot (empty_helm beside Helmet...).
fn is_empty_picture(id: &str, slot: i8) -> bool {
    let name = leaf(id);
    let picture = match slot { 0 => "empty_helm", 7 => "empty_cloak2", 5 => "epmty_glove_r", 6 => "empty_glove_l", 1 => "empty_shoulder2", 2 => "empty_torso", 3 => "empty_glove", 4 => "empty_boots1", _ => return false };
    name == picture && id.starts_with(PREVIEW)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    fn source() -> Layout {
        let shaping = format!("{PREVIEW}/window_shaping/links/icon_slot_bg1");
        let coloring = format!("{PREVIEW}/window_coloring/links/icon_slot_bg1");
        let ids: Vec<(String, &str, bool)> = vec![(format!("{TABS}/ShapingTab"), "Button", false), (format!("{TABS}/ShapingTab/normal"), "Label", false), (format!("{TABS}/ShapingTab/pressed"), "Label", false),
            (format!("{TABS}/ColoringTab"), "Button", false), (format!("{TABS}/PvEEquipmentTab"), "Button", false),
            (shaping.clone(), "Label", false), (format!("{shaping}/Helmet"), "Slot", false), (format!("{shaping}/empty_helm"), "Label", false),
            (format!("{shaping}/ToggleButtonArea/ToggleButtonArea/ToggleVisibility"), "Button", false), (format!("{shaping}/ButtonReset/ButtonArea/ButtonResetVisibility"), "Button", false),
            (format!("{coloring}/Helmet"), "Slot", false), (format!("{PREVIEW}/DragRotateAreaLabel"), "Label", false),
            (ENTRY.into(), "Label", false), (format!("{ENTRY}/TemplateSlot/TemplateIcon"), "Icon", true), (format!("{ENTRY}/TemplateSlot/TemplateIcon/Label_fx"), "Label", false),
            (format!("{ENTRY}/TemplateSlot/TemplateIcon/Label_shop"), "Label", false), (format!("{ENTRY}/TemplateSlot/TemplateIcon/Label_transmog"), "Label", false), (format!("{ENTRY}/TemplateSlot/TemplateIcon/highlight"), "Label", false),
            (format!("{SKIN_AREA}/pages/PageNumText"), "TextLabel", true), (format!("{SKIN_AREA}/pages/NextPageBtn"), "Button", false),
            (format!("{SKIN_AREA}/FilterOwnedItems"), "Button", false), (format!("{SKIN_AREA}/FilterOwnedItems/normal"), "Label", false),
            (format!("{BAR}/ProgressBarTextLabel"), "TextLabel", true), (format!("{BAR}/progress/progresspoints"), "TextLabel", true),
            (format!("{BAR}/LabelProgressItem2/LabelProgressItem/LabelProgressItem_All"), "Label", false), (format!("{BAR}/LabelProgressItem2/LabelProgressItem/LabelProgressItem_HelmetSlot"), "Label", false),
            (format!("{DYE_AREA}/pages/PageNumText"), "TextLabel", true),
            (PRICE.into(), "Label", false), (format!("{PRICE}/Gold"), "TextLabel", true), (format!("{PRICE}/Copper"), "TextLabel", true), (format!("{INLAY}/PriceArea/RCDisplay/MoneyDisplayRC"), "TextLabel", true),
            (format!("{INLAY}/PriceArea/ApplyButton"), "Button", false), (format!("{INLAY}/PriceArea/ApplyButton/normal"), "Label", false), (format!("{INLAY}/PriceArea/ApplyButton/disabled"), "Label", false)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        layout(super::super::TRANSMOG, &refs)
    }

    #[test]
    fn the_desk_shapes_the_picked_slot_with_the_chosen_skin() {
        let transmog = Transmog {
            tab: SHAPING, slots: vec![Slot { slot: 0, icon: Icon { image: "helm.png".into(), ..Default::default() }, skinned: true, dyed: false }], slot: Some(0), target: Some("Old Helm".into()),
            choices: vec![Choice { id: "k1".into(), effect: true, worn: true, ..Default::default() }, Choice { id: "k2".into(), icon: Icon { image: "k2.png".into(), ..Default::default() }, shop: true, owned: true, chosen: true, ..Default::default() }],
            page: 0, pages: 1, progress: (1, 2), price_vc: 12_345, price_rc: 199, ready: true, ..Default::default()
        };
        let prepared = prepare_transmog(&source(), &transmog);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(!ids.iter().any(|id| is_template(id) || id.contains("window_coloring") || id.contains("ToggleVisibility") || id.contains("PvEEquipmentTab") || id.starts_with(DYE_AREA)));
        assert!(find(&prepared, &format!("{TABS}/ShapingTab/normal")).hidden);
        assert_eq!(find(&prepared, &format!("{TABS}/ColoringTab")).event, "Sys:TransmogTab:1");
        let helmet = format!("{PREVIEW}/window_shaping/links/icon_slot_bg1/Helmet");
        assert_eq!(find(&prepared, &helmet).event, "Sys:TransmogSlot:0");
        assert!(ids.contains(&format!("{helmet}/Image")) && !ids.iter().any(|id| id.ends_with("/empty_helm")));
        assert_eq!(find(&prepared, &format!("{PREVIEW}/window_shaping/links/icon_slot_bg1/ButtonReset/ButtonArea/ButtonResetVisibility")).event, "Sys:TransmogReset:0");
        // The grid: the worn skin marked, the chosen one highlighted, flags by kind.
        assert_eq!(find(&prepared, &format!("{SKIN_AREA}/Cell1")).event, "Sys:Skin:k2");
        assert_eq!(find(&prepared, &format!("{SKIN_AREA}/Cell1")).rect[0], 10.0 + CELL.0);
        assert!(ids.contains(&format!("{SKIN_AREA}/Cell0/Slot/Icon/Label_fx")) && ids.contains(&format!("{SKIN_AREA}/Cell0/Slot/Icon/Label_transmog")) && !ids.contains(&format!("{SKIN_AREA}/Cell0/Slot/Icon/Label_shop")));
        assert!(ids.contains(&format!("{SKIN_AREA}/Cell1/Slot/Icon/highlight")) && !ids.contains(&format!("{SKIN_AREA}/Cell0/Slot/Icon/highlight")));
        assert_eq!(find(&prepared, &format!("{PREVIEW}/Chosen")).art.as_ref().unwrap().tex.as_deref(), Some("k2.png"));
        // The bar: the helmet's icon, the owned count.
        assert!(ids.iter().any(|id| id.ends_with("LabelProgressItem_HelmetSlot")) && !ids.iter().any(|id| id.ends_with("LabelProgressItem_All")));
        assert_eq!(text_of(&prepared, &format!("{BAR}/ProgressBarTextLabel")), "Old Helm (1/2)");
        assert_eq!((text_of(&prepared, &format!("{INLAY}/PriceArea/VCDisplay/Price/Gold")), text_of(&prepared, &format!("{INLAY}/PriceArea/VCDisplay/Price/Copper")), text_of(&prepared, &format!("{INLAY}/PriceArea/RCDisplay/MoneyDisplayRC"))), ("1".into(), "45".into(), "199".into()));
        assert_eq!(find(&prepared, &format!("{SKIN_AREA}/FilterOwnedItems")).event, "Sys:TransmogFilter:0");
        assert!(find(&prepared, &format!("{INLAY}/PriceArea/ApplyButton")).enabled);
        // Nothing to apply: Apply is disabled (its disabled face, no click).
        let idle = prepare_transmog(&source(), &Transmog { ready: false, ..transmog.clone() });
        assert!(!find(&idle, &format!("{INLAY}/PriceArea/ApplyButton")).enabled);
        // The dyes' tab: its own grid and preview.
        let dyes = prepare_transmog(&source(), &Transmog { tab: COLORING, ..transmog.clone() });
        assert!(dyes.widgets.iter().any(|w| w.id == format!("{DYE_AREA}/Cell0")) && !dyes.widgets.iter().any(|w| w.id.contains("window_shaping") || w.id.starts_with(&format!("{SKIN_AREA}/"))));
        // Nothing picked: the hint.
        let empty = prepare_transmog(&source(), &Transmog::default());
        assert_eq!(text_of(&empty, &format!("{BAR}/ProgressBarTextLabel")), "Pick a worn item");
        assert!(empty.widgets.iter().any(|w| w.id.ends_with("/empty_helm")));
    }
}
