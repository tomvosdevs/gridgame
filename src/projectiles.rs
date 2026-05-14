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

use bevy_ecs::name::Name;
use bevy_gearbox::GearboxSet;
use bevy_ghx_grid::ghx_grid::cartesian::{coordinates::Cartesian3D, grid::CartesianGrid};

use crate::{
    NODE_SIZE,
    abilities::abilities_templates::{Marker, Projectile},
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
    q_projectile: Query<(&GridTarget, &Transform, &ProjectileEffect), With<Marker<Projectile>>>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    names_q: Query<&Name>,
    playing_q: Query<&PlayingEntity>,
    mut commands: Commands,
) {
    let entity = add.entity;
    let Ok((target, transform, effect)) = q_projectile.get(entity) else {
        return;
    };
    println!(
        "projectile entity: {:?}",
        names_q.get(entity).unwrap_or(&Name::new("some entity"))
    );
    println!("--> init projectile");

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

    commands.entity(entity).insert(MovingProjectile::new(
        dir,
        target_world_pos,
        target.entity,
        effect.speed,
    ));
}

pub fn handle_projectiles(
    mut projectiles_q: Query<(Entity, &MovingProjectile, &mut Transform)>,
    time: Res<Time>,
    mut hit_writer: MessageWriter<HitReceived>,
    mut cmd: Commands,
) {
    for (projectile_entity, projectile, mut tf) in &mut projectiles_q {
        tf.translation += projectile.dir * projectile.speed * time.delta_secs();
        if tf.translation.distance(projectile.target_pos) < 0.1 {
            if let Some(entity) = projectile.target_entity {
                hit_writer.write(HitReceived {
                    entity: entity,
                    hit_by: projectile_entity,
                });
            }
        }
    }
}
