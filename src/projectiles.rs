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

use bevy_diesel::{invoke::Ability, prelude::InvokedBy};
use bevy_ecs::{hierarchy::ChildOf, message::MessageReader, name::Name};
use bevy_gearbox::GearboxSet;
use bevy_ghx_grid::ghx_grid::cartesian::{coordinates::Cartesian3D, grid::CartesianGrid};

use crate::{
    NODE_SIZE,
    abilities::abilities_templates::{
        ActionCastData, AttachedToPlayer, HasRootInvoker, Marker, Projectile,
    },
    game_flow::turns::{PlayingEntity, ToWorldPos},
    grid_abilities_backend::{GridGoOff, GridTarget, HitReceived},
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
        app.add_systems(Update, handle_projectiles.before(GearboxSet));
    }
}

pub fn init_projectile(
    mut reader: MessageReader<GridGoOff>,
    projectile_q: Query<(&InvokedBy, &ProjectileEffect), With<Marker<Projectile>>>,
    invoked_by_q: Query<&InvokedBy>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    tf_q: Query<&Transform>,
    names_q: Query<&Name>,
    playing_q: Query<&PlayingEntity>,
    attached_to_player_q: Query<&AttachedToPlayer>,
    mut cmd: Commands,
) {
    for go_off in reader.read() {
        let projectile_entity = go_off.entity;
        let target = go_off.target;

        let Ok((invoked_by, effect)) = projectile_q.get(projectile_entity) else {
            return;
        };

        let invoker = invoked_by.0;
        println!(
            "projectile entity: {:?}",
            names_q
                .get(projectile_entity)
                .unwrap_or(&Name::new("some entity"))
        );

        cmd.entity(invoker).log_components();

        println!("DDD = projectile invoked by :");
        let maybe_invoker_invoker = invoked_by_q.get(invoker);

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

        let attacking_player = invoker;

        let projectile_tf = tf_q
            .get(attacking_player)
            .ok()
            .map(|v| v)
            .or_else(|| tf_q.get(maybe_invoker_invoker.unwrap().0).ok().map(|v| v))
            .expect("should find tf on invoker or invoker's invoker")
            .clone();
        let dir = (target_world_pos - projectile_tf.translation).normalize_or_zero();

        let dir = if dir == Vec3::ZERO { Vec3::NEG_Y } else { dir };

        println!("sending projectile to dir : {:?}", dir);

        cmd.entity(projectile_entity).insert((
            projectile_tf,
            MovingProjectile::new(dir, target_world_pos, target.entity, effect.speed),
        ));
    }
}

pub fn handle_projectiles(
    mut projectiles_q: Query<(Entity, &MovingProjectile, &mut Transform)>,
    invoked_by_q: Query<&InvokedBy>,
    root_invoker_q: Query<&HasRootInvoker>,
    cast_data_q: Query<&ActionCastData>,
    time: Res<Time>,
    mut hit_writer: MessageWriter<HitReceived>,
    mut cmd: Commands,
) {
    for (projectile_entity, projectile, mut tf) in &mut projectiles_q {
        tf.translation += projectile.dir * projectile.speed * time.delta_secs();
        if tf.translation.distance(projectile.target_pos) < 0.2 {
            if let Some(hit_player) = projectile.target_entity {
                let invoker = invoked_by_q
                    .get(projectile_entity)
                    .expect("No invoked by here mate")
                    .0;

                // TODO : see how to remove attacking player her or how to get it properly, maybe by adding a AbilityOfPlayer() component on the invoker/caster ?
                cmd.entity(projectile_entity).remove::<MovingProjectile>();

                let cast_data = ActionCastData::new(invoker, invoker);

                hit_writer.write(HitReceived {
                    hit_player,
                    ability_entity: projectile_entity,
                    cast_data: cast_data.clone(),
                });
            }
        }
    }
}
