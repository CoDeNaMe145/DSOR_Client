//! NPC portraits, current quest speech bubbles, and the NPC trading window.
use super::*;
use crate::gameplay::dialogue::Dialogue;
use dsor_proto::commands::{actors, inventory, ClientCommand};

pub(super) const WINDOWS: [&str; 3] = ["npcscreen", "questscreen2", "tradingscreen"];
pub(super) const CLOSE: usize = usize::MAX;
const PAGE_SIZE: usize = 4;
mod merchant;
mod quest;
use merchant::MerchantView;

#[derive(Asset, TypePath, Deserialize, Default)]
pub(super) struct Catalog {
    portraits: HashMap<String, String>,
    icons: HashMap<String, String>,
    texts: HashMap<String, String>,
    /// Quest texts that colour words, with their inline tags (quest::runs);
    /// `texts` holds them plain (tools/dialogue_catalog.py).
    #[serde(default)]
    marked: HashMap<String, String>,
    #[serde(default)]
    trading: merchant::Catalog,
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
    pub(in crate::ui) fn text(&self, key: &str) -> String { self.texts.get(&key.to_lowercase()).cloned().unwrap_or_default() }
    pub(in crate::ui) fn quest(&self, id: &str, field: &str) -> String { self.text(&format!("quest.{id}.s_{field}")) }
    pub(in crate::ui) fn title(&self, id: &str) -> String {
        let title = self.quest(id, "title");
        if title.is_empty() { readable(id) } else { title }
    }
    fn dialog(&self, id: &str, short: bool) -> String {
        self.text(&format!("quest.{id}{}", if short { "Short" } else { "" }))
    }
    /// What the NPC says: the text of `key` in the player's gender (its `.f`
    /// variant for a woman, when there is one), with its inline tags.
    /// EVIDENCE: release261 QuestWidget2::Refresh (0x1404D3844) passes the
    ///   player's gender and name to the locale lookup.
    fn spoken(&self, key: &str, female: bool) -> String {
        let key = key.to_lowercase();
        let pick = |key: &str| self.marked.get(key).or_else(|| self.texts.get(key)).filter(|t| !t.is_empty()).cloned();
        female.then(|| pick(&format!("{key}.f"))).flatten().or_else(|| pick(&key)).unwrap_or_default()
    }
}

pub(in crate::ui) fn readable(id: &str) -> String {
    id.replace(['_', '-'], " ").split_whitespace().map(|s| {
        let mut chars = s.chars();
        chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Quest(String), Talk(String, String), Shop(String),
    Accept(String, u8), ContinueTalk(String), Back, Previous, Next, Close,
    /// The quest screen's Back: the conversation ends (QuestWidget2 0x1404CF76C
    /// closes the quest screen and the NPC screen).
    Leave,
    Category(u16), Buyback(bool), Usable(bool),
    /// Buy one of the offer (its resource id), or buy back a sold stack (its id).
    Buy(String), Restore(u32),
    /// Hand a quest's items over: the barter id, the quest.
    Deliver(String, String),
    /// Open one of the NPC's desks (controls::systems: the smith's salvage and
    /// workbench, the jeweller's gem desk, the tailor's transmog desk).
    Desk(&'static str),
}

/// The desks an NPC's profession offers (the NPC data's profession ids).
fn desks(profession: &str) -> &'static [(&'static str, &'static str)] {
    use controls::systems::{GEMDESK, SALVAGE, TRANSMOG, WORKBENCH};
    match profession {
        "jewelry" => &[(GEMDESK, "Gems and sockets")],
        "smith" | "simple_smith" => &[(SALVAGE, "Melt items"), (WORKBENCH, "Workbench")],
        "crafting" | "crafting_cake" => &[(WORKBENCH, "Workbench")],
        "tailor" => &[(TRANSMOG, "Transmogrification")],
        _ => &[],
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice { label: String, icon: String, action: Action }
impl Choice {
    fn new(label: impl Into<String>, icon: &str, action: Action) -> Self { Self { label: label.into(), icon: icon.into(), action } }
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Detail { action: Action, page: usize }
#[derive(Clone, PartialEq)]
pub(super) struct View {
    name: String,
    portrait: String,
    body: Option<String>,
    merchant: Option<MerchantView>,
    /// A quest or a conversation open (questscreen2).
    quest: Option<quest::Screen>,
    options: Vec<Choice>,
}
#[derive(Resource, Default)]
pub(super) struct Presentation {
    catalog: Handle<Catalog>,
    items: Handle<controls::items::Catalog>,
    generation: u64,
    page: usize,
    detail: Option<Detail>,
    merchant: merchant::Selection,
    view: Option<View>,
    /// A transaction sent and not yet answered: (inventory revision, time).
    pending: Option<(u64, f64)>,
    /// A large Andermant purchase waiting for the player's word (asked by
    /// controls::consent; sync orders it once granted).
    consent: Option<Consent>,
}

/// The offer a large Andermant purchase would buy, its Andermant price, and
/// whether the player agreed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Consent { pub offer: String, pub andermant: u32, pub granted: bool }

impl Presentation {
    /// The large Andermant purchase asking for the player's word, if any.
    pub(in crate::ui) fn consent(&self) -> Option<&Consent> { self.consent.as_ref() }
    /// The player's answer: the purchase is ordered (sync) or dropped.
    pub(in crate::ui) fn answer_consent(&mut self, granted: bool) {
        if granted { if let Some(c) = &mut self.consent { c.granted = true; } } else { self.consent = None; }
    }
    /// A purchase asking for the player's word, as sync holds one back.
    #[cfg(test)]
    pub(in crate::ui) fn ask_consent(&mut self, offer: &str, andermant: u32) {
        self.consent = Some(Consent { offer: offer.into(), andermant, granted: false });
    }
    /// The shop whose selection is open.
    fn shop(&self) -> Option<String> {
        match &self.detail { Some(Detail { action: Action::Shop(shop), .. }) => Some(shop.clone()), _ => None }
    }
    pub(super) fn choice_label(&self, index: usize) -> Option<&str> {
        if index == CLOSE { Some("Close") }
        else { self.view.as_ref()?.options.get(index).map(|choice| choice.label.as_str()) }
    }
    pub(in crate::ui) fn catalog(&self) -> &Handle<Catalog> { &self.catalog }
    /// The shop whose selection is on screen, and whether it buys items.
    pub(super) fn open_shop(&self) -> Option<(&str, bool)> {
        let merchant = self.view.as_ref()?.merchant.as_ref()?;
        match &self.detail { Some(Detail { action: Action::Shop(shop), .. }) => Some((shop.as_str(), merchant.can_sell)), _ => None }
    }
}

pub(super) fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Presentation {
        catalog: assets.load("interface/dialogue.catalog.json"),
        items: assets.load("interface/inventory.items.json"),
        generation: 0, page: 0, detail: None, merchant: default(), view: None, pending: None, consent: None,
    });
}

/// Sends one order: one copy of an offer, or a sold stack back. One order at a
/// time: the server answers with the inventory, the wallet and, for item
/// trades, the buyback list (an Offer that keeps the shop).
fn order(shop: &str, action: &Action, net: &mut crate::network::net::Net, pending: &mut Option<(u64, f64)>, now: f64,
    audio: Option<&mut crate::audio::AudioRequests>) {
    let order = match action {
        Action::Buy(offer) => inventory::Transaction::buy(shop, offer, 1),
        Action::Restore(item) => inventory::Transaction::restore(shop, *item),
        _ => return,
    };
    net.send(&ClientCommand::Transaction(order));
    *pending = Some((net.inventory.revision, now));
    // EVIDENCE: release261 UI::TradingWidget::TradingPort::HandleMessage
    //   (0x140560848) plays these once ClientTradingManager::BuyOffer
    //   (0x140279648) has queued the order; HandleEvent (0x14055FB8C)
    //   the same after a buyback.
    if let Some(audio) = audio {
        audio.ui("ui/trader/ui_trader_buy");
        audio.ui("ui/general/ui_click");
    }
}

fn menu(info: Option<&actors::NpcInfo>, target: &crate::gameplay::dialogue::Target, catalog: &Catalog) -> Vec<Choice> {
    let mut choices = Vec::new();
    // Online, the authoritative role descriptor supplies the shop name. The
    // authored shop is also useful for the offline NPC preview.
    let shop = info.map(|i| i.name.as_str()).filter(|s| !s.is_empty()).unwrap_or(&target.profile.shop);
    if !target.profile.shop.is_empty() && !shop.is_empty() {
        choices.push(Choice::new("View selection", "shop", Action::Shop(shop.to_owned())));
    }
    for (window, label) in desks(&target.profile.profession) {
        choices.push(Choice::new(*label, "shop", Action::Desk(window)));
    }
    if let Some(info) = info {
        for entry in &info.interactions {
            let choice = match entry.kind {
                0 => Choice::new(catalog.title(&entry.name), "quest", Action::Quest(entry.name.clone())),
                2 => {
                    let short = catalog.dialog(&entry.name, true);
                    Choice::new(if short.is_empty() { catalog.title(&entry.quest) } else { short }, "talk", Action::Talk(entry.name.clone(), entry.quest.clone()))
                }
                // A delivery (NPC_DELIVERY_DIALOG): its own text, else the quest's name.
                // SOURCE: DSOR_Server services/quests.py (:491-494), the barter id as
                //   the interaction's name, the quest as its quest.
                3 => {
                    let short = catalog.text(&format!("quest.{}.s_{}bartershort", entry.quest, entry.name));
                    Choice::new(if short.is_empty() { catalog.title(&entry.quest) } else { short }, "quest", Action::Deliver(entry.name.clone(), entry.quest.clone()))
                }
                // Task-target markers are not conversation options.
                _ => continue,
            };
            if !choices.iter().any(|c: &Choice| c.action == choice.action) { choices.push(choice); }
        }
    }
    choices
}

/// What the conversation's view is built from (sync): the dialogue's generation,
/// open, its NPC, Net::ui_revision, the detail open and the menu's page, the
/// shop's filters, an order pending, the catalogs loaded, our class and gender.
type ViewKey = (u64, bool, Option<u32>, Option<u64>, Option<Detail>, usize, (Option<u16>, bool, bool), bool, [bool; 3], Option<(u8, u8)>);

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
    players: Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
    time: Res<Time>,
    mut desk_windows: Query<(&Window2018, &mut Visibility)>,
    (quest_ui, quest_catalogs, mut audio, settings): (Option<Res<crate::gameplay::quests::QuestUi>>, Option<Res<Assets<crate::gameplay::quests::QuestCatalog>>>,
        Option<ResMut<crate::audio::AudioRequests>>, Res<super::Settings>),
    mut seen: Local<Option<ViewKey>>,
) {
    if presentation.generation != dialogue.generation {
        presentation.generation = dialogue.generation;
        presentation.page = 0;
        presentation.detail = None;
        presentation.merchant = default();
        presentation.view = None;
        presentation.pending = None;
        presentation.consent = None;
        // Old buttons belong to the old NPC, even if their release arrives now.
        dialogue.choice = None;
    }
    // The server's answer changes the inventory; a lost one must not lock the shop.
    if presentation.pending.is_some_and(|(revision, at)| net.as_ref().is_none_or(|n| n.inventory.revision != revision) || time.elapsed_secs_f64() - at > 2.0) {
        presentation.pending = None;
    }
    // A large Andermant purchase the player agreed to (controls::consent).
    if presentation.consent.as_ref().is_some_and(|c| c.granted) {
        let offer = presentation.consent.take().map(|c| c.offer).unwrap_or_default();
        if let (Some(shop), Some(net), None) = (presentation.shop(), net.as_mut().filter(|n| !n.offline && n.inventory.ready), presentation.pending) {
            order(&shop, &Action::Buy(offer), net, &mut presentation.pending, time.elapsed_secs_f64(), audio.as_deref_mut());
        }
    }
    // The view is built when what it shows may have changed: a choice, the
    // conversation's page, the server's word (Net::ui_revision), a catalog
    // loaded, our class or gender. Never every frame (docs/ui.md, "Per-frame cost").
    let player = players.iter().next().map(|c| (c.desc.class, c.desc.gender));
    let assets = [catalogs.contains(&presentation.catalog), item_catalogs.contains(&presentation.items),
        quest_catalogs.as_ref().zip(quest_ui.as_ref()).is_some_and(|(assets, ui)| assets.contains(&ui.catalog))];
    let key: ViewKey = (dialogue.generation, dialogue.open, dialogue.target.as_ref().map(|t| t.actor), net.as_ref().map(|n| n.ui_revision),
        presentation.detail.clone(), presentation.page, (presentation.merchant.category, presentation.merchant.buyback, presentation.merchant.usable),
        presentation.pending.is_some(), assets, player);
    if dialogue.choice.is_none() && seen.as_ref() == Some(&key) { return; }
    *seen = Some(key);
    let fallback = Catalog::default();
    let catalog = catalogs.get(&presentation.catalog).unwrap_or(&fallback);
    // A trader opened without an NPC (Dialogue::trade): its selection at once,
    // under no NPC (no portrait, no menu); Back closes it.
    let remote = dialogue.remote.clone().filter(|_| dialogue.open && dialogue.target.is_none());
    if let Some(shop) = &remote {
        if presentation.detail.is_none() { presentation.detail = Some(Detail { action: Action::Shop(shop.clone()), page: 0 }); }
    }
    let active = dialogue.target.as_ref().filter(|_| dialogue.open).cloned().or_else(|| remote.as_ref().map(|shop|
        crate::gameplay::dialogue::Target { entity: Entity::PLACEHOLDER, actor: 0, template: shop.clone(), profile: default() }));
    let mut next = None;
    if let Some(target) = active {
        let info = net.as_ref().and_then(|n| n.npc_dialogue.infos.get(&target.actor));
        let available = menu(info, &target, catalog);
        if let Some(index) = dialogue.choice.take() {
            let action = if index == CLOSE { Some(Action::Close) }
                else { presentation.view.as_ref().and_then(|v| v.options.get(index)).map(|c| c.action.clone()) };
            match action {
                Some(Action::Close | Action::Leave) => dialogue.close(),
                Some(Action::Desk(window)) => {
                    for (w, mut visibility) in &mut desk_windows {
                        if w.0 == window { *visibility = Visibility::Inherited; }
                    }
                    dialogue.close();
                }
                Some(Action::Back) if remote.is_some() => dialogue.close(),
                Some(Action::Back) => presentation.detail = None,
                Some(Action::Category(category)) => {
                    presentation.merchant.category = Some(category);
                    if let Some(detail) = &mut presentation.detail { detail.page = 0; }
                }
                Some(Action::Buyback(buyback)) => {
                    presentation.merchant.buyback = buyback;
                    if let Some(detail) = &mut presentation.detail { detail.page = 0; }
                }
                Some(Action::Usable(usable)) => {
                    presentation.merchant.usable = usable;
                    if let Some(detail) = &mut presentation.detail { detail.page = 0; }
                }
                Some(Action::Previous) => {
                    if let Some(detail) = &mut presentation.detail { detail.page = detail.page.saturating_sub(1); }
                }
                Some(Action::Next) => {
                    if let Some(detail) = &mut presentation.detail { detail.page += 1; }
                    else { presentation.page += 1; }
                }
                Some(action @ (Action::Buy(_) | Action::Restore(_))) => {
                    // Offline, the same checks and question; only the order needs the server.
                    if let (Some(shop), Some(net), None) = (presentation.shop(), net.as_mut(), presentation.pending) {
                        let offer = match &action {
                            Action::Buy(id) => net.npc_dialogue.offers.get(&shop).and_then(|o| o.offers.iter().flatten().find(|o| o.id == *id)).cloned(),
                            _ => None,
                        };
                        let wallet = net.wallet.is_some().then(|| controls::currency::Wallet::of(net));
                        match offer {
                            // Greyed on its card, which takes no click (merchant::prepare):
                            // never ordered.
                            // DEVIATION: the original's IsOfferPurchasable (0x14027C184)
                            //   names the missing currency (gui.window.messages
                            //   insufficientVCToBuy...) and checks for a free bag slot
                            //   (noFreeInventorySlot); the server decides the slot here.
                            Some(o) if wallet.as_ref().is_some_and(|w| !merchant::affordable(&o, &shop, &catalog.trading, w)) => {}
                            // From LARGE_ANDERMANT on, the player's word first (controls::consent),
                            // unless they asked not to be asked again.
                            Some(o) if merchant::andermant(&o) >= merchant::LARGE_ANDERMANT
                                && !settings.current.skipped_dialogs.contains(&controls::consent::DIALOG_ID) => {
                                presentation.consent = Some(Consent { andermant: merchant::andermant(&o), offer: o.id, granted: false });
                            }
                            _ if !net.offline && net.inventory.ready =>
                                order(&shop, &action, net, &mut presentation.pending, time.elapsed_secs_f64(), audio.as_deref_mut()),
                            _ => {}
                        }
                    }
                }
                Some(action @ (Action::Quest(_) | Action::Talk(_, _) | Action::Shop(_))) if available.iter().any(|c| c.action == action) => {
                    if let (Action::Shop(shop), Some(net)) = (&action, net.as_mut()) {
                        if !net.offline { net.npc_dialogue.offers.remove(shop); net.touch_ui(); }
                        net.send(&ClientCommand::RequestOffer(inventory::RequestOffer { actor: target.actor, by_name: false, shop: String::new() }));
                    }
                    presentation.merchant = default();
                    presentation.detail = Some(Detail { action, page: 0 });
                }
                Some(Action::Accept(id, operation)) if available.iter().any(|c| c.action == Action::Quest(id.clone())) => {
                    if let Some(net) = net.as_mut().filter(|n| !n.offline) {
                        let state = net.npc_dialogue.quests.get(&id.to_lowercase()).copied();
                        if (operation == 0 && state.is_none_or(|s| s >= 2)) || (operation == 1 && state == Some(1)) {
                            // The NPC's menu again while it offers something else, else the
                            // conversation ends.
                            // EVIDENCE: release261 QuestWidget2 (0x1404D1194): OpenQuest plays
                            //   ui/quests/ui_quest_accept; then 0x1404D0F84 asks the NPC
                            //   anew and reopens its screen if it has another entry
                            //   (0x140490BFC), else closes it.
                            let more = available.iter().any(|c| c.action != Action::Quest(id.clone()));
                            net.send(&ClientCommand::Quest(actors::Quest { quest_id: id, state: operation, parameter: 0 }));
                            net.send(&ClientCommand::NpcRequest(actors::NpcRequest { actor: target.actor }));
                            if operation == 0 { if let Some(audio) = audio.as_mut() { audio.ui("ui/quests/ui_quest_accept"); } }
                            if more { presentation.detail = None; } else { dialogue.close(); }
                        }
                    }
                }
                Some(Action::Deliver(barter, _)) if available.iter().any(|c| matches!(&c.action, Action::Deliver(b, _) if *b == barter)) => {
                    // The server takes the items and answers with the inventory, the
                    // task's progress and the NPC's new options.
                    if let Some(net) = net.as_mut().filter(|n| !n.offline) {
                        net.send(&ClientCommand::Deliver(actors::Deliver { actor: target.actor, value: barter }));
                        net.send(&ClientCommand::NpcRequest(actors::NpcRequest { actor: target.actor }));
                        dialogue.close();
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
                body: None, merchant: None, quest: None, options: Vec::new(),
            };
            let player = players.iter().next();
            let facts = quest::Facts {
                catalog,
                quests: quest_catalogs.as_ref().zip(quest_ui.as_ref()).and_then(|(assets, ui)| assets.get(&ui.catalog)),
                items: item_catalogs.get(&presentation.items),
                entry: None,
                female: player.is_some_and(|c| c.desc.gender == 1),
                name: net.as_ref().and_then(|n| n.local_name.as_deref()).unwrap_or(""),
                class: crate::actors::character::CLASSES.get(player.map_or(0, |c| c.desc.class as usize)).copied().unwrap_or("warrior"),
                level: net.as_ref().and_then(|n| n.level),
                xp: net.as_ref().and_then(|n| n.xp),
            };
            if let Some(detail) = &presentation.detail {
                let body = match &detail.action {
                    // Accept (if any), then Back: the buttons send these options.
                    Action::Quest(id) => {
                        let state = net.as_ref().and_then(|n| n.npc_dialogue.quests.get(&id.to_lowercase())).copied();
                        let (field, accept) = match state { Some(0) => ("progress", None), Some(1) => ("complete", Some(1)), _ => ("offer", Some(0)) };
                        if let Some(operation) = accept {
                            view.options.push(Choice::new(quest::accept_label(catalog, operation == 0), "quest", Action::Accept(id.clone(), operation)));
                        }
                        view.options.push(Choice::new(quest::back_label(catalog), "close", Action::Leave));
                        let entry = net.as_ref().and_then(|n| n.npc_dialogue.log.get(&id.to_lowercase()));
                        view.quest = Some(quest::quest(id, field, &quest::Facts { entry, ..facts }, accept.map(|_| 0), view.options.len() - 1));
                        None
                    }
                    Action::Talk(id, _) => {
                        view.options.push(Choice::new(quest::talk_label(catalog), "talk", Action::ContinueTalk(id.clone())));
                        view.options.push(Choice::new(quest::back_label(catalog), "close", Action::Leave));
                        view.quest = Some(quest::conversation(id, &facts, 0, 1));
                        None
                    }
                    Action::Shop(shop) => {
                        let offers = net.as_ref().and_then(|n| n.npc_dialogue.offers.get(shop));
                        let class = players.iter().next().map(|c| c.desc.class);
                        let level = net.as_ref().and_then(|n| n.level);
                        // The wallet greys what it does not pay (CurrencyChanged bumps
                        // Net::ui_revision: rebuilt when it changes).
                        let wallet = net.as_ref().filter(|n| n.wallet.is_some()).map(|n| controls::currency::Wallet::of(n));
                        let merchant = merchant::build(offers, &presentation.merchant, detail.page,
                            item_catalogs.get(&presentation.items), &catalog.trading, class, level, wallet.as_ref());
                        merchant.choices(&mut view.options);
                        let body = if offers.is_none() {
                            if net.as_ref().is_some_and(|n| n.offline) { "The merchant's selection is available when playing online." }
                            else { "Loading selection..." }
                        } else if merchant.items.is_empty() {
                            if merchant.buyback { "There are no items to buy back." }
                            else { "There are no offers matching this selection." }
                        } else { "" };
                        view.merchant = Some(merchant);
                        view.options.push(Choice::new("Back", "close", Action::Back));
                        Some(body.to_owned())
                    }
                    _ => None,
                };
                view.body = body;
            } else {
                let pages = available.len().div_ceil(PAGE_SIZE).max(1);
                let page = presentation.page % pages;
                view.options.extend(available.into_iter().skip(page * PAGE_SIZE).take(PAGE_SIZE));
                if pages > 1 { view.options.push(Choice::new(format!("More ({}/{pages})", page + 1), "talk", Action::Next)); }
                view.options.push(Choice::new("Close", "close", Action::Close));
            }
            if let (Some(merchant), Some(detail)) = (&view.merchant, &mut presentation.detail) { detail.page = merchant.page; }
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
    Art { tex: Some(texture), pos: vec![l, t, r, t, r, b, l, b], uv: vec![0., 0., 1., 0., 1., 1., 0., 1.], color: vec![1.; 16], idx: vec![0, 1, 2, 0, 2, 3], saturation: 1.0 }
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
        // The options in the menu only; a quest or a conversation keeps the portrait.
        for (index, choice) in view.options.iter().enumerate().filter(|_| view.body.is_none() && view.quest.is_none()) {
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
        // Where the layout puts it, against the screen's left edge.
        // FAILURE (before 2026-10-24): moved 12 left and 50 down, the portrait
        //   and its name sat under the 2026 client's (a capture of Thabo's offer:
        //   the name plate centred at 128, 293, as authored).
        for w in &mut layout.widgets { w.k = 0.0; }
    } else if source.window == "questscreen2" {
        if let Some(screen) = &view.quest { quest::prepare(source, screen, &mut layout); }
        for w in &mut layout.widgets { w.k = 0.0; }
    } else if source.window == "tradingscreen" {
        if let Some(merchant) = &view.merchant { merchant::prepare(source, view, merchant, &mut layout); }
    }
    layout
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
    fn long_dialogue_wraps_without_losing_unicode_words() {
        let text = "Agathon’s light shines. ".repeat(100);
        let lines = quest::wrap(&quest::runs(&text), 513.0, 9.5 * FONT_SCALE);
        assert!(lines.len() > 1);
        let words: Vec<String> = lines.iter().map(|l| l.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>()
            .join(" ").split_whitespace().map(str::to_owned).collect();
        assert_eq!(words, text.split_whitespace().collect::<Vec<_>>());
    }
    #[test]
    fn spoken_texts_take_the_gender_and_the_highlights() {
        let catalog: Catalog = serde_json::from_value(serde_json::json!({ "portraits": {}, "icons": {},
            "texts": { "quest.q.s_offer": "Hi man.", "quest.q.s_offer.f": "Hi woman.", "quest.q.s_progress": "Still here?" },
            "marked": { "quest.q.s_offer": "Hi <QuestHighlightStart>man</>." } })).unwrap();
        assert_eq!(catalog.spoken("quest.Q.s_offer", false), "Hi <QuestHighlightStart>man</>.");
        assert_eq!(catalog.spoken("quest.q.s_offer", true), "Hi woman.");
        assert_eq!(catalog.spoken("quest.q.s_progress", true), "Still here?");
        assert_eq!(catalog.spoken("quest.q.s_complete", false), "");
    }
}
