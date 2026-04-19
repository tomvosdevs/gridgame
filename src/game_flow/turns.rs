use std::collections::HashMap;

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
    state::{app::AppExtStates, state::States},
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::prelude::Invokes;
use bevy_gauge::prelude::AttributesMut;
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::{GridNode, bevy_egui::egui::Vec2, proc_gen::generator::Generator};

use crate::{
    GridCell, NODE_SIZE,
    abilities::abilities_templates::{Marker, Projectile, basic_projectile_ability},
    deck::deck_and_cards::{
        ActiveDeck, CardPile, CardState, Deck, DrawHand, InDrawPile, StatelessCard,
        UnassignedDeckState, spawn_test_deck,
    },
    grid_abilities_backend::HitFilter,
};

pub struct TurnsPlugin;

impl Plugin for TurnsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_state(CombatState::DeterminePlayOrder)
            .add_systems(Startup, |mut cmd: Commands| {
                let projectile_ability = basic_projectile_ability(&mut cmd, None);
                cmd.spawn(AbilityTest(projectile_ability));
            })
            .add_observer(spawn_combat_playing_entities)
            .add_observer(handle_combat_start)
            .add_observer(handle_turn_start)
            .add_observer(handle_turn_end)
            .add_systems(
                Startup,
                (spawn_dev_text, define_test_gamestatus)
                    .chain()
                    .after(spawn_test_deck),
            )
            .add_systems(
                Update,
                (
                    draw_dev_text,
                    keyboard_update_turn_test,
                    start_combat_test,
                    attach_visuals,
                ),
            );
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
        attributes.set(deck_entity, "SoulLife.Max", cards_count as f32);
        attributes.set(deck_entity, "SoulLife.Current", cards_count as f32);

        for card_entity in deck_pile.iter() {
            println!("found active deck pile");

            if !instance_cards_q.contains(card_entity) {
                warn!(
                    "All card attached to entities with 'CurrentDeck' should have be in the 'StatelessCard' state when CombatStart is triggered"
                );
                continue;
            }

            let mut card_cmds = cmd.entity(card_entity);
            card_cmds.remove::<StatelessCard>();
            card_cmds.insert(CardState::<InDrawPile>::new());
            println!("updated deck cards state");
        }
    }

    let entities_by_turn_order: Vec<Entity> = playing_q
        .iter()
        .sort_by::<&Speed>(|val1, val2| val2.0.cmp(&val1.0))
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
    println!("turn started on : {:?}", e.entity);
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

pub fn define_test_gamestatus(mut cmd: Commands) {
    cmd.insert_resource(GameStatus { in_combat: false });
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
        grid_origin_offset: Vec3,
        player_mesh_size: Vec2,
        pos: UVec2,
        grid_nodes: &Vec<&GridNode>,
    ) -> (Transform, CartesianPosition);
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
        grid_origin_offset: Vec3,
        player_mesh_size: Vec2,
        pos: UVec2,
        grid_nodes: &Vec<&GridNode>,
    ) -> (Transform, CartesianPosition) {
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
        let translation = Vec3::new(
            (cell_grid_pos.x as f32 * node_size.x) - (node_size.x * 0.5),
            (cell_grid_pos.y as f32 * node_size.y) + player_mesh_size.y + node_size.y,
            (cell_grid_pos.z as f32 * node_size.z) - (node_size.z * 0.5),
        ) + grid_origin_offset;
        (Transform::from_translation(translation), cell_grid_pos)
    }
}

#[derive(Component)]
pub struct AbilityTest(pub Entity);

pub fn spawn_combat_playing_entities(
    _: On<CombatInit>,
    mut cmd: Commands,
    q: Query<Entity, With<Deck>>,
    grid_q: Query<(&CartesianGrid<Cartesian3D>, &GlobalTransform)>,
    grid_nodes_q: Query<(Entity, &GridNode), With<GridCell>>,
    _players_q: Query<Entity, With<PlayingEntity>>,
    _cells_q: Query<(Entity, &GridNode)>,
    _grid: Single<&CartesianGrid<Cartesian3D>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (grid, grid_tf) = grid_q.single().expect("Expected to find one single grid");
    let player_size = Vec2::new(0.7, 1.2);
    let player_test_mesh_handle = meshes.add(Capsule3d::new(player_size.x, player_size.y));
    let player_test_mat_handle =
        materials.add(StandardMaterial::from_color(Srgba::new(0.8, 0.1, 0.5, 1.0)));

    let test_deck = q
        .iter()
        .next()
        .expect("expected to find at least one Deck entity");

    let grid_nodes: Vec<&GridNode> = grid_nodes_q.iter().map(|(_, n)| n).collect();

    let grid_origin_pos = grid_tf.translation();
    let center_pos = UVec2::new(grid.size_x() / 2, grid.size_z() / 2);
    let found_tf = Transform::create_ground_grid_pos_bundle(
        &grid,
        grid_origin_pos,
        player_size,
        center_pos,
        &grid_nodes,
    );
    println!("grid pos is : {:?}", grid_origin_pos);

    cmd.spawn((
        Name::new("Player 1"),
        PlayingEntity::new_ally(),
        Mesh3d::from(player_test_mesh_handle.clone()),
        MeshMaterial3d::from(player_test_mat_handle.clone()),
        Creature::from_stats(6, 3, 1),
        found_tf,
        // TODO: Use actual target
        // GridInvokerTarget::position(found_tf.1),
        Invokes::new(),
        CurrentDeckReference(test_deck),
    ));
    cmd.spawn((
        PlayingEntity::new_ally(),
        Mesh3d::from(player_test_mesh_handle.clone()),
        MeshMaterial3d::from(player_test_mat_handle.clone()),
        Creature::from_stats(1, 3, 1),
        Transform::create_ground_grid_pos_bundle(
            &grid,
            grid_origin_pos,
            player_size,
            UVec2::new(1, 2),
            &grid_nodes,
        ),
        CurrentDeckReference(test_deck),
    ));
    cmd.spawn((
        PlayingEntity::new_ennemy(),
        Mesh3d::from(player_test_mesh_handle.clone()),
        MeshMaterial3d::from(player_test_mat_handle.clone()),
        Creature::from_stats(5, 4, 1),
        Transform::create_ground_grid_pos_bundle(
            &grid,
            grid_origin_pos,
            player_size,
            UVec2::new(1, 3),
            &grid_nodes,
        ),
        CurrentDeckReference(test_deck),
    ));

    cmd.trigger(CombatStart);
}

#[derive(Resource)]
pub struct GameStatus {
    pub in_combat: bool,
}

pub fn start_combat_test(
    mut cmd: Commands,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    wfc_generator: Single<&Generator<Cartesian3D, CartesianGrid<Cartesian3D>>>,
    mut game_status: ResMut<GameStatus>,
) {
    if !keyboard_input.just_pressed(KeyCode::KeyC) || game_status.in_combat {
        return;
    }

    if wfc_generator.nodes_left() > 0 {
        return;
    }

    game_status.in_combat = true;

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
    game_status: Res<GameStatus>,
) {
    if !keys.just_pressed(KeyCode::ArrowRight) || !game_status.in_combat {
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

#[derive(States, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CombatState {
    DeterminePlayOrder,
    PlayerTurn(i32),
    EnemyTurn(i32),
    EnvironmentTurn(i32),
}

#[derive(Component)]
pub struct MemberOf<const ID: i32>;
pub type AllyTag = MemberOf<0>;
pub type EnnemyTag = MemberOf<1>;
pub type EnvironmentTag = MemberOf<2>;

#[derive(Component, PartialEq, Eq)]
pub enum PlayingTeam {
    Ally,
    Ennemy,
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
pub struct PlayingEntity;

#[derive(Component)]
pub struct CurrentDeckReference(pub Entity);

impl PlayingEntity {
    pub fn new_ally() -> impl Bundle {
        (PlayingEntity, MemberOf::<0>, PlayingTeam::Ally)
    }

    pub fn new_ennemy() -> impl Bundle {
        (PlayingEntity, MemberOf::<1>, PlayingTeam::Ennemy)
    }

    pub fn new_environmental() -> impl Bundle {
        (PlayingEntity, MemberOf::<2>, PlayingTeam::Environment)
    }

    pub fn new_teamless() -> impl Bundle {
        PlayingEntity
    }
}

#[derive(Component, Default)]
pub struct Speed(pub i32);
#[derive(Component, Default)]
pub struct Strength(pub i32);
#[derive(Component, Default)]
pub struct MeleeRange(pub i32);

#[derive(Component, Default)]
pub struct TurnOrder(pub i32);

#[derive(Component)]
#[require(Speed, Strength, MeleeRange)]
pub struct Creature;

impl Creature {
    pub fn from_stats(speed: i32, strength: i32, melee_range: i32) -> impl Bundle {
        (
            Creature,
            Speed(speed),
            Strength(strength),
            MeleeRange(melee_range),
        )
    }
}
