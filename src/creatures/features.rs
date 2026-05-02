use std::collections::HashMap;

use bevy::ecs::{
    component::Component, entity::Entity, resource::Resource, system::Query, world::World,
};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::CartesianPosition;

use crate::{creatures::definitions::CreatureKind, deck::card_builders::CardPool};

pub struct MoveCtx {
    pub moving_entity: Entity,
    pub from: CartesianPosition,
    pub to: CartesianPosition,
}

pub trait OnMoveHook: Send + Sync + 'static {
    fn priority(&self) -> i32 {
        0
    } // determines chain order
    fn run(&self, ctx: &mut MoveCtx, world: &World);
}

pub trait CreatureFeature: Component {}

#[derive(Component)]
pub struct Flying;

impl CreatureFeature for Flying {}
