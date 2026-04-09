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
            .add_systems(
                Update,
                (
                    move_projectiles,
                    test_log_projectile,
                    debug_propagate_system::<Grid3DBackend>,
                ),
            )
            .add_observer(|e: On<Add, Name>, q: Query<&Name>| {
                if let Ok(name) = q.get(e.entity) {
                    println!("New name spawned : {:?}", name);
                }
            })
            .add_systems(Update, debug_pipeline);
    }
}

pub fn debug_propagate_system<B: SpatialBackend>(
    mut reader: MessageReader<GoOffOrigin<B::Pos>>,
    mut ctx: B::Context<'_, '_>,
    q_sub_effects: Query<&SubEffects>,
    q_target_mutator: Query<Option<&TargetMutator<B>>>,
    q_invoker: Query<&InvokedBy>,
    q_child_of: Query<&ChildOf>,
    q_invoker_target: Query<&InvokerTarget<B::Pos>>,
    mut writer: MessageWriter<GoOff<B::Pos>>,
) {
    for origin in reader.read() {
        let root_entity = origin.entity;
        let passed_target = origin.target;

        let invoker = resolve_invoker(&q_invoker, root_entity);
        let root = resolve_root(&q_child_of, root_entity);
        let invoker_target: Target<B::Pos> = q_invoker_target
            .get(invoker)
            .copied()
            .map(Target::from)
            .unwrap_or_default();

        println!("got here == 1");

        // Resolve the root's own target list — apply its TargetMutator if
        // present, otherwise use the passed target verbatim. This matches
        // the behavior applied to children below.
        let root_targets: Vec<Target<B::Pos>> =
            if let Ok(Some(mutator)) = q_target_mutator.get(root_entity) {
                println!("got here == 2");
                let mut targets = generate_targets::<B>(
                    &mutator.generator,
                    &mut ctx,
                    invoker,
                    invoker_target,
                    root,
                    B::Pos::default(),
                    passed_target,
                );
                targets = B::apply_filter(
                    &mut ctx,
                    targets,
                    &mutator.generator.filter,
                    invoker,
                    passed_target.position,
                );
                targets
            } else {
                vec![passed_target]
            };

        // Fire GoOff on the root entity itself (one per resolved target).
        for &target in &root_targets {
            println!(
                "SENDING GO_OFF => target entity, position: {:?}, {:?}",
                target.entity, target.position
            );
            // writer.write(GoOff::new(root_entity, target));
        }

        // Walk the tree: (entity, targets_for_this_entity)
        let mut stack: Vec<(Entity, Vec<Target<B::Pos>>)> = vec![(root_entity, root_targets)];

        while let Some((parent, in_targets)) = stack.pop() {
            println!("got here == 3");
            let Ok(subs) = q_sub_effects.get(parent) else {
                continue;
            };

            for &child in subs.into_iter() {
                println!("got here == 4");
                let out_targets = if let Ok(Some(mutator)) = q_target_mutator.get(child) {
                    println!("got here == 5");
                    let mut aggregated = Vec::new();
                    for passed in in_targets.iter() {
                        println!("got here == 6");
                        let mut targets = generate_targets::<B>(
                            &mutator.generator,
                            &mut ctx,
                            invoker,
                            invoker_target,
                            root,
                            B::Pos::default(),
                            *passed,
                        );
                        let origin_pos = passed.position;
                        targets = B::apply_filter(
                            &mut ctx,
                            targets,
                            &mutator.generator.filter,
                            invoker,
                            origin_pos,
                        );
                        aggregated.append(&mut targets);
                    }
                    aggregated
                } else {
                    in_targets.clone()
                };

                // Write one GoOff per target (batch messages instead of Vec)
                for &target in &out_targets {
                    println!("GO_OFF bis => {:?}", target.position);
                    // writer.write(GoOff::new(child, target));
                }
                stack.push((child, out_targets));
            }
        }
    }
}

fn debug_pipeline(
    mut origin_reader: MessageReader<GoOffOrigin<CartesianPosition>>,
    mut go_off_reader: MessageReader<GridGoOff>,
    mut start_invoke_reader: MessageReader<GridStartInvoke>,
    mut m_start_invoke_reader: MessageReader<Matched<GridStartInvoke>>,
    q_entered: Query<(Entity, &Active), Added<Active>>,
    q_names: Query<&Name>,
    // q_transitions: Query<&Transitions>,
    // q_source: Query<&Source>,
    // q_target: Query<&Target>,
    q_children: Query<&Children>,
    mut cmd: Commands,
) {
    for i in start_invoke_reader.read() {
        println!("received START INVOKE : {:?} -> {:?}", i.entity, i.target)
    }
    for i in m_start_invoke_reader.read() {
        println!("received M_START INVOKE : {:?} -> {:?}", i.source, i.target)
    }

    // for ts in &q_transitions {
    //     for t in ts {
    //         if let Ok(src) = q_source.get(*t) {
    //             if let Ok(tgt) = q_source.get(*t) {
    //                 println!(
    //                     "this is a transition from : {:?} to -> {:?}",
    //                     q_names.get(src.0).unwrap(),
    //                     q_names.get(tgt.0).unwrap()
    //                 );
    //             }
    //         }
    //     }
    // }
    for (e, a) in &q_entered {
        println!(
            "entered state {:?}",
            q_names.get(e).unwrap_or(&Name::new("No Entity Name"))
        );

        cmd.entity(e).log_components();

        // if let Ok(ts) = q_transitions.get(e) {
        //     println!("transitions : {:?}", ts);
        //     for t in ts {
        //         if let Ok(tn) = q_names.get(*t) {
        //             println!("found transitions = {:?}", tn);
        //         }
        //     }
        // }

        if let Ok(ch) = q_children.get(e) {
            for c in ch {
                if let Ok(cn) = q_names.get(*c) {
                    println!("found child = {:?}", cn);
                }
            }
        }
    }
    for msg in origin_reader.read() {
        println!(
            "GoOffOrigin on {:?} with target {:?}",
            msg.entity, msg.target
        );
    }
    for msg in go_off_reader.read() {
        println!("GoOff on {:?} with target {:?}", msg.entity, msg.target);
    }
}

fn test_log_projectile(
    q_projectile: Query<(&ProjectileEffect, Option<&GridTarget>, Option<&Transform>)>,
    q_grid_target: Query<(Entity, &GridTarget)>,
    mut cmd: Commands,
) {
    for (et, gt) in &q_grid_target {
        println!("There is a target with -->");
        cmd.entity(et).log_components();
    }

    for (proj, gt, tf) in q_projectile {
        println!("--> found projectile effect");
        if let Some(gt) = gt {
            println!("--> found Grid target");
        }
        if let Some(tf) = tf {
            println!("--> found transform");
        }
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

    let target_world_pos = target.position.as_world_pos(grid_tf.translation());
    let dir = (target_world_pos - transform.translation).normalize_or_zero();
    let dir = if dir == Vec3::ZERO { Vec3::NEG_Y } else { dir };

    commands
        .entity(entity)
        .insert(MovingProjectile::new(dir, effect.speed));
}

pub fn move_projectiles(
    mut projectiles_q: Query<(&MovingProjectile, &mut Transform)>,
    time: Res<Time>,
) {
    for (projectile, mut tf) in &mut projectiles_q {
        println!("--> moving projectile");
        tf.translation += projectile.dir * projectile.speed * time.delta_secs();
    }
}
