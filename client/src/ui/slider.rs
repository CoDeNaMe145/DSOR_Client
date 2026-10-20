//! Percentage sliders shared by game settings and Bevy tool panels.
use bevy::{prelude::*, ui::RelativeCursorPosition, window::PrimaryWindow};

pub(crate) fn value_at(x: f32, left: f32, right: f32, min: u8, max: u8) -> u8 {
    if !x.is_finite() || right <= left {
        return min;
    }
    let fraction = ((x - left) / (right - left)).clamp(0.0, 1.0);
    min + (fraction * max.saturating_sub(min) as f32).round() as u8
}

pub(crate) fn grab_offset(x: f32, left: f32, right: f32, fraction: f32, radius: f32) -> f32 {
    let offset = x - (left + (right - left) * fraction);
    if offset.abs() <= radius {
        offset
    } else {
        0.0
    }
}

#[derive(Component)]
pub(crate) struct PercentSlider {
    pub value: u8,
    pub edited: bool,
    min: u8,
    max: u8,
}
impl PercentSlider {
    pub fn new(value: u8, min: u8, max: u8) -> Self {
        Self {
            value: value.clamp(min, max),
            min,
            max,
            edited: false,
        }
    }
    fn fraction(&self) -> f32 {
        (self.value.clamp(self.min, self.max) - self.min) as f32
            / self.max.saturating_sub(self.min).max(1) as f32
    }
}

#[derive(Component)]
struct Part {
    root: Entity,
    thumb: bool,
}

pub(crate) fn spawn(
    parent: &mut ChildSpawnerCommands,
    width: f32,
    value: u8,
    min: u8,
    max: u8,
) -> Entity {
    let mut root = parent.spawn((
        PercentSlider::new(value, min, max),
        Button,
        RelativeCursorPosition::default(),
        Node {
            width: px(width),
            height: px(26),
            flex_shrink: 0.0,
            ..default()
        },
    ));
    let id = root.id();
    root.with_children(|root| {
        root.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(6),
                right: px(6),
                top: px(11),
                height: px(4),
                ..default()
            },
            BackgroundColor(Color::srgb(0.22, 0.22, 0.27)),
        ))
        .with_children(|rail| {
            rail.spawn((
                Part {
                    root: id,
                    thumb: false,
                },
                Node {
                    height: percent(100),
                    width: percent(0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.68, 0.50, 0.25)),
            ));
            rail.spawn((
                Part {
                    root: id,
                    thumb: true,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(0),
                    top: px(-7),
                    width: px(12),
                    height: px(18),
                    margin: UiRect::left(px(-6)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.91, 0.75, 0.46)),
            ));
        });
    });
    id
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SliderInput;

pub(crate) struct SliderPlugin;
impl Plugin for SliderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, input.in_set(SliderInput))
            .add_systems(PostUpdate, paint.before(bevy::ui::UiSystems::Layout));
    }
}

fn input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut sliders: Query<(
        Entity,
        &Interaction,
        &RelativeCursorPosition,
        &ComputedNode,
        &InheritedVisibility,
        &mut PercentSlider,
    )>,
    mut drag: Local<Option<(Entity, f32)>>,
) {
    if !windows.single().is_ok_and(|w| w.focused) {
        *drag = None;
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        *drag = None;
        for (entity, interaction, cursor, node, visible, slider) in &sliders {
            if *interaction != Interaction::Pressed || !visible.get() {
                continue;
            }
            if let Some(cursor) = cursor.normalized {
                // ComputedNode is in physical pixels; use its scale factor for
                // the six logical pixels reserved for either end of the thumb.
                let width = node.size().x * node.inverse_scale_factor();
                let x = cursor.x * width;
                *drag = Some((
                    entity,
                    grab_offset(x, 6., width - 6., slider.fraction(), 8.),
                ));
                break;
            }
        }
    }
    if let Some((entity, grab)) = *drag {
        if let Ok((_, _, cursor, node, visible, mut slider)) = sliders.get_mut(entity) {
            if !visible.get() {
                *drag = None;
                return;
            }
            if mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left) {
                if let Some(cursor) = cursor.normalized {
                    let width = node.size().x * node.inverse_scale_factor();
                    let value = value_at(
                        cursor.x * width - grab,
                        6.,
                        width - 6.,
                        slider.min,
                        slider.max,
                    );
                    if slider.value != value {
                        slider.value = value;
                        slider.edited = true;
                    }
                }
            }
        } else {
            *drag = None;
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        *drag = None;
    }
}

fn paint(sliders: Query<&PercentSlider>, mut parts: Query<(&Part, &mut Node)>) {
    for (part, mut node) in &mut parts {
        let Ok(slider) = sliders.get(part.root) else {
            continue;
        };
        let value = percent(slider.fraction() * 100.0);
        if part.thumb {
            if node.left != value {
                node.left = value;
            }
        } else if node.width != value {
            node.width = value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_slider_keeps_capture_through_drag_outside_and_cancels_on_hide() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        let slider = app
            .world_mut()
            .spawn((
                PercentSlider::new(45, 0, 100),
                Interaction::Pressed,
                RelativeCursorPosition {
                    normalized: Some(Vec2::splat(0.5)),
                    cursor_over: true,
                },
                ComputedNode {
                    size: Vec2::new(212., 26.),
                    ..default()
                },
                InheritedVisibility::VISIBLE,
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().get::<PercentSlider>(slider).unwrap().value, 50);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        *app.world_mut().get_mut::<Interaction>(slider).unwrap() = Interaction::None;
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(slider)
            .unwrap()
            .normalized = Some(Vec2::new(2., 2.));
        app.update();
        assert_eq!(app.world().get::<PercentSlider>(slider).unwrap().value, 100);
        *app.world_mut()
            .get_mut::<InheritedVisibility>(slider)
            .unwrap() = InheritedVisibility::HIDDEN;
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(slider)
            .unwrap()
            .normalized = Some(Vec2::ZERO);
        app.update();
        assert_eq!(app.world().get::<PercentSlider>(slider).unwrap().value, 100);
        *app.world_mut()
            .get_mut::<InheritedVisibility>(slider)
            .unwrap() = InheritedVisibility::VISIBLE;
        app.update();
        assert_eq!(app.world().get::<PercentSlider>(slider).unwrap().value, 100);
    }

    #[test]
    fn track_clicks_and_thumb_drags_clamp_without_wrapping_or_jumping() {
        assert_eq!(value_at(-200., 10., 210., 0, 100), 0);
        assert_eq!(value_at(500., 10., 210., 0, 100), 100);
        assert_eq!(value_at(84., 10., 210., 0, 100), 37);
        let grab = grab_offset(115., 10., 210., 0.5, 8.);
        assert_eq!(value_at(115. - grab, 10., 210., 0, 100), 50);
        assert_eq!(value_at(155. - grab, 10., 210., 0, 100), 70);
        assert_eq!(grab_offset(190., 10., 210., 0.5, 8.), 0.);
        assert_eq!(value_at(110., 10., 210., 40, 100), 70);
    }
}
