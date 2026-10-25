//! The confirmation the original asks before a large Andermant purchase
//! (crate::ui::dialogue::sync holds the purchase back): the confirmation dialog
//! (items::DESTROY_DIALOG) with its DefaultDoNotShowAgain layer, centred: "You
//! are about to spend a large amount of Andermant! (N)". Confirm orders it;
//! Cancel, Escape or the end of the conversation drop it. "Do not show this
//! window again", ticked when confirming, stops asking
//! (Preferences::skipped_dialogs).
//!
//! EVIDENCE: release261 dro_client64:
//!   - UI::TradingWidget::BuyItemClick (0x14055C418): an offer whose Andermant
//!     price reaches TemplateManager +0x1C3C (_Globals
//!     LargeRCExpensesNotificationThreshold, 20000) goes to
//!     DrasaUIManager::AskForLargeRCExpenseConsent (0x1403E9E6C) instead of
//!     being ordered.
//!   - AskForLargeRCExpenseConsent: gui.window.messages largeRCExpenses with the
//!     price ("{0:i}"), then ShowConfirmDialog (0x1403F57B4) with layer type 1,
//!     alignment 0 and DialogId 6. ShowConfirmDialog sends the purchase at once
//!     when the settings' list (ClientSettingsManager +0x198) holds that id.
//!   - UI::ConfirmWidget: SetConfirmLayer (0x1403D2EF8) 0 Default, 1
//!     DefaultDoNotShowAgain (with its CheckBox), 2 DeleteItem, 3
//!     DeleteItemDoNotShowAgain; UpdateAlignment (0x1403D32AC) 0 DialogCenter;
//!     its events confirm, cancel or close, doNotShowAgainClick (0x1403D2A28).
//! INFERRED: the box counts when confirming only (its handler 0x1403D289C and
//!   the confirm path were not traced).
use super::*;

/// The dialog's page group and pages (section, items::prepare_confirm): an item
/// to destroy, a large Andermant purchase.
pub(in crate::ui) const PAGES: &str = "confirm_layer";
pub(in crate::ui) const DESTROY: u8 = 0;
pub(in crate::ui) const SPEND: u8 = 1;
/// The original's DialogId of this question (Preferences::skipped_dialogs).
pub(in crate::ui) const DIALOG_ID: u32 = 6;
const QUESTION: &str = "Canvas/DialogCenter/Content/DefaultDoNotShowAgain/Question";
const CHECKBOX: &str = "Canvas/DialogCenter/Content/DefaultDoNotShowAgain/CheckBox";
/// gui.window.messages largeRCExpenses.
const TEXT: &str = "You are about to spend a large amount of Andermant! ({0:i})";

/// The question written on the dialog.
#[derive(Component)]
pub(in crate::ui) struct ConsentText;

/// Opens the dialog for a purchase waiting for the player's word, writes its
/// question, takes the answer, and closes it once the purchase is answered or
/// gone. Nothing to do while nothing is asked.
#[allow(clippy::too_many_arguments)]
pub(in crate::ui) fn sync(mut commands: Commands, mut windows: Windows, mut pointer: ResMut<Pointer>,
    mut presentation: ResMut<crate::ui::dialogue::Presentation>, mut settings: ResMut<settings::Settings>,
    (layouts, loaded, assets, screen): (Res<Assets<Layout>>, Res<Loaded>, Res<AssetServer>, Res<Screen>),
    old: Query<Entity, With<ConsentText>>, mut written: Local<Option<(Entity, u32, u32, u32)>>) {
    let wanted = presentation.consent().filter(|c| !c.granted).map(|c| c.andermant);
    let answer = pointer.consent.take();
    if wanted.is_none() && written.is_none() { return; }
    let Some((root, mut c, mut v)) = windows.iter_mut().find(|(_, n, _, _, _)| n.0 == items::DESTROY_DIALOG).map(|(e, _, c, v, _)| (e, c, v)) else { return };
    let open = *v != Visibility::Hidden && c.page(PAGES) == SPEND;
    let mut asking = wanted;
    if let Some(yes) = answer.filter(|_| open && wanted.is_some()) {
        if yes && c.toggles.contains(CHECKBOX) { settings.skip_dialog(DIALOG_ID); }
        presentation.answer_consent(yes);
        asking = None;
    } else if wanted.is_some() && written.is_some() && !open {
        // Closed another way (Escape): no purchase.
        presentation.answer_consent(false);
        asking = None;
    } else if wanted.is_some() && written.is_none() {
        // An item's destroy question is answered first.
        if *v != Visibility::Hidden { return; }
        c.set_page(PAGES, SPEND);
        c.toggles.remove(CHECKBOX);
        c.hovered = None;
        c.pressed = None;
        *v = Visibility::Inherited;
    }
    if asking.is_none() && open { *v = Visibility::Hidden; }
    // The question while asked, written again when the price or the screen changes.
    let key = asking.map(|price| (root, price, screen.width.to_bits(), screen.scale.to_bits()));
    if *written == key { return; }
    *written = key;
    for e in &old { commands.entity(e).despawn(); }
    let Some(price) = asking else { return };
    let Some(w) = loaded.layouts.iter().filter_map(|(h, _, _)| layouts.get(h)).find(|l| l.window == items::DESTROY_DIALOG)
        .and_then(|l| l.widgets.iter().find(|w| w.id == QUESTION)) else { return };
    let [l, t, r, b] = w.rect;
    // As the destroy question (items::destroy_dialog) is written.
    let font = TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(9.0 * FONT_SCALE * screen.scale), ..default() };
    commands.spawn((ConsentText, Text2d::new(TEXT.replace("{0:i}", &price.to_string())), font, TextColor(Color::srgb(0.9, 0.78, 0.54)),
        bevy::text::TextLayout::justify(bevy::text::Justify::Center), Anchor::CENTER, bevy::text::TextBounds::new_horizontal((r - l) * screen.scale),
        Transform::from_translation(screen.world(w.k, (l + r) * 0.5, (t + b) * 0.5).extend(WINDOW_ITEMS_DEPTH + 0.2)).with_scale(Vec3::splat(1.0 / screen.scale)),
        RenderLayers::layer(LAYER), ChildOf(root)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dialogue::Presentation;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Layout>().init_asset::<Font>()
            .init_resource::<Pointer>().init_resource::<Presentation>().init_resource::<settings::Settings>()
            .init_resource::<Loaded>().init_resource::<Screen>()
            .add_systems(Update, sync);
        let layout = items::prepare_confirm(&template::fixture::layout(items::DESTROY_DIALOG,
            &[("Canvas/DialogCenter/Content/DefaultDoNotShowAgain", "Label", false), (QUESTION, "TextLabel", true), (CHECKBOX, "Button", false)]));
        let dialog = app.world_mut().spawn((Window2018(items::DESTROY_DIALOG.into()), WindowControls::new(&layout),
            Visibility::Hidden, Transform::default())).id();
        let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(layout);
        app.world_mut().resource_mut::<Loaded>().layouts.push((handle, 610.0, None));
        (app, dialog)
    }

    fn shown(app: &App, dialog: Entity) -> bool {
        *app.world().get::<Visibility>(dialog).unwrap() != Visibility::Hidden
            && app.world().get::<WindowControls>(dialog).unwrap().page(PAGES) == SPEND
    }

    fn question(app: &mut App) -> Vec<String> {
        app.world_mut().query_filtered::<&Text2d, With<ConsentText>>().iter(app.world()).map(|t| t.0.clone()).collect()
    }

    fn answer(app: &mut App, yes: bool) {
        app.world_mut().resource_mut::<Pointer>().consent = Some(yes);
        app.update();
    }

    /// Asked: the purchase layer with the price; Confirm grants it, Cancel,
    /// closing the dialog or the end of the conversation drop it.
    #[test]
    fn the_purchase_waits_for_the_players_word() {
        let (mut app, dialog) = app();
        app.update();
        assert!(!shown(&app, dialog) && question(&mut app).is_empty(), "nothing asked: nothing shown");
        app.world_mut().resource_mut::<Presentation>().ask_consent("statue", 25_000);
        app.update();
        assert!(shown(&app, dialog));
        assert_eq!(question(&mut app), ["You are about to spend a large amount of Andermant! (25000)"]);
        answer(&mut app, true);
        assert_eq!(app.world().resource::<Presentation>().consent().map(|c| (c.offer.as_str(), c.granted)), Some(("statue", true)));
        assert!(!shown(&app, dialog) && question(&mut app).is_empty());

        app.world_mut().resource_mut::<Presentation>().ask_consent("statue", 25_000);
        app.update();
        answer(&mut app, false);
        assert!(app.world().resource::<Presentation>().consent().is_none() && !shown(&app, dialog));

        app.world_mut().resource_mut::<Presentation>().ask_consent("statue", 25_000);
        app.update();
        *app.world_mut().get_mut::<Visibility>(dialog).unwrap() = Visibility::Hidden;
        app.update();
        assert!(app.world().resource::<Presentation>().consent().is_none(), "closed (Escape): no purchase");
        assert!(question(&mut app).is_empty());

        app.world_mut().resource_mut::<Presentation>().ask_consent("statue", 25_000);
        app.update();
        app.world_mut().resource_mut::<Presentation>().answer_consent(false);
        app.update();
        assert!(!shown(&app, dialog) && question(&mut app).is_empty(), "the conversation ended: the dialog closes");
    }

    /// An item's destroy question keeps the dialog until it is answered.
    #[test]
    fn a_destroy_question_is_answered_first() {
        let (mut app, dialog) = app();
        app.world_mut().get_mut::<WindowControls>(dialog).unwrap().set_page(PAGES, DESTROY);
        *app.world_mut().get_mut::<Visibility>(dialog).unwrap() = Visibility::Inherited;
        app.world_mut().resource_mut::<Presentation>().ask_consent("statue", 25_000);
        app.update();
        assert_eq!(app.world().get::<WindowControls>(dialog).unwrap().page(PAGES), DESTROY);
        *app.world_mut().get_mut::<Visibility>(dialog).unwrap() = Visibility::Hidden;
        app.update();
        assert!(shown(&app, dialog));
    }
}
