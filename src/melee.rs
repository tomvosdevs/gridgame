use bevy::{
    app::{App, Plugin},
    ecs::{
        component::Component,
        lifecycle::Add,
        message::MessageWriter,
        observer::On,
        query::With,
        system::{Commands, Query, Single},
    },
    math::Vec3,
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::prelude::InvokedBy;
use bevy_ecs::entity::Entity;
use bevy_ghx_grid::ghx_grid::cartesian::{coordinates::Cartesian3D, grid::CartesianGrid};

use crate::{
    NODE_SIZE,
    abilities::abilities_templates::ActionCastData,
    game_flow::turns::{PlayingEntity, ToWorldPos},
    grid_abilities_backend::{GridTarget, HitReceived},
};

pub struct MeleePlugin;

impl Plugin for MeleePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(init_melee);
    }
}

#[derive(Component)]
pub struct MeleeEffect;

pub fn init_melee(
    add: On<Add, GridTarget>,
    melee_q: Query<(&GridTarget, &Transform, &MeleeEffect)>,
    invoked_by_q: Query<&InvokedBy>,
    playing_q: Query<Entity, With<PlayingEntity>>,
    cast_data_q: Query<&ActionCastData>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    mut hit_writer: MessageWriter<HitReceived>,
) {
    let ability_entity = add.entity;
    let Ok((target, transform, effect)) = melee_q.get(ability_entity) else {
        return;
    };

    let invoker = invoked_by_q
        .get(ability_entity)
        .expect("No invoked by here mate")
        .0;

    let cast_data = cast_data_q
        .get(invoker)
        .expect("Invoker entity should always have ActionCastData");

    let target_world_pos = target.position.clone().as_world_pos(grid_tf.translation())
        - Vec3::new(0., NODE_SIZE.y, 0.);

    let Some(hit_player) = target.entity else {
        return;
    };

    println!("MELEE HIT SENT");
    hit_writer.write(HitReceived {
        hit_player,
        ability_entity,
        cast_data: cast_data.clone(),
    });
}
