use std::collections::HashMap;

use bevy::{
    app::{App, Plugin, Update},
    color::palettes::tailwind::{GREEN_950, RED_100, RED_600, RED_900},
    gizmos::gizmos::Gizmos,
    log::warn,
    math::{UVec3, Vec3},
    picking::events::{Click, Pointer},
    time::Time,
    transform::components::{GlobalTransform, Transform},
};
use bevy_ecs::{
    component::Component,
    entity::Entity,
    event::EntityEvent,
    lifecycle::Add,
    observer::On,
    query::{With, Without},
    system::{Commands, Query, Res, Single},
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
    plugin::NorthstarPlugin,
    prelude::{
        AgentPos, CardinalIsoNeighborhood, DebugGridBuilder, DebugOffset, NextPos, Pathfind,
        PathfindMode, PathfindingFailed,
    },
};
use pyri_state::{pattern::StatePattern, prelude::StateFlush};

use crate::{
    GridCell, NODE_SIZE,
    game_flow::turns::{CurrentPlayingEntity, GameState, PlayingEntity, Speed},
    utils::{AsFlippedUVec3, AsUVec3, CombatGridQ},
};

pub struct GridMovementPlugin;

impl Plugin for GridMovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(NorthstarPlugin::<CardinalIsoNeighborhood>::default())
            .add_observer(setup_grid_settings)
            .add_observer(handle_move_request)
            .add_observer(handle_cell_click)
            .add_observer(handle_failed_pathfinding)
            .add_systems(
                StateFlush,
                GameState::LoadingGrid.on_exit(build_initial_grid),
            )
            .add_systems(Update, (draw_debug_nav_cells, move_agents));
    }
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

impl MoveRequest {
    pub fn new(entity: Entity, destination_cell: Entity) -> Self {
        Self {
            entity,
            destination_cell,
        }
    }
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

pub fn handle_move_request(
    e: On<MoveRequest>,
    mut cmd: Commands,
    agents_q: Query<(Entity, &AgentPos), (Without<Pathfind>, With<PlayingEntity>)>,
    cells_q: Query<(&GridNode, &GlobalTransform), With<GridCell>>,
    grid: CombatGridQ,
) {
    let Ok((agent_entity, agent_pos)) = agents_q.get(e.entity) else {
        warn!("Already pathfinding on this agent");
        return;
    };

    let Ok((node, node_tf)) = cells_q.get(e.destination_cell) else {
        warn!("Move req cancelled, target is not a GridCell");
        return;
    };

    let target_position = grid.pos_from_index(node.0).as_flipped_uvec3();
    println!(
        "trying to pf from {:?} -> to {:?}",
        agent_pos.0, target_position
    );
    cmd.entity(agent_entity).insert(Pathfind {
        goal: target_position,
        mode: Some(PathfindMode::AStar),
        limits: SearchLimits::default(),
    });
}

#[derive(Component)]
struct StepDistanceTraveled {
    current: f32,
    target_distance: f32,
}

impl StepDistanceTraveled {
    fn from_target_distance(target_distance: f32) -> Self {
        Self {
            current: 0.0,
            target_distance,
        }
    }
}

fn handle_failed_pathfinding(e: On<Add, PathfindingFailed>, mut cmd: Commands) {
    let mut e_cmds = cmd.entity(e.entity);
    e_cmds.remove::<PathfindingFailed>();
    e_cmds.remove::<Pathfind>();
}

fn move_agents(
    mut query: Query<
        (
            Entity,
            &mut Transform,
            &GlobalTransform,
            &mut AgentPos,
            &NextPos,
            Option<&Speed>,
        ),
        With<PlayingEntity>,
    >,
    next_world_pos_q: Query<&NextWorldPos>,
    grid_tf: Single<&GlobalTransform, With<CartesianGrid<Cartesian3D>>>,
    time: Res<Time>,
    mut cmd: Commands,
) {
    // Maybe split this into different queries so we only access agent_pos at the last call or idk
    for (entity, mut tf, global_tf, mut agent_pos, next_pos, opt_speed) in &mut query {
        let Ok(next_world_pos) = next_world_pos_q.get(entity) else {
            let flipped_pos = UVec3::new(next_pos.0.x, next_pos.0.z, next_pos.0.y);
            let world_pos = grid_tf.into_inner().translation() + flipped_pos.as_vec3();
            let world_pos = world_pos + (NODE_SIZE / 2.0).with_y(2.0);
            cmd.entity(entity).insert(NextWorldPos(world_pos));
            return;
        };

        let speed = opt_speed.map(|s| s.current).unwrap_or(1) as f32;
        if opt_speed.is_none() {
            warn!("Couldnt find speed, using default");
        }

        let dir = (next_world_pos.0 - tf.translation).normalize();
        let move_offset = dir * speed * time.delta_secs();

        if (tf.translation + move_offset).distance(next_world_pos.0) > 0.1 {
            tf.translation += move_offset;
            return;
        }

        tf.translation = next_world_pos.0;

        agent_pos.0 = next_pos.0;
        let mut e_cmds = cmd.entity(entity);
        e_cmds.remove::<NextPos>();
        e_cmds.remove::<NextWorldPos>();
    }
}
