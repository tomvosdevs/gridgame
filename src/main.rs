use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;

use bevy::DefaultPlugins;
use bevy::app::{App, Startup};
use bevy::asset::{AssetServer, Handle};
use bevy::camera::ScalingMode;
use bevy::color::palettes::tailwind::{GRAY_300, ORANGE_400};
use bevy::light::DirectionalLightShadowMap;
use bevy::log::LogPlugin;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::platform::collections::Equivalent;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy_diesel::prelude::SpatialBackend;
use bevy_ecs_tilemap::helpers::square_grid::diamond::INV_DIAMOND_BASIS;
use bevy_ecs_tilemap::helpers::square_grid::neighbors::Neighbors;
use bevy_ecs_tilemap::prelude::*;
use bevy_gauge::plugin::AttributesPlugin;
use bevy_ghx_grid::debug_plugin::view::DebugGridView;
use bevy_ghx_grid::debug_plugin::{DebugGridView3dBundle, GridDebugPlugin};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::{
    CARTESIAN_3D_DIRECTIONS, Cartesian3D, CartesianPosition, GridDelta,
};
use bevy_ghx_grid::ghx_grid::cartesian::grid::CartesianGrid;
use bevy_ghx_grid::ghx_grid::coordinate_system::CoordinateSystem;
use bevy_ghx_grid::ghx_grid::direction::Direction;
use bevy_ghx_grid::ghx_grid::grid::{Grid, GridIndex};
use bevy_ghx_proc_gen::GridNode;
use bevy_ghx_proc_gen::assets::{BundleInserter, ModelAsset, ModelsAssets};
use bevy_ghx_proc_gen::debug_plugin::generation::GenerationViewMode;
use bevy_ghx_proc_gen::debug_plugin::{DebugPluginConfig, ProcGenDebugPlugins};
use bevy_ghx_proc_gen::default_bundles::PbrMesh;
use bevy_ghx_proc_gen::proc_gen::generator::builder::GeneratorBuilder;
use bevy_ghx_proc_gen::proc_gen::generator::model::{
    Model, ModelCollection, ModelInstance, ModelRotation,
};
use bevy_ghx_proc_gen::proc_gen::generator::rules::RulesBuilder;
use bevy_ghx_proc_gen::proc_gen::generator::socket::{
    Socket, SocketCollection, SocketsCartesian3D,
};
use bevy_ghx_proc_gen::simple_plugin::ProcGenSimplePlugins;
use bevy_ghx_proc_gen::spawner_plugin::NodesSpawner;
use bevy_tween::BevyTweenRegisterSystems;
use bevy_tween::prelude::Interpolator;
use rand::RngExt;

use crate::abilities::AbilitiesTemplatePlugin;
use crate::actions::{
    Action, ActionCast, ActionEffect, ActionPlugin, ActionPoints, Damage, Electric, Fire,
    MainTarget, Melee, MovementPoints, Physical, Piercing, Range, Ranged, UsedAction, Water,
};
use crate::combat::{CombatPlugin, SelectedAction};
use crate::deck_and_cards::DeckAndCardsPlugin;
use crate::effects::{Burning, EffectsPlugin};
use crate::grid_abilities_backend::{Grid3DBackend, HitFilterPlugin};
use crate::states::{CombatState, TeamHitFilter, TurnsPlugin};
use crate::tiles_templates::Targetable;
use crate::ui::GameUiPlugin;

pub mod abilities;
pub mod actions;
pub mod combat;
pub mod deck_and_cards;
pub mod effects;
pub mod grid_abilities_backend;
pub mod projectiles;
pub mod states;
pub mod tiles_templates;
pub mod ui;

#[derive(Resource)]
pub struct CursorPos(Vec2);
impl Default for CursorPos {
    fn default() -> Self {
        // Initialize the cursor pos at some far away place. It will get updated
        // correctly when the cursor moves.
        Self(Vec2::new(-1000.0, -1000.0))
    }
}

// We need to keep the cursor position updated based on any `CursorMoved` events.
pub fn update_cursor_pos(
    camera_q: Query<(&GlobalTransform, &Camera)>,
    mut cursor_moved_events: MessageReader<CursorMoved>,
    mut cursor_pos: ResMut<CursorPos>,
) {
    for cursor_moved in cursor_moved_events.read() {
        // To get the mouse's world position, we have to transform its window position by
        // any transforms on the camera. This is done by projecting the cursor position into
        // camera space (world space).
        for (cam_t, cam) in camera_q.iter() {
            if let Ok(pos) = cam.viewport_to_world_2d(cam_t, cursor_moved.position) {
                *cursor_pos = CursorPos(pos);
            }
        }
    }
}

pub trait TileStat {}

#[derive(Component, Debug, Default)]
pub struct MaxArmor(i32);

#[derive(Component, Debug)]
#[require(MaxArmor)]
pub struct Armor(i32);

#[derive(Component, Debug, Default)]
pub struct MaxHealth(i32);

#[derive(Component, Debug)]
#[require(MaxHealth)]
pub struct Health(i32);

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthState {
    Alive,
    Dead,
}

impl Health {
    pub fn apply_damage(self: &mut Self, value: i32) -> HealthState {
        let health_left = (self.0 - value).max(0);
        self.0 = health_left;
        if health_left > 0 {
            HealthState::Alive
        } else {
            HealthState::Dead
        }
    }
}

#[derive(Component, Debug)]
pub struct Beast;

#[derive(Component, Debug)]
pub struct ActiveCamera;

pub fn spawn_beasts(mut cmd: Commands) {
    cmd.spawn(Beast);
}

pub fn print_beasts(query: Query<&Beast>) {
    for beast in &query {
        println!("{:?}", beast);
    }
}

#[derive(Component, Debug)]
pub struct TilemapEffectsTimer(Timer);

pub fn tick_tilemap_effects_timer(time: Res<Time>, mut q: Query<&mut TilemapEffectsTimer>) {
    for mut t in &mut q {
        t.0.tick(time.delta());
    }
}

pub fn spread_tiles_effects(
    mut cmd: Commands,
    tilemap_effects_timer_q: Query<(&TilemapEffectsTimer)>,
    grid_q: Query<&CartesianGrid<Cartesian3D>>,
    cell_q: Query<(
        Entity,
        &GridNode,
        &mut MeshMaterial3d<StandardMaterial>,
        Option<&mut Burning>,
    )>,
) {
    let tilemap_effects_timer = tilemap_effects_timer_q
        .single()
        .expect("more than 1 tilemap effects timer found");

    if !tilemap_effects_timer.0.just_finished() {
        return;
    }

    let grid = grid_q.single().expect("found more tha one tilemap");
    // for (tile_storage, tilemap_size) in &grid_q {
    //
    let non_burning: HashMap<GridIndex, Entity> = cell_q
        .iter()
        .filter_map(|(ent, node, _, opt_burn)| {
            if opt_burn.is_none() {
                Some((node.0, ent))
            } else {
                None
            }
        })
        .collect();

    for (burning_entity, burning_node, _, _) in cell_q
        .iter()
        .filter(|(_, _, _, burning_opt)| burning_opt.is_some())
    {
        let mut neighbors_indices: Vec<Option<GridIndex>> = Vec::new();
        neighbors_indices.resize_with(7, || None);
        grid.get_neighbours_in_all_directions(burning_node.0, &mut neighbors_indices);

        for n_index in neighbors_indices {
            if let Some(n_index) = n_index {
                if let Some(n_ent) = non_burning.get(&n_index) {
                    cmd.entity(*n_ent).insert(Burning(1));
                }
            }
        }
    }
}

fn update_tiles_texture(
    assets_refs_q: Query<&AssetsReferences>,
    materials: ResMut<Assets<StandardMaterial>>,
    mut cell_q: Query<(&mut MeshMaterial3d<StandardMaterial>, &GridNode), With<Burning>>,
) {
    let assets_references = assets_refs_q
        .single()
        .expect("there should be only ONE AssetsReferenes Component");
    let burning_material_str_ref = "burning_material";
    if let Some(burning_material_untyped_handle) = assets_references.0.get(burning_material_str_ref)
    {
        match burning_material_untyped_handle.clone().try_typed() {
            Ok(handle) => {
                for (mut material, node) in &mut cell_q {
                    if material.0 != handle {
                        material.0 = handle.clone();
                    }
                }
            }
            Err(_) => error!("The asset type did not match"),
        }
    } else {
        error!(
            "The '{:?}' entry in the assets reference HashMap was not found",
            burning_material_str_ref
        );
    }
}

/// Used to define an asset (not yet loaded) for a model: via an asset path, and an optionnal grid offset when spawned in Bevy
#[derive(Clone)]
pub struct ModelAssetDef {
    /// Path of the asset
    pub path: &'static str,
    /// Offset in grid coordinates
    pub grid_offset: GridDelta,
    /// Offset in world coordinates
    pub offset: Vec3,
    pub components_spawner: fn(&mut EntityCommands),
}

impl ModelAssetDef {
    pub fn new(path: &'static str) -> Self {
        Self {
            path,
            grid_offset: GridDelta::new(0, 0, 0),
            offset: Vec3::ZERO,
            components_spawner: |_| {},
        }
    }

    pub fn with_grid_offset(mut self, offset: GridDelta) -> Self {
        self.grid_offset = offset;
        self
    }

    pub fn with_offset(mut self, offset: Vec3) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_components(mut self, spawn_cmds: fn(&mut EntityCommands)) -> Self {
        self.components_spawner = spawn_cmds;
        self
    }

    pub fn path(&self) -> &'static str {
        self.path
    }
    pub fn offset(&self) -> &GridDelta {
        &self.grid_offset
    }
}

pub enum SocketGroups {
    SingleSocket,
    AllSeperate,
    SidesTopBot,
    // XSidesZSidesYSides,
    Specific(&'static str),
}

impl SocketGroups {
    pub fn as_directions_lists(self) -> Vec<Vec<Direction>> {
        match self {
            SocketGroups::SingleSocket => {
                vec![vec![Direction::XForward]]
            }
            SocketGroups::AllSeperate => todo!(),
            SocketGroups::SidesTopBot => todo!(),
            SocketGroups::Specific(_) => todo!(),
        }
    }
}

pub struct GridCellSockets {
    name: &'static str,
    socket_groups: Option<SocketGroups>,
}

impl GridCellSockets {
    pub fn with_sockets(mut self, socket_groups: SocketGroups) -> Self {
        self.socket_groups = Some(socket_groups);
        self
    }

    pub fn empty(name: &'static str) -> Self {
        Self {
            name,
            socket_groups: None,
        }
    }
}

pub struct GridRulesGenerator {
    sockets: SocketCollection,
    cells: Vec<GridCellSockets>,
    models_assets: Vec<Vec<ModelAssetDef>>,
    models: ModelCollection<Cartesian3D>,
}

impl GridRulesGenerator {
    pub fn new() -> Self {
        Self {
            sockets: SocketCollection::new(),
            cells: Vec::new(),
            models_assets: Vec::new(),
            models: ModelCollection::new(),
        }
    }

    pub fn add_cell(mut self: Self, name: &'static str) -> GridCellSockets {
        let cell = GridCellSockets::empty(name);
        cell
    }

    fn add_socket_to_cell(direction: Direction, cell_name: &'static str) {}
}

#[derive(Component, Clone, Debug, Default)]
#[require(Targetable)]
pub struct GridCell;

#[derive(Resource, Default)]
pub struct HoveredCell(pub Option<Entity>);

#[derive(Default, Clone)]
pub struct GridCellSocketsComponents<A: Bundle + Clone + Default = ()> {
    components: A,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

impl GridCellSocketsComponents<()> {
    pub fn from_mesh_and_material(
        mesh: &Handle<Mesh>,
        material: &Handle<StandardMaterial>,
    ) -> Self {
        Self {
            components: (),
            mesh: mesh.clone(),
            material: material.clone(),
        }
    }
}

impl<A: Bundle + Clone + Default> GridCellSocketsComponents<A> {
    pub fn with_components(
        mesh: Handle<Mesh>,
        material: Handle<StandardMaterial>,
        components: A,
    ) -> Self {
        Self {
            components,
            mesh,
            material,
        }
    }
}

impl<A: Bundle + Clone + Default> BundleInserter for GridCellSocketsComponents<A> {
    fn insert_bundle(
        &self,
        command: &mut EntityCommands,
        translation: Vec3,
        scale: Vec3,
        rotation: ModelRotation,
    ) {
        command
            .insert((
                Transform::from_translation(translation)
                    .with_scale(scale)
                    .with_rotation(Quat::from_rotation_y(rotation.rad())),
                Mesh3d(self.mesh.clone()),
                MeshMaterial3d(self.material.clone()),
                GridCell,
                Targetable,
                Health(100),
                MaxHealth(100),
                self.components.clone(),
            ))
            // TODO : Switch observer setup to a Added<GridCell> system
            .observe(tag_hovered_gridcell)
            .observe(untag_hoverout_gridcell);

        let mut rng = rand::rng();
        let rand_is_burning = rng.random_bool(1.0 / 8.0);
        if rand_is_burning {
            command.insert(Burning(1));
        }
    }
}

#[derive(Component)]
pub struct AssetsReferences(HashMap<String, UntypedHandle>);

pub fn rules_and_assets(
    mut cmd: &mut Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) -> (
    ModelInstance,
    ModelInstance,
    ModelsAssets<GridCellSocketsComponents>,
    ModelCollection<Cartesian3D>,
    SocketCollection,
) {
    let cube_mesh = meshes.add(Mesh::from(Cuboid {
        half_size: Vec3::splat(BLOCK_SIZE / 2.0),
    }));
    let green_mat = materials.add(Color::srgb(0., 1., 0.));
    let red_mat = materials.add(Color::srgb(1., 0., 0.));
    let dark_red_mat = materials.add(Color::srgb(1., 0.1, 0.3));
    let blue_mat = materials.add(Color::srgb(0., 0., 1.));
    let burning_material = materials.add(Color::srgb(1., 0.1, 1.));

    let mut assets_reference_map: HashMap<String, UntypedHandle> = HashMap::new();
    assets_reference_map.insert("green_material".to_string(), green_mat.clone().untyped());
    assets_reference_map.insert("red_material".to_string(), red_mat.clone().untyped());
    assets_reference_map.insert(
        "dark_red_material".to_string(),
        dark_red_mat.clone().untyped(),
    );
    assets_reference_map.insert("blue_material".to_string(), blue_mat.clone().untyped());
    assets_reference_map.insert(
        "burning_material".to_string(),
        burning_material.clone().untyped(),
    );
    let highlighter_material = materials.add(Color::srgba(0., 0., 0., 0.2));
    assets_reference_map.insert(
        "highlighter_material".to_string(),
        highlighter_material.untyped(),
    );
    cmd.spawn(AssetsReferences(assets_reference_map));

    let mut sockets = SocketCollection::new();

    let void = sockets.create();
    let void_bottom = sockets.create();
    let void_top = sockets.create();

    let grass_top = sockets.create();
    let grass_bottom = sockets.create();
    let grass_sides = sockets.create();

    let ground_rock_bottom = sockets.create();
    let ground_rock_top = sockets.create();
    let ground_rock_sides = sockets.create();

    let rock_bottom = sockets.create();
    let rock_top = sockets.create();
    let rock_sides = sockets.create();

    let water_bottom = sockets.create();
    let water_top = sockets.create();
    let water_sides = sockets.create();

    // let mut models_assets: Vec<Vec<ModelAssetDef>> = Vec::new();
    let mut models_assets = ModelsAssets::<GridCellSocketsComponents>::new();
    let mut model_sockets_mapping = ModelCollection::new();

    model_sockets_mapping
        .create(SocketsCartesian3D::Simple {
            x_pos: void,
            x_neg: void,
            z_pos: void,
            z_neg: void,
            y_pos: void_top,
            y_neg: void_bottom,
        })
        .with_weight(30.)
        .with_name("void");

    models_assets.add_asset(
        1,
        GridCellSocketsComponents::from_mesh_and_material(&cube_mesh, &green_mat),
    );

    model_sockets_mapping
        .create(SocketsCartesian3D::Simple {
            x_pos: grass_sides,
            x_neg: grass_sides,
            z_pos: grass_sides,
            z_neg: grass_sides,
            y_pos: grass_top,
            y_neg: grass_bottom,
        })
        .with_name("grass");

    models_assets.add_asset(
        2,
        GridCellSocketsComponents::from_mesh_and_material(&cube_mesh, &red_mat),
    );
    model_sockets_mapping
        .create(SocketsCartesian3D::Simple {
            x_pos: rock_sides,
            x_neg: rock_sides,
            z_pos: rock_sides,
            z_neg: rock_sides,
            y_pos: rock_top,
            y_neg: rock_bottom,
        })
        .with_name("rock");

    models_assets.add_asset(
        3,
        GridCellSocketsComponents::from_mesh_and_material(&cube_mesh, &dark_red_mat),
    );
    let ground_rock_instance = model_sockets_mapping
        .create(SocketsCartesian3D::Simple {
            x_pos: ground_rock_sides,
            x_neg: ground_rock_sides,
            z_pos: ground_rock_sides,
            z_neg: ground_rock_sides,
            y_pos: ground_rock_top,
            y_neg: ground_rock_bottom,
        })
        .with_name("rock")
        .instance();

    models_assets.add_asset(
        4,
        GridCellSocketsComponents::from_mesh_and_material(&cube_mesh, &blue_mat),
    );
    let water_instance = model_sockets_mapping
        .create(SocketsCartesian3D::Simple {
            x_pos: water_sides,
            x_neg: water_sides,
            z_pos: water_sides,
            z_neg: water_sides,
            y_pos: water_top,
            y_neg: water_bottom,
        })
        .with_name("water")
        .instance();

    sockets
        .add_connections(vec![
            (void, vec![void, rock_sides]),
            (ground_rock_sides, vec![grass_sides, ground_rock_sides]),
            (water_sides, vec![water_sides, grass_sides]),
            (
                grass_sides,
                vec![water_sides, grass_sides, ground_rock_sides],
            ),
            (rock_sides, vec![rock_sides]),
        ])
        // For this generation, our rotation axis is Y+, so we define connection on the Y axis with `add_rotated_connection` for sockets that still need to be compatible when rotated.
        // Note: But in reality, in this example, we don't really need it. None of our models uses any rotation, apart from ModelRotation::Rot0 (notice that there's no call to `with_rotations` on any of the models).
        // Simply using `add_connections` would give the same result (it allows connections with relative_rotation = Rot0)
        .add_rotated_connections(vec![
            (void_bottom, vec![void_top]),
            (water_top, vec![void_bottom]),
            (grass_top, vec![void_bottom]),
            (rock_top, vec![void_bottom, rock_bottom]),
            (ground_rock_top, vec![void_bottom, rock_bottom]),
            (rock_bottom, vec![rock_top]),
        ]);

    (
        water_instance,
        ground_rock_instance,
        models_assets,
        model_sockets_mapping,
        sockets,
    )
}

// -----------------  Configurable values ---------------------------

/// Modify these values to control the map size.
const GRID_HEIGHT: u32 = 7;
const GRID_X: u32 = 40;
const GRID_Z: u32 = 40;
// ------------------------------------------------------------------

/// Size of a block in world units
const BLOCK_SIZE: f32 = 1.;
const NODE_SIZE: Vec3 = Vec3::splat(BLOCK_SIZE);

const ASSETS_SCALE_FACTOR: f32 = BLOCK_SIZE / 4.; // Models are 4 units wide
const ASSETS_SCALE: Vec3 = Vec3::splat(ASSETS_SCALE_FACTOR);

fn startup_3d(
    mut cmd: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let camera_position = Vec3::new(45., 3. * GRID_HEIGHT as f32 + 20.0, 0.75 * GRID_Z as f32);

    cmd.spawn((
        Camera3d::default(),
        Name::new("Camera"),
        ActiveCamera,
        Projection::from(OrthographicProjection {
            // 6 world units per pixel of window height.
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 24.0,
            },
            ..OrthographicProjection::default_3d()
        }),
        Transform::from_translation(camera_position).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    cmd.spawn(TilemapEffectsTimer(Timer::from_seconds(
        0.5,
        TimerMode::Repeating,
    )));

    // Spawn test attack/ Spawn test action
    Action::spawn_empty(&mut cmd)
        .with_name("Mon attaque qui split")
        .with_range(4)
        .with_mp_cost(3)
        .with_ap_cost(4)
        .with_melee_damage(200)
        .with_effects(vec![ActionEffect::Split])
        .get_entity();

    // Scene lights
    cmd.insert_resource(GlobalAmbientLight {
        color: Color::Srgba(ORANGE_400),
        brightness: 0.05,
        ..default()
    });

    cmd.spawn((
        Name::new("Main light"),
        Transform {
            translation: Vec3::new(0.0, 0.0, 0.0),
            rotation: Quat::from_euler(EulerRot::ZYX, 0., -PI / 5., -PI / 3.),
            ..default()
        },
        DirectionalLight {
            shadows_enabled: true,
            illuminance: 1800.,
            color: Color::srgb(1.0, 0.85, 0.65),
            ..default()
        },
    ));

    cmd.spawn((
        Name::new("Back light"),
        Transform {
            translation: Vec3::new(5.0, 10.0, 2.0),
            rotation: Quat::from_euler(EulerRot::ZYX, 0., PI * 4. / 5., -PI / 3.),
            ..default()
        },
        DirectionalLight {
            shadows_enabled: false,
            illuminance: 550.,
            color: Color::srgb(1.0, 0.85, 0.65),
            ..default()
        },
    ));

    let (water_instance, ground_rock_instance, models_assets, models, socket_collection) =
        rules_and_assets(&mut cmd, meshes, materials);

    let rules = Arc::new(
        RulesBuilder::new_cartesian_3d(models, socket_collection)
            .build()
            .unwrap(),
    );
    let grid = CartesianGrid::new_cartesian_3d(GRID_X, GRID_HEIGHT, GRID_Z, false, false, false);

    let mut initial_grid_constraints = grid.new_grid_data(None);
    // initial_grid_constraints.set_all_y(0, Some(ground_rock_instance));
    *initial_grid_constraints.get_3d_mut(12, 0, 12) = Some(water_instance);

    let gen_builder = GeneratorBuilder::new()
        // We share the Rules between all the generators
        .with_shared_rules(rules.clone())
        .with_grid(grid.clone())
        .with_initial_grid(initial_grid_constraints)
        .unwrap();

    let node_spawner = NodesSpawner::new(models_assets, NODE_SIZE, NODE_SIZE);

    let mut gen_builder = gen_builder.clone();
    let observer = gen_builder.add_queued_observer();
    let generator = gen_builder.build().unwrap();

    cmd.spawn((
        Name::new(format!("Grid main")),
        Transform::from_translation(Vec3 {
            x: (grid.size_x() as f32 * -0.5),
            y: 0.,
            z: -(grid.size_z() as f32) * 0.5,
        }),
        grid.clone(),
        generator,
        observer,
        // We also share the ModelsAssets between all the generators
        node_spawner.clone(),
        DebugGridView3dBundle {
            view: DebugGridView::new(false, true, Color::Srgba(GRAY_300), NODE_SIZE),
            ..default()
        },
    ));
}

pub fn count_cells(mut cmd: Commands, q: Query<Entity, With<GridCell>>) {
    for e in &q {
        cmd.entity(e).log_components();
    }
}

pub fn log_actions(mut cmd: Commands, q: Query<Entity, With<Action>>) {
    for action_ent in &q {
        println!("ACTION COMP LIST = ");
        cmd.entity(action_ent).log_components();
    }
}

#[derive(Component, Debug)]
pub struct CursorTarget(Entity);

pub fn tag_hovered_gridcell(mut hover: On<Pointer<Over>>, mut hovered: ResMut<HoveredCell>) {
    hovered.0 = Some(hover.entity);
    hover.propagate(false);
}

pub fn untag_hoverout_gridcell(mut hover: On<Pointer<Out>>, mut hovered: ResMut<HoveredCell>) {
    // Only clear if this entity is still the current target
    if hovered.0 == Some(hover.entity) {
        hovered.0 = None;
    }
    hover.propagate(false);
}

pub fn trigger_action(
    mut click: On<Pointer<Click>>,
    mut cmd: Commands,
    mut actions_q: Query<
        (
            &Action,
            &Range,
            &ActionPoints,
            // Option<&MovementPoints>,
            // Option<&Physical>,
            // Option<&Ranged>,
            // Option<&Melee>,
            // Option<&Piercing>,
            // Option<&Fire>,
            // Option<&Electric>,
            // Option<&Water>,
        ),
        (Without<MainTarget>, Without<UsedAction>),
    >,
    selected_action: Res<SelectedAction>,
) {
    // Is an action selected ?
    let Some(action_ent) = selected_action.0 else {
        return;
    };
    // Get the relevant stats (range, ap etc will serve for cast checks later)
    let Ok((action, range, ap)) = actions_q.get(action_ent) else {
        return;
    };

    cmd.entity(action_ent).insert(MainTarget(click.entity));
    cmd.trigger(ActionCast { entity: action_ent });
    cmd.entity(action_ent).insert(UsedAction);
}

pub fn sync_cursor_target(
    mut cmd: Commands,
    hovered: Res<HoveredCell>,
    current: Query<Entity, With<CursorTarget>>,
) {
    if !hovered.is_changed() {
        return; // nothing to do this frame
    }

    // Remove from old target
    for ent in &current {
        if Some(ent) != hovered.0 {
            cmd.entity(ent).remove::<CursorTarget>();
        }
    }

    // Add to new target
    if let Some(ent) = hovered.0 {
        cmd.entity(ent).insert(CursorTarget(ent));
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SkewMaterial {
    #[uniform(100)]
    pub target_pos: Vec2,
    #[uniform(100)]
    pub card_pos: Vec2,
    #[uniform(100)]
    pub tilt_strength: f32,
    #[uniform(100)]
    pub flatten: f32,
}

impl Default for SkewMaterial {
    fn default() -> Self {
        Self {
            target_pos: Vec2::ZERO,
            card_pos: Vec2::ZERO,
            tilt_strength: 0.0,
            flatten: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SkewData {
    pub target_pos: Vec2,
    pub card_pos: Vec2,
    pub tilt_strength: f32,
}

impl SkewData {
    pub fn with_target_pos_offset(mut self: Self, offset: Vec2) -> Self {
        self.target_pos += offset;
        self
    }

    pub fn with_card_pos_offset(mut self: Self, offset: Vec2) -> Self {
        self.card_pos += offset;
        self
    }

    pub fn with_tilt_strength_offset(mut self: Self, offset: f32) -> Self {
        self.tilt_strength += offset;
        self
    }

    pub fn with_target_pos(mut self: Self, val: Vec2) -> Self {
        self.target_pos = val;
        self
    }

    pub fn with_card_pos(mut self: Self, val: Vec2) -> Self {
        self.card_pos = val;
        self
    }

    pub fn with_tilt_strength(mut self: Self, val: f32) -> Self {
        self.tilt_strength = val;
        self
    }
}

impl From<SkewData> for SkewMaterial {
    fn from(value: SkewData) -> Self {
        Self {
            target_pos: value.target_pos,
            card_pos: value.card_pos,
            tilt_strength: value.tilt_strength,
            flatten: 1.0,
        }
    }
}

impl From<SkewMaterial> for SkewData {
    fn from(value: SkewMaterial) -> Self {
        Self {
            target_pos: value.target_pos,
            card_pos: value.card_pos,
            tilt_strength: value.tilt_strength,
        }
    }
}

pub struct InterpolateSkew {
    pub start: SkewData,
    pub end: SkewData,
}

impl Interpolator for InterpolateSkew {
    type Item = ExtendedMaterial<bevy::prelude::StandardMaterial, SkewMaterial>;

    fn interpolate(
        &self,
        item: &mut Self::Item,
        value: bevy_tween::interpolate::CurrentValue,
        _previous_value: bevy_tween::interpolate::PreviousValue,
    ) {
        item.extension.card_pos = self.start.card_pos.lerp(self.end.card_pos, value);
        item.extension.target_pos = self.start.target_pos.lerp(self.end.target_pos, value);
        item.extension.tilt_strength = self.start.tilt_strength.lerp(self.end.tilt_strength, value);
    }
}

pub fn custom_interpolators_plugin(app: &mut App) {
    app.add_tween_systems(
        PostUpdate,
        bevy_tween::asset_tween_system::<InterpolateSkew, ()>(),
    );
}

impl MaterialExtension for SkewMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/skew_material.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/skew_material.wgsl".into()
    }
    fn deferred_vertex_shader() -> ShaderRef {
        "shaders/skew_material.wgsl".into()
    }
}

fn main() {
    App::new()
        .add_plugins((
            MeshPickingPlugin,
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: String::from("Drill"),
                        ..Default::default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(LogPlugin {
                    filter: "info,wgpu_core=error,wgpu_hal=error,ghx_proc_gen=debug".into(),
                    level: bevy::log::Level::DEBUG,
                    ..default()
                }),
            ProcGenSimplePlugins::<Cartesian3D, GridCellSocketsComponents>::default(),
            GridDebugPlugin::<Cartesian3D>::new(),
        ))
        .add_plugins(TilemapPlugin)
        .add_plugins((
            MaterialPlugin::<ExtendedMaterial<StandardMaterial, SkewMaterial>>::default(),
            custom_interpolators_plugin,
            EffectsPlugin,
            ActionPlugin,
            GameUiPlugin,
            CombatPlugin,
            TurnsPlugin,
            DeckAndCardsPlugin,
        ))
        .add_plugins(AbilitiesTemplatePlugin)
        .add_plugins(Grid3DBackend::plugin())
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(HoveredCell(None))
        .insert_state(CombatState::DeterminePlayOrder)
        .add_systems(Startup, startup_3d)
        // .init_resource::<CursorPos>()
        // .add_systems(Startup, (startup, spawn_beasts))
        // .add_systems(Update, update_cursor_pos)
        // .add_systems(Update, tag_hovered_tile)
        // // .add_systems(Update, print_transforms)
        // // .add_systems(Update, print_tooltip_pos)
        .add_systems(Update, tick_tilemap_effects_timer)
        .add_systems(Update, spread_tiles_effects)
        .add_systems(Update, update_tiles_texture)
        .add_systems(Update, sync_cursor_target)
        // .add_systems(Update, log_actions)
        //
        // .add_systems(Update, count_cells)
        .run();
}
