use std::{collections::HashMap, default, ops::DerefMut, vec};

use bevy::{
    app::{App, FixedPostUpdate, FixedUpdate, Plugin, Startup, Update},
    asset::{AssetServer, Assets},
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
        system::{Commands, Query, Res, ResMut, Single},
    },
    input::{ButtonInput, keyboard::KeyCode},
    log::{tracing_subscriber::reload::Handle, warn},
    math::{
        UVec2, UVec3, Vec3,
        primitives::{Capsule3d, Sphere},
    },
    mesh::{Mesh, Mesh3d},
    pbr::{MeshMaterial3d, StandardMaterial},
    sprite::Text2d,
    state::{
        app::AppExtStates,
        condition::in_state,
        state::{NextState, OnEnter, State, States},
    },
    transform::components::{GlobalTransform, Transform},
    ui::Node,
};
use bevy_diesel::gauge::prelude::AttributesMut;
use bevy_diesel::prelude::Invokes;
use bevy_ecs::{
    hierarchy::ChildOf,
    lifecycle::Add,
    message::Message,
    relationship::OrderedRelationshipSourceCollection,
    schedule::{IntoScheduleConfigs, SystemCondition},
};
use bevy_flair::style::{StyleSheet, components::Styled};

use bevy_prng::WyRand;
use bevy_rand::global::GlobalRng;
use bevy_replicon::{
    client::Remote,
    postcard_utils,
    prelude::{
        ClientState, ClientTriggerExt, FromClient, Replicated, SendTargets, ServerState,
        ServerTriggerExt, ToClients,
    },
    server::ReplicationUserdata,
};
use moonshine_kind::{InsertInstance, Instance, SpawnInstance};

use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::{
    BattleData, BattleTick, BoardUtilsCommandsExt, CardDir, CardInPile, DeckDataSupplier, DrawCard,
    DrawPile, EnemyData, HandPile, InHand, MainSceneUiRoot, PendingDraw, PileWithCards, PlayerData,
    PosInDeck,
    abilities::abilities_templates::{Marker, Projectile},
    creatures::{
        definitions::{Creature, CreatureKind},
        generation::CreatureGenerationRequested,
    },
    deck::deck_and_cards::{
        ActiveDeck, CardPile, CardState, Deck, DrawHand, InDrawPile, StatelessCard,
        UnassignedDeckState,
    },
    magnetic_effect, spawn_card,
    stats::players::{MeleeRange, Speed, Strength},
};

pub struct TurnsPlugin;

pub fn in_turn_draw(
    serv_state: Res<State<ServerState>>,
    battle_global_state: Res<State<BattleGlobalState>>,
    turn_state: Res<State<BattleTurnsState>>,
) -> bool {
    match serv_state.get() {
        ServerState::Stopped => false,
        ServerState::Running => match battle_global_state.get() {
            BattleGlobalState::Running => match turn_state.get() {
                BattleTurnsState::Draw => true,
                _ => false,
            },
            _ => false,
        },
    }
}

pub fn in_turn_tick(
    serv_state: Res<State<ServerState>>,
    battle_global_state: Res<State<BattleGlobalState>>,
    turn_state: Res<State<BattleTurnsState>>,
) -> bool {
    match serv_state.get() {
        ServerState::Stopped => false,
        ServerState::Running => match battle_global_state.get() {
            BattleGlobalState::Running => match turn_state.get() {
                BattleTurnsState::Tick => true,
                _ => false,
            },
            _ => false,
        },
    }
}

pub fn in_turn_apply_effects(
    serv_state: Res<State<ServerState>>,
    battle_global_state: Res<State<BattleGlobalState>>,
    turn_state: Res<State<BattleTurnsState>>,
) -> bool {
    match serv_state.get() {
        ServerState::Stopped => false,
        ServerState::Running => match battle_global_state.get() {
            BattleGlobalState::Running => match turn_state.get() {
                BattleTurnsState::ApplyEffects => true,
                _ => false,
            },
            _ => false,
        },
    }
}

pub fn in_turn_increment(
    serv_state: Res<State<ServerState>>,
    battle_global_state: Res<State<BattleGlobalState>>,
    turn_state: Res<State<BattleTurnsState>>,
) -> bool {
    match serv_state.get() {
        ServerState::Stopped => false,
        ServerState::Running => match battle_global_state.get() {
            BattleGlobalState::Running => match turn_state.get() {
                BattleTurnsState::IncrementTurnId => true,
                _ => false,
            },
            _ => false,
        },
    }
}

pub fn should_update_subtick(
    serv_state: Res<State<ServerState>>,
    battle_global_state: Res<State<BattleGlobalState>>,
    turn_state: Res<State<BattleTurnsState>>,
) -> bool {
    match serv_state.get() {
        ServerState::Stopped => false,
        ServerState::Running => match battle_global_state.get() {
            BattleGlobalState::Running => match turn_state.get() {
                BattleTurnsState::IncrementTurnId => false,
                _ => true,
            },
            _ => false,
        },
    }
}

pub fn inside_battle(
    serv_state: Res<State<ServerState>>,
    battle_global_state: Res<State<BattleGlobalState>>,
) -> bool {
    match serv_state.get() {
        ServerState::Stopped => false,
        ServerState::Running => match battle_global_state.get() {
            BattleGlobalState::Running => true,
            _ => false,
        },
    }
}

impl Plugin for TurnsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RunData::init())
            .insert_resource(BattleData::NoBattle)
            .init_state::<BattleGlobalState>()
            .init_state::<BattleTurnsState>()
            .add_observer(handle_playing_gen_req)
            .add_observer(handle_combat_init)
            // Turn states
            .add_systems(FixedUpdate, handle_turn_draw.run_if(in_turn_draw))
            .add_systems(FixedUpdate, handle_turn_tick.run_if(in_turn_tick))
            .add_systems(
                FixedUpdate,
                apply_pending_effects.run_if(in_turn_apply_effects),
            )
            .add_systems(
                FixedPostUpdate,
                increment_battle_turn.run_if(in_turn_increment),
            )
            .add_systems(
                FixedPostUpdate,
                trigger_turn_next_state.run_if(inside_battle),
            )
            .add_observer(handle_draw_from_pile)
            .add_observer(setup_battle_context)
            .add_observer(handle_player_board_spawned)
            .add_observer(handle_enemy_board_spawned)
            .add_systems(Startup, spawn_dev_text)
            .add_systems(FixedUpdate, draw_dev_text);
    }
}

fn trigger_turn_next_state(
    curr_turn_state: Res<State<BattleTurnsState>>,
    mut next_turn_state: ResMut<NextState<BattleTurnsState>>,
) {
    next_turn_state.set(curr_turn_state.get_next_state());
}

fn apply_pending_effects(q: Query<(Entity, &PendingDraw)>, mut cmd: Commands) {
    for (e, p) in &q {
        cmd.entity(e).remove::<PendingDraw>();
        cmd.trigger(DrawCard { card: e });
    }
}

fn handle_turn_tick(q: Query<&PosInDeck>) {
    for e in &q {
        continue;
    }
}

#[derive(Event, Clone, Debug)]
pub struct BattleTriggered;

#[derive(Component, Debug, Serialize, Deserialize)]
#[require(Replicated)]
pub struct PlayerBoardMarker;

#[derive(Component, Debug, Serialize, Deserialize)]
#[require(Replicated)]
pub struct EnemyBoardMarker;

pub fn increment_battle_turn(mut battle_data: ResMut<BattleData>) {
    match &mut battle_data.into_inner() {
        BattleData::NoBattle => {}
        BattleData::InCombat { battle_tick } => {
            battle_tick.increment_next_turn();
        }
    }
}

pub fn handle_player_board_spawned(
    e: On<Add, PlayerBoardMarker>,
    asset_server: Res<AssetServer>,
    client_state: Res<State<ClientState>>,
    mut cmd: Commands,
) {
    match client_state.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }
    let hand_board_stylesheet = asset_server.load("styles/main.css");
    cmd.entity(e.entity)
        .insert(board_ui_bundle::<PlayerData>(Styled::new(
            hand_board_stylesheet.clone(),
        )));
}

pub fn handle_enemy_board_spawned(
    e: On<Add, EnemyBoardMarker>,
    asset_server: Res<AssetServer>,
    client_state: Res<State<ClientState>>,
    mut cmd: Commands,
) {
    match client_state.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }
    let hand_board_stylesheet = asset_server.load("styles/main.css");
    cmd.entity(e.entity)
        .insert(board_ui_bundle::<EnemyData>(Styled::new(
            hand_board_stylesheet.clone(),
        )));
}

fn setup_battle_context(
    _: On<BattleTriggered>,
    mut next_battle_state: ResMut<NextState<BattleGlobalState>>,
    mut cmd: Commands,
) {
    println!("setting up ui");

    // Draw piles
    let player_draw_pile = cmd
        .spawn_instance(DrawPile)
        .insert((Replicated, PileWithCards::init()))
        .instance();
    let enemy_draw_pile = cmd
        .spawn_instance(DrawPile)
        .insert((Replicated, PileWithCards::init()))
        .instance();
    // Hand piles
    let player_hand_pile = cmd
        .spawn_instance(HandPile)
        .insert((Replicated, PileWithCards::init()))
        .instance();
    let enemy_hand_pile = cmd
        .spawn_instance(HandPile)
        .insert((Replicated, PileWithCards::init()))
        .instance();
    // Ui boards
    let player_board_ui = cmd.spawn((PlayerBoardMarker, Replicated)).id();
    let enemy_board_ui = cmd.spawn((EnemyBoardMarker, Replicated)).id();

    cmd.insert_resource(PlayerData {
        ui_entity: player_board_ui,
        draw_pile: player_draw_pile,
        hand_pile: player_hand_pile,
    });

    cmd.insert_resource(EnemyData {
        ui_entity: enemy_board_ui,
        draw_pile: enemy_draw_pile,
        hand_pile: enemy_hand_pile,
    });

    let battle_data = BattleData::InCombat {
        battle_tick: BattleTick::initial(),
    };

    let mut message: Vec<u8> = Vec::new();
    postcard_utils::to_extend_mut(&battle_data, &mut message)
        .expect("Could not serialize battle data");
    cmd.insert_resource(ReplicationUserdata(message));
    cmd.insert_resource(battle_data.clone());

    cmd.trigger(EnteredCombat);
    next_battle_state.set(BattleGlobalState::Init);
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
        .spawn((Name::new("Some player"), PlayingEntity::new_ally()))
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

pub fn board_ui_bundle<D: DeckDataSupplier>(styles: Styled) -> impl Bundle {
    (
        Node::default(),
        MainSceneUiRoot::<D>::new(),
        Replicated,
        styles,
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, States, Default, PartialEq, Eq, Hash)]
pub enum BattleGlobalState {
    #[default]
    OutOfCombat,
    Init,
    AwaitingClient,
    Running,
    Ended,
}

#[derive(Clone, Debug, Serialize, Deserialize, States, Default, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum BattleTurnsState {
    #[default]
    Draw,
    Tick,
    ApplyEffects,
    IncrementTurnId,
}

impl BattleTurnsState {
    pub fn get_next_state(&self) -> BattleTurnsState {
        match self {
            BattleTurnsState::Draw => BattleTurnsState::Tick,
            BattleTurnsState::Tick => BattleTurnsState::ApplyEffects,
            BattleTurnsState::ApplyEffects => BattleTurnsState::IncrementTurnId,
            BattleTurnsState::IncrementTurnId => BattleTurnsState::Draw,
        }
    }
}

#[derive(Resource)]
pub struct RunData {
    pub current_day: i32,
    pub current_hour: i32,
    pub wins: i32,
    pub loses: i32,
}

impl RunData {
    pub fn init() -> Self {
        Self {
            current_day: 0,
            current_hour: 0,
            wins: 0,
            loses: 0,
        }
    }
}

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = RequirementOfPathOpt, linked_spawn)]
pub struct PathOption {
    requirements: Vec<Entity>,
}

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = PathOption)]
pub struct RequirementOfPathOpt(Entity);

#[derive(Event, Debug, Clone, Serialize, Deserialize)]
pub struct CheckClientBattleReady;

#[derive(Event, Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmBattleReady;

fn handle_combat_init(
    _: On<EnteredCombat>,
    player_deck: Res<PlayerData>,
    enemy_deck: Res<EnemyData>,
    curr_battle_state: Res<State<BattleGlobalState>>,
    mut next_battle_state: ResMut<NextState<BattleGlobalState>>,
    mut cmd: Commands,
) {
    match curr_battle_state.get() {
        BattleGlobalState::AwaitingClient => {
            return;
        }
        BattleGlobalState::Running => {
            return;
        }
        _ => {}
    }

    let player_draw_pile = player_deck.draw_pile;
    let player_hand_pile = player_deck.hand_pile;
    let enemy_draw_pile = enemy_deck.draw_pile;
    let enemy_hand_pile = enemy_deck.hand_pile;

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        player_draw_pile,
        player_hand_pile,
        true,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        player_draw_pile,
        player_hand_pile,
        true,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        player_draw_pile,
        player_hand_pile,
        true,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        player_draw_pile,
        player_hand_pile,
        true,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        player_draw_pile,
        player_hand_pile,
        true,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        enemy_draw_pile,
        enemy_hand_pile,
        false,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        enemy_draw_pile,
        enemy_hand_pile,
        false,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        enemy_draw_pile,
        enemy_hand_pile,
        false,
    );

    spawn_card(
        (magnetic_effect(CardDir::Around, 1)),
        &mut cmd,
        enemy_draw_pile,
        enemy_hand_pile,
        false,
    );
    println!("combat init");

    cmd.server_trigger(ToClients {
        targets: SendTargets::CLIENTS_ONLY,
        message: CheckClientBattleReady,
    });

    next_battle_state.set(BattleGlobalState::AwaitingClient);
}

pub fn confirm_server_battle_ready(
    _: On<CheckClientBattleReady>,
    state: Res<State<ClientState>>,
    mut cmd: Commands,
) {
    match state.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }
    cmd.client_trigger(ConfirmBattleReady);
}

pub fn handle_client_confirm_battle_start(
    _: On<FromClient<ConfirmBattleReady>>,

    mut next_battle_state: ResMut<NextState<BattleGlobalState>>,
) {
    next_battle_state.set(BattleGlobalState::Running);
}

#[derive(Debug, Clone)]
pub enum DrawAmount {
    Fixed(i32),
    FillHand,
}

#[derive(EntityEvent, Clone, Debug)]
pub struct DrawFromPile {
    #[event_target]
    hand_pile: Entity,
    draw_pile: Entity,
    draw_amount: DrawAmount,
}

impl DrawFromPile {
    pub fn new(
        hand_pile: Instance<HandPile>,
        draw_pile: Instance<DrawPile>,
        draw_amount: DrawAmount,
    ) -> Self {
        Self {
            hand_pile: hand_pile.entity(),
            draw_pile: draw_pile.entity(),
            draw_amount,
        }
    }
}

fn handle_turn_draw(player_data: Res<PlayerData>, enemy_data: Res<EnemyData>, mut cmd: Commands) {
    cmd.trigger(DrawFromPile::new(
        player_data.hand_pile,
        player_data.draw_pile,
        DrawAmount::FillHand,
    ));

    cmd.trigger(DrawFromPile::new(
        enemy_data.hand_pile,
        enemy_data.draw_pile,
        DrawAmount::FillHand,
    ));
}

pub fn handle_draw_from_pile(
    e: On<DrawFromPile>,
    mut q: Query<(Entity, &mut PileWithCards)>,
    mut cmd: Commands,
) {
    let Ok([(draw_entity, mut draw_pile), (hand_entity, hand_pile)]) =
        q.get_many_mut([e.draw_pile, e.hand_pile])
    else {
        println!("FAILED, loging HAND then DRAW == >");
        cmd.entity(e.draw_pile).log_components();
        cmd.entity(e.hand_pile).log_components();
        return;
    };

    let max_hand_size: usize = 5;

    if hand_pile.len() >= max_hand_size {
        return;
    }

    let max_draw_amount = match e.draw_amount {
        DrawAmount::Fixed(amount) => amount,
        DrawAmount::FillHand => max_hand_size as i32,
    };

    for _ in 0..max_draw_amount {
        if draw_pile.is_empty() {
            break;
        }
        let drawn = draw_pile.0.pop_front().unwrap();
        cmd.entity(drawn)
            .insert((JustDrawn, InHand, CardInPile(hand_entity)));
    }
}

#[derive(Component, Debug, Clone)]
pub struct JustDrawn;

pub fn start_combat_test(mut cmd: Commands, keyboard_input: Res<ButtonInput<KeyCode>>) {
    if !keyboard_input.just_pressed(KeyCode::KeyC) {
        return;
    }

    cmd.trigger(CombatInit);
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

#[derive(Resource)]
pub struct DevText(String);

#[derive(Component)]
pub struct DevTextTarget;

#[derive(Resource)]
pub struct CurrentPlayingEntity(pub Entity);

#[derive(Resource)]
pub struct _CombatData {
    pub current_turn: u16,
    pub entities_next_turn: HashMap<Entity, u16>,
    turn_ended: bool,
}

impl _CombatData {
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
pub struct EnteredCombat;

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

// #[derive(Clone, Debug, Component)]
// pub enum TeamHitFilter {
//     Enemies,
//     Allies,
// }

// impl HitFilter for TeamHitFilter {
//     type Lookup = PlayingTeam;

//     fn can_target(&self, invoker: Option<&Self::Lookup>, target: Option<&Self::Lookup>) -> bool {
//         match (self, invoker, target) {
//             (TeamHitFilter::Enemies, Some(i), Some(t)) => i != t,
//             _ => true, // no team info → allow (e.g. hitting terrain)
//         }
//     }
// }

#[derive(Component, Default)]
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
