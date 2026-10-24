//! The money in the top left corner (currencybar) and its currency bag
//! (currencybag), as the 2026 client shows them in game.
//!
//! The bar shows the Andermant, the gold, silver and copper, and one more
//! currency the player picked in the bag -- the Badges of Honor until then --
//! each shortened to its place (`clamped`). Its dropdown (the picked currency)
//! and its bag button open the bag under it: the Glyphs of Power, then every
//! shops' currency (SystemsCatalog::currencies), each with its whole balance.
//! A click on one shows it on the bar and keeps it in the preferences.
//!
//! EVIDENCE: release261 UI::CurrencyBarWidget (currencybarwidget.cc):
//!   - Load (0x1403D93E0) keeps four money displays, LabelDefault,
//!     LabelIngamePayment, LabelDefaultRCOnly and LabelIngamePaymentRCOnly, and
//!     shows one (0x1403DA620): in game with payment offered, LabelIngamePayment
//!     (0x1403DA038; the character screen's is LabelIngamePaymentRCOnly,
//!     crate::ui::selection). It hides the bag button's glow (shown a moment
//!     when a currency grows).
//!   - GetClampedIntString(value, maxDigits) (0x1403D8BA8): the Andermant with 9
//!     digits (0x1403DA7F0), the gold with 5 (0x1403DAA44: copper / 10000; the
//!     silver and copper "%d" of the rest), the picked currency with 10
//!     (0x1403DAE14).
//!   - The pick (0x1403DA32C): its icon, its amount; the honour by default, read
//!     back from the settings' "/Settings/CurrencyDisplay" currency (0x14020F828).
//!   UI::CurrencyBagWidget (currencybagwidget.cc): Load (0x1403D70B0) clones
//!   CurrencyTemplate for "UpgradeCurrency" (ui/icon_currency_glyph), then for
//!   each item currency (Managers::TemplateManager::LoadItemCurrency), amounts
//!   "%llu" (0x1403D7A04); the frame grows with the rows (0x1403D74B0); it opens
//!   centred under the bar's CurrencybagReferenceLabel (0x1403D7674); a row's
//!   CurrencyChange event picks its currency, saves it and closes the bag
//!   (0x1403D6D90).
use std::collections::HashMap;

use bevy::prelude::*;

use super::template::{clone_rows, leaf, make_button, map_y, set_text, shift};
use super::{Action, Widget, Windows};
use crate::gameplay::systems::{texture_path, SystemsCatalog};
use crate::network::net::Net;
use crate::ui::{Layout, Loaded, Window2018};

pub(in crate::ui) const BAR: &str = "currencybar";
pub(in crate::ui) const BAG: &str = "currencybag";

/// The bar's money display in game.
const SHOWN: &str = "Canvas/LabelIngamePayment";
/// Its alternatives, for other situations (no payment offered, the character screen).
const OTHERS: [&str; 3] = ["Canvas/LabelDefault", "Canvas/LabelDefaultRCOnly", "Canvas/LabelIngamePaymentRCOnly"];
/// The display's plate (currencybar_bg), and the dropdown over the picked
/// currency (its hover face fills the row).
const PLATE: &str = "Canvas/LabelIngamePayment/Background";
const DROPDOWN: &str = "Canvas/LabelIngamePayment/ButtonDropdown";
/// The bag button's highlight, shown a moment when a currency grows.
const HIGHLIGHT: &str = "Canvas/LabelIngamePayment/CurrencybagButton/Label_IconAndGlow/glow";
pub(in crate::ui) const PICKED_AMOUNT: &str = "Canvas/LabelIngamePayment/MoneyDisplay/CurrencyDisplay/template_amount";
const PICKED_ICON: &str = "Canvas/LabelIngamePayment/MoneyDisplay/CurrencyDisplay/template_icon";
const REFERENCE: &str = "Canvas/LabelIngamePayment/CurrencybagReferenceLabel";
/// CurrencybagReferenceLabel's rect, when the bar's layout is not at hand.
const REFERENCE_RECT: [f32; 4] = [68.8, 28.4, 203.1, 39.1];

const CENTER: &str = "Canvas/CenterLabel";
const MENU: &str = "Canvas/CenterLabel/MenuUpLabel";
const FRAME: &str = "Canvas/CenterLabel/MenuUpLabel/Currencybag";
const LIST: &str = "Canvas/CenterLabel/MenuUpLabel/Currencybag/List";
const TEMPLATE: &str = "Canvas/CenterLabel/MenuUpLabel/Currencybag/List/CurrencyTemplate";
/// How far a row's amount stops short of the row's right end (the frame's
/// right edge starts there): the amount may use the row's width.
const AMOUNT_PAD: f32 = 6.0;

/// The currencies the client's own code names, and their icons.
pub(in crate::ui) const GLYPHS: &str = "UpgradeCurrency";
pub(in crate::ui) const HONOR: &str = "money_hc_no_auto_use";
pub(in crate::ui) const DRAKEN: &str = "e_chr_2012_event_currency";
const GLYPHS_ICON: &str = "textures/ui/icon_currency_glyph.png";
const HONOR_ICON: &str = "textures/ui/icon_currency_honor.png";
const DRAKEN_ICON: &str = "textures/ui/icon_currency_event.png";

/// GetClampedIntString: the value with at most `digits` digits, by thousands
/// (k, M, G) past them.
pub(in crate::ui) fn clamped(mut value: u64, digits: u32) -> String {
    let limit = 10u64.saturating_pow(digits);
    let mut steps = 0;
    while steps < 3 && value >= limit {
        value /= 1000;
        steps += 1;
    }
    format!("{value}{}", ["", "k", "M", "G"][steps])
}

/// What the bar and the bag count: the wallet (Andermant, gold in copper,
/// honour, draken), the Glyphs of Power and the keyed game currencies.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Wallet {
    pub andermant: u64,
    pub copper: u64,
    pub honor: u64,
    pub draken: u64,
    pub glyphs: u64,
    pub keyed: HashMap<String, u64>,
}

impl Wallet {
    pub(in crate::ui) fn of(net: &Net) -> Self {
        let w = net.wallet.unwrap_or_default();
        Self { andermant: w[0], copper: w[1], honor: w[2], draken: w[4], glyphs: net.glyphs.unwrap_or(0), keyed: net.game_currencies.clone() }
    }

    /// A currency of the bag by its id: 0 for one the server never sent.
    pub(in crate::ui) fn balance(&self, id: &str) -> u64 {
        match id {
            GLYPHS => self.glyphs,
            HONOR => self.honor,
            DRAKEN => self.draken,
            other => self.keyed.get(other).copied().unwrap_or(0),
        }
    }
}

/// A currency's icon: the client's own three, else its shops' (the catalog).
fn icon_of(id: &str, catalog: Option<&SystemsCatalog>) -> Option<String> {
    match id {
        GLYPHS => Some(GLYPHS_ICON.into()),
        HONOR => Some(HONOR_ICON.into()),
        DRAKEN => Some(DRAKEN_ICON.into()),
        _ => catalog?.currencies.iter().find(|c| c.id == id).map(|c| texture_path(&c.icon)),
    }
}

/// The currency the bar shows beside the money: the player's pick
/// (Preferences::currency), resolved; an empty or unknown pick is the honour.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Shown {
    pub id: String,
    pub icon: String,
}

impl Shown {
    fn of(picked: &str, catalog: Option<&SystemsCatalog>) -> Self {
        match icon_of(picked, catalog) {
            Some(icon) => Self { id: picked.into(), icon },
            None => Self { id: HONOR.into(), icon: HONOR_ICON.into() },
        }
    }
}

/// The bar follows the player's pick: rebuilt (its icon) when the pick, or
/// what it resolves to once the catalog loads, changes; nothing otherwise.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync_bar(
    mut commands: Commands,
    settings: Res<super::settings::Settings>,
    state: Res<super::systems::State>,
    catalogs: Res<Assets<SystemsCatalog>>,
    mut shown: ResMut<Shown>,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018)>,
    mut seen: Local<Option<(String, bool)>>,
) {
    let catalog = catalogs.get(&state.catalog);
    let picked = &settings.current.currency;
    if seen.as_ref().is_some_and(|(p, c)| p == picked && *c == catalog.is_some()) { return; }
    *seen = Some((picked.clone(), catalog.is_some()));
    let next = Shown::of(picked, catalog);
    if *shown == next { return; }
    let rebuild = shown.icon != next.icon;
    *shown = next;
    if !rebuild { return; }
    for (entity, window) in &windows {
        if window.0 == BAR { commands.entity(entity).despawn_related::<Children>(); }
    }
    for i in 0..loaded.layouts.len() {
        if layouts.get(&loaded.layouts[i].0).is_some_and(|l| l.window == BAR) { loaded.spawned[i] = false; }
    }
}

/// Whether the bar draws a widget of a money display: the in-game one without
/// the bag button's highlight. None outside the displays (the anchors).
pub(in crate::ui) fn bar_part(id: &str) -> Option<bool> {
    if subtree(id, SHOWN) { return Some(!subtree(id, HIGHLIGHT)); }
    OTHERS.iter().any(|d| subtree(id, d)).then_some(false)
}

/// Whether `id` is `root` or under it.
fn subtree(id: &str, root: &str) -> bool {
    id.strip_prefix(root).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The bar: the in-game money display, the picked currency's icon.
pub(in crate::ui) fn prepare_bar(source: &Layout, shown: &Shown) -> Layout {
    let mut layout = source.clone();
    for w in &mut layout.widgets {
        if let Some(drawn) = bar_part(&w.id) { w.hidden = !drawn; }
        if w.id == PICKED_ICON && !shown.icon.is_empty() {
            if let Some(art) = &mut w.art { art.tex = Some(shown.icon.clone()); }
        }
    }
    // The plate the money lies on and the dropdown's hover fill come after the
    // money in the layout: drawn first, under it.
    // FAILURE (2026-10-23): in source order the opaque plate hid the money, the
    //   payment button and the picked currency; the hover fill, the picked one.
    let below = |w: &Widget| [PLATE, DROPDOWN].iter().any(|root| subtree(&w.id, root));
    let (under, rest): (Vec<Widget>, Vec<Widget>) = layout.widgets.into_iter().partition(below);
    let at = rest.iter().position(|w| w.id == SHOWN).map_or(0, |i| i + 1);
    layout.widgets = rest;
    layout.widgets.splice(at..at, under);
    layout
}

/// The bag's rows: each currency, its icon and balance.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui) struct Bag {
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Row {
    pub id: String,
    pub icon: String,
    pub amount: u64,
}

pub(in crate::ui) fn bag(sources: &super::systems::Sources) -> Bag {
    let wallet = Wallet::of(sources.net);
    let listed = std::iter::once((GLYPHS, GLYPHS_ICON.to_owned()))
        .chain(sources.catalog.currencies.iter().map(|c| (c.id.as_str(), texture_path(&c.icon))));
    Bag { rows: listed.map(|(id, icon)| Row { id: id.to_owned(), icon, amount: wallet.balance(id) }).collect() }
}

/// A y of the authored one-row frame, the frame grown by `extra`: what lies
/// below its middle moves down, what spans it stretches (half a pixel of
/// slack: the top edge ends a hair inside it).
fn grown(y: f32, top: f32, bottom: f32, extra: f32) -> f32 {
    if y <= top + 0.5 { y } else if y >= bottom - 0.5 { y + extra } else { y + extra * (y - top) / (bottom - top) }
}

/// The bag, a row per currency, under the bar's reference label.
pub(in crate::ui) fn prepare_bag(source: &Layout, bag: &Bag, bar: Option<&Layout>) -> Layout {
    let rect = |id: &str| source.widgets.iter().find(|w| w.id == id).map(|w| w.rect);
    let (Some(template), Some(frame), Some(menu)) = (rect(TEMPLATE), rect(FRAME), rect(MENU)) else { return source.clone() };
    let row = template[3] - template[1];
    let extra = row * bag.rows.len().saturating_sub(1) as f32;
    // MenuUpLabel centred under the reference label, its top on the label's bottom.
    let (reference, k) = bar.and_then(|b| b.widgets.iter().find(|w| w.id == REFERENCE)).map_or((REFERENCE_RECT, 0.0), |w| (w.rect, w.k));
    let dx = (reference[0] + reference[2] - menu[0] - menu[2]) * 0.5;
    let dy = reference[3] - menu[1];
    let prefix = format!("{TEMPLATE}/");
    let mut layout = Layout { window: source.window.clone(), widgets: Vec::new() };
    for widget in &source.widgets {
        if widget.id.starts_with(&prefix) { continue; }
        if widget.id == TEMPLATE {
            for (i, currency) in bag.rows.iter().enumerate() {
                let name = format!("{LIST}/Currency{i}");
                for mut w in clone_rows(source, TEMPLATE, &name, row * i as f32) {
                    if w.id == name { make_button(&mut w, &format!("Currency:{}", currency.id)); }
                    match leaf(&w.id) {
                        "template_icon" => if let Some(art) = &mut w.art { art.tex = Some(currency.icon.clone()); },
                        "template_amount" => {
                            set_text(&mut w, &currency.amount.to_string());
                            w.rect[2] = w.rect[2].max(template[2] - AMOUNT_PAD);
                        }
                        _ => {}
                    }
                    layout.widgets.push(w);
                }
            }
            continue;
        }
        let mut w = widget.clone();
        // The positioning helper's invisible square would take the bar's clicks.
        if w.id == CENTER { w.hidden = true; }
        map_y(&mut w, |y| grown(y, frame[1], frame[3], extra));
        layout.widgets.push(w);
    }
    for w in &mut layout.widgets {
        shift(w, dx, dy);
        w.k = k;
    }
    layout
}

/// The bar's dropdown and bag button open the bag; its rows pick their currency.
pub(super) fn action(window: &str, w: &Widget) -> Option<Action> {
    match window {
        BAR => matches!(w.event.as_str(), "CurrencyBag" | "Currencybag").then_some(Action::Window(BAG)),
        BAG => w.event.strip_prefix("Currency:").map(|id| Action::Currency(id.to_owned())),
        _ => None,
    }
}

/// A press anywhere but on the bag or a button opening it closes the bag
/// (it opens on that button's release).
pub(super) fn dismiss(windows: &mut Windows, top: Option<(Entity, Option<usize>)>) {
    let keep = top.is_some_and(|(e, button)| windows.get(e).is_ok_and(|(_, name, controls, _, _)| name.0 == BAG
        || button.and_then(|i| controls.buttons.get(i)).is_some_and(|b| b.action == Action::Window(BAG))));
    if keep { return; }
    for (_, name, _, mut visibility, _) in windows.iter_mut() {
        if name.0 == BAG && *visibility != Visibility::Hidden { *visibility = Visibility::Hidden; }
    }
}

/// The width a money label's text must keep within (reference pixels): its
/// place, less a pixel for the font's own rounding.
pub(in crate::ui) fn fit_width(window: &str, w: &Widget) -> Option<f32> {
    let money = matches!(leaf(&w.id), "andermant_display" | "gold_display" | "silver_display" | "copper_display" | "template_amount");
    let place = (window == BAR && bar_part(&w.id) == Some(true)) || window == BAG;
    (money && place).then(|| (w.rect[2] - w.rect[0] - 1.0).max(1.0))
}

/// The font size that keeps `text` within `width` (reference pixels): its own
/// `size`, or smaller.
pub(in crate::ui) fn fitted(text: &str, size: f32, width: f32) -> f32 {
    let natural = crate::network::chat::text_width(text, size);
    if natural > width && natural > 0.0 { size * width / natural } else { size }
}

/// A currency as its hover text names it (gui.window.sale acString, ecString,
/// ucString, icString: "{0:i} Badge of Honor", "{0:l} {0:s}"), the item's
/// name from the item catalog.
pub(in crate::ui) fn describe(id: &str, amount: u64, items: Option<&super::items::Catalog>) -> String {
    let name = match id {
        GLYPHS => "Glyph of Power".to_owned(),
        HONOR => "Badge of Honor".to_owned(),
        DRAKEN => "Draken".to_owned(),
        _ => items.map(|c| c.name(id, None).to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| crate::ui::dialogue::readable(id)),
    };
    format!("{amount} {name}")
}

/// The bar's hover texts for its money labels: the whole amounts the labels
/// shorten (gui.window.sale rcString, vcString).
pub(in crate::ui) fn tip(id: &str, wallet: &Wallet) -> Option<String> {
    if bar_part(id) != Some(true) { return None; }
    if id.contains("/RealCurrency") { return Some(format!("{} Andermant", wallet.andermant)); }
    if id.contains("/VirtualCurrency") {
        let c = wallet.copper;
        return Some(format!("{} Gold {} Silver {} Copper", c / 10_000, c % 10_000 / 100, c % 100));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gameplay::systems::CurrencyDef;
    use crate::ui::controls::template::fixture::{find, text_of};

    fn layout(window: &str, widgets: &[(&str, [f32; 4], Option<&str>)]) -> Layout {
        serde_json::from_value(serde_json::json!({ "window": window, "widgets": widgets.iter().map(|(id, [l, t, r, b], text)| {
            let state = ["normal", "pressed", "mouseover"].contains(&leaf(id)).then(|| leaf(id).to_owned());
            serde_json::json!({ "id": id, "type": if leaf(id) == "CurrencyTemplate" { "Button" } else { "Label" }, "k": 0.5,
                "rect": [l, t, r, b], "hidden": true, "state": state, "button": state.as_ref().map(|_| id.rsplit_once('/').unwrap().0),
                "art": {"tex": "textures/ui/icon_arena.png", "pos": [l, t, r, t, r, b, l, b], "uv": vec![0.0; 8], "color": vec![1.0; 16], "idx": [0, 1, 2, 0, 2, 3]},
                "text": text.map(|t| serde_json::json!({"text": t, "key": t, "size": 9.0, "color": vec![1.0; 4], "align": [0, 1]})) })
        }).collect::<Vec<_>>() })).unwrap()
    }

    /// GetClampedIntString: the digits it has room for, then thousands.
    #[test]
    fn amounts_are_shortened_by_thousands_past_their_digits() {
        assert_eq!(clamped(99_999, 5), "99999");
        assert_eq!(clamped(100_000, 5), "100k");
        assert_eq!(clamped(100_000_000, 5), "100M");
        assert_eq!(clamped(999_999_999, 9), "999999999");
        assert_eq!(clamped(4_294_967_295, 9), "4294967k");
        assert_eq!(clamped(u64::MAX, 10), "18446744073G");
        assert_eq!(clamped(0, 10), "0");
    }

    /// The text shrinks to its place, never grows.
    #[test]
    fn long_amounts_shrink_to_their_place() {
        let size = 10.8;
        assert_eq!(fitted("12", size, 18.0), size);
        let shrunk = fitted("710712427", size, 42.0);
        assert!(shrunk < size);
        assert!(crate::network::chat::text_width("710712427", shrunk) <= 42.0 + 1e-3);
    }

    /// In game the bar draws its payment display only; the bag button's glow
    /// waits for a currency to grow; the pick's icon is set.
    #[test]
    fn the_bar_shows_the_in_game_display_and_the_picked_icon() {
        let source = layout(BAR, &[("Canvas", [0.0, 0.0, 300.0, 60.0], None), ("Canvas/LabelDefault", [0.0, 0.0, 266.0, 40.0], None),
            ("Canvas/LabelDefault/MoneyDisplay/RealCurrency/andermant_display", [38.0, 0.0, 94.0, 17.0], Some("")),
            ("Canvas/LabelIngamePayment", [0.0, 0.0, 270.0, 23.0], None),
            ("Canvas/LabelIngamePayment/MoneyDisplay/RealCurrency/andermant_display", [64.1, 2.0, 120.1, 20.7], Some("")),
            (PICKED_ICON, [87.7, 24.5, 108.2, 43.4], None), (PICKED_AMOUNT, [123.3, 24.2, 166.3, 42.9], Some("10")),
            (PLATE, [-2.1, -2.0, 266.4, 52.9], None), (&format!("{PLATE}/CurrencyDisplayHover"), [4.6, 6.9, 273.1, 61.8], None),
            (DROPDOWN, [69.8, 26.8, 208.0, 41.9], None), (&format!("{DROPDOWN}/mouseover"), [69.8, 26.8, 208.0, 41.9], None),
            ("Canvas/LabelIngamePayment/CurrencybagButton/Label_IconAndGlow/Label_icon", [44.9, 24.3, 67.8, 46.9], None), (HIGHLIGHT, [39.7, 20.6, 72.9, 53.8], None),
            ("Canvas/LabelIngamePaymentRCOnly", [0.0, 0.0, 270.0, 23.0], None)]);
        // As exported: the plain display shown (the exporter's ROOTS), the others hidden.
        let mut exported = source.clone();
        for w in &mut exported.widgets { w.hidden = w.id.starts_with("Canvas/LabelIngamePayment"); }
        let catalog = SystemsCatalog { currencies: vec![CurrencyDef { id: "materi_fragment_01".into(), icon: "ui/icon_currency_materi".into() }], ..default() };
        let shown = Shown::of("materi_fragment_01", Some(&catalog));
        let bar = prepare_bar(&exported, &shown);
        let hidden = |id: &str| find(&bar, id).hidden;
        assert!(!hidden("Canvas") && hidden("Canvas/LabelDefault") && hidden("Canvas/LabelIngamePaymentRCOnly") && hidden(HIGHLIGHT));
        assert!(!hidden("Canvas/LabelIngamePayment") && !hidden(PICKED_AMOUNT) && !hidden("Canvas/LabelIngamePayment/CurrencybagButton/Label_IconAndGlow/Label_icon"));
        assert_eq!(find(&bar, PICKED_ICON).art.as_ref().unwrap().tex.as_deref(), Some("textures/ui/icon_currency_materi.png"));
        // The plate and the dropdown's hover fill under the money, though listed after it.
        let index = |id: &str| bar.widgets.iter().position(|w| w.id == id).unwrap();
        let display = index("Canvas/LabelIngamePayment");
        assert_eq!([PLATE, &format!("{PLATE}/CurrencyDisplayHover"), DROPDOWN, &format!("{DROPDOWN}/mouseover")].map(index),
            [display + 1, display + 2, display + 3, display + 4]);
        assert!(index(&format!("{DROPDOWN}/mouseover")) < index(PICKED_AMOUNT));
        assert_eq!(bar.widgets.len(), source.widgets.len());
        // Only the shown display's money labels are kept to their place.
        assert!(fit_width(BAR, find(&bar, "Canvas/LabelIngamePayment/MoneyDisplay/RealCurrency/andermant_display")).is_some_and(|w| (w - 55.0).abs() < 1e-3));
        assert_eq!(fit_width(BAR, find(&bar, "Canvas/LabelDefault/MoneyDisplay/RealCurrency/andermant_display")), None);
        // No pick, or one unknown: the honour, as the game.
        assert_eq!(Shown::of("", Some(&catalog)).id, HONOR);
        assert_eq!(Shown::of("gone_currency", None).icon, HONOR_ICON);
        assert_eq!(Shown::of(GLYPHS, None).icon, GLYPHS_ICON);
    }

    /// The glyphs first, then the catalog's currencies in its order, each with
    /// its balance; a row a currency, the frame grown with them, under the bar.
    #[test]
    fn the_bag_lists_every_currency_under_the_bar() {
        let catalog = SystemsCatalog { currencies: vec![CurrencyDef { id: "money_cc".into(), icon: "ui/ui_currency_cooking_coins".into() },
            CurrencyDef { id: HONOR.into(), icon: "ui/icon_currency_honor".into() }, CurrencyDef { id: "materi_fragment_01".into(), icon: "ui/icon_currency_materi".into() }], ..default() };
        let mut world = World::new();
        world.init_resource::<Time<Real>>();
        crate::network::net::start_offline(&mut world, 1, 0);
        {
            let mut net = world.non_send_mut::<Net>();
            net.wallet = Some([1250, 4_573_210, 870, 0, 35]);
            net.glyphs = Some(710_712_427);
            net.game_currencies.insert("materi_fragment_01".into(), 15_931);
        }
        let net = world.non_send::<Net>();
        let state = super::super::systems::State::default();
        let sources = super::super::systems::Sources { net: &net, catalog: &catalog, items: None, names: None, state: &state, class: None };
        let bag = bag(&sources);
        assert_eq!(bag.rows.iter().map(|r| (r.id.as_str(), r.amount)).collect::<Vec<_>>(),
            [(GLYPHS, 710_712_427), ("money_cc", 0), (HONOR, 870), ("materi_fragment_01", 15_931)]);
        assert_eq!(bag.rows[0].icon, GLYPHS_ICON);

        let source = layout(BAG, &[("Canvas", [444.8, 371.2, 579.2, 435.2], None), (CENTER, [498.8, 371.2, 524.4, 396.8], None),
            (MENU, [486.4, 384.0, 537.6, 435.2], None), (FRAME, [453.8, 402.31, 570.2, 416.89], None), (LIST, [453.5, 402.05, 570.5, 417.15], None),
            (TEMPLATE, [453.8, 399.9, 570.8, 419.06], None), (&format!("{TEMPLATE}/normal"), [453.8, 402.05, 570.8, 417.15], None),
            (&format!("{TEMPLATE}/normal/CurrencyDisplay/template_amount"), [500.5, 399.9, 543.6, 418.5], Some("10")),
            (&format!("{TEMPLATE}/normal/CurrencyDisplay/template_icon"), [464.9, 400.2, 485.5, 419.1], None),
            (&format!("{FRAME}/topedge"), [463.6, 383.6, 560.4, 402.35], None), (&format!("{FRAME}/leftedge"), [444.8, 402.31, 453.8, 416.89], None),
            (&format!("{FRAME}/bottomedge"), [463.6, 416.89, 560.4, 431.1], None)]);
        let mut bar = layout(BAR, &[(REFERENCE, REFERENCE_RECT, None)]);
        bar.widgets[0].k = 0.0;
        let prepared = prepare_bag(&source, &bag, Some(&bar));
        let row = 419.06 - 399.9;
        let extra = 3.0 * row;
        let rect = |id: &str| find(&prepared, id).rect;
        // Centred under the reference label, its top on the label's bottom, at its k.
        let (dx, dy) = ((REFERENCE_RECT[0] + REFERENCE_RECT[2] - 486.4 - 537.6) * 0.5, REFERENCE_RECT[3] - 384.0);
        assert!(prepared.widgets.iter().all(|w| w.k == 0.0));
        assert!((rect(MENU)[1] - REFERENCE_RECT[3]).abs() < 1e-3);
        // The top edge stays, the sides stretch, the bottom moves down by the rows.
        assert!((rect(&format!("{FRAME}/topedge"))[3] - (402.35 + dy)).abs() < 1e-3);
        assert!((rect(&format!("{FRAME}/leftedge"))[3] - (416.89 + extra + dy)).abs() < 1e-3);
        assert!((rect(&format!("{FRAME}/bottomedge"))[1] - (416.89 + extra + dy)).abs() < 1e-3);
        assert!(find(&prepared, CENTER).hidden);
        // A button a currency, in order, each with its icon and whole amount.
        let fourth = format!("{LIST}/Currency3");
        assert_eq!(find(&prepared, &fourth).event, "Currency:materi_fragment_01");
        assert!((rect(&fourth)[1] - (399.9 + extra + dy)).abs() < 1e-3);
        assert_eq!(text_of(&prepared, &format!("{LIST}/Currency0/normal/CurrencyDisplay/template_amount")), "710712427");
        assert_eq!(find(&prepared, &format!("{LIST}/Currency2/normal/CurrencyDisplay/template_icon")).art.as_ref().unwrap().tex.as_deref(),
            Some("textures/ui/icon_currency_honor.png"));
        // The amount may use the row's width.
        let amount = rect(&format!("{LIST}/Currency0/normal/CurrencyDisplay/template_amount"));
        assert!((amount[2] - (570.8 - AMOUNT_PAD + dx)).abs() < 1e-3);
        assert!(!prepared.widgets.iter().any(|w| w.id.contains("CurrencyTemplate")));
        assert_eq!(action(BAG, find(&prepared, &fourth)), Some(Action::Currency("materi_fragment_01".into())));
    }

    /// The bar is rebuilt for a new pick (its icon), never on idle frames.
    #[test]
    fn the_bar_is_rebuilt_only_when_the_pick_changes() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<SystemsCatalog>().init_asset::<Layout>()
            .init_resource::<super::super::systems::State>().init_resource::<Shown>().init_resource::<super::super::settings::Settings>()
            .add_systems(Update, sync_bar);
        let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(layout(BAR, &[("Canvas", [0.0, 0.0, 300.0, 60.0], None)]));
        app.insert_resource(Loaded { layouts: vec![(handle, 20.0, None)], spawned: vec![true], ..default() });
        app.world_mut().spawn(Window2018(BAR.into())).with_children(|bar| { bar.spawn(Transform::default()); });
        let spawned = |app: &App| app.world().resource::<Loaded>().spawned[0];
        let pick = |app: &mut App, id: &str| app.world_mut().resource_mut::<super::super::settings::Settings>().current.currency = id.into();
        pick(&mut app, "");
        app.update();
        assert_eq!(app.world().resource::<Shown>().id, HONOR);
        app.world_mut().resource_mut::<Loaded>().spawned[0] = true;
        for _ in 0..10 { app.update(); }
        assert!(spawned(&app), "idle frames leave the bar alone");
        pick(&mut app, GLYPHS);
        app.update();
        assert!(!spawned(&app), "a new pick rebuilds the bar");
        assert_eq!(app.world().resource::<Shown>().icon, GLYPHS_ICON);
        app.world_mut().resource_mut::<Loaded>().spawned[0] = true;
        pick(&mut app, "not_a_currency");
        app.update();
        app.update();
        assert!(!spawned(&app), "an unknown pick falls back to the honour");
        assert_eq!(app.world().resource::<Shown>().id, HONOR);
    }

    #[test]
    fn hover_texts_name_the_whole_amounts() {
        let wallet = Wallet { andermant: 1_234_567_890, copper: 1_000_000_123_456, honor: 8_883_102, keyed: HashMap::from([("bone".to_owned(), 343_014)]), ..default() };
        assert_eq!(tip("Canvas/LabelIngamePayment/MoneyDisplay/RealCurrency/andermant_display", &wallet).as_deref(), Some("1234567890 Andermant"));
        assert_eq!(tip("Canvas/LabelIngamePayment/MoneyDisplay/VirtualCurrency/gold_display", &wallet).as_deref(), Some("100000012 Gold 34 Silver 56 Copper"));
        assert_eq!(tip("Canvas/LabelDefault/MoneyDisplay/RealCurrency/andermant_display", &wallet), None);
        assert_eq!(describe(HONOR, wallet.balance(HONOR), None), "8883102 Badge of Honor");
        assert_eq!(wallet.balance("bone"), 343_014);
        assert_eq!(wallet.balance("unsent"), 0);
    }
}
