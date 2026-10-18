//! The Lighting page's global render controls: the lighting panel's multipliers
//! (crate::rendering::lighting::FIELDS, saved to look.json), each with - / +
//! (Shift: x10) and a value to click and type (Enter applies, Esc cancels).
//! While a value takes text the game's keys and the free camera are held
//! (crate::diagnostics::lighting_menu). Reset puts every multiplier back to 1,
//! the map as authored. The RENDER SWITCHES card flips, live, how the light is
//! put together (crate::rendering::lighting::RenderSwitches), to compare the
//! original's way with the others; it is not saved.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;

use super::window::menu_text;
use crate::rendering::lighting::{save, AoMode, Lighting, RenderSwitches, FIELDS};

#[derive(Component)]
struct ValueText(usize);
#[derive(Component)]
struct NumericField(usize);
#[derive(Component)]
struct StepButton(usize, f32);
#[derive(Component)]
struct ResetButton;
#[derive(Component, Clone, Copy)]
enum Switch { LightSum, Occlusion, ShadowFloor, Rim }
#[derive(Component)]
struct SwitchText(Switch);

const SWITCHES: [(Switch, &str); 4] = [
    (Switch::LightSum, "Light sum"),
    (Switch::Occlusion, "Ambient occlusion"),
    (Switch::ShadowFloor, "Shadow floor"),
    (Switch::Rim, "Rim light"),
];

impl Switch {
    /// The setting as shown.
    fn label(self, s: &RenderSwitches) -> &'static str {
        match self {
            Switch::LightSum => if s.gamma_light { "Gamma" } else { "Linear" },
            Switch::Occlusion => match s.ao {
                AoMode::Linear => "Once, linear",
                AoMode::Squared => "Squared",
                AoMode::Clamped => "x1.3, squared",
            },
            Switch::ShadowFloor => if s.shadow_floor { "On" } else { "Off" },
            Switch::Rim => if s.rim { "On" } else { "Off" },
        }
    }

    fn next(self, s: &mut RenderSwitches) {
        match self {
            Switch::LightSum => s.gamma_light = !s.gamma_light,
            Switch::Occlusion => s.ao = match s.ao {
                AoMode::Linear => AoMode::Squared,
                AoMode::Squared => AoMode::Clamped,
                AoMode::Clamped => AoMode::Linear,
            },
            Switch::ShadowFloor => s.shadow_floor = !s.shadow_floor,
            Switch::Rim => s.rim = !s.rim,
        }
    }
}

/// A button as wide as a field's - / + / value (24 + 5 + 24 + 5 + 58..100),
/// narrowing like the value at the window's minimum width.
fn wide_button(row: &mut ChildSpawnerCommands, marker: impl Bundle, text: impl Bundle) {
    row.spawn((
        Button,
        marker,
        Node {
            flex_grow: 1.0,
            flex_basis: Val::Px(116.0),
            min_width: Val::Px(58.0),
            max_width: Val::Px(158.0),
            min_height: Val::Px(25.0),
            flex_shrink: 1.0,
            padding: UiRect::axes(Val::Px(5.0), Val::Px(3.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
    ))
    .with_children(|button| {
        button.spawn(text);
    });
}

fn field_row() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_height: Val::Px(30.0),
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(5.0),
        align_items: AlignItems::Center,
        ..default()
    }
}

fn row_label(label: &'static str) -> impl Bundle {
    (
        Text::new(label),
        TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
        TextColor(Color::srgb(0.83, 0.86, 0.91)),
        // Shrinks (wrapping) before the controls leave the card at the window's
        // 520 px minimum.
        Node { width: Val::Px(140.0), min_width: Val::Px(60.0), flex_shrink: 1.0, ..default() },
    )
}

fn button_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text),
        TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
        TextColor(Color::srgb(0.94, 0.95, 0.97)),
    )
}

/// The value being typed, if any.
#[derive(Resource, Default)]
pub(super) struct NumericEditor {
    field: Option<usize>,
    draft: String,
    replace_on_next_input: bool,
}

impl NumericEditor {
    pub(super) fn typing(&self) -> bool { self.field.is_some() }

    pub(super) fn cancel(&mut self) {
        self.field = None;
        self.draft.clear();
        self.replace_on_next_input = false;
    }
}

/// The GLOBAL RENDER CONTROLS card.
pub(super) fn spawn(page: &mut ChildSpawnerCommands) {
    page.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
    ))
    .with_children(|card| {
        card.spawn(menu_text("GLOBAL RENDER CONTROLS", 12.0, Color::srgb(0.91, 0.73, 0.43)));
        card.spawn(menu_text(
            "Click a value to type; Enter applies; Esc cancels. Shift adjusts by x10.",
            11.0,
            Color::srgb(0.64, 0.70, 0.79),
        ));
        for (i, field) in FIELDS.iter().enumerate() {
            card.spawn(Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(30.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(5.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new(field.label),
                    TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
                    TextColor(Color::srgb(0.83, 0.86, 0.91)),
                    // Shrinks (wrapping) before the controls leave the card at the window's
        // 520 px minimum.
        Node { width: Val::Px(140.0), min_width: Val::Px(60.0), flex_shrink: 1.0, ..default() },
                ));
                for (sign, label) in [(-1.0, "-"), (1.0, "+")] {
                    row.spawn((
                        Button,
                        StepButton(i, sign),
                        Node {
                            width: Val::Px(24.0),
                            min_height: Val::Px(25.0),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.20, 0.23, 0.29)),
                    ))
                    .with_children(|button| {
                        button.spawn((
                            Text::new(label),
                            TextFont { font_size: bevy::text::FontSize::Px(13.0), ..default() },
                            TextColor(Color::srgb(0.94, 0.95, 0.97)),
                        ));
                    });
                }
                row.spawn((
                    Button,
                    NumericField(i),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(58.0),
                        max_width: Val::Px(100.0),
                        min_height: Val::Px(25.0),
                        flex_shrink: 0.0,
                        padding: UiRect::axes(Val::Px(5.0), Val::Px(3.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.07, 0.09, 0.13)),
                ))
                .with_children(|value| {
                    value.spawn((
                        ValueText(i),
                        Text::new(""),
                        TextFont { font_size: bevy::text::FontSize::Px(12.0), ..default() },
                        TextColor(Color::srgb(0.94, 0.95, 0.97)),
                    ));
                });
            });
        }
        card.spawn(field_row()).with_children(|row| {
            row.spawn(row_label("All (map as authored)"));
            wide_button(row, ResetButton, button_text("Reset to 1"));
        });
    });
    page.spawn((
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            padding: UiRect::all(Val::Px(12.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.12, 0.145, 0.19)),
    ))
    .with_children(|card| {
        card.spawn(menu_text("RENDER SWITCHES", 12.0, Color::srgb(0.91, 0.73, 0.43)));
        card.spawn(menu_text(
            "How the light is put together, to compare the ways. Click to change. Defaults (the closest to the original measured): Gamma, Once linear, Off, On. Not saved: a restart goes back to them (or to DSOR_FLAGS).",
            11.0,
            Color::srgb(0.64, 0.70, 0.79),
        ));
        for (switch, label) in SWITCHES {
            card.spawn(field_row()).with_children(|row| {
                row.spawn(row_label(label));
                wide_button(row, switch, (SwitchText(switch), button_text("")));
            });
        }
    });
}

/// The page systems, run by the menu after the editors are blurred.
pub(super) fn systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (press_buttons, press_reset, press_switches, edit_numeric_fields, refresh_values, refresh_switches).chain()
}

fn press_buttons(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Query<(&Interaction, &StepButton), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
) {
    let mul = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 10.0 } else { 1.0 };
    let mut changed = false;
    for (interaction, StepButton(i, sign)) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let f = &FIELDS[*i];
        let v = (f.get)(&lighting) + f.step * sign * mul;
        (f.set)(&mut lighting, v);
        changed = true;
    }
    if changed {
        editor.cancel();
        save(&lighting);
    }
}

fn press_reset(
    buttons: Query<&Interaction, (Changed<Interaction>, With<ResetButton>)>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        *lighting = Lighting::default();
        editor.cancel();
        save(&lighting);
    }
}

fn press_switches(
    buttons: Query<(&Interaction, &Switch), Changed<Interaction>>,
    switches: Option<ResMut<RenderSwitches>>,
) {
    let Some(mut switches) = switches else { return };
    for (interaction, switch) in &buttons {
        if *interaction == Interaction::Pressed {
            switch.next(&mut switches);
        }
    }
}

fn refresh_switches(
    switches: Option<Res<RenderSwitches>>,
    added: Query<(), Added<SwitchText>>,
    mut texts: Query<(&SwitchText, &mut Text)>,
) {
    let Some(switches) = switches else { return };
    if !switches.is_changed() && added.is_empty() {
        return;
    }
    for (SwitchText(switch), mut t) in &mut texts {
        t.0 = switch.label(&switches).to_owned();
    }
}

pub(super) fn format_edit_value(value: f32, step: f32) -> String {
    if step >= 1.0 {
        return format!("{value:.0}");
    }
    let mut result = format!("{value:.5}");
    while result.contains('.') && result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.pop();
    }
    result
}

fn edit_numeric_fields(
    mut events: MessageReader<KeyboardInput>,
    fields: Query<(&Interaction, &NumericField), Changed<Interaction>>,
    mut lighting: ResMut<Lighting>,
    mut editor: ResMut<NumericEditor>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    for (interaction, NumericField(index)) in &fields {
        if *interaction == Interaction::Pressed {
            editor.field = Some(*index);
            editor.draft = format_edit_value((FIELDS[*index].get)(&lighting), FIELDS[*index].step);
            editor.replace_on_next_input = true;
        }
    }
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let Some(index) = editor.field else { continue };
        match &event.logical_key {
            Key::Enter => {
                if let Ok(value) = editor.draft.trim().parse::<f32>() {
                    if value.is_finite() {
                        (FIELDS[index].set)(&mut lighting, value);
                        save(&lighting);
                        editor.cancel();
                    }
                }
            }
            Key::Escape => editor.cancel(),
            Key::Backspace => {
                editor.draft.pop();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars)
                if chars.as_str().eq_ignore_ascii_case("a")
                    && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)) =>
            {
                editor.draft.clear();
                editor.replace_on_next_input = false;
            }
            Key::Character(chars) => {
                let allowed = chars.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
                if allowed {
                    if editor.replace_on_next_input {
                        editor.draft.clear();
                        editor.replace_on_next_input = false;
                    }
                    if editor.draft.len() + chars.len() <= 24 {
                        editor.draft.push_str(chars);
                    }
                }
            }
            _ => {}
        }
    }
}

fn refresh_values(lighting: Res<Lighting>, editor: Res<NumericEditor>, mut texts: Query<(&ValueText, &mut Text)>) {
    if !lighting.is_changed() && !editor.is_changed() {
        return;
    }
    for (ValueText(i), mut t) in &mut texts {
        t.0 = if editor.field == Some(*i) {
            format!("{}_", editor.draft)
        } else {
            format_edit_value((FIELDS[*i].get)(&lighting), FIELDS[*i].step)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Lighting { sun: 1.35, fog: 0.03, shadows: false, ..default() })
            .insert_resource(RenderSwitches { gamma_light: true, ao: AoMode::Linear, shadow_floor: false, rim: true })
            .init_resource::<NumericEditor>()
            .add_systems(Update, systems());
        app
    }

    fn press(app: &mut App, button: Entity) {
        app.world_mut().entity_mut(button).insert(Interaction::Pressed);
        app.update();
        app.world_mut().entity_mut(button).insert(Interaction::None);
        app.update();
    }

    #[test]
    fn reset_puts_every_multiplier_back_to_one() {
        let mut app = app();
        let reset = app.world_mut().spawn((Button, ResetButton, Interaction::None)).id();
        app.world_mut().resource_mut::<NumericEditor>().field = Some(0);
        press(&mut app, reset);
        let l = app.world().resource::<Lighting>();
        for f in FIELDS {
            assert_eq!((f.get)(l), (f.get)(&Lighting::default()), "{}", f.label);
        }
        assert!(!app.world().resource::<NumericEditor>().typing(), "a typed value is dropped");
    }

    #[test]
    fn switches_cycle_live_and_show_their_setting() {
        let mut app = app();
        let ao = app.world_mut().spawn((Button, Switch::Occlusion, Interaction::None)).id();
        let sum = app.world_mut().spawn((Button, Switch::LightSum, Interaction::None)).id();
        let text = app.world_mut().spawn((SwitchText(Switch::Occlusion), Text::new(""))).id();
        app.update();
        assert_eq!(app.world().get::<Text>(text).unwrap().0, "Once, linear", "shown when the page opens");
        for (mode, label) in [(AoMode::Squared, "Squared"), (AoMode::Clamped, "x1.3, squared"), (AoMode::Linear, "Once, linear")] {
            press(&mut app, ao);
            assert_eq!(app.world().resource::<RenderSwitches>().ao, mode);
            assert_eq!(app.world().get::<Text>(text).unwrap().0, label);
        }
        press(&mut app, sum);
        assert!(!app.world().resource::<RenderSwitches>().gamma_light);
        // The multipliers are untouched by the switches.
        assert_eq!(app.world().resource::<Lighting>().sun, 1.35);
    }

    #[test]
    fn every_switch_has_two_or_more_settings_with_distinct_labels() {
        for (switch, _) in SWITCHES {
            let mut s = RenderSwitches { gamma_light: true, ao: AoMode::Linear, shadow_floor: false, rim: true };
            let first = switch.label(&s);
            switch.next(&mut s);
            assert_ne!(switch.label(&s), first);
        }
    }
}
