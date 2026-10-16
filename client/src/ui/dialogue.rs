//! NPC portraits, current quest speech bubbles, and the NPC trading window.
use super::*;
use crate::gameplay::dialogue::Dialogue;
use dsor_proto::commands::{actors, inventory, ClientCommand};

pub(super) const WINDOWS: [&str; 3] = ["npcscreen", "questscreen2", "tradingscreen"];
pub(super) const CLOSE: usize = usize::MAX;
const PAGE_SIZE: usize = 4;
const OFFER_PAGE_SIZE: usize = 12;

#[derive(Asset, TypePath, Deserialize, Default)]
pub(super) struct Catalog {
    portraits: HashMap<String, String>,
    icons: HashMap<String, String>,
    texts: HashMap<String, String>,
}
#[derive(Default, TypePath)]
pub(super) struct CatalogLoader;
impl AssetLoader for CatalogLoader {
    type Asset = Catalog;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(&self, reader: &mut dyn Reader, _: &(), _: &mut LoadContext<'_>) -> Result<Catalog, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)
    }
    fn extensions(&self) -> &[&str] { &["dialogue.catalog.json"] }
}

impl Catalog {
    fn text(&self, key: &str) -> String { self.texts.get(&key.to_lowercase()).cloned().unwrap_or_default() }
    fn quest(&self, id: &str, field: &str) -> String { self.text(&format!("quest.{id}.s_{field}")) }
    fn title(&self, id: &str) -> String {
        let title = self.quest(id, "title");
        if title.is_empty() { readable(id) } else { title }
    }
    fn dialog(&self, id: &str, short: bool) -> String {
        self.text(&format!("quest.{id}{}", if short { "Short" } else { "" }))
    }
}

fn readable(id: &str) -> String {
    id.replace(['_', '-'], " ").split_whitespace().map(|s| {
        let mut chars = s.chars();
        chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Quest(String), Talk(String, String), Shop(String),
    Accept(String, u8), ContinueTalk(String), Back, Previous, Next, Close,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice { label: String, icon: String, action: Action }
impl Choice {
    fn new(label: impl Into<String>, icon: &str, action: Action) -> Self { Self { label: label.into(), icon: icon.into(), action } }
}
#[derive(Clone, PartialEq, Eq)]
struct Detail { action: Action, page: usize }
#[derive(Clone, PartialEq, Eq)]
struct MerchantItem { name: String, icon: String, amount: u32, price: String }
#[derive(Clone, PartialEq, Eq)]
struct MerchantView { items: Vec<MerchantItem>, page: usize, pages: usize }
#[derive(Clone, PartialEq, Eq)]
pub(super) struct View {
    name: String,
    portrait: String,
    title: String,
    body: Option<String>,
    merchant: Option<MerchantView>,
    options: Vec<Choice>,
}
#[derive(Resource, Default)]
pub(super) struct Presentation {
    catalog: Handle<Catalog>,
    items: Handle<controls::items::Catalog>,
    generation: u64,
    page: usize,
    detail: Option<Detail>,
    view: Option<View>,
}

pub(super) fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Presentation {
        catalog: assets.load("interface/dialogue.catalog.json"),
        items: assets.load("interface/inventory.items.json"),
        generation: 0, page: 0, detail: None, view: None,
    });
}

fn menu(info: Option<&actors::NpcInfo>, target: &crate::gameplay::dialogue::Target, catalog: &Catalog) -> Vec<Choice> {
    let mut choices = Vec::new();
    // Online, the authoritative role descriptor supplies the shop name. The
    // authored shop is also useful for the offline NPC preview.
    let shop = info.map(|i| i.name.as_str()).filter(|s| !s.is_empty()).unwrap_or(&target.profile.shop);
    if !target.profile.shop.is_empty() && !shop.is_empty() {
        choices.push(Choice::new("View selection", "shop", Action::Shop(shop.to_owned())));
    }
    if let Some(info) = info {
        for entry in &info.interactions {
            let choice = match entry.kind {
                0 => Choice::new(catalog.title(&entry.name), "quest", Action::Quest(entry.name.clone())),
                2 => {
                    let short = catalog.dialog(&entry.name, true);
                    Choice::new(if short.is_empty() { catalog.title(&entry.quest) } else { short }, "talk", Action::Talk(entry.name.clone(), entry.quest.clone()))
                }
                // Task-target markers are not conversation options.
                _ => continue,
            };
            if !choices.iter().any(|c: &Choice| c.action == choice.action) { choices.push(choice); }
        }
    }
    choices
}

fn text_pages(text: &str) -> Vec<String> {
    let mut pages = Vec::new();
    let mut page = String::new();
    for paragraph in text.split('\n') {
        for word in paragraph.split_whitespace() {
            if page.chars().count() + word.chars().count() > 780 { pages.push(std::mem::take(&mut page)); }
            if !page.is_empty() && !page.ends_with('\n') { page.push(' '); }
            page.push_str(word);
        }
        if !page.is_empty() { page.push('\n'); }
    }
    if !page.is_empty() { pages.push(page.trim_end().to_owned()); }
    if pages.is_empty() { pages.push(String::new()); }
    pages
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    mut commands: Commands,
    mut dialogue: ResMut<Dialogue>,
    mut presentation: ResMut<Presentation>,
    catalogs: Res<Assets<Catalog>>,
    item_catalogs: Res<Assets<controls::items::Catalog>>,
    mut net: Option<NonSendMut<crate::network::net::Net>>,
    mut loaded: ResMut<Loaded>,
    layouts: Res<Assets<Layout>>,
    windows: Query<(Entity, &Window2018)>,
) {
    if presentation.generation != dialogue.generation {
        presentation.generation = dialogue.generation;
        presentation.page = 0;
        presentation.detail = None;
        presentation.view = None;
        // Old buttons belong to the old NPC, even if their release arrives now.
        dialogue.choice = None;
    }
    let fallback = Catalog::default();
    let catalog = catalogs.get(&presentation.catalog).unwrap_or(&fallback);
    let active = dialogue.target.as_ref().filter(|_| dialogue.open).cloned();
    let mut next = None;
    if let Some(target) = active {
        let info = net.as_ref().and_then(|n| n.npc_dialogue.infos.get(&target.actor));
        let available = menu(info, &target, catalog);
        if let Some(index) = dialogue.choice.take() {
            let action = if index == CLOSE { Some(Action::Close) }
                else { presentation.view.as_ref().and_then(|v| v.options.get(index)).map(|c| c.action.clone()) };
            match action {
                Some(Action::Close) => dialogue.close(),
                Some(Action::Back) => presentation.detail = None,
                Some(Action::Previous) => {
                    if let Some(detail) = &mut presentation.detail { detail.page = detail.page.saturating_sub(1); }
                }
                Some(Action::Next) => {
                    if let Some(detail) = &mut presentation.detail { detail.page += 1; }
                    else { presentation.page += 1; }
                }
                Some(action @ (Action::Quest(_) | Action::Talk(_, _) | Action::Shop(_))) if available.iter().any(|c| c.action == action) => {
                    if let (Action::Shop(shop), Some(net)) = (&action, net.as_mut()) {
                        if !net.offline { net.npc_dialogue.offers.remove(shop); }
                        net.send(&ClientCommand::RequestOffer(inventory::RequestOffer { actor: target.actor, by_name: false, shop: String::new() }));
                    }
                    presentation.detail = Some(Detail { action, page: 0 });
                }
                Some(Action::Accept(id, operation)) if available.iter().any(|c| c.action == Action::Quest(id.clone())) => {
                    if let Some(net) = net.as_mut().filter(|n| !n.offline) {
                        let state = net.npc_dialogue.quests.get(&id.to_lowercase()).copied();
                        if (operation == 0 && state.is_none_or(|s| s >= 2)) || (operation == 1 && state == Some(1)) {
                            net.send(&ClientCommand::Quest(actors::Quest { quest_id: id, state: operation, parameter: 0 }));
                            net.send(&ClientCommand::NpcRequest(actors::NpcRequest { actor: target.actor }));
                            dialogue.close();
                        }
                    }
                }
                Some(Action::ContinueTalk(id)) if available.iter().any(|c| matches!(&c.action, Action::Talk(value, _) if *value == id)) => {
                    if let Some(net) = net.as_mut() {
                        net.send(&ClientCommand::Talk(actors::Talk { actor: target.actor, value: id }));
                        net.send(&ClientCommand::NpcRequest(actors::NpcRequest { actor: target.actor }));
                    }
                    presentation.detail = None;
                }
                _ => {}
            }
        }
        if dialogue.open {
            let mut view = View {
                name: if target.profile.title.is_empty() { readable(&target.template) } else { target.profile.title.clone() },
                portrait: catalog.portraits.get(&target.profile.portrait).or_else(|| catalog.portraits.get("npc_portraits/wip")).cloned().unwrap_or_default(),
                title: String::new(), body: None, merchant: None, options: Vec::new(),
            };
            if let Some(detail) = &presentation.detail {
                let mut confirm = None;
                let body = match &detail.action {
                    Action::Quest(id) => {
                        view.title = catalog.title(id);
                        let state = net.as_ref().and_then(|n| n.npc_dialogue.quests.get(&id.to_lowercase())).copied();
                        let field = match state { Some(0) => "progress", Some(1) => "complete", _ => "offer" };
                        if net.as_ref().is_some_and(|n| !n.offline) {
                            if field == "offer" { confirm = Some(Choice::new("Accept quest", "quest", Action::Accept(id.clone(), 0))); }
                            if field == "complete" { confirm = Some(Choice::new("Complete quest", "quest", Action::Accept(id.clone(), 1))); }
                        }
                        catalog.quest(id, field)
                    }
                    Action::Talk(id, quest) => {
                        view.title = catalog.title(quest);
                        confirm = Some(Choice::new("Continue", "talk", Action::ContinueTalk(id.clone())));
                        catalog.dialog(id, false)
                    }
                    Action::Shop(shop) => {
                        view.title = view.name.clone();
                        let offers = net.as_ref().and_then(|n| n.npc_dialogue.offers.get(shop));
                        let all: Vec<_> = offers.into_iter().flat_map(|o| o.offers.iter().flatten()).collect();
                        let pages = all.len().div_ceil(OFFER_PAGE_SIZE).max(1);
                        let page = detail.page.min(pages - 1);
                        let items = item_catalogs.get(&presentation.items);
                        let cards = all.into_iter().skip(page * OFFER_PAGE_SIZE).take(OFFER_PAGE_SIZE).map(|o| {
                            let definition = items.and_then(|c| c.items.get(&o.item));
                            let mut prices = Vec::new();
                            if o.price_vc > 0 {
                                let (gold, silver, copper) = (o.price_vc / 10000, o.price_vc / 100 % 100, o.price_vc % 100);
                                if gold > 0 { prices.push(format!("{gold} gold")); }
                                if silver > 0 { prices.push(format!("{silver} silver")); }
                                if copper > 0 { prices.push(format!("{copper} copper")); }
                            }
                            for (amount, currency) in [(o.price_rc, "Andermant"), (o.price_arena, "Badges of Honor"), (o.price_gc, "Glyphs"), (o.price_ec, "Draken"), (o.price_uc, "tokens"), (o.price_ic, "tokens")] {
                                if amount > 0 { prices.push(format!("{amount} {currency}")); }
                            }
                            MerchantItem {
                                name: definition.map(|d| d.name.clone()).unwrap_or_else(|| readable(&o.item)),
                                icon: definition.map(|d| d.icon.clone()).unwrap_or_default(), amount: o.amount,
                                price: if prices.is_empty() { "Free".into() } else { prices.join(", ") },
                            }
                        }).collect();
                        view.merchant = Some(MerchantView { items: cards, page, pages });
                        if page > 0 { view.options.push(Choice::new("Previous page", "talk", Action::Previous)); }
                        if page + 1 < pages { view.options.push(Choice::new("Next page", "talk", Action::Next)); }
                        match offers {
                            None if net.as_ref().is_some_and(|n| n.offline) => "The merchant's selection is available when playing online.".into(),
                            None => "Loading selection...".into(),
                            Some(o) if o.offers.iter().flatten().next().is_none() => "There are no offers available.".into(),
                            Some(_) => String::new(),
                        }
                    }
                    _ => String::new(),
                };
                let pages = if view.merchant.is_some() { vec![body] } else { text_pages(&body) };
                let page = if view.merchant.is_some() { 0 } else { detail.page % pages.len() };
                view.body = Some(pages[page].clone());
                if pages.len() > 1 {
                    view.title = format!("{} ({}/{})", view.title, page + 1, pages.len());
                    view.options.push(Choice::new("Next page", "talk", Action::Next));
                }
                if page + 1 == pages.len() { view.options.extend(confirm); }
                view.options.push(Choice::new("Back", "close", Action::Back));
            } else {
                let pages = available.len().div_ceil(PAGE_SIZE).max(1);
                let page = presentation.page % pages;
                view.options.extend(available.into_iter().skip(page * PAGE_SIZE).take(PAGE_SIZE));
                if pages > 1 { view.options.push(Choice::new(format!("More ({}/{pages})", page + 1), "talk", Action::Next)); }
                view.options.push(Choice::new("Close", "close", Action::Close));
            }
            next = Some(view);
        }
    }
    // Replace geometry only when content changes; pointer state selects the
    // original normal/hover/pressed artwork without rebuilding each frame.
    if presentation.view != next || (next.is_none() && windows.iter().any(|(_, w)| WINDOWS.contains(&w.0.as_str()))) {
        presentation.view = next;
        for (entity, window) in &windows {
            if WINDOWS.contains(&window.0.as_str()) { commands.entity(entity).despawn(); }
        }
        for i in 0..loaded.layouts.len() {
            if layouts.get(&loaded.layouts[i].0).is_some_and(|l| WINDOWS.contains(&l.window.as_str())) { loaded.spawned[i] = false; }
        }
    }
}

fn map_widget(w: &mut Widget, x: impl Fn(f32) -> f32, y: impl Fn(f32) -> f32) {
    w.rect = [x(w.rect[0]), y(w.rect[1]), x(w.rect[2]), y(w.rect[3])];
    if let Some(a) = &mut w.art { for p in a.pos.chunks_exact_mut(2) { p[0] = x(p[0]); p[1] = y(p[1]); } }
}

fn stretch(x: f32, left: f32, right: f32, extra: f32) -> f32 {
    x + if x <= left { 0.0 } else if x >= right { extra } else { extra * (x - left) / (right - left) }
}

fn image_art(rect: [f32; 4], texture: String) -> Art {
    let [l, t, r, b] = rect;
    Art { tex: Some(texture), pos: vec![l, t, r, t, r, b, l, b], uv: vec![0., 0., 1., 0., 1., 1., 0., 1.], color: vec![1.; 16], idx: vec![0, 1, 2, 0, 2, 3] }
}

fn set_text(w: &mut Widget, text: &str) {
    if let Some(label) = &mut w.text { label.text = text.into(); label.key = "gui.runtime.dialogue".into(); }
}

pub(super) fn prepare(source: &Layout, presentation: &Presentation, catalogs: &Assets<Catalog>) -> Layout {
    let mut layout = Layout { window: source.window.clone(), widgets: Vec::new() };
    let Some(view) = &presentation.view else { return layout };
    let catalog = catalogs.get(&presentation.catalog);
    if source.window == "npcscreen" {
        if view.merchant.is_some() { return layout; }
        for widget in &source.widgets {
            if !widget.id.contains("/QuestButton") {
                let mut w = widget.clone();
                if w.id.ends_with("/PortraitLabel") {
                    if let Some(a) = &mut w.art { a.tex = (!view.portrait.is_empty()).then(|| view.portrait.clone()); }
                }
                if w.id.contains("/NameLabel") {
                    let extra = (view.name.chars().count() as f32 * 7.0 - 19.07).max(0.0).min(160.0);
                    map_widget(&mut w, |x| stretch(x, 119.11, 138.18, extra) - extra * 0.5, |y| y);
                    if w.id.ends_with("/TextLabel") { set_text(&mut w, &view.name); }
                }
                layout.widgets.push(w);
            }
        }
        for (index, choice) in view.options.iter().enumerate().filter(|_| view.body.is_none()) {
            let prefix = format!("Canvas/NPCLabel/QuestButton{index}");
            let center = source.widgets.iter().find(|w| w.id == format!("{prefix}/normal/BubbleFrameLabel"));
            let Some(center) = center else { continue };
            let [left, _, right, _] = center.rect;
            let extra = (choice.label.chars().count() as f32 * 6.8 + 12.0 - (right - left)).clamp(30.0, 220.0);
            for widget in source.widgets.iter().filter(|w| w.id == prefix || w.id.starts_with(&(prefix.clone() + "/"))) {
                let leaf = widget.id.rsplit('/').next().unwrap_or("");
                if matches!(leaf, "QuestTitle" | "QuestZone" | "QuestLevel" | "SpacerLabel") { continue; }
                let mut w = widget.clone();
                if w.id == prefix { w.event = format!("NpcChoice:{index}"); }
                if leaf == "CenteredText" { set_text(&mut w, &choice.label); }
                if w.id.ends_with("/leftedge/Label") {
                    if let Some(texture) = catalog.and_then(|c| c.icons.get(&choice.icon)) {
                        let mut art = image_art(w.rect, texture.clone());
                        if choice.icon == "quest" { for color in art.color.chunks_exact_mut(4) { color[..3].copy_from_slice(&[0.45, 0.8, 0.2]); } }
                        w.art = Some(art);
                    }
                }
                map_widget(&mut w, |x| stretch(x, left, right, extra), |y| y);
                layout.widgets.push(w);
            }
        }
        for w in &mut layout.widgets {
            map_widget(w, |x| x - 12.0, |y| y + 50.0);
            w.k = 0.0;
        }
    } else if source.window == "questscreen2" {
        if view.body.is_none() || view.merchant.is_some() { return layout; }
        conversation(source, view, &mut layout);
    } else if source.window == "tradingscreen" {
        if let Some(merchant) = &view.merchant { merchant_layout(source, view, merchant, &mut layout); }
    }
    layout
}

fn conversation(source: &Layout, view: &View, layout: &mut Layout) {
    let body = view.body.as_deref().unwrap_or("");
    // Stretch the speech bubble's center, preserving its authored corners and
    // tail. The portrait remains in npcscreen alongside the bubble.
    let lines = body.lines().map(|line| line.chars().count().div_ceil(66).max(1)).sum::<usize>();
    let extra = (lines as f32 * 17.0 + 12.0 - 26.15).max(45.0);
    let prefix = "Canvas/AlignmentLabel/QuestLabel";
    for widget in source.widgets.iter().filter(|w| w.id == prefix || w.id.starts_with(&(prefix.to_owned() + "/"))) {
        let mut w = widget.clone();
        map_widget(&mut w, |x| x, |y| stretch(y, 92.84, 118.99, extra));
        if w.id.ends_with("/HeadlineLabel") {
            set_text(&mut w, &view.title);
            w.rect[3] = 91.0;
        }
        if w.id.ends_with("/TextLabel") {
            set_text(&mut w, body);
            w.rect[3] = 118.99 + extra;
            if let Some(label) = &mut w.text { label.align = [0, 0]; label.word_break = true; }
        }
        layout.widgets.push(w);
    }
    let mut choices: Vec<_> = view.options.iter().enumerate().collect();
    choices.sort_by_key(|(_, c)| !matches!(c.action, Action::Back));
    for (slot, (index, choice)) in choices.into_iter().enumerate() {
        let name = if matches!(choice.action, Action::Back) { "Back" } else { "Accept" };
        let prefix = format!("Canvas/AlignmentLabel/ButtonSetBackAccept/{name}");
        let Some(button) = source.widgets.iter().find(|w| w.id == prefix) else { continue };
        let left = button.rect[0];
        for widget in source.widgets.iter().filter(|w| w.id == prefix || w.id.starts_with(&(prefix.clone() + "/"))) {
            if widget.id.ends_with("/icon") { continue; }
            let mut w = widget.clone();
            let replacement = format!("Canvas/AlignmentLabel/Choice{index}");
            w.id = w.id.replacen(&prefix, &replacement, 1);
            w.button = w.button.map(|b| b.replacen(&prefix, &replacement, 1));
            w.hidden = false;
            if widget.id == prefix { w.event = format!("NpcChoice:{index}"); }
            else if w.state.is_some() {
                w.text = Some(Label { text: choice.label.clone(), key: "gui.runtime.dialogue".into(), size: 10.0,
                    color: [1.0, 0.83, 0.52, 1.0], align: [1, 1], bold: true, word_break: false });
            }
            map_widget(&mut w, |x| stretch(x, left + 30.0, left + 70.0, 45.0) - left + 243.87 + slot as f32 * 155.0,
                |y| y + extra + 32.0);
            layout.widgets.push(w);
        }
    }
    for w in &mut layout.widgets { map_widget(w, |x| x - 12.0, |y| y + 50.0); w.k = 0.0; }
    // The current quest layout includes a soft, left-side backdrop so its
    // translucent speech bubble remains readable over the world.
    if let Some(background) = source.widgets.iter().find(|w| w.id == "Canvas/AlignmentLabel/BackgroundLabel") {
        layout.widgets.insert(0, background.clone());
    }
}

fn merchant_layout(source: &Layout, view: &View, merchant: &MerchantView, layout: &mut Layout) {
    let root = "Canvas/Label_blur/Label_texture";
    let list = format!("{root}/ListBackground/List");
    // Keep the current trading frame and list controls. Authored product,
    // currency, sale and buyback templates are instantiated from actual offers.
    for widget in &source.widgets {
        let id = &widget.id;
        if ["/PremiumBtn", "/RestoreEntry", "/Tabs", "/TraderName", "/switchbuy_bg"].iter().any(|p| id.contains(p)) { continue; }
        if id.starts_with(&format!("{root}/ListBackground/")) && !id.starts_with(&(list.clone() + "/")) { continue; }
        if id.contains("/Template") || id.contains("/showuse_bg") { continue; }
        let mut w = widget.clone();
        if w.event == "Close" { w.event = format!("NpcChoice:{CLOSE}"); }
        if w.id.ends_with("/WindowTitle") { set_text(&mut w, &view.name); }
        if w.id.ends_with("/PageNumText") { set_text(&mut w, &format!("{} / {}", merchant.page + 1, merchant.pages)); }
        for (event, action) in [("PrevPage", Action::Previous), ("NextPage", Action::Next)] {
            if w.event == event {
                if let Some(index) = view.options.iter().position(|c| c.action == action) { w.event = format!("NpcChoice:{index}"); }
                else { w.enabled = false; }
            }
        }
        layout.widgets.push(w);
    }
    let entry = format!("{list}/TemplateListEntry/TemplateItemEntry1");
    for (index, item) in merchant.items.iter().enumerate() {
        for widget in source.widgets.iter().filter(|w| w.id == entry || w.id.starts_with(&(entry.clone() + "/"))) {
            if ["/MoneyDisplay", "/FlagDiscount", "/LabelDiscount"].iter().any(|p| widget.id.contains(p)) { continue; }
            let mut w = widget.clone();
            w.id = w.id.replacen(&entry, &format!("{list}/Offer{index}"), 1);
            w.hidden = false;
            if w.id.ends_with("/ItemDescription") { set_text(&mut w, &item.name); }
            if w.id.ends_with("/ItemSlot/Icon") {
                w.art = (!item.icon.is_empty()).then(|| image_art(w.rect, item.icon.clone()));
                set_text(&mut w, &if item.amount > 1 { item.amount.to_string() } else { String::new() });
            }
            map_widget(&mut w, |x| x + (index % 2) as f32 * 235.46, |y| y + (index / 2) as f32 * 64.0);
            layout.widgets.push(w);
        }
        if let Some(template) = source.widgets.iter().find(|w| w.id == format!("{entry}/MoneyDisplay/RealCurrency/andermant_display")) {
            let mut price = template.clone();
            price.id = format!("{list}/Offer{index}/Price");
            price.rect = [71.56, 199.5, 236.0, 222.5];
            set_text(&mut price, &item.price);
            if let Some(label) = &mut price.text { label.size = 8.0; label.color = [0.9, 0.78, 0.54, 1.0]; label.word_break = true; }
            map_widget(&mut price, |x| x + (index % 2) as f32 * 235.46, |y| y + (index / 2) as f32 * 64.0);
            layout.widgets.push(price);
        }
    }
    if let Some(template) = source.widgets.iter().find(|w| w.id == format!("{entry}/Label_empty/ItemDescription")) {
        let mut caption = template.clone();
        caption.id = format!("{root}/SelectionCaption");
        caption.rect = [18.0, 94.0, 465.0, 145.0];
        let text = if merchant.items.is_empty() { view.body.as_deref().unwrap_or("") } else { "View selection" };
        set_text(&mut caption, text);
        if let Some(label) = &mut caption.text { label.size = 11.0; }
        layout.widgets.push(caption);
    }
    // The trading screen uses its own modern title bar and close control; a
    // source button at the foot returns to the NPC's choice menu.
    if let Some(index) = view.options.iter().position(|c| c.action == Action::Back) {
        let prefix = format!("{root}/switchbuy_bg/BuyTabBtn");
        for widget in source.widgets.iter().filter(|w| w.id == prefix || w.id.starts_with(&(prefix.clone() + "/"))) {
            if widget.id.ends_with("/Label_icon") { continue; }
            let mut w = widget.clone();
            w.id = w.id.replacen(&prefix, &format!("{root}/BackButton"), 1);
            w.button = w.button.map(|b| b.replacen(&prefix, &format!("{root}/BackButton"), 1));
            if widget.id == prefix { w.event = format!("NpcChoice:{index}"); }
            set_text(&mut w, "Back");
            if let Some(label) = &mut w.text { label.color = [1.0, 0.83, 0.52, 1.0]; }
            map_widget(&mut w, |x| x + 12.0, |y| y + 528.0);
            layout.widgets.push(w);
        }
    }
    for w in &mut layout.widgets { map_widget(w, |x| x + 12.0, |y| y + 12.0); w.k = 0.0; }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localized_options_exclude_task_markers_and_duplicate_quests() {
        let catalog = Catalog { texts: HashMap::from([("quest.bone.s_title".into(), "Bone Oracle (1/2)".into())]), ..default() };
        let target = crate::gameplay::dialogue::Target { entity: Entity::PLACEHOLDER, actor: 7, template: "npc".into(), profile: crate::actors::npc::NpcTemplate { shop: "shop".into(), ..default() } };
        let info = actors::NpcInfo { enabled: true, interactions: vec![
            actors::NpcInteraction { name: "bone".into(), kind: 0, ..default() },
            actors::NpcInteraction { name: "bone".into(), kind: 1, ..default() },
            actors::NpcInteraction { name: "bone".into(), kind: 0, ..default() },
        ], ..default() };
        let choices = menu(Some(&info), &target, &catalog);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].label, "View selection");
        assert_eq!(choices[1].label, "Bone Oracle (1/2)");
    }
    #[test]
    fn long_dialogue_paginates_without_losing_unicode_words() {
        let text = "Agathon’s light shines. ".repeat(100);
        let pages = text_pages(&text);
        assert!(pages.len() > 1);
        assert_eq!(pages.join(" ").split_whitespace().collect::<Vec<_>>(), text.split_whitespace().collect::<Vec<_>>());
    }
}
