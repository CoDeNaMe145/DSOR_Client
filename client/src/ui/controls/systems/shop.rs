//! The shop (shopwindow, UI::ShopWidget).
//!
//! A tab per shop, with its icon: the Andermant item shop (global_rc_trader,
//! the offers RequestOffer returns) and the premium shop ("$$$": the
//! Andermant the server sells, OcpOfferResponse). Each tab lists its category
//! tree (_Template_ShopCategory; the open category's children under it); the
//! deals category shows the banner and the shop's highlighted offers, the
//! others their offers as cards two a row (icon, name, price, Buy), the
//! voucher category its code entry. The wheel scrolls the offers; Buy books
//! the offer (an item for Andermant, Transaction; Andermant for money,
//! OcpBooking); the payment button opens the premium shop.
//! SOURCE: DSOR_Server services/commerce.py (RequestOffer by shop name,
//!   LOCAL_OCP_OFFERS, OcpBooking "external"), protocol/shop_protocol.py
//!   (OFFER_CATEGORY_IDS); the client's ShopWidget element names (ShopTab,
//!   icon_tab, ui/icon_tab_ocp_andermant, ui/icon_tab_cash, DealsPage,
//!   OffersPage, TemplateListEntry, InlayAndermant, InlayCash, VoucherLabel).
use super::super::template::{clone_rows, has_part, icon_layers, image_art, is_template, leaf, selected, set_text, without, Icon};
use super::Sources;
use crate::gameplay::systems::texture_path;
use crate::ui::Layout;

pub(in crate::ui) const ITEM_SHOP: &str = "global_rc_trader";
const PREMIUM_SHOP: &str = "$$$";
/// The shops of the tabs, their icons.
const SHOPS: [(&str, &str); 2] = [(ITEM_SHOP, "ui/icon_tab_ocp_andermant"), (PREMIUM_SHOP, "ui/icon_tab_cash")];
pub(in crate::ui) const INLAY: &str = "Canvas/Shopwindow/BackGroundLabel/Frame/Frame/Frame/Inlay";
pub(in crate::ui) const CATEGORIES: &str = "Canvas/Shopwindow/BackGroundLabel/Frame/Frame/Frame/Inlay/CategoriesListBackground/CategoriesList";
pub(in crate::ui) const OFFERS: &str = "Canvas/Shopwindow/BackGroundLabel/Frame/Frame/Frame/Inlay/OffersPage/LongList";
/// Category rows from 130 (32 and 23 high); card rows of 81 in the lists.
const CATEGORY_TOP: f32 = 130.0;
const CATEGORY_BOTTOM: f32 = 600.0;
const CARD_HEIGHT: f32 = 81.0;
const OFFERS_TOP: f32 = 130.0;
const OFFERS_BOTTOM: f32 = 623.0;
const DEALS_TOP: f32 = 352.0;
const DEALS_BOTTOM: f32 = 627.0;
pub(in crate::ui) const VISIBLE_ROWS: usize = 6;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct CategoryRow {
    pub id: String,
    pub title: String,
    pub sub: bool,
    pub selected: bool,
}

/// An offer's card.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Card {
    /// What Buy books: the offer id, or the premium offer's product key.
    pub key: String,
    pub name: String,
    pub icon: Icon,
    /// Andermant; or the money price ("1.99 €") of a premium offer.
    pub andermant: u32,
    pub cash: String,
    pub flag: String,
    /// How many the offer gives (written on its icon past one).
    pub amount: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) enum Page { #[default] Offers, Deals, Voucher }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Shop {
    pub tab: usize,
    pub categories: Vec<CategoryRow>,
    pub page: Page,
    pub cards: Vec<Card>,
    pub banner: String,
    /// The offers have not arrived yet.
    pub loading: bool,
    /// Card rows scrolled off the list's top.
    pub scroll: usize,
}

pub(super) fn shop(sources: &Sources) -> Shop {
    let catalog = sources.catalog;
    let state = sources.state;
    let tab = state.shop_tab.min(SHOPS.len() - 1);
    let (shop, _) = SHOPS[tab];
    let tree: Vec<_> = catalog.shop_categories.iter().filter(|c| c.shop == shop).collect();
    let mut tops: Vec<_> = tree.iter().filter(|c| c.parent.is_empty()).collect();
    tops.sort_by_key(|c| (c.priority, c.id.clone()));
    let chosen = state.shop_category.get(tab).cloned().flatten().filter(|c| tree.iter().any(|k| &k.id == c))
        .or_else(|| tops.first().map(|c| c.id.clone())).unwrap_or_default();
    let chosen_top = tree.iter().find(|c| c.id == chosen).map(|c| if c.parent.is_empty() { c.id.clone() } else { c.parent.clone() }).unwrap_or_default();
    let mut categories = Vec::new();
    for top in &tops {
        categories.push(CategoryRow { id: top.id.clone(), title: top.title.clone(), sub: false, selected: top.id == chosen });
        if top.id != chosen_top { continue; }
        let mut subs: Vec<_> = tree.iter().filter(|c| c.parent == top.id).collect();
        subs.sort_by_key(|c| (c.priority, c.id.clone()));
        categories.extend(subs.iter().map(|c| CategoryRow { id: c.id.clone(), title: c.title.clone(), sub: true, selected: c.id == chosen }));
    }
    let category = tree.iter().find(|c| c.id == chosen);
    let page = if chosen.eq_ignore_ascii_case("DEALS") { Page::Deals } else if category.is_some_and(|c| c.tabs.iter().any(|t| t == "EVoucher")) { Page::Voucher } else { Page::Offers };
    let mut cards = Vec::new();
    let mut loading = false;
    if shop == ITEM_SHOP {
        // The category's offer kinds (its children's when it names none).
        let mut kinds: Vec<u16> = category.map(|c| c.offers.iter().filter_map(|o| catalog.offer_categories.get(o).copied()).collect()).unwrap_or_default();
        if kinds.is_empty() {
            kinds = tree.iter().filter(|c| c.parent == chosen).flat_map(|c| c.offers.iter().filter_map(|o| catalog.offer_categories.get(o).copied())).collect();
        }
        match sources.net.npc_dialogue.offers.get(ITEM_SHOP) {
            Some(offer) => {
                let all: Vec<_> = offer.offers.iter().flatten().collect();
                let picked: Vec<_> = if page == Page::Deals {
                    let deals: Vec<_> = all.iter().filter(|o| o.is_recommended || o.is_new || o.timed_offer || !o.discount_events.is_empty()).copied().collect();
                    if deals.is_empty() { all.iter().take(6).copied().collect() } else { deals }
                } else {
                    all.iter().filter(|o| kinds.contains(&o.category)).copied().collect()
                };
                cards = picked.into_iter().map(|o| Card {
                    key: o.id.clone(), name: sources.name(&o.item, None), icon: sources.icon(&o.item, None), andermant: o.price_rc * o.amount.max(1) / o.amount.max(1),
                    cash: String::new(), flag: if o.is_new { "new".into() } else if o.is_recommended { "recommended".into() } else { String::new() }, amount: o.amount,
                }).collect();
            }
            None => loading = true,
        }
    } else if category.is_some_and(|c| c.tabs.iter().any(|t| t == "RealCurrency")) || chosen == "Andermant" {
        // The premium shop's Andermant: the server's offers, a product key each
        // (1_realCurrency_1500.0000_0_NONE).
        cards = sources.net.ocp_offers.iter().map(|o| {
            let amount = o.unknown_string_3.split("realCurrency_").nth(1).and_then(|s| s.split('.').next()).and_then(|n| n.parse::<u64>().ok());
            Card {
                key: o.unknown_string_3.clone(), name: amount.map_or_else(|| o.unknown_string_3.clone(), |a| format!("{} Andermant", thousands(a))),
                icon: Icon { image: texture_path("icons/money_rc_pile"), ..Default::default() }, andermant: 0,
                cash: format!("{:.2} {}", o.unknown_f32_1, o.unknown_string_2), flag: if o.featured { "recommended".into() } else { String::new() }, amount: 1,
            }
        }).collect();
        loading = sources.net.ocp_offers.is_empty() && !sources.net.offline;
    }
    let banner = catalog.shop_banners.iter().find(|b| b.category.eq_ignore_ascii_case(&chosen) || b.category.is_empty()).map(|b| b.texture.clone()).unwrap_or_default();
    let scroll = state.scroll.get(OFFERS).copied().unwrap_or(0);
    Shop { tab, categories, page, cards, banner, loading, scroll }
}

pub(in crate::ui) fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 { out.push(','); }
        out.push(c);
    }
    out
}

/// The shop: tabs with their shops' icons, the category tree, the open
/// category's page and cards.
pub(super) fn prepare_shop(source: &Layout, shop: &Shop) -> Layout {
    let pages = |id: &str| {
        let offers = has_part(id, "OffersPage");
        let deals = has_part(id, "DealsPage");
        let voucher = has_part(id, "VoucherLabel");
        (offers && shop.page != Page::Offers) || (deals && shop.page != Page::Deals) || (voucher && shop.page != Page::Voucher)
    };
    let mut layout = without(source, |id| {
        is_template(id) || id.ends_with("/Caption") || pages(id) || has_part(id, "ShopTab3")
            || has_part(id, "CinemaBackground") || has_part(id, "TieredSpenderLabel") || has_part(id, "PaymentMethodSettingsLabel")
            || has_part(id, "SteamPaymentButton") || has_part(id, "ExternalPaymentButton") || has_part(id, "MoreOffersIndicator")
            || has_part(id, "FlagLabelCategory") || has_part(id, "FlagLabelSubcategory") || has_part(id, "DealsCounterLabel")
            || (has_part(id, "LoadingLabel") && !shop.loading) || (id.contains("/AdvertisementArtwork/ShowButton"))
    });
    for w in &mut layout.widgets {
        let id = w.id.clone();
        // The tabs: their shops' icons; the open one pressed.
        if let Some(n) = id.split('/').find_map(|p| p.strip_prefix("ShopTab")).and_then(|n| n.parse::<usize>().ok()) {
            if n - 1 == shop.tab { selected(w); }
            if leaf(&id) == "icon_tab" {
                if let Some((_, icon)) = SHOPS.get(n - 1) { w.art = Some(image_art(w.rect, texture_path(icon))); w.hidden = false; }
            }
        }
        if id.ends_with("/DealsPage/AdvertisementArtwork") && !shop.banner.is_empty() { w.art = Some(image_art(w.rect, texture_path(&shop.banner))); }
        if leaf(&id) == "FeedbackText" { set_text(w, "Enter the code of a voucher to redeem it."); }
    }
    // The categories: the tops, the open one's children under it.
    let mut y = CATEGORY_TOP;
    for (n, category) in shop.categories.iter().enumerate() {
        let template = format!("{CATEGORIES}/{}", if category.sub { "TemplateSubCategory" } else { "TemplateCategory" });
        let Some(root) = source.widgets.iter().find(|w| w.id == template) else { continue };
        let height = root.rect[3] - root.rect[1];
        if y + height > CATEGORY_BOTTOM { break; }
        let replacement = format!("{CATEGORIES}/Cat{n}");
        for mut w in clone_rows(source, &template, &replacement, y - root.rect[1]) {
            if w.id == replacement { w.event = format!("Sys:ShopCategory:{}", category.id); }
            if w.text.is_some() { set_text(&mut w, &category.title); }
            if category.selected { selected(&mut w); }
            layout.widgets.push(w);
        }
        y += height;
    }
    // The cards, two a row.
    let (list, top, bottom, skip) = match shop.page {
        Page::Offers => (format!("{INLAY}/OffersPage"), OFFERS_TOP, OFFERS_BOTTOM, shop.scroll),
        Page::Deals => (format!("{INLAY}/DealsPage"), DEALS_TOP, DEALS_BOTTOM, 0),
        Page::Voucher => return layout,
    };
    let template = format!("{INLAY}/TemplateListEntry");
    let Some(root) = source.widgets.iter().find(|w| w.id == template) else { return layout };
    for (row, pair) in shop.cards.chunks(2).enumerate().skip(skip) {
        let y = top + (row - skip) as f32 * CARD_HEIGHT;
        if y + CARD_HEIGHT > bottom + 0.5 { break; }
        for (side, card) in pair.iter().enumerate() {
            let entry = format!("{template}/TemplateItemEntry{}", side + 1);
            let replacement = format!("{list}/Row{row}/Entry{side}");
            let cash = !card.cash.is_empty();
            for mut w in clone_rows(source, &entry, &replacement, y - root.rect[1]) {
                let rest = w.id[replacement.len()..].to_owned();
                if rest.starts_with("/InlaySales") || rest.starts_with("/GlowLabel") || rest.starts_with("/background_") { continue; }
                if (cash && rest.starts_with("/InlayAndermant")) || (!cash && rest.starts_with("/InlayCash")) { continue; }
                if has_part(&rest, "FlagLabelDiscount") || has_part(&rest, "FlagLabelBonus") || has_part(&rest, "FlagLabelSmS") || has_part(&rest, "FlagLabelPhone")
                    || has_part(&rest, "FlagLabelTerminal") || has_part(&rest, "FlagLabelMostPopular") { continue; }
                if has_part(&rest, "FlagLabelNew") && card.flag != "new" { continue; }
                if has_part(&rest, "FlagLabelRecommend") && card.flag != "recommended" { continue; }
                if leaf(&rest) == "DiscountTimer" { continue; }
                w.hidden = false;
                match leaf(&w.id) {
                    "ItemDescription" => set_text(&mut w, &card.name),
                    "andermant_display" => set_text(&mut w, &thousands(card.andermant as u64)),
                    "price_label" => set_text(&mut w, &card.cash),
                    "BuyButton" => w.event = format!("Sys:Buy:{}", card.key),
                    "Icon" if rest.ends_with("/Icon") => {
                        layout.widgets.extend(icon_layers(&mut w, &card.icon));
                        set_text(&mut w, &if card.amount > 1 { card.amount.to_string() } else { String::new() });
                    }
                    _ => {}
                }
                layout.widgets.push(w);
            }
        }
    }
    let rows = shop.cards.len().div_ceil(2);
    if shop.page == Page::Offers {
        super::super::quests::place_handle(&mut layout, &format!("{OFFERS}/ScrollBarBackground/ScrollBarBoundaries"), shop.scroll, rows);
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::template::fixture::{find, layout, text_of};

    #[test]
    fn the_shop_lists_its_tabs_categories_and_cards() {
        let card = format!("{INLAY}/TemplateListEntry/TemplateItemEntry1");
        let ids: Vec<(String, &str, bool)> = vec![(format!("{INLAY}/Tabs/ShopTab1"), "Button", false), (format!("{INLAY}/Tabs/ShopTab1/pressed"), "Label", false), (format!("{INLAY}/Tabs/ShopTab1/Label_empty/Label_empty1/icon_tab"), "Label", false),
            (format!("{INLAY}/Tabs/ShopTab1/Caption"), "TextLabel", true), (format!("{INLAY}/Tabs/ShopTab3"), "Button", false),
            (format!("{CATEGORIES}/TemplateCategory"), "Button", false), (format!("{CATEGORIES}/TemplateCategory/TextLabel"), "TextLabel", true),
            (format!("{CATEGORIES}/TemplateSubCategory"), "Button", false), (format!("{CATEGORIES}/TemplateSubCategory/TextLabel"), "TextLabel", true),
            (format!("{INLAY}/TemplateListEntry"), "Label", false), (card.clone(), "Label", false), (format!("{card}/InlayAndermant/Label_empty/Label_empty/ItemDescription"), "TextLabel", true),
            (format!("{card}/InlayAndermant/MoneyDisplay/RealCurrency/andermant_display"), "TextLabel", true), (format!("{card}/InlayAndermant/BuyButton"), "Button", false),
            (format!("{card}/InlayAndermant/Icon"), "Icon", false), (format!("{card}/InlayCash/BuyButton"), "Button", false), (format!("{card}/InlayCash/MoneyDisplay/Prices/price_label"), "TextLabel", true),
            (format!("{INLAY}/TemplateListEntry/TemplateItemEntry2"), "Label", false), (format!("{INLAY}/TemplateListEntry/TemplateItemEntry2/InlayAndermant/BuyButton"), "Button", false),
            (format!("{INLAY}/OffersPage/SortPriceButton"), "Button", false), (format!("{INLAY}/DealsPage/AdvertisementArtwork"), "Label", false), (format!("{INLAY}/VoucherLabel/RedeemButton"), "Button", false)];
        let refs: Vec<(&str, &str, bool)> = ids.iter().map(|(a, b, c)| (a.as_str(), *b, *c)).collect();
        let source = layout("shopwindow", &refs);
        let shop = Shop { tab: 0, categories: vec![CategoryRow { id: "general".into(), title: "General".into(), sub: false, selected: true }, CategoryRow { id: "potion".into(), title: "Potions".into(), sub: true, selected: false }],
            page: Page::Offers, cards: vec![Card { key: "o1".into(), name: "Health potion".into(), andermant: 1500, icon: Icon { image: "p.png".into(), ..Default::default() }, ..Default::default() },
                Card { key: "o2".into(), name: "Mana potion".into(), andermant: 20, ..Default::default() }, Card { key: "o3".into(), name: "Elixir".into(), ..Default::default() }], ..Default::default() };
        let prepared = prepare_shop(&source, &shop);
        let ids: Vec<_> = prepared.widgets.iter().map(|w| w.id.clone()).collect();
        assert_eq!(find(&prepared, &format!("{INLAY}/Tabs/ShopTab1/Label_empty/Label_empty1/icon_tab")).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/icon_tab_ocp_andermant.png"));
        assert_eq!(find(&prepared, &format!("{INLAY}/Tabs/ShopTab1/pressed")).state.as_deref(), Some("normal"));
        assert!(!ids.iter().any(|id| id.ends_with("/Caption") || id.contains("ShopTab3") || id.contains("DealsPage") || id.contains("VoucherLabel") || is_template(id)));
        assert_eq!((find(&prepared, &format!("{CATEGORIES}/Cat1")).event.as_str(), text_of(&prepared, &format!("{CATEGORIES}/Cat1/TextLabel"))), ("Sys:ShopCategory:potion", "Potions".into()));
        let entry = format!("{INLAY}/OffersPage/Row0/Entry0");
        assert_eq!(text_of(&prepared, &format!("{entry}/InlayAndermant/Label_empty/Label_empty/ItemDescription")), "Health potion");
        assert_eq!(text_of(&prepared, &format!("{entry}/InlayAndermant/MoneyDisplay/RealCurrency/andermant_display")), "1,500");
        assert_eq!(find(&prepared, &format!("{entry}/InlayAndermant/BuyButton")).event, "Sys:Buy:o1");
        assert!(ids.contains(&format!("{entry}/InlayAndermant/Icon/Image")) && !ids.iter().any(|id| id.contains("InlayCash")));
        assert_eq!(find(&prepared, &format!("{INLAY}/OffersPage/Row1/Entry0")).rect[1], 192.0 + OFFERS_TOP - 192.0 + CARD_HEIGHT);
        assert!(ids.contains(&format!("{INLAY}/OffersPage/Row0/Entry1/InlayAndermant/BuyButton")));
        let premium = prepare_shop(&source, &Shop { tab: 1, cards: vec![Card { key: "1_realCurrency_1500.0000_0_NONE".into(), name: "1,500 Andermant".into(), cash: "1.99 €".into(), ..Default::default() }], ..Default::default() });
        assert_eq!(text_of(&premium, &format!("{INLAY}/OffersPage/Row0/Entry0/InlayCash/MoneyDisplay/Prices/price_label")), "1.99 €");
        assert!(!premium.widgets.iter().any(|w| w.id.contains("/Row0/Entry0/InlayAndermant")));
        let voucher = prepare_shop(&source, &Shop { page: Page::Voucher, ..Default::default() });
        assert!(voucher.widgets.iter().any(|w| w.id.contains("VoucherLabel")) && !voucher.widgets.iter().any(|w| w.id.contains("OffersPage")));
        assert_eq!(thousands(114_000), "114,000");
    }
}
