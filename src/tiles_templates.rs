use std::marker::PhantomData;

use bevy::ecs::component::Component;

use crate::{GridCell, effects::Humid};

#[derive(Component)]
#[require(GridCell)]
pub struct BaseGrassCell;

#[derive(Component)]
#[require(GridCell)]
pub struct BaseRockCell;

#[derive(Component)]
#[require(GridCell, Humid)]
pub struct BaseWaterCell;

// Tags

#[derive(Component)]
pub struct Brittle;

#[derive(Component)]
pub struct InvulnerableTo<T>
where
    T: Component,
{
    _status: PhantomData<T>,
}

pub struct GricCellBuilder {}
