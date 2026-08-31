//! Peek up/down animation for the head-only Live2D display.
//!
//! The occlusion system in the binary only flips [`HeadDisplayWanted`]; this
//! module turns edges into tweens on the display's [`UiTransform`] so the head
//! pops UP from behind the chat capsule with an elastic (bouncy) overshoot and
//! sinks back DOWN behind it when the pet becomes visible again.
//!
//! UI y+ is downward, so the peek offset is a positive px value: the head
//! rests at offset 0 and starts/ends at +PEEK_DISTANCE (lower, tucked into
//! the capsule).

use std::time::Duration;

use bevy::prelude::*;
use bevy_tweening::{AnimCompletedEvent, Lens, Tween, TweenAnim};

use crate::renderer::HeadDisplay;

const PEEK_DISTANCE: f32 = 60.0;
const PEEK_UP_SECS: f32 = 0.9;
const PEEK_DOWN_SECS: f32 = 0.25;

#[derive(Resource, Default)]
pub struct HeadDisplayWanted(pub bool);

#[derive(Component)]
pub(crate) struct HeadSlidingOut;

struct HeadPeekLens {
    start: f32,
    end: f32,
}

impl Lens<UiTransform> for HeadPeekLens {
    fn lerp(&mut self, mut target: Mut<UiTransform>, ratio: f32) {
        let y = self.start + (self.end - self.start) * ratio;
        target.translation = Val2 {
            x: Val::Px(0.0),
            y: Val::Px(y),
        };
    }
}

pub(crate) fn head_display_anim(
    mut commands: Commands,
    wanted: Option<Res<HeadDisplayWanted>>,
    head: Query<(Entity, &UiTransform, &Visibility), With<HeadDisplay>>,
) {
    let Some(wanted) = wanted else { return };
    if !wanted.is_changed() {
        return;
    }
    let Ok((entity, ui_tf, vis)) = head.single() else {
        return;
    };
    let cur_y = match ui_tf.translation.y {
        Val::Px(v) => v,
        _ => 0.0,
    };
    if wanted.0 {
        let start = if *vis == Visibility::Hidden { PEEK_DISTANCE } else { cur_y };
        let tween = Tween::new(
            EaseFunction::ElasticOut,
            Duration::from_secs_f32(PEEK_UP_SECS),
            HeadPeekLens { start, end: 0.0 },
        );
        commands
            .entity(entity)
            .insert((Visibility::Visible, TweenAnim::new(tween)))
            .remove::<HeadSlidingOut>();
    } else {
        if *vis == Visibility::Hidden {
            return;
        }
        let tween = Tween::new(
            EaseFunction::QuadraticIn,
            Duration::from_secs_f32(PEEK_DOWN_SECS),
            HeadPeekLens {
                start: cur_y,
                end: PEEK_DISTANCE,
            },
        );
        commands
            .entity(entity)
            .insert(TweenAnim::new(tween))
            .insert(HeadSlidingOut);
    }
}

pub(crate) fn head_slide_out_finished(
    mut completed: MessageReader<AnimCompletedEvent>,
    sliding_out: Query<(), With<HeadSlidingOut>>,
    mut head: Query<&mut Visibility, With<HeadDisplay>>,
) {
    for ev in completed.read() {
        if sliding_out.contains(ev.anim_entity) {
            if let Ok(mut vis) = head.get_mut(ev.anim_entity) {
                *vis = Visibility::Hidden;
            }
        }
    }
}
