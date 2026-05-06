use bevy::math::UVec3;
use bevy_ecs::system::Single;
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};

pub trait AsUVec3 {
    fn as_uvec3(&self) -> UVec3;
}

pub trait AsFlippedUVec3 {
    fn as_flipped_uvec3(&self) -> UVec3;
}

pub trait AsFlippedCPos {
    fn as_flipped_cartesian_pos(&self) -> CartesianPosition;
}

impl AsFlippedCPos for UVec3 {
    fn as_flipped_cartesian_pos(&self) -> CartesianPosition {
        CartesianPosition {
            x: self.x,
            y: self.z,
            z: self.y,
        }
    }
}

impl AsUVec3 for CartesianPosition {
    fn as_uvec3(&self) -> UVec3 {
        UVec3 {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

impl AsFlippedUVec3 for CartesianPosition {
    fn as_flipped_uvec3(&self) -> UVec3 {
        UVec3 {
            x: self.x,
            y: self.z,
            z: self.y,
        }
    }
}

pub type CombatGridQ<'w, 's> = Single<'w, 's, &'static CartesianGrid<Cartesian3D>>;
