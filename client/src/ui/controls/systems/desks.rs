//! The smith's salvage desk (salvagewindow) and workbench (recipebookwb), the
//! jeweller's gem desk (gemdeskwb).
//!
//! The salvage desk melts up to nine carried items picked by a right click
//! (SalvageCommand: glyphs and dust back). The gem desk shows the item picked
//! (worn or carried) with its sockets: a gem right-clicked goes into the first
//! free one, a socket's button takes its gem out, the next socket's button buys
//! it (InventoryCommand 0x11 [item, socket], 0x14 [socket], 0x15). The workbench
//! (UI::RecipeBookWidget) has a tab per WorkbenchTab (its icon), each a tree of
//! categories listing their recipe books (the known ones; the locked ones it
//! shows, with how to unlock them); the filters keep the books one can craft
//! now or the locked ones. The book chosen shows its text and the items it
//! uses; its recipe (the first the carried items make, else its first) its
//! ingredients ("template:amount", how many are carried), result, prices for
//! the amount chosen; Craft sends it, paid in gold or Andermant.
//! SOURCE: DSOR_Server services/inventory_interactions.py (_handle_salvage,
//!   _handle_crafting), protocol/gem_protocol.py, _Template_SocketExtensionCosts.
use super::super::template::{clone_at, clone_rows, dim, grey, has_part, icon_layers, is_template, leaf, make_button, rename, selected, set_text, without, wrap, Icon};
use crate::gameplay::systems::texture_path;
use super::Sources;
use crate::ui::Layout;

pub(in crate::ui) const SALVAGE_SLOTS: usize = 9;
const SALVAGE_AREA: &str = "Canvas/BackGroundLabel/BackgroundLabel/BackgroundLabel1/ValuesBackgroundLabel";
const SOCKETS: &str = "Canvas/BackGroundLabel/SocketsBackgroundLabel/SocketsLabel";
/// The desk's sockets (Socket0..9).
const MOST_SOCKETS: usize = 10;
const GEM_ITEM: &str = "Canvas/BackGroundLabel/ItemLabel";
const BOOK: &str = "Canvas/Label_blur/RecipeBook/Background/RecipeBook";
pub(super) const LIST: &str = "Canvas/Label_blur/RecipeBook/Background/RecipeBook/RecipesListBackground/CategoriesList";
const BENCH: &str = "Canvas/Label_blur/RecipeBook/Background/BackgroundCrafting/ContentLabel";
/// The list's rows from 148 to 432 (categories 31 high, the others 24 or 23).
const LIST_TOP: f32 = 148.0;
const LIST_BOTTOM: f32 = 432.0;
const MOST_INGREDIENTS: usize = 4;
/// A book's text: lines of 46 characters, ten of them.
const INFO_WIDTH: usize = 46;
const MOST_INFO_LINES: usize = 10;
const CHECKED: &str = "textures/ui/icon_checkmark.png";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Salvage {
    pub items: Vec<(u32, String, Icon)>,
}

pub(super) fn salvage(sources: &Sources) -> Salvage {
    Salvage { items: sources.state.salvage.iter().filter_map(|id| sources.net.inventory.items.get(id))
        .map(|i| (i.id, sources.name(&i.template, Some(i)), sources.icon(&i.template, Some(i)))).collect() }
}

/// The salvage desk: the picked items in its slots (a click takes one out).
pub(super) fn prepare_salvage(source: &Layout, salvage: &Salvage) -> Layout {
    let mut layout = without(source, |id| is_template(id));
    for w in &mut layout.widgets {
        if leaf(&w.id) == "CraftingResourceValueLabel" { set_text(w, &salvage.items.len().to_string()); }
    }
    for (n, (_, _, icon)) in salvage.items.iter().take(SALVAGE_SLOTS).enumerate() {
        for mut w in clone_at(source, &format!("{SALVAGE_AREA}/ItemSlot{n}/TemplateIcon"), &format!("{SALVAGE_AREA}/ItemSlot{n}/Icon"), 0.0, 0.0) {
            let layers = icon_layers(&mut w, icon);
            make_button(&mut w, &format!("Sys:Unpick:{n}"));
            set_text(&mut w, "");
            w.hidden = false;
            layout.widgets.extend(layers);
            layout.widgets.push(w);
        }
    }
    layout
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Socket {
    pub open: bool,
    /// Its gem only placed, not set yet (State::gem_preview).
    pub preview: bool,
    /// Its gem's template (the hover card, tooltips).
    pub template: Option<String>,
    pub gem: Option<(String, Icon)>,
    /// The Andermant to open it, when it is the next one.
    pub price: Option<u32>,
}

/// The gem desk: the item picked and its sockets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Gems {
    pub target: Option<(String, Icon)>,
    /// The item's level: its gems are valued at it.
    pub level: u32,
    pub sockets: Vec<Socket>,
}

pub(super) fn gems(sources: &Sources) -> Gems {
    let Some(item) = sources.state.gem_target.and_then(|id| sources.net.inventory.items.get(&id)) else { return Gems::default() };
    let count = item.tenth as usize;
    let trinket = sources.items.and_then(|c| c.items.get(&item.template)).is_some_and(|m| m.item_category.ends_with("Trinket"));
    let sockets = (0..MOST_SOCKETS).map(|n| Socket {
        open: n < count,
        preview: sources.state.gem_preview.iter().any(|(s, _)| *s == n),
        template: item.names.get(n).filter(|g| !g.is_empty()).cloned()
            .or_else(|| sources.state.gem_preview.iter().find(|(s, _)| *s == n).and_then(|(_, gem)| sources.net.inventory.items.get(gem)).map(|g| g.template.clone())),
        gem: None,
        price: (n == count).then(|| sources.catalog.socket_price(n + 1, item.kind, trinket)),
    }).map(|mut s: Socket| { s.gem = s.template.as_ref().map(|g| (sources.name(g, None), sources.icon(g, None))); s }).collect();
    Gems { target: Some((sources.name(&item.template, Some(item)), sources.icon(&item.template, Some(item)))), sockets, level: item.durability }
}

/// The gem desk: the item's name and icon, its sockets open or closed, their
/// gems, the next socket's price.
pub(super) fn prepare_gemdesk(source: &Layout, gems: &Gems) -> Layout {
    // Socket<n>, not SocketsLabel.
    let socket_of = |id: &str| id.split('/').find_map(|p| p.strip_prefix("Socket").and_then(|n| n.parse::<usize>().ok()));
    let mut layout = without(source, |id| {
        // Jeweler: the jeweller's own window, which the desk is (no other).
        is_template(id) || id.starts_with("Canvas/CraftingBackgroundLabel") || id.contains("/StatsLabel") || has_part(id, "ButtonJeweler")
            // A placed gem shows the open padlock (click: set it), a set gem
            // the closed one (click: take it out).
            || (gems.target.is_some() && id.ends_with("/ItemDragAndDropTargetLabel")) || (gems.target.is_none() && id.contains("/ItemLabel/ItemSlot"))
            || socket_of(id).is_some_and(|n| {
                let socket = gems.sockets.get(n);
                let open = socket.is_some_and(|s| s.open);
                let preview = socket.is_some_and(|s| s.preview);
                (id.contains("/SocketOpen") && !open) || (id.contains("/SocketClosed") && open)
                    || (has_part(id, "IconClose") && preview) || (has_part(id, "IconOpen") && !preview)
                    || (id.contains("/ItemSlot") && socket.is_none_or(|s| s.gem.is_none()))
                    || (has_part(id, "Button") && socket.is_none_or(|s| s.gem.is_none()))
                    || (has_part(id, "BuyButton") && socket.is_none_or(|s| s.price.is_none()))
            })
            || leaf(id).strip_prefix("Headline").is_some_and(|rest| {
                let (n, disabled) = (rest.trim_end_matches("Disabled").parse::<usize>().ok(), rest.ends_with("Disabled"));
                n.is_some_and(|n| gems.sockets.get(n).is_some_and(|s| s.open) == disabled)
            })
    });
    let mut extra = Vec::new();
    for w in &mut layout.widgets {
        if w.id == format!("{GEM_ITEM}/ItemSlot/Icon") {
            if let Some((_, icon)) = &gems.target { extra.extend(icon_layers(w, icon)); set_text(w, ""); w.hidden = false; }
        }
        if leaf(&w.id) == "ItemNameTextLabel" { set_text(w, gems.target.as_ref().map_or("", |(name, _)| name.as_str())); }
        if let Some(n) = socket_of(&w.id) {
            if w.id.ends_with("/ItemSlot/Icon") {
                if let Some((_, icon)) = gems.sockets.get(n).and_then(|s| s.gem.as_ref()) {
                    extra.extend(icon_layers(w, icon));
                    // Hovered, the gem's card (GemDeskWidget::SocketUI::GenerateItemTooltip).
                    make_button(w, &format!("Sys:SocketGem:{n}"));
                    set_text(w, ""); w.hidden = false;
                }
            }
        }
    }
    layout.widgets.extend(extra);
    for (n, socket) in gems.sockets.iter().enumerate() {
        let Some(price) = socket.price else { continue };
        for mut w in clone_rows(source, &format!("{SOCKETS}/Socket{n}/TemplateRC"), &format!("{SOCKETS}/Socket{n}/Price"), 0.0) {
            if leaf(&w.id) == "Andermant" { set_text(&mut w, &price.to_string()); }
            layout.widgets.push(w);
        }
    }
    layout
}

/// A row of the workbench's list: a category, a sub-category, a known book or
/// a locked one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum RowKind { #[default] Category, Sub, Book, Locked }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Row {
    pub id: String,
    pub title: String,
    pub kind: RowKind,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Ingredient {
    pub template: String,
    pub icon: Icon,
    pub have: u32,
    pub need: u32,
    /// Put in its slot by the player (State::workbench_placed).
    pub placed: bool,
}

/// The book chosen: its text, the items it uses, the recipe it crafts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Detail {
    pub title: String,
    pub text: String,
    pub locked: bool,
    /// A locked book's origin: its icon and name.
    pub origin: Option<(String, String)>,
    pub placeholders: Vec<Icon>,
    /// The recipe crafted (the first the carried items make, else its first).
    pub recipe: String,
    pub result: Icon,
    /// The crafted item's template (the hover card).
    pub result_template: String,
    pub ingredients: Vec<Ingredient>,
    pub price_vc: u64,
    pub price_rc: u32,
    /// How many to craft, of how many the ingredients allow.
    pub amount: u32,
    pub most: u32,
}

/// The workbench: its tabs, the open tab's list, the book chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Workbench {
    pub tab: usize,
    pub tabs: Vec<String>,
    pub rows: Vec<Row>,
    pub scroll: usize,
    pub detail: Option<Detail>,
    /// The filters: books one can craft now, locked books only.
    pub craftable_only: bool,
    pub locked_only: bool,
}

/// A recipe's ingredient: "template:amount" (one without an amount).
pub(super) fn ingredient(value: &str) -> (&str, u32) {
    match value.split_once(':') {
        Some((template, amount)) => (template, amount.trim().parse().unwrap_or(1).max(1)),
        None => (value, 1),
    }
}

/// How many of a template are carried.
fn carried_amount(sources: &Sources, template: &str) -> u32 {
    sources.carried(template).iter().filter_map(|id| sources.net.inventory.items.get(id)).map(|i| i.third.max(1)).sum()
}

/// How many times a recipe can be crafted with what is carried.
fn craftable(sources: &Sources, recipe: &crate::gameplay::systems::RecipeDef) -> u32 {
    recipe.ingredients.iter().map(|i| { let (t, n) = ingredient(i); carried_amount(sources, t) / n }).min().unwrap_or(0)
}

pub(super) fn workbench(sources: &Sources) -> Workbench {
    let catalog = sources.catalog;
    let state = sources.state;
    let known: Vec<String> = sources.net.recipes.iter().map(|(book, _)| book.to_lowercase()).collect();
    let tab = state.workbench_tab.min(catalog.workbench_tabs.len().saturating_sub(1));
    let tab_id = catalog.workbench_tabs.get(tab).map(|t| t.id.clone()).unwrap_or_default();
    let category_of = |id: &str| catalog.workbench_categories.iter().find(|c| c.id == id);
    // The open category and its top.
    let open = state.recipe_category.clone().filter(|c| category_of(c).is_some_and(|c| c.tab == tab_id));
    let open_top = open.as_ref().and_then(|c| category_of(c)).map(|c| if c.parent.is_empty() { c.id.clone() } else { c.parent.clone() });
    let book_title = |id: &str, book: &crate::gameplay::systems::RecipeBookDef| {
        if !book.title.is_empty() { return book.title.clone(); }
        book.recipes.first().and_then(|r| catalog.recipes.get(r)).map(|r| sources.name(&r.crafted, None)).unwrap_or_else(|| crate::ui::dialogue::readable(id))
    };
    // A category's books: known, or locked ones it lists; as the filters keep.
    let books = |category: &str| -> Vec<Row> {
        let mut rows: Vec<(i32, Row)> = catalog.recipe_books.iter().filter(|(_, b)| b.category == category).filter_map(|(id, b)| {
            let is_known = catalog.book_known(id, &known);
            if !is_known && !b.unlockable { return None; }
            if state.workbench_locked_only && is_known { return None; }
            if state.workbench_craftable_only && (!is_known || !b.recipes.iter().filter_map(|r| catalog.recipes.get(r)).any(|r| craftable(sources, r) > 0)) { return None; }
            Some((b.priority, Row { id: id.clone(), title: book_title(id, b), kind: if is_known { RowKind::Book } else { RowKind::Locked }, selected: state.recipe.as_deref() == Some(id.as_str()) }))
        }).collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.title.cmp(&b.1.title)).then(a.1.id.cmp(&b.1.id)));
        rows.into_iter().map(|(_, r)| r).collect()
    };
    let mut tops: Vec<_> = catalog.workbench_categories.iter().filter(|c| c.tab == tab_id && c.parent.is_empty()).collect();
    tops.sort_by_key(|c| (c.priority, c.id.clone()));
    let mut rows = Vec::new();
    for top in tops {
        rows.push(Row { id: top.id.clone(), title: top.title.clone(), kind: RowKind::Category, selected: open.as_deref() == Some(top.id.as_str()) });
        if open_top.as_deref() != Some(top.id.as_str()) { continue; }
        let mut subs: Vec<_> = catalog.workbench_categories.iter().filter(|c| c.parent == top.id).collect();
        subs.sort_by_key(|c| (c.priority, c.id.clone()));
        rows.extend(books(&top.id));
        for sub in subs {
            rows.push(Row { id: sub.id.clone(), title: sub.title.clone(), kind: RowKind::Sub, selected: open.as_deref() == Some(sub.id.as_str()) });
            if open.as_deref() == Some(sub.id.as_str()) { rows.extend(books(&sub.id)); }
        }
    }
    let detail = state.recipe.as_ref().and_then(|id| catalog.recipe_books.get(id).map(|b| (id, b))).map(|(id, book)| {
        let locked = !catalog.book_known(id, &known);
        let recipes: Vec<(&String, &crate::gameplay::systems::RecipeDef)> = book.recipes.iter().filter_map(|r| catalog.recipes.get(r).map(|d| (r, d))).collect();
        let (recipe_id, recipe) = recipes.iter().find(|(_, r)| craftable(sources, r) > 0).or(recipes.first()).map(|(id, r)| ((*id).clone(), Some(*r))).unwrap_or_default();
        let most = recipe.map_or(0, |r| craftable(sources, r));
        let amount = state.craft_amount.clamp(1, most.max(1));
        let title = book_title(id, book);
        Detail {
            text: if locked && !book.locked.is_empty() { unlock_text(&book.locked, &title) } else { book.description.clone() },
            title,
            locked,
            origin: (locked && !book.locked_icon.is_empty()).then(|| (book.locked_icon.clone(), origin_name(&book.locked_icon))),
            placeholders: if locked { Vec::new() } else { book.placeholders.iter().filter(|p| !p.icon.is_empty()).map(|p| Icon { image: texture_path(&p.icon), ..Default::default() }).collect() },
            recipe: recipe_id,
            result: recipe.map(|r| sources.icon(&r.crafted, None)).unwrap_or_default(),
            result_template: recipe.map(|r| ingredient(&r.crafted).0.to_owned()).unwrap_or_default(),
            ingredients: recipe.map(|r| r.ingredients.iter().take(MOST_INGREDIENTS).enumerate().map(|(n, i)| {
                let (template, need) = ingredient(i);
                Ingredient { template: template.to_owned(), icon: sources.icon(template, None), have: carried_amount(sources, template), need: need * amount,
                    placed: state.workbench_placed.contains(&n) }
            }).collect()).unwrap_or_default(),
            price_vc: recipe.map_or(0, |r| r.price_vc as u64 * amount as u64),
            price_rc: recipe.map_or(0, |r| r.price_rc * amount),
            amount, most,
        }
    });
    Workbench {
        tab, tabs: catalog.workbench_tabs.iter().map(|t| t.icon.clone()).collect(), rows,
        scroll: state.scroll.get(LIST).copied().unwrap_or(0), detail,
        craftable_only: state.workbench_craftable_only, locked_only: state.workbench_locked_only,
    }
}

/// A locked book's text: the unlock item is the book's own recipe (its title);
/// the achievement or quest it names at run time is left out.
fn unlock_text(text: &str, title: &str) -> String {
    text.replace("{0:s:itemname}", title).replace(": {1:s}", "").replace(" {1:s}", "").replace("{1:s}", "")
}

/// An origin icon's name: ui/icon_origin_questreward -> "Quest reward".
fn origin_name(icon: &str) -> String {
    let key = icon.rsplit('/').next().unwrap_or(icon).trim_start_matches("icon_origin_");
    match key {
        "questreward" | "quest" => "Quest reward".into(),
        "achievement" | "achievements" => "Achievement".into(),
        "gamblertrader" => "Gambler".into(),
        "bonecoins" => "Bone coin trader".into(),
        "leaderboards" => "Leaderboards".into(),
        other => {
            let words = other.replace('_', " ");
            let mut chars = words.chars();
            chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str())
        }
    }
}

/// The list's row heights by kind (its templates').
fn row_height(kind: RowKind) -> f32 {
    match kind { RowKind::Category => 31.0, RowKind::Sub | RowKind::Book => 24.0, RowKind::Locked => 23.0 }
}

/// How far the list scrolls: its rows less those that fit.
pub(super) fn list_limit(bench: &Workbench) -> usize {
    let mut room = LIST_BOTTOM - LIST_TOP;
    let mut fitting = 0;
    for row in bench.rows.iter().rev() {
        room -= row_height(row.kind);
        if room < 0.0 { break; }
        fitting += 1;
    }
    bench.rows.len() - fitting
}

/// The workbench: the tabs, the list, the chosen book's text and usable
/// items, its recipe's ingredients, result and prices, the amount, Craft.
pub(super) fn prepare_workbench(source: &Layout, bench: &Workbench) -> Layout {
    let crafting = bench.detail.as_ref().is_some_and(|d| !d.locked && !d.recipe.is_empty());
    let mut layout = without(source, |id| {
        is_template(id) || id.contains("/WorkbenchPreviewLabel") || id.contains("/DropdownMenuEnchantmentSlot")
            || id.contains("/ButtonBuySlot") || id.contains("/privilegeDiscountLabel") || id.contains("/GlyphDisplay")
            // The crafting animations (their smoke and glows); the origin box for a locked book.
            || id.split('/').any(|p| p.starts_with("Anim"))
            || (id.contains("/RecipeOriginBG") && bench.detail.as_ref().is_none_or(|d| d.origin.is_none()))
            || id.contains("/BackgroundBook_cooking")
            || id.split('/').find_map(|p| p.strip_prefix("TabCrafting")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n >= bench.tabs.len())
            // The list's hint: choose a book; or none known.
            || (id.ends_with("/Infotext_ChooseRecipe") && (bench.detail.is_some() || bench.rows.is_empty()))
            || (id.ends_with("/Infotext_NoRecipeAvailable") && (bench.detail.is_some() || !bench.rows.is_empty()))
            || (bench.detail.as_ref().is_none_or(|d| d.placeholders.is_empty()) && (id.ends_with("/UseableItems") || has_part_prefix(id, "ItemSlot", "/RecipesInfotextArea/")))
            || bench.detail.as_ref().is_some_and(|d| has_part_prefix(id, "ItemSlot", "/RecipesInfotextArea/") && id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n >= d.placeholders.len()))
            || (!crafting && (id.contains("/WorkbenchResultLabel") || id.contains("/Craftingresource_requiredAmount") || id.contains("/BulkCraftingAmount")
                || id.contains("/VCDisplay") || id.contains("/CraftButtonRC") || id.contains("/ClearAllButton")))
            || bench.detail.as_ref().is_some_and(|d| d.price_rc == 0) && id.contains("/CraftButtonRC")
            || bench.detail.as_ref().is_some_and(|d| {
                id.split('/').find_map(|p| p.strip_prefix("Craftingresource_requiredAmount")).and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n > d.ingredients.len())
            })
    });
    let ready = bench.detail.as_ref().is_some_and(|d| crafting && d.most > 0);
    let mut extra = Vec::new();
    for w in &mut layout.widgets {
        let id = w.id.clone();
        // The tabs: their icons; the open one pressed.
        if let Some(n) = id.split('/').find_map(|p| p.strip_prefix("TabCrafting")).and_then(|n| n.parse::<usize>().ok()) {
            if leaf(&id) == format!("TabCrafting{n}") { w.event = format!("Sys:WorkbenchTab:{n}"); }
            if leaf(&id) == "Icon" { if let (Some(icon), Some(art)) = (bench.tabs.get(n), &mut w.art) { art.tex = Some(texture_path(icon)); } }
            if n == bench.tab { selected(w); }
        }
        // The filters: checked when on.
        let filter = if id.contains("/FilterCraftableRecipes/") || leaf(&id) == "FilterCraftableRecipes" { Some((0, bench.craftable_only)) }
            else if id.contains("/FilterLockedRecipes/") || leaf(&id) == "FilterLockedRecipes" { Some((1, bench.locked_only)) } else { None };
        if let Some((n, on)) = filter {
            if matches!(leaf(&id), "FilterCraftableRecipes" | "FilterLockedRecipes") { w.event = format!("Sys:WorkbenchFilter:{n}"); }
            if on && w.state.is_some() { if let Some(art) = &mut w.art { art.tex = Some(CHECKED.into()); } }
        }
        match leaf(&id) {
            "Infotext_ChooseRecipe" => set_text(w, "Choose a recipe from the list."),
            "Infotext_NoRecipeAvailable" => set_text(w, "No recipe of this kind is known."),
            "UseableItems" => set_text(w, "Usable items"),
            "RecipeOrigin" => set_text(w, &bench.detail.as_ref().and_then(|d| d.origin.as_ref()).map_or(String::new(), |(_, name)| name.clone())),
            "MoneyDisplayRC" => set_text(w, &bench.detail.as_ref().map_or(String::new(), |d| d.price_rc.to_string())),
            "amount" => set_text(w, &bench.detail.as_ref().map_or(String::new(), |d| d.amount.to_string())),
            "decrease_amount_button" => w.event = "Sys:CraftAmount:-1".into(),
            "increase_amount_button" => w.event = "Sys:CraftAmount:1".into(),
            "min" => w.event = "Sys:CraftAmount:min".into(),
            "max" => w.event = "Sys:CraftAmount:max".into(),
            // Nothing to craft with: Craft is disabled (its face, no click).
            "CraftButton" | "CraftButtonRC" if !ready => w.enabled = false,
            _ => {}
        }
        if id.contains("/RecipeOriginBG/OriginIconLabel") && leaf(&id) == "Icon" {
            if let (Some((icon, _)), Some(art)) = (bench.detail.as_ref().and_then(|d| d.origin.as_ref()), &mut w.art) { art.tex = Some(texture_path(icon)); }
        }
        if let (Some(d), Some(n)) = (&bench.detail, id.split('/').find_map(|p| p.strip_prefix("ItemSlot")).and_then(|n| n.parse::<usize>().ok())) {
            if id.contains("/RecipesInfotextArea/") && leaf(&id) == "Icon0" {
                if let Some(icon) = d.placeholders.get(n) { extra.extend(icon_layers(w, icon)); set_text(w, ""); }
            }
        }
    }
    layout.widgets.extend(extra);
    // The list.
    let list = format!("{BOOK}/RecipesListBackground/CategoriesList");
    let mut y = LIST_TOP;
    for (n, row) in bench.rows.iter().enumerate().skip(bench.scroll) {
        let template = format!("{list}/{}", match row.kind { RowKind::Category => "TemplateCategory", RowKind::Sub => "TemplateSubCategory", RowKind::Book => "TemplateSubSubCategory", RowKind::Locked => "TemplateSubSubCategoryLocked" });
        let Some(root) = source.widgets.iter().find(|w| w.id == template) else { continue };
        let height = row_height(row.kind);
        if y + height > LIST_BOTTOM + 0.5 { break; }
        let replacement = format!("{list}/Row{n}");
        for mut w in clone_rows(source, &template, &replacement, y - root.rect[1]) {
            if w.id == replacement {
                w.event = match row.kind { RowKind::Category | RowKind::Sub => format!("Sys:Category:{}", row.id), _ => format!("Sys:Recipe:{}", row.id) };
            }
            if w.text.is_some() { set_text(&mut w, &row.title); }
            if row.selected { selected(&mut w); }
            layout.widgets.push(w);
        }
        y += height;
    }
    super::super::quests::place_handle(&mut layout, &format!("{list}/ScrollBarBackground/ScrollBarBoundaries"), bench.scroll, bench.rows.len());
    let Some(detail) = &bench.detail else { return layout };
    // The book's title and text.
    let info = format!("{BOOK}/RecipesInfotextArea/RecipeInfotextList");
    for mut w in clone_rows(source, &format!("{info}/TemplateHeadline"), &format!("{info}/Headline"), 0.0) {
        set_text(&mut w, &detail.title);
        layout.widgets.push(w);
    }
    if let Some(line) = source.widgets.iter().find(|w| w.id == format!("{info}/TemplateInfotext")) {
        let height = line.rect[3] - line.rect[1];
        for (n, text) in wrap(&detail.text, INFO_WIDTH, MOST_INFO_LINES).iter().enumerate() {
            for mut w in clone_rows(source, &format!("{info}/TemplateInfotext"), &format!("{info}/Line{n}"), n as f32 * height) {
                set_text(&mut w, text);
                layout.widgets.push(w);
            }
        }
    }
    if !crafting { return layout; }
    for (n, ingredient) in detail.ingredients.iter().enumerate() {
        for mut w in clone_at(source, &format!("{BENCH}/WorkbenchBackgroundLabel/CraftingAreaLabel/TemplateSlot{n}"), &format!("{BENCH}/WorkbenchBackgroundLabel/CraftingAreaLabel/Slot{n}"), 0.0, 0.0) {
            rename(&mut w, "TemplateIcon", "Icon");
            if leaf(&w.id) == "Icon" {
                let mut layers = icon_layers(&mut w, &ingredient.icon);
                // An empty slot shows its ingredient faintly, a short one dimmed.
                if !ingredient.placed { for l in &mut layers { dim(l, 0.2); } }
                else if ingredient.have < ingredient.need { for l in &mut layers { dim(l, 0.5); } }
                make_button(&mut w, &format!("Sys:BenchItem:{n}"));
                set_text(&mut w, "");
                layout.widgets.extend(layers);
            }
            layout.widgets.push(w);
        }
        if let Some(w) = layout.widgets.iter_mut().find(|w| w.id == format!("{BENCH}/WorkbenchBackgroundLabel/Craftingresource_requiredAmount{}", n + 1)) {
            set_text(w, &format!("{}/{}", ingredient.have, ingredient.need));
            if ingredient.have < ingredient.need { grey(w); }
        }
    }
    for mut w in clone_at(source, &format!("{BENCH}/WorkbenchResultLabel/TemplateIcon"), &format!("{BENCH}/WorkbenchResultLabel/Icon"), 0.0, 0.0) {
        let layers = icon_layers(&mut w, &detail.result);
        make_button(&mut w, &format!("Sys:BenchItem:{}", usize::MAX));
        set_text(&mut w, "");
        layout.widgets.extend(layers);
        layout.widgets.push(w);
    }
    let money = format!("{BENCH}/WorkbenchBackgroundLabel/VCDisplay/VCDisplayTemplates/TemplateGoldSilverCopper");
    for mut w in clone_rows(source, &money, &format!("{BENCH}/WorkbenchBackgroundLabel/VCDisplay/Price"), 0.0) {
        let vc = detail.price_vc;
        match leaf(&w.id) {
            "Gold" => set_text(&mut w, &(vc / 10_000).to_string()),
            "Silver" => set_text(&mut w, &(vc / 100 % 100).to_string()),
            "Copper" => set_text(&mut w, &(vc % 100).to_string()),
            _ => {}
        }
        w.hidden = false;
        layout.widgets.push(w);
    }
    layout
}

/// Whether a path has a part starting with `prefix` inside `within`.
fn has_part_prefix(id: &str, prefix: &str, within: &str) -> bool {
    id.contains(within) && id.split('/').any(|p| p.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    #[test]
    fn the_salvage_desk_shows_its_picks_as_buttons() {
        let ids: Vec<(String, &str, bool)> = (0..3).map(|n| (format!("{SALVAGE_AREA}/ItemSlot{n}/TemplateIcon"), "Icon", true)).chain([(format!("{SALVAGE_AREA}/CraftingResourceValueLabel"), "TextLabel", true), (format!("{SALVAGE_AREA}/ButtonSalvage"), "Button", false)]).collect();
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout(super::super::SALVAGE, &refs);
        let salvage = Salvage { items: vec![(7, "Sword".into(), Icon { image: "s.png".into(), frame: "f.png".into(), ..Default::default() }), (9, "Ring".into(), Icon::default())] };
        let prepared = prepare_salvage(&source, &salvage);
        let slot = find(&prepared, &format!("{SALVAGE_AREA}/ItemSlot0/Icon"));
        assert_eq!((slot.kind.as_str(), slot.event.as_str(), slot.hidden), ("Button", "Sys:Unpick:0", false));
        assert_eq!(find(&prepared, &format!("{SALVAGE_AREA}/ItemSlot0/Icon/Frame")).art.as_ref().unwrap().tex.as_deref(), Some("f.png"));
        assert!(prepared.widgets.iter().any(|w| w.id == format!("{SALVAGE_AREA}/ItemSlot1/Icon")) && !prepared.widgets.iter().any(|w| w.id.contains("ItemSlot2") || w.id.contains("Template")));
        assert_eq!(text_of(&prepared, &format!("{SALVAGE_AREA}/CraftingResourceValueLabel")), "2");
    }

    #[test]
    fn the_gem_desk_opens_sockets_and_prices_the_next() {
        let mut ids: Vec<(String, &str, bool)> = vec![(format!("{GEM_ITEM}/ItemSlot/Icon"), "Icon", true), (format!("{GEM_ITEM}/ItemNameTextLabel"), "TextLabel", true), (format!("{GEM_ITEM}/ItemDragAndDropTargetLabel"), "TextLabel", true)];
        for n in 0..3 {
            for part in ["SocketOpen", "SocketClosed", "ItemSlot/Icon", "Button", "BuyButton", "TemplateRC", "TemplateRC/Andermant"] {
                ids.push((format!("{SOCKETS}/Socket{n}/{part}"), if part.ends_with("Andermant") || part.ends_with("Icon") { "TextLabel" } else { "Label" }, part.ends_with("Andermant") || part.ends_with("Icon")));
            }
            ids.push((format!("{SOCKETS}/Headline{n}"), "TextLabel", true));
            ids.push((format!("{SOCKETS}/Headline{n}Disabled"), "TextLabel", true));
        }
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout(super::super::GEMDESK, &refs);
        let gems = Gems { target: Some(("Helmet".into(), Icon { image: "h.png".into(), ..Default::default() })), sockets: vec![
            Socket { open: true, preview: false, template: None, gem: Some(("Ruby".into(), Icon { image: "r.png".into(), ..Default::default() })), price: None },
            Socket { open: false, preview: false, template: None, gem: None, price: Some(75) }, Socket { open: false, preview: false, template: None, gem: None, price: None }] };
        let prepared = prepare_gemdesk(&source, &gems);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert_eq!(text_of(&prepared, &format!("{GEM_ITEM}/ItemNameTextLabel")), "Helmet");
        assert!(ids.contains(&format!("{GEM_ITEM}/ItemSlot/Icon/Image")) && !ids.iter().any(|id| id.ends_with("ItemDragAndDropTargetLabel")));
        assert!(ids.contains(&format!("{SOCKETS}/Socket0/SocketOpen")) && !ids.contains(&format!("{SOCKETS}/Socket0/SocketClosed")));
        assert!(ids.contains(&format!("{SOCKETS}/Socket1/SocketClosed")) && !ids.contains(&format!("{SOCKETS}/Socket1/SocketOpen")));
        assert!(ids.contains(&format!("{SOCKETS}/Socket0/ItemSlot/Icon/Image")) && ids.contains(&format!("{SOCKETS}/Socket0/Button")) && !ids.contains(&format!("{SOCKETS}/Socket0/BuyButton")));
        assert!(ids.contains(&format!("{SOCKETS}/Socket1/BuyButton")) && !ids.contains(&format!("{SOCKETS}/Socket1/Button")) && !ids.contains(&format!("{SOCKETS}/Socket2/BuyButton")));
        assert_eq!(text_of(&prepared, &format!("{SOCKETS}/Socket1/Price/Andermant")), "75");
        assert!(ids.contains(&format!("{SOCKETS}/Headline0")) && !ids.contains(&format!("{SOCKETS}/Headline0Disabled")) && ids.contains(&format!("{SOCKETS}/Headline1Disabled")));
        let empty = prepare_gemdesk(&source, &Gems::default());
        assert!(empty.widgets.iter().any(|w| w.id.ends_with("ItemDragAndDropTargetLabel")) && empty.widgets.iter().any(|w| w.id.ends_with("/Socket0/SocketClosed")));
        assert!(!empty.widgets.iter().any(|w| w.id.contains("/Socket0/Button") || w.id.contains("/Socket0/BuyButton") || w.id.contains("/Socket0/ItemSlot") || w.id.contains("/Socket0/SocketOpen")));
    }

    #[test]
    fn the_workbench_lists_its_tab_s_books_and_details_the_chosen_one() {
        let list = LIST;
        let info = format!("{BOOK}/RecipesInfotextArea");
        let tabs = "Canvas/Label_blur/RecipeBook/Background/TabBarLabel";
        let area = format!("{BENCH}/WorkbenchBackgroundLabel");
        let mut ids: Vec<(String, &str, bool)> = Vec::new();
        for template in ["TemplateCategory", "TemplateSubCategory", "TemplateSubSubCategory", "TemplateSubSubCategoryLocked"] {
            ids.push((format!("{list}/{template}"), "Button", false));
            ids.push((format!("{list}/{template}/normal"), "Label", false));
            ids.push((format!("{list}/{template}/normal/TextLabel"), "TextLabel", true));
        }
        for n in 0..3 {
            ids.push((format!("{tabs}/TabCrafting{n}"), "Button", false));
            ids.push((format!("{tabs}/TabCrafting{n}/normal"), "Label", false));
            ids.push((format!("{tabs}/TabCrafting{n}/normal/Icon"), "Label", false));
        }
        ids.extend([(format!("{info}/RecipeInfotextList/TemplateHeadline"), "TextLabel", true), (format!("{info}/RecipeInfotextList/TemplateInfotext"), "TextLabel", true),
            (format!("{info}/Infotext_ChooseRecipe"), "TextLabel", true), (format!("{info}/Infotext_NoRecipeAvailable"), "TextLabel", true), (format!("{info}/UseableItems"), "TextLabel", true),
            (format!("{info}/ItemSlot0/Icon0"), "Icon", true), (format!("{info}/ItemSlot1/Icon0"), "Icon", true),
            (format!("{area}/CraftingAreaLabel/TemplateSlot0/TemplateIcon"), "Icon", true), (format!("{area}/Craftingresource_requiredAmount1"), "TextLabel", true), (format!("{area}/Craftingresource_requiredAmount2"), "TextLabel", true),
            (format!("{BENCH}/WorkbenchResultLabel/TemplateIcon"), "Icon", true), (format!("{area}/VCDisplay/VCDisplayTemplates/TemplateGoldSilverCopper/Gold"), "TextLabel", true),
            (format!("{area}/CraftButtonRCMoneyDisplay/MoneyDisplayRC"), "TextLabel", true), (format!("{area}/CraftButton"), "Button", false), (format!("{area}/CraftButtonRC"), "Button", false),
            (format!("{area}/BulkCraftingAmount/amount"), "TextLabel", true), (format!("{area}/BulkCraftingAmount/increase_amount_button"), "Button", false),
            (format!("{area}/AnimAccepLabel/SmokeLabel/SmokeLabel09"), "Label", false),
            (format!("{BOOK}/FilterCraftableRecipes"), "Button", false), (format!("{BOOK}/FilterCraftableRecipes/normal"), "Label", false),
            (format!("{info}/RecipeOriginBG"), "Label", false), (format!("{info}/RecipeOriginBG/RecipeOrigin"), "TextLabel", true), (format!("{info}/RecipeOriginBG/OriginIconLabel/Icon"), "Icon", false)]);
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout(super::super::WORKBENCH, &refs);
        let bench = Workbench {
            tab: 1, tabs: vec!["ui/workbench_tab_questtab".into(), "ui/workbench_tab_gemtab".into()],
            rows: vec![Row { id: "gems".into(), title: "Gems".into(), kind: RowKind::Category, selected: true }, Row { id: "ruby".into(), title: "Ruby".into(), kind: RowKind::Book, selected: true },
                Row { id: "opal".into(), title: "Opal".into(), kind: RowKind::Locked, selected: false }],
            detail: Some(Detail { title: "Ruby".into(), text: "Combine rubies.".into(), placeholders: vec![Icon { image: "p.png".into(), ..Default::default() }], recipe: "r1".into(),
                result: Icon { image: "e.png".into(), ..Default::default() }, ingredients: vec![Ingredient { template: "ruby_a".into(), icon: Icon { image: "h.png".into(), ..Default::default() }, have: 2, need: 3, placed: true }],
                price_vc: 20_000, price_rc: 4, amount: 1, most: 0, ..Default::default() }),
            ..Default::default()
        };
        let prepared = prepare_workbench(&source, &bench);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert!(!ids.iter().any(|id| id.contains("Template") || id.contains("/Anim") || id.contains("TabCrafting2")));
        assert_eq!(find(&prepared, &format!("{tabs}/TabCrafting0")).event, "Sys:WorkbenchTab:0");
        assert!(find(&prepared, &format!("{tabs}/TabCrafting1/normal")).hidden);
        assert_eq!(find(&prepared, &format!("{tabs}/TabCrafting1/normal/Icon")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/workbench_tab_gemtab.png"));
        // The list: a category, its book, a locked book, each its template's height.
        assert_eq!((find(&prepared, &format!("{list}/Row0")).event.as_str(), find(&prepared, &format!("{list}/Row0")).rect[1]), ("Sys:Category:gems", LIST_TOP));
        assert_eq!((find(&prepared, &format!("{list}/Row1")).event.as_str(), find(&prepared, &format!("{list}/Row1")).rect[1]), ("Sys:Recipe:ruby", LIST_TOP + 31.0));
        assert_eq!((text_of(&prepared, &format!("{list}/Row2/normal/TextLabel")), find(&prepared, &format!("{list}/Row2")).rect[1]), ("Opal".into(), LIST_TOP + 55.0));
        // The book: its title, text, usable items; no hints.
        assert_eq!(text_of(&prepared, &format!("{info}/RecipeInfotextList/Headline")), "Ruby");
        assert_eq!(text_of(&prepared, &format!("{info}/RecipeInfotextList/Line0")), "Combine rubies.");
        assert!(ids.contains(&format!("{info}/ItemSlot0/Icon0/Image")) && !ids.iter().any(|id| id.contains("/ItemSlot1")));
        assert!(!ids.iter().any(|id| id.ends_with("Infotext_ChooseRecipe") || id.ends_with("Infotext_NoRecipeAvailable")));
        // Its recipe: the ingredient short, the prices, Craft disabled.
        assert_eq!(find(&prepared, &format!("{area}/CraftingAreaLabel/Slot0/Icon/Image")).art.as_ref().unwrap().color[3], 0.5);
        assert_eq!(text_of(&prepared, &format!("{area}/Craftingresource_requiredAmount1")), "2/3");
        assert!(!ids.contains(&format!("{area}/Craftingresource_requiredAmount2")));
        assert_eq!(text_of(&prepared, &format!("{area}/VCDisplay/Price/Gold")), "2");
        assert_eq!((text_of(&prepared, &format!("{area}/CraftButtonRCMoneyDisplay/MoneyDisplayRC")), text_of(&prepared, &format!("{area}/BulkCraftingAmount/amount"))), ("4".into(), "1".into()));
        assert!(!find(&prepared, &format!("{area}/CraftButton")).enabled && !find(&prepared, &format!("{area}/CraftButtonRC")).enabled);
        assert_eq!(find(&prepared, &format!("{area}/BulkCraftingAmount/increase_amount_button")).event, "Sys:CraftAmount:1");
        assert_eq!(find(&prepared, &format!("{BOOK}/FilterCraftableRecipes")).event, "Sys:WorkbenchFilter:0");
        // Nothing known in the tab: the hint says so.
        let empty = prepare_workbench(&source, &Workbench { tabs: bench.tabs.clone(), ..Default::default() });
        assert!(empty.widgets.iter().any(|w| w.id.ends_with("Infotext_NoRecipeAvailable")) && !empty.widgets.iter().any(|w| w.id.ends_with("Infotext_ChooseRecipe")));
        assert!(!find(&empty, &format!("{area}/CraftButton")).enabled && !empty.widgets.iter().any(|w| w.id.contains("/CraftButtonRC")));
        // A locked book: its origin, its text with the unlock item named.
        let locked = prepare_workbench(&source, &Workbench { detail: Some(Detail { title: "Opal".into(), locked: true, origin: Some(("ui/icon_origin_questreward".into(), "Quest reward".into())), ..Default::default() }), ..bench.clone() });
        assert_eq!(text_of(&locked, &format!("{info}/RecipeOriginBG/RecipeOrigin")), "Quest reward");
        assert_eq!(find(&locked, &format!("{info}/RecipeOriginBG/OriginIconLabel/Icon")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/icon_origin_questreward.png"));
        assert!(!prepared.widgets.iter().any(|w| w.id.contains("/RecipeOriginBG")));
        assert_eq!(unlock_text("Add the {0:s:itemname} to your workbench. Complete the achievement: {1:s}.", "Opal"), "Add the Opal to your workbench. Complete the achievement.");
        assert_eq!((origin_name("ui/icon_origin_questreward"), origin_name("ui/icon_origin_event")), ("Quest reward".into(), "Event".into()));
        assert_eq!(ingredient("ruby_a:5"), ("ruby_a", 5));
        assert_eq!(ingredient("ruby_a"), ("ruby_a", 1));
    }
}
