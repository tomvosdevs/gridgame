use std::{collections::HashMap, time::Duration};

use bevy::{
    app::{App, Plugin, Startup, Update},
    asset::Assets,
    camera::visibility::Visibility,
    color::{
        Alpha,
        palettes::tailwind::{
            GREEN_300, GREEN_950, RED_100, RED_400, RED_600, RED_900, YELLOW_400,
        },
    },
    gizmos::gizmos::Gizmos,
    log::warn,
    math::{UVec3, Vec3, VectorSpace, primitives::Cuboid},
    mesh::{Mesh, Mesh3d},
    pbr::{MeshMaterial3d, StandardMaterial},
    picking::{
        Pickable,
        events::{Click, Over, Pointer},
    },
    reflect::Struct,
    time::Time,
    transform::components::{GlobalTransform, Transform},
};
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    event::EntityEvent,
    lifecycle::Add,
    observer::On,
    query::{With, Without},
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Commands, Query, Res, ResMut, Single},
};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_northstar::{
    CardinalIsoGrid, SearchLimits,
    debug::NorthstarDebugPlugin,
    filter,
    grid::GridSettingsBuilder,
    nav::Nav,
    path::Path,
    plugin::NorthstarPlugin,
    prelude::{
        AgentPos, CardinalIsoNeighborhood, DebugGridBuilder, DebugOffset, NextPos, Pathfind,
        PathfindArgs, PathfindMode, PathfindingFailed,
    },
};
use bevy_tween::{
    bevy_time_runner::{TimeRunner, TimeSpan},
    combinator::{sequence, tween},
    prelude::{AnimationBuilderExt, EaseKind, TransformTargetStateExt},
    tween::{IntoTarget, TweenInterpolationValue},
};
use bevy_tweening::Tween;
use pyri_state::{pattern::StatePattern, prelude::StateFlush};

use crate::{
    GridCell, NODE_SIZE,
    game_flow::turns::{CurrentPlayingEntity, FromGrid, GameState, PlayingEntity, Speed},
    ui::TweenAnimator,
    utils::{AsFlippedCPos, AsFlippedUVec3, AsUVec3, CombatGridQ},
};

pub struct GridMovementPlugin;

impl Plugin for GridMovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(NorthstarPlugin::<CardinalIsoNeighborhood>::default())
            .add_observer(setup_grid_settings)
            .add_observer(handle_move_request)
            .add_observer(handle_cell_hover)
            .add_observer(handle_cell_click)
            .add_observer(handle_failed_pathfinding)
            .add_observer(preview_move)
            .add_observer(create_next_world_pos)
            .add_systems(Startup, register_preview_entities)
            .add_systems(
                StateFlush,
                GameState::LoadingGrid.on_exit(build_initial_grid),
            )
            .add_systems(Update, (draw_debug_nav_cells, handle_agent_pathing))
            .add_systems(Update, animate_agent_movement.after(handle_agent_pathing));
    }
}

fn handle_cell_hover(
    e: On<Pointer<Over>>,
    q: Query<Entity, (With<GridCell>, With<GridNode>)>,
    preview_cells_q: Query<Entity, With<PathTilePreview>>,
    mut cmd: Commands,
    playing_entity: Option<Res<CurrentPlayingEntity>>,
) {
    let Some(playing_entity) = playing_entity else {
        return;
    };
    let Ok(node_entity) = q.get(e.entity) else {
        return;
    };

    for cell in &preview_cells_q {
        cmd.entity(cell).despawn();
    }

    cmd.trigger(PreviewPathRequest::new(playing_entity.0, node_entity));
}

fn handle_cell_click(
    e: On<Pointer<Click>>,
    q: Query<Entity, (With<GridCell>, With<GridNode>)>,
    mut cmd: Commands,
    playing_entity: Res<CurrentPlayingEntity>,
) {
    let Ok(node_entity) = q.get(e.entity) else {
        return;
    };

    println!("Ptr down on cell");

    cmd.trigger(MoveRequest::new(playing_entity.0, node_entity));
}

#[derive(EntityEvent)]
pub struct MoveRequest {
    pub entity: Entity,
    pub destination_cell: Entity,
}

#[derive(EntityEvent)]
pub struct PreviewPathRequest {
    pub entity: Entity,
    pub destination_cell: Entity,
}

impl PreviewPathRequest {
    pub fn new(entity: Entity, destination_cell: Entity) -> Self {
        Self {
            entity,
            destination_cell,
        }
    }
}

impl MoveRequest {
    pub fn new(entity: Entity, destination_cell: Entity) -> Self {
        Self {
            entity,
            destination_cell,
        }
    }
}

#[derive(EntityEvent)]
pub struct MovementCompleted {
    pub entity: Entity,
}

fn setup_grid_settings(
    e: On<Add, CartesianGrid<Cartesian3D>>,
    q: Query<&CartesianGrid<Cartesian3D>>,
    mut commands: Commands,
) {
    let grid = q.get(e.entity).unwrap();

    // Configure the grid
    let grid_settings =
        GridSettingsBuilder::new_3d(grid.size_x(), (grid.size_z() + 2), grid.size_y())
            .enable_collision()
            .chunk_size(16)
            .default_impassable()
            .build();

    // Spawn the grid used for pathfinding.
    commands.spawn(CardinalIsoGrid::new(&grid_settings));
}

#[derive(Component)]
struct DebugNavCell(UVec3);

fn build_initial_grid(
    mut nav_grid_q: Query<&mut CardinalIsoGrid>,
    grid_q: Query<&CartesianGrid<Cartesian3D>>,
    cells_q: Query<(&GridCell, &GridNode)>,
    mut cmd: Commands,
) {
    let grid = grid_q.single().expect("expected a single grid data");
    let mut nav_grid = nav_grid_q
        .single_mut()
        .expect("Expected a single cardinal grid");

    let positions: HashMap<usize, CartesianPosition> = cells_q
        .iter()
        .map(|(_, n)| (n.0, grid.pos_from_index(n.0)))
        .collect();

    let walkable_cells: Vec<(&GridCell, UVec3)> = cells_q
        .iter()
        .filter_map(|(c, n)| {
            let cell_pos = positions.get(&n.0).unwrap();
            if (cell_pos.y
                >= positions
                    .iter()
                    .filter(|(_, p)| p.x == cell_pos.x && p.z == cell_pos.z)
                    .map(|(_, p)| p.y)
                    .max()
                    .unwrap())
            {
                return Some((c, cell_pos.as_flipped_uvec3()));
            }
            None
        })
        .collect();

    for (cell, pos) in walkable_cells {
        nav_grid.set_nav(pos, Nav::Passable(0));
    }

    nav_grid.build();
}

fn draw_debug_nav_cells(
    q: Query<&DebugNavCell>,
    mut gizmos: Gizmos,
    grid_q: Query<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
) {
    let grid_tf = grid_q.single().expect("Only one grid should exist");
    for nav_cell in &q {
        gizmos.sphere(grid_tf.translation() + nav_cell.0.as_vec3(), 1.0, RED_600);
    }
}

#[derive(Component)]
struct NextWorldPos(Vec3);

fn handle_move_request(
    e: On<MoveRequest>,
    mut cmd: Commands,
    agents_q: Query<(Entity, &AgentPos), With<PlayingEntity>>,
    cells_q: Query<(&GridNode, &GlobalTransform), With<GridCell>>,
    grid: CombatGridQ,
    preview_tiles_q: Query<Entity, With<PathTilePreview>>,
) {
    let Ok((agent_entity, agent_pos)) = agents_q.get(e.entity) else {
        return;
    };

    let Ok((node, node_tf)) = cells_q.get(e.destination_cell) else {
        warn!("Move req cancelled, target is not a GridCell");
        return;
    };

    for preview_tile in &preview_tiles_q {
        cmd.entity(preview_tile).despawn();
    }

    let target_position = grid.pos_from_index(node.0).as_flipped_uvec3();
    println!(
        "trying to pf from {:?} -> to {:?}",
        agent_pos.0, target_position
    );

    let mut e_cmds = cmd.entity(agent_entity);

    e_cmds.insert(Pathfind {
        goal: target_position,
        mode: Some(PathfindMode::AStar),
        limits: SearchLimits::default(),
    });
}

fn handle_failed_pathfinding(e: On<Add, PathfindingFailed>, mut cmd: Commands) {
    let mut e_cmds = cmd.entity(e.entity);
    e_cmds.remove::<PathfindingFailed>();
    e_cmds.remove::<Pathfind>();
}

#[derive(Resource)]
struct PreviewEntities {
    valid_tile: Entity,
    invalid_tile: Entity,
    slow_tile: Entity,
}

#[derive(Component)]
struct PathTilePreview;

fn register_preview_entities(
    mut cmd: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mesh = meshes.add(Cuboid::new(0.95, 0.2, 0.95));
    let mat = materials.add(StandardMaterial::from_color(GREEN_300.with_alpha(0.3)));
    let mat_invalid = materials.add(StandardMaterial::from_color(RED_400.with_alpha(0.3)));
    let mat_slow = materials.add(StandardMaterial::from_color(YELLOW_400.with_alpha(0.3)));

    let tile_preview_entity = cmd
        .spawn((
            Mesh3d::from(mesh.clone()),
            MeshMaterial3d::from(mat),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();

    let tile_preview_invalid_entity = cmd
        .spawn((
            Mesh3d::from(mesh.clone()),
            MeshMaterial3d::from(mat_invalid),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();

    let tile_preview_slow_entity = cmd
        .spawn((
            Mesh3d::from(mesh),
            MeshMaterial3d::from(mat_slow),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();

    cmd.insert_resource(PreviewEntities {
        valid_tile: tile_preview_entity,
        invalid_tile: tile_preview_invalid_entity,
        slow_tile: tile_preview_slow_entity,
    });
}

fn preview_move(
    e: On<PreviewPathRequest>,
    mut cmd: Commands,
    q: Query<(&Speed, &AgentPos), (With<PlayingEntity>, Without<Path>)>,
    grid: Single<(&GlobalTransform, &CartesianGrid<Cartesian3D>), With<CartesianGrid<Cartesian3D>>>,
    preview_entities: Res<PreviewEntities>,
    previous_preview_tiles_q: Query<Entity, With<PathTilePreview>>,
    grid_nodes_q: Query<&GridNode, With<GridCell>>,
    nav_grid: Single<&CardinalIsoGrid>,
) {
    let Ok((speed, agent_pos)) = q.get(e.entity) else {
        return;
    };

    let (grid_tf, grid) = grid.into_inner();
    let nav_grid = nav_grid.into_inner();
    let target_cell = grid_nodes_q
        .get(e.destination_cell)
        .expect("Cell entity supplied to PreviewMoveRequest does not exist");

    let target_pos = grid.pos_from_index(target_cell.0).as_flipped_uvec3();

    for preview_tile in &previous_preview_tiles_q {
        cmd.entity(preview_tile).despawn();
    }

    let grid_offset = grid_tf.translation();

    let Some(mut path) = nav_grid.pathfind(&mut PathfindArgs::new(agent_pos.0, target_pos).astar())
    else {
        return;
    };

    for (i, step) in path.as_mut_slices().iter().enumerate() {
        let step_grid_pos = step.as_vec3();
        let step_world_pos = Vec3::new(step_grid_pos.x, step_grid_pos.z, step_grid_pos.y);
        let step_world_pos = grid_offset
            + step_world_pos
            + Vec3::new((NODE_SIZE.x / 2.0), 1.05, (NODE_SIZE.z / 2.0));
        let tile_instance = match (i as i32) >= speed.current {
            true => cmd
                .entity(preview_entities.invalid_tile)
                .clone_and_spawn()
                .id(),
            false => cmd
                .entity(preview_entities.valid_tile)
                .clone_and_spawn()
                .id(),
        };
        cmd.entity(tile_instance).insert((
            Visibility::Visible,
            PathTilePreview,
            Transform::from_translation(step_world_pos),
        ));
    }
}

#[derive(Component)]
pub enum ActiveAgentAnimation {
    Straight {
        direction: Vec3,
        target_pos: Vec3,
    },
    Jump {
        start_pos: Vec3,
        target_pos: Vec3,
        y_diff: f32,
    },
}

#[derive(Component)]
struct JumpAnimStep(i32);

fn create_next_world_pos(
    e: On<Add, NextPos>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &GlobalTransform,
            &mut AgentPos,
            &NextPos,
        ),
        (With<PlayingEntity>, Without<ActiveAgentAnimation>),
    >,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    mut cmd: Commands,
) {
    let (entity, tf, gtf, agent_pos, next_pos) = q.get(e.entity).expect("should have found");
    let flipped_pos = UVec3::new(next_pos.0.x, next_pos.0.z, next_pos.0.y);
    let world_pos = grid_tf.into_inner().translation() + flipped_pos.as_vec3();
    let world_pos = world_pos + (NODE_SIZE / 2.0).with_y(2.0);
    println!("generated next world pos : {:?}", world_pos);
    cmd.entity(entity).insert(NextWorldPos(world_pos));
}

fn handle_agent_pathing(
    q: Query<
        (Entity, &Transform, &NextWorldPos, &Path, &Speed),
        (
            With<PlayingEntity>,
            Without<ActiveAgentAnimation>,
            With<NextPos>,
        ),
    >,
    next_world_pos_q: Query<&NextWorldPos>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    time: Res<Time>,
    mut cmd: Commands,
) {
    // Maybe split this into different queries so we only access agent_pos at the last call or idk
    for (entity, tf, next_world_pos, path, speed) in &q {
        let target_pos = next_world_pos.0;
        let direction = (target_pos - tf.translation).normalize();

        let y_diff = (target_pos.y - tf.translation.y).round();
        println!("found diff of : {:?}", y_diff);
        println!("req anim to : {:?}", next_world_pos.0);
        if y_diff != 0.0 {
            cmd.entity(entity).insert((
                ActiveAgentAnimation::Jump {
                    y_diff,
                    start_pos: tf.translation,
                    target_pos,
                },
                JumpAnimStep(0),
            ));
            return;
        }

        cmd.entity(entity).insert(ActiveAgentAnimation::Straight {
            direction,
            target_pos,
        });
    }
}

fn animate_agent_movement(
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &GlobalTransform,
            &mut AgentPos,
            &NextPos,
            &Path,
            &ActiveAgentAnimation,
            &Speed,
        ),
        (With<PlayingEntity>),
    >,
    mut jump_anim_step_q: Query<&mut JumpAnimStep>,
    time: Res<Time>,
    time_runner_q: Query<&TimeRunner>,
    mut cmd: Commands,
) {
    for (entity, mut tf, global_tf, mut agent_pos, next_pos, path, agent_anim, speed) in &mut q {
        match agent_anim {
            ActiveAgentAnimation::Straight {
                direction,
                target_pos,
            } => {
                let move_offset = direction * (speed.current as f32) * time.delta_secs();
                if (tf.translation + move_offset).distance(*target_pos) > 0.1 {
                    println!("REAL moving by : {:?}", move_offset);
                    tf.translation += move_offset;
                    continue;
                }

                tf.translation = *target_pos;
            }
            ActiveAgentAnimation::Jump {
                y_diff,
                start_pos,
                target_pos,
            } => {
                let mut jump_anim_step = jump_anim_step_q.get_mut(entity).expect("nahh");

                let Ok(time_runner) = time_runner_q.get(entity) else {
                    println!("crating tween");
                    let mut dir_to_target = (target_pos - start_pos).normalize();
                    let y_diff = *y_diff;
                    if y_diff < 0.0 {
                        dir_to_target.y = 0.0;
                    }
                    let half_dist_to_target = target_pos.distance(*start_pos) / 2.0;
                    let jump_height_offset = Vec3::ZERO.with_y(y_diff.max(0.0) + 1.0);
                    let jump_peak_pos =
                        *start_pos + (dir_to_target * half_dist_to_target) + jump_height_offset;
                    let tween_target = entity.into_target();
                    let mut tf_state = tween_target.transform_state(tf.clone());
                    println!("jump from {:?}, to -> {:?}", start_pos, target_pos);
                    cmd.entity(entity).animation().insert_tween_here(
                        Duration::from_millis(300),
                        EaseKind::Linear,
                        tf_state.translation_to(jump_peak_pos),
                    );

                    continue;
                };
                println!("animating to => {:?}", target_pos);

                if !time_runner.is_completed() {
                    println!("not complete");
                    continue;
                }
                if jump_anim_step.0 == 0 {
                    jump_anim_step.0 = 1;
                    let mut dir_to_target = (target_pos - start_pos).normalize();
                    let y_diff = *y_diff;
                    if y_diff < 0.0 {
                        dir_to_target.y = 0.0;
                    }
                    let half_dist_to_target = target_pos.distance(*start_pos) / 2.0;
                    let jump_height_offset = Vec3::ZERO.with_y(y_diff.max(0.0) + 1.0);
                    let jump_peak_pos =
                        *start_pos + (dir_to_target * half_dist_to_target) + jump_height_offset;
                    let tween_target = entity.into_target();

                    tf.translation = jump_peak_pos;

                    let mut tf_state = tween_target.transform_state(tf.clone());
                    println!("jump from {:?}, to -> {:?}", start_pos, target_pos);
                    cmd.entity(entity).animation().insert_tween_here(
                        Duration::from_millis(300),
                        EaseKind::Linear,
                        tf_state.translation_to(*target_pos),
                    );

                    continue;
                }

                println!("tween complete");
                tf.translation = *target_pos;
            }
        };

        agent_pos.0 = next_pos.0;
        let mut e_cmds = cmd.entity(entity);
        e_cmds.remove::<JumpAnimStep>();
        e_cmds.remove::<ActiveAgentAnimation>();
        e_cmds.remove::<TimeRunner>();
        e_cmds.remove::<EaseKind>();
        e_cmds.remove::<TimeSpan>();
        e_cmds.remove::<TweenInterpolationValue>();
        e_cmds.remove::<NextPos>();
        e_cmds.remove::<NextWorldPos>();

        if path.path().last() != Some(&next_pos.0) {
            continue;
        }

        e_cmds.remove::<Pathfind>();
        e_cmds.remove::<Path>();
        cmd.trigger(MovementCompleted { entity: entity });
    }
}
