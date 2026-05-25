use bevy::{
    app::{App, Plugin, Update},
    ecs::{
        component::Component,
        entity::Entity,
        lifecycle::Add,
        message::MessageWriter,
        observer::On,
        query::With,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query, Res, Single},
    },
    math::{Vec3, VectorSpace},
    reflect::Reflect,
    time::Time,
    transform::components::{GlobalTransform, Transform},
};

use bevy_diesel::prelude::InvokedBy;
use bevy_ecs::{hierarchy::ChildOf, name::Name};
use bevy_gearbox::GearboxSet;
use bevy_ghx_grid::ghx_grid::cartesian::{coordinates::Cartesian3D, grid::CartesianGrid};

use crate::{
    NODE_SIZE,
    abilities::abilities_templates::{ActionCastData, Marker, Projectile},
    deck::card_blueprints::SubAbilityOf,
    game_flow::turns::{PlayingEntity, ToWorldPos},
    grid_abilities_backend::{GridTarget, HitReceived},
};

pub enum ProjectilePath {}

#[derive(Component, Clone, Debug, Reflect)]
pub struct ProjectileEffect {
    pub speed: f32,
}

impl Default for ProjectileEffect {
    fn default() -> Self {
        Self { speed: 20.0 }
    }
}

impl ProjectileEffect {
    pub fn new(speed: f32) -> Self {
        Self { speed }
    }
}

#[derive(Component, Clone, Debug, Reflect)]
pub struct MovingProjectile {
    pub dir: Vec3,
    pub target_pos: Vec3,
    pub target_entity: Option<Entity>,
    pub speed: f32,
}

impl MovingProjectile {
    pub fn new(dir: Vec3, target_pos: Vec3, target_entity: Option<Entity>, speed: f32) -> Self {
        Self {
            dir,
            target_pos,
            target_entity,
            speed,
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(init_projectile)
            .add_systems(Update, handle_projectiles.before(GearboxSet));
    }
}

fn init_projectile(
    add: On<Add, GridTarget>,
    projectile_q: Query<(&GridTarget, &Transform, &ProjectileEffect), With<Marker<Projectile>>>,
    invoked_by_q: Query<&InvokedBy>,
    cast_data_q: Query<&ActionCastData>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    names_q: Query<&Name>,
    playing_q: Query<&PlayingEntity>,
    mut cmd: Commands,
) {
    let entity = add.entity;
    let Ok((target, transform, effect)) = projectile_q.get(entity) else {
        return;
    };
    println!(
        "projectile entity: {:?}",
        names_q.get(entity).unwrap_or(&Name::new("some entity"))
    );

    println!("DDD = projectile invoked by :");
    let root_invoker = invoked_by_q
        .get(entity)
        .expect("Could not find invoked by on ability")
        .0;

    let cast_data = cast_data_q
        .get(root_invoker)
        .expect("action cast data should be on root invoker");

    cmd.entity(entity)
        .insert(SubAbilityOf(cast_data.source_caster_entity));

    let offset = match target.entity {
        Some(e) => match playing_q.get(e) {
            Ok(_) => Vec3::ZERO.with_y(1.3),
            Err(_) => Vec3::ZERO,
        },
        None => Vec3::ZERO,
    };

    let target_world_pos = target.position.clone().as_world_pos(grid_tf.translation())
        - Vec3::new(0., NODE_SIZE.y, 0.)
        + offset;
    println!(
        "pos of target was and is : {:?} -> {:?}",
        target.position, target_world_pos
    );
    let dir = (target_world_pos - transform.translation).normalize_or_zero();

    let dir = if dir == Vec3::ZERO { Vec3::NEG_Y } else { dir };

    println!("sending projectile to dir : {:?}", dir);

    cmd.entity(entity).insert(MovingProjectile::new(
        dir,
        target_world_pos,
        target.entity,
        effect.speed,
    ));
}

pub fn handle_projectiles(
    mut projectiles_q: Query<(Entity, &MovingProjectile, &mut Transform)>,
    invoked_by_q: Query<&InvokedBy>,
    cast_data_q: Query<&ActionCastData>,
    time: Res<Time>,
    mut hit_writer: MessageWriter<HitReceived>,
    mut cmd: Commands,
) {
    for (projectile_entity, projectile, mut tf) in &mut projectiles_q {
        tf.translation += projectile.dir * projectile.speed * time.delta_secs();
        if tf.translation.distance(projectile.target_pos) < 0.1 {
            if let Some(hit_player) = projectile.target_entity {
                let invoker = invoked_by_q
                    .get(projectile_entity)
                    .expect("No invoked by here mate")
                    .0;
                let cast_data = cast_data_q
                    .get(invoker)
                    .expect("Invoker entity should always have ActionCastData");

                // TODO : see how to remove attacking player her or how to get it properly, maybe by adding a AbilityOfPlayer() component on the invoker/caster ?

                println!("here we have :");
                cmd.entity(projectile_entity).log_components();

                hit_writer.write(HitReceived {
                    hit_player,
                    ability_entity: projectile_entity,
                    cast_data: cast_data.clone(),
                });
            }
        }
    }
}
