use std::{collections::HashMap, default};

use bevy::{
    app::{App, Plugin, Startup, Update},
    asset::Assets,
    color::{Srgba, palettes::css::RED},
    ecs::{
        bundle::Bundle,
        change_detection::DetectChanges,
        component::Component,
        entity::Entity,
        event::{EntityEvent, Event},
        name::Name,
        observer::On,
        query::{Added, With},
        relationship::RelationshipTarget,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query, Res, ResMut, Single},
    },
    input::{ButtonInput, keyboard::KeyCode},
    log::warn,
    math::{
        UVec2, UVec3, Vec3,
        primitives::{Capsule3d, Sphere},
    },
    mesh::{Mesh, Mesh3d},
    pbr::{MeshMaterial3d, StandardMaterial},
    sprite::Text2d,
    state::{condition::in_state, state::States},
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::prelude::Invokes;
use bevy_gauge::{
    AttributeComponent,
    prelude::{Attributes, AttributesMut},
};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::{GridNode, bevy_egui::egui::Vec2, proc_gen::generator::Generator};
use bevy_northstar::{
    CardinalIsoGrid,
    prelude::{AgentOfGrid, AgentPos, Blocking},
};
use bevy_prng::WyRand;
use bevy_rand::global::GlobalRng;
use pyri_state::{
    access::{CurrentRef, NextMut},
    pattern::StatePattern,
    prelude::{State, StateFlush},
    setup::AppExtState,
};
use rand::RngExt;

use crate::{
    GRID_X, GRID_Z, GridCell, NODE_SIZE,
    abilities::abilities_templates::{Marker, Projectile},
    creatures::{
        definitions::{Creature, CreatureKind},
        generation::CreatureGenerationRequested,
    },
    deck::{
        card_blueprints::register_blueprints,
        card_builders::DefaultDeckGenRequested,
        deck_and_cards::{
            ActiveDeck, CardPile, CardState, Deck, DrawHand, InDrawPile, StatelessCard,
            UnassignedDeckState,
        },
    },
    grid_abilities_backend::HitFilter,
    movement::MoveRequest,
    tiles_templates::Targetable,
    utils::{AsFlippedUVec3, AsUVec3},
};

pub struct TurnsPlugin;

impl Plugin for TurnsPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .init_state::<CombatState>()
            .add_observer(handle_playing_gen_req)
            .add_observer(spawn_combat_playing_entities)
            .add_observer(handle_combat_start)
            .add_observer(handle_turn_start)
            .add_observer(handle_turn_end)
            .add_systems(
                StateFlush,
                GameState::InCombat.on_enter(request_test_playing_gen),
            )
            .add_systems(Startup, spawn_dev_text)
            .add_systems(Update, (draw_dev_text, check_grid_gen_status))
            .add_systems(
                Update,
                GameState::InCombat.on_update((
                    keyboard_update_turn_test,
                    start_combat_test,
                    attach_visuals,
                )),
            );
    }
}

#[derive(Event)]
pub struct RequestPlayingGeneration {
    kind: CreatureKind,
    team: PlayingTeam,
}

impl RequestPlayingGeneration {
    pub fn ally_from_kind(kind: CreatureKind) -> Self {
        Self {
            kind,
            team: PlayingTeam::Ally,
        }
    }

    pub fn enemy_from_kind(kind: CreatureKind) -> Self {
        Self {
            kind,
            team: PlayingTeam::Enemy,
        }
    }
}

pub fn handle_playing_gen_req(e: On<RequestPlayingGeneration>, mut cmd: Commands) {
    let entity = cmd
        .spawn((
            Name::new("Some player"),
            PlayingEntity::new_ally(),
            Invokes::new(),
        ))
        .id();

    match e.team {
        PlayingTeam::Ally => cmd.entity(entity).insert(PlayingEntity::new_ally()),
        PlayingTeam::Enemy => cmd.entity(entity).insert(PlayingEntity::new_ennemy()),
        PlayingTeam::Environment => cmd
            .entity(entity)
            .insert(PlayingEntity::new_environmental()),
    };

    cmd.trigger(CreatureGenerationRequested::new(entity, e.kind));
}

pub fn request_test_playing_gen(mut cmd: Commands) {
    for _ in 0..5 {
        cmd.trigger(RequestPlayingGeneration::ally_from_kind(
            CreatureKind::TestRanged,
        ));
    }
}

fn handle_combat_start(
    _: On<CombatStart>,
    mut cmd: Commands,
    playing_q: Query<(Entity, &Speed), With<PlayingEntity>>,
    deck_pile_q: Query<(Entity, &CardPile), With<Deck>>,
    instance_cards_q: Query<&CardState<UnassignedDeckState>>,
    playing_current_deck_ref_q: Query<&CurrentDeckReference, With<PlayingEntity>>,
    mut attributes: AttributesMut<(With<Deck>, With<CardPile>)>,
) {
    let current_deck_entities: Vec<Entity> =
        playing_current_deck_ref_q.iter().map(|p| p.0).collect();

    for (deck_entity, deck_pile) in deck_pile_q
        .iter()
        .filter(|(e, _)| current_deck_entities.contains(e))
    {
        let cards_count = deck_pile.iter().count();
        cmd.entity(deck_entity).insert(ActiveDeck);

        for card_entity in deck_pile.iter() {
            if !instance_cards_q.contains(card_entity) {
                warn!(
                    "All card attached to entities with 'CurrentDeck' should have be in the 'StatelessCard' state when CombatStart is triggered"
                );
                continue;
            }

            let mut card_cmds = cmd.entity(card_entity);
            card_cmds.remove::<StatelessCard>();
            card_cmds.insert(CardState::<InDrawPile>::new());
        }
    }

    let entities_by_turn_order: Vec<Entity> = playing_q
        .iter()
        .sort_by::<&Speed>(|val1, val2| val2.current.cmp(&val1.current))
        .map(|(ent, _)| ent)
        .collect();

    cmd.insert_resource(CombatData::init_new_combat(&entities_by_turn_order));
    for (idx, ent) in entities_by_turn_order.iter().enumerate() {
        cmd.entity(*ent).insert(TurnOrder(idx as i32));
        if idx == 0 {
            cmd.trigger(EntityTurnStart { entity: *ent });
        }
    }
}

fn handle_turn_start(
    e: On<EntityTurnStart>,
    mut cmd: Commands,
    q: Query<&CurrentDeckReference>,
    q_decks: Query<Entity, With<ActiveDeck>>,
) {
    let entity_current_deck = q.get(e.entity).expect("entity doesn have a deck");
    let deck_entity = q_decks
        .get(entity_current_deck.0)
        .expect("deck doesn't exist");

    cmd.trigger(DrawHand::from_deck_entity(deck_entity));
    cmd.insert_resource(CurrentPlayingEntity(e.entity));
}

fn handle_turn_end(e: On<EntityTurnEnd>, mut cmd: Commands, mut combat_data: ResMut<CombatData>) {
    combat_data.end_entity_turn(e.entity);

    let next_playing_entity = combat_data.get_next_playing_entity();

    // Add back if the end of a GLOBAL turn should do something
    // if combat_data.turn_just_ended() {

    // }

    cmd.trigger(EntityTurnStart {
        entity: next_playing_entity,
    });
}

pub trait ToWorldPos {
    fn as_world_pos(self: &Self, grid_origin_pos: Vec3) -> Vec3;
}

impl ToWorldPos for CartesianPosition {
    fn as_world_pos(self: &Self, grid_origin_pos: Vec3) -> Vec3 {
        Vec3::new(
            (self.x as f32 * NODE_SIZE.x) - (NODE_SIZE.x * 0.5),
            (self.y as f32 * NODE_SIZE.y) + (NODE_SIZE.y * 0.5),
            (self.z as f32 * NODE_SIZE.z) - (NODE_SIZE.z * 0.5),
        ) + grid_origin_pos
    }
}

pub trait FromGrid {
    fn crate_grid_pos_bundle(grid: &CartesianGrid<Cartesian3D>, pos: UVec3) -> impl Bundle;

    fn create_ground_grid_pos_bundle(
        grid: &CartesianGrid<Cartesian3D>,
        nav_grid: Entity,
        grid_origin_offset: Vec3,
        player_mesh_size: Vec2,
        pos: UVec2,
        grid_nodes: &Vec<&GridNode>,
    ) -> (Transform, impl Bundle);
}

impl FromGrid for Transform {
    fn crate_grid_pos_bundle(grid: &CartesianGrid<Cartesian3D>, pos: UVec3) -> impl Bundle {
        let node_size = NODE_SIZE;
        let index = grid.index_from_coords(pos.x, pos.y, pos.z);
        let grid_pos = grid.pos_from_index(index);
        let translation = Vec3::new(
            grid_pos.x as f32 * node_size.x,
            grid_pos.y as f32 * node_size.y,
            grid_pos.z as f32 * node_size.z,
        );
        (Transform::from_translation(translation), grid_pos)
    }

    fn create_ground_grid_pos_bundle(
        grid: &CartesianGrid<Cartesian3D>,
        nav_grid: Entity,
        grid_origin_offset: Vec3,
        player_mesh_size: Vec2,
        pos: UVec2,
        grid_nodes: &Vec<&GridNode>,
    ) -> (Transform, impl Bundle) {
        let node_size = NODE_SIZE;

        println!("grid nodes : {:?}", grid_nodes.iter().count());

        let ground = grid_nodes
            .iter()
            .filter_map(|n| {
                let node_grid_pos = grid.pos_from_index(n.0);
                if node_grid_pos.x == pos.x && node_grid_pos.z == pos.y {
                    println!("MATCH : {:?}", node_grid_pos);
                    return Some(node_grid_pos.y);
                }
                None
            })
            .max()
            .expect("could not find a highest z (height) pos");

        println!("ground y pos is : {:?}", ground);

        let groud_pos_index = grid.index_from_coords(pos.x, ground, pos.y);
        let cell_grid_pos = grid.pos_from_index(groud_pos_index);
        println!("result pos : {:?}", cell_grid_pos);
        let translation = Vec3::new(
            (cell_grid_pos.x as f32 * node_size.x) + (player_mesh_size.x * 0.5),
            (cell_grid_pos.y as f32 * node_size.y) + player_mesh_size.y + node_size.y,
            (cell_grid_pos.z as f32 * node_size.z) + (player_mesh_size.x * 0.5),
        ) + grid_origin_offset;
        (
            Transform::from_translation(translation),
            (
                cell_grid_pos,
                AgentPos(cell_grid_pos.as_flipped_uvec3()),
                Blocking,
                AgentOfGrid(nav_grid),
            ),
        )
    }
}

#[derive(Component)]
pub struct AbilityTest(pub Entity);

fn get_rand_pos_in_grid(rng: &mut WyRand, side_padding: u32) -> UVec2 {
    UVec2::new(
        rng.random_range((0 + side_padding)..GRID_X - (1 + side_padding)),
        rng.random_range((0 + side_padding)..GRID_Z - (1 + side_padding)),
    )
}

pub fn get_random_available_pos(
    taken_positions: &mut Vec<UVec2>,
    rng: &mut WyRand,
    side_padding: u32,
) -> UVec2 {
    let mut pos = get_rand_pos_in_grid(rng, side_padding);
    while taken_positions.contains(&pos) {
        pos = get_rand_pos_in_grid(rng, side_padding);
    }
    taken_positions.push(pos.clone());
    pos
}

pub fn spawn_combat_playing_entities(
    _: On<CombatInit>,
    mut cmd: Commands,
    q: Query<Entity, With<Deck>>,
    grid_q: Query<(&CartesianGrid<Cartesian3D>, &GlobalTransform)>,
    nav_grid_q: Query<Entity, With<CardinalIsoGrid>>,
    grid_nodes_q: Query<(Entity, &GridNode), With<GridCell>>,
    players_q: Query<Entity, With<PlayingEntity>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut rng: Single<&mut WyRand, With<GlobalRng>>,
) {
    let (grid, grid_tf) = grid_q.single().expect("Expected to find one single grid");
    let nav_grid = nav_grid_q.single().expect("Expected one nav grid");
    println!("grid rotation : {:?}", grid_tf.rotation());
    let player_size = Vec2::new(0.7, 1.2);
    let player_test_mesh_handle = meshes.add(Capsule3d::new(player_size.x, player_size.y));
    let player_test_mat_handle =
        materials.add(StandardMaterial::from_color(Srgba::new(0.8, 0.1, 0.5, 1.0)));

    let grid_nodes: Vec<&GridNode> = grid_nodes_q.iter().map(|(_, n)| n).collect();

    let grid_origin_pos = grid_tf.translation();

    let mut taken_positions: Vec<UVec2> = Vec::new();
    for (_, entity) in players_q.iter().enumerate() {
        let side_pad = 4;
        let flat_pos = get_random_available_pos(&mut taken_positions, &mut rng, side_pad);

        cmd.entity(entity).insert((
            Mesh3d::from(player_test_mesh_handle.clone()),
            MeshMaterial3d::from(player_test_mat_handle.clone()),
            Transform::create_ground_grid_pos_bundle(
                &grid,
                nav_grid,
                grid_origin_pos,
                player_size,
                flat_pos,
                &grid_nodes,
            ),
        ));
    }

    cmd.trigger(CombatStart);
}

fn check_grid_gen_status(
    wfc_generator: Single<&Generator<Cartesian3D, CartesianGrid<Cartesian3D>>>,
    mut next_game_state: NextMut<GameState>,
) {
    if wfc_generator.nodes_left() > 0 {
        return;
    }

    next_game_state.enter(GameState::InCombat);
}

pub fn start_combat_test(mut cmd: Commands, keyboard_input: Res<ButtonInput<KeyCode>>) {
    if !keyboard_input.just_pressed(KeyCode::KeyC) {
        return;
    }

    cmd.trigger(CombatInit);
}

fn attach_visuals(
    mut commands: Commands,
    q_projectiles: Query<Entity, Added<Marker<Projectile>>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for entity in q_projectiles.iter() {
        println!("adding mesh to projectile");
        let mesh_handle = Sphere::new(3.0);
        let mat_handle = StandardMaterial::from_color(RED);
        let mesh = meshes.add(mesh_handle);
        let mat = materials.add(mat_handle);
        commands
            .entity(entity)
            .insert((Mesh3d(mesh), MeshMaterial3d(mat)));
    }
}

pub fn spawn_dev_text(mut cmd: Commands) {
    cmd.insert_resource(DevText("".to_string()));
    cmd.spawn((DevTextTarget, Text2d::new("")));
}

// ======================
// Update targeted systems
// ======================

pub fn draw_dev_text(mut q: Query<&mut Text2d, With<DevTextTarget>>, text: Res<DevText>) {
    let mut text2d = q
        .single_mut()
        .expect("Only one Text2d as dev text target was expected");

    if text.is_changed() {
        text2d.0 = text.0.clone();
    };
}

pub fn keyboard_update_turn_test(
    keys: Res<ButtonInput<KeyCode>>,
    mut cmd: Commands,
    curr_ent: Option<Res<CurrentPlayingEntity>>,
) {
    if !keys.just_pressed(KeyCode::ArrowRight) {
        return;
    }
    let Some(curr_ent) = curr_ent else {
        return;
    };

    println!("ici man");
    cmd.trigger(EntityTurnEnd { entity: curr_ent.0 });
}

#[derive(Resource)]
pub struct DevText(String);

#[derive(Component)]
pub struct DevTextTarget;

#[derive(Resource)]
pub struct CurrentPlayingEntity(pub Entity);

#[derive(Resource)]
pub struct CombatData {
    pub current_turn: u16,
    pub entities_next_turn: HashMap<Entity, u16>,
    turn_ended: bool,
}

impl CombatData {
    pub fn init_new_combat(entities_in_order: &Vec<Entity>) -> Self {
        let entities_next_turn: HashMap<Entity, u16> =
            entities_in_order.iter().map(|e| (*e, 0)).collect();
        Self {
            current_turn: 0,
            entities_next_turn,
            turn_ended: false,
        }
    }

    pub fn end_entity_turn(self: &mut Self, entity: Entity) {
        self.entities_next_turn.entry(entity).and_modify(|turn| {
            *turn += 1;
        });
    }

    pub fn get_next_playing_entity(self: &mut Self) -> Entity {
        let current_turn = self.current_turn;
        let didnt_play_curr_turn: Vec<&Entity> = self
            .entities_next_turn
            .iter()
            .filter_map(|(ent, next_turn)| {
                if *next_turn == current_turn {
                    Some(ent)
                } else {
                    None
                }
            })
            .collect();

        // Means all entities have played the current turn
        if didnt_play_curr_turn.is_empty() {
            self.turn_ended = true;
            return *self.entities_next_turn.iter().next().unwrap().0;
        }

        **didnt_play_curr_turn.iter().next().unwrap()
    }

    pub fn turn_just_ended(&self) -> bool {
        return self.turn_ended;
    }

    pub fn start_next_turn(&mut self) {
        self.current_turn += 1;
    }
}

#[derive(Event)]
pub struct CombatInit;

#[derive(Event)]
pub struct CombatStart;

#[derive(EntityEvent)]
pub struct EntityTurnStart {
    entity: Entity,
}

#[derive(EntityEvent)]
pub struct EntityTurnEnd {
    entity: Entity,
}

#[derive(Event)]
pub struct GlobalTurnStart;

#[derive(Event)]
pub struct GlobalTurnEnd;

#[derive(EntityEvent)]
pub struct DrawCard {
    entity: Entity,
}

#[derive(State, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    LoadingGrid,
    InCombat,
}

#[derive(State, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum CombatState {
    #[default]
    DeterminePlayOrder,
    PlayerTurn(i32),
    EnemyTurn(i32),
    EnvironmentTurn(i32),
}

#[derive(Component)]
pub struct MemberOf<const ID: i32>;
pub type AllyTag = MemberOf<0>;
pub type EnemyTag = MemberOf<1>;
pub type EnvironmentTag = MemberOf<2>;

#[derive(Component, PartialEq, Eq)]
pub enum PlayingTeam {
    Ally,
    Enemy,
    Environment,
}

#[derive(Clone, Debug, Component)]
pub enum TeamHitFilter {
    Enemies,
    Allies,
}

impl HitFilter for TeamHitFilter {
    type Lookup = PlayingTeam;

    fn can_target(&self, invoker: Option<&Self::Lookup>, target: Option<&Self::Lookup>) -> bool {
        match (self, invoker, target) {
            (TeamHitFilter::Enemies, Some(i), Some(t)) => i != t,
            _ => true, // no team info → allow (e.g. hitting terrain)
        }
    }
}

#[derive(Component, Default)]
#[require(Targetable)]
pub struct PlayingEntity;

#[derive(Component)]
pub struct CurrentDeckReference(pub Entity);

impl PlayingEntity {
    pub fn new_ally() -> impl Bundle {
        (PlayingEntity, MemberOf::<0>, PlayingTeam::Ally)
    }

    pub fn new_ennemy() -> impl Bundle {
        (PlayingEntity, MemberOf::<1>, PlayingTeam::Enemy)
    }

    pub fn new_environmental() -> impl Bundle {
        (PlayingEntity, MemberOf::<2>, PlayingTeam::Environment)
    }

    pub fn new_teamless() -> impl Bundle {
        PlayingEntity
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct Speed {
    #[read]
    #[write]
    pub current: i32,
}

impl Speed {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct Strength {
    #[read]
    #[write]
    pub current: i32,
}

impl Strength {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct MeleeRange {
    #[read]
    #[write]
    pub current: i32,
}

impl MeleeRange {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct RangedRange {
    #[read]
    #[write]
    pub current: i32,
}

impl RangedRange {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default)]
pub struct TurnOrder(pub i32);

impl Creature {
    pub fn from_stats(
        kind: CreatureKind,
        speed: i32,
        strength: i32,
        melee_range: i32,
    ) -> impl Bundle {
        (
            Creature::new(kind),
            Speed::new(speed),
            Strength::new(strength),
            MeleeRange::new(melee_range),
        )
    }
}
