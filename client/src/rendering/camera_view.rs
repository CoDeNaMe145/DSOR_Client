//! Lens settings shared by fixed game cameras and the editor's camera monitor.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Lens { pub fov: f32, pub aspect: f32, pub near: f32, pub far: f32 }

impl Lens {
    pub fn projection(&self) -> Projection {
        Projection::Perspective(PerspectiveProjection {
            fov: self.fov.clamp(1., 170.).to_radians(), aspect_ratio: self.aspect.clamp(0.25, 4.),
            near: self.near.max(0.001), far: self.far.max(self.near + 0.1), ..default()
        })
    }
}
