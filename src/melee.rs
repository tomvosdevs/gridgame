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
use bevy_ghx_grid::ghx_grid::cartesian::{coordinates::Cartesian3D, grid::CartesianGrid};

use crate::{
    NODE_SIZE,
    game_flow::turns::ToWorldPos,
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
    q_melee: Query<(&GridTarget, &Transform, &MeleeEffect)>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    mut hit_writer: MessageWriter<HitReceived>,
    mut commands: Commands,
) {
    println!("in init melee");
    let entity = add.entity;
    let Ok((target, transform, effect)) = q_melee.get(entity) else {
        return;
    };

    let target_world_pos = target.position.clone().as_world_pos(grid_tf.translation())
        - Vec3::new(0., NODE_SIZE.y, 0.);

    let Some(target_entity) = target.entity else {
        return;
    };

    println!("MELEE HIT SENT");
    hit_writer.write(HitReceived {
        entity: target_entity,
        hit_by: entity,
    });
}
