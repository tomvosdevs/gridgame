use bevy::{
    app::{App, Plugin, Update},
    ecs::{
        component::Component,
        entity::Entity,
        hierarchy::{ChildOf, Children},
        lifecycle::Add,
        message::{MessageReader, MessageWriter},
        name::Name,
        observer::On,
        query::{Added, With},
        schedule::{IntoScheduleConfigs, common_conditions::run_once},
        system::{Commands, Query, Res, Single},
    },
    math::Vec3,
    reflect::Reflect,
    time::Time,
    transform::components::{GlobalTransform, Transform},
};

use bevy_diesel::{
    effect::{GoOff, GoOffOrigin, SubEffects},
    prelude::{InvokedBy, SpatialBackend, generate_targets, resolve_invoker, resolve_root},
    target::{InvokerTarget, Target, TargetMutator},
};
use bevy_gearbox::{Active, GearboxMessage, Matched, Source, Transitions};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};

use crate::{
    NODE_SIZE,
    grid_abilities_backend::{Grid3DBackend, GridGoOff, GridStartInvoke, GridTarget},
    states::{FromGrid, ToWorldPos},
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
    pub speed: f32,
}

impl MovingProjectile {
    pub fn new(dir: Vec3, speed: f32) -> Self {
        Self { dir, speed }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct ProjectilePlugin;

impl Plugin for ProjectilePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(init_projectile)
            .add_systems(Update, move_projectiles);
    }
}

fn init_projectile(
    add: On<Add, GridTarget>,
    q_projectile: Query<(&GridTarget, &Transform, &ProjectileEffect)>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    mut commands: Commands,
) {
    println!("--> init projectile");
    let entity = add.entity;
    let Ok((target, transform, effect)) = q_projectile.get(entity) else {
        return;
    };

    let target_world_pos =
        target.position.as_world_pos(grid_tf.translation()) - Vec3::new(0., NODE_SIZE.y, 0.);
    println!(
        "pos of target was and is : {:?} -> {:?}",
        target.position, target_world_pos
    );
    let dir = (target_world_pos - transform.translation).normalize_or_zero();

    let dir = if dir == Vec3::ZERO { Vec3::NEG_Y } else { dir };

    println!("sending projectile to dir : {:?}", dir);

    commands
        .entity(entity)
        .insert(MovingProjectile::new(dir, effect.speed));
}

pub fn move_projectiles(
    mut projectiles_q: Query<(&MovingProjectile, &mut Transform)>,
    time: Res<Time>,
) {
    for (projectile, mut tf) in &mut projectiles_q {
        tf.translation += projectile.dir * projectile.speed * time.delta_secs();
    }
}
