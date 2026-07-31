use std::collections::HashMap;

use bevy::{
    app::{App, Plugin, Startup, Update},
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
    transform::components::{GlobalTransform, Transform},
    ui::Node,
};
use bevy_diesel::prelude::Invokes;
use bevy_ecs::{hierarchy::ChildOf, message::Message};
use bevy_flair::style::{StyleSheet, components::NodeStyleSheet};
use bevy_gauge::prelude::AttributesMut;
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
use moonshine_kind::{Instance, SpawnInstance};

use rand::RngExt;

use crate::{
    BoardUtilsCommandsExt, CardDir, CardInPile, CardsPile, DeckDataSupplier, DrawCard, EnemyData,
    MainSceneUiRoot, PlayerData,
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
    utils::AsFlippedUVec3,
};

pub struct TurnsPlugin;

impl Plugin for TurnsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RunData::init())
            .add_systems(
                Startup,
                |mut cmd: Commands, asset_server: Res<AssetServer>| {
                    println!("setting up ui");
                    let ui_styles = asset_server.load("styles/main.css");

                    // Draw piles
                    let player_draw_pile = cmd.spawn_instance(CardsPile::init()).instance();
                    let enemy_draw_pile = cmd.spawn_instance(CardsPile::init()).instance();
                    // Hand piles
                    let player_hand_pile = cmd.spawn_instance(CardsPile::init()).instance();
                    let enemy_hand_pile = cmd.spawn_instance(CardsPile::init()).instance();
                    // Ui boards
                    let player_board_ui = cmd.spawn_empty().id();
                    let enemy_board_ui = cmd.spawn_empty().id();

                    cmd.insert_resource(PlayerData {
                        ui_entity: player_board_ui,
                        draw_pile_entity: player_draw_pile,
                        hand_pile_entity: player_hand_pile,
                    });
                    cmd.insert_resource(EnemyData {
                        ui_entity: enemy_board_ui,
                        draw_pile_entity: enemy_draw_pile,
                        hand_pile_entity: enemy_hand_pile,
                    });

                    cmd.entity(player_board_ui)
                        .insert(board_ui_bundle::<PlayerData>(NodeStyleSheet::new(
                            ui_styles.clone(),
                        )));
                    cmd.entity(enemy_board_ui)
                        .insert(board_ui_bundle::<EnemyData>(NodeStyleSheet::new(
                            ui_styles.clone(),
                        )));

                    cmd.trigger(EnteredCombat);
                },
            )
            .add_observer(handle_playing_gen_req)
            // .add_observer(spawn_combat_playing_entities)
            .add_observer(handle_combat_start)
            .add_observer(handle_turn_start)
            .add_observer(handle_fill_hand)
            .add_observer(handle_turn_end)
            .add_systems(Update, handle_battle_init)
            .add_systems(Startup, spawn_dev_text)
            .add_systems(Update, draw_dev_text);
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

pub fn board_ui_bundle<D: DeckDataSupplier>(styles: NodeStyleSheet) -> impl Bundle {
    (Node::default(), MainSceneUiRoot::<D>::new(), styles)
}

#[derive(Resource)]
pub struct BattleData {
    pub is_player_turn: bool,
}

impl BattleData {
    pub fn new() -> Self {
        Self {
            is_player_turn: true,
        }
    }

    pub fn get_current_turn_kind(&self) -> TurnKind {
        match self.is_player_turn {
            true => TurnKind::Player,
            false => TurnKind::Enemy,
        }
    }

    pub fn get_trigger_next_turn_kind(&mut self) -> TurnKind {
        self.is_player_turn = !self.is_player_turn;
        match self.is_player_turn {
            true => TurnKind::Player,
            false => TurnKind::Enemy,
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

fn handle_combat_start(
    _: On<EnteredCombat>,
    mut cmd: Commands,
    player_deck: Res<PlayerData>,
    enemy_deck: Res<EnemyData>,
) {
    let player_draw_pile = player_deck.draw_pile_entity;
    let player_hand_pile = player_deck.hand_pile_entity;
    let enemy_draw_pile = enemy_deck.draw_pile_entity;
    let enemy_hand_pile = enemy_deck.hand_pile_entity;

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
}

pub fn handle_battle_init(battle_data: Option<Res<BattleData>>, mut cmd: Commands) {
    let Some(battle_data) = battle_data else {
        return;
    };

    if battle_data.is_added() {
        match battle_data.get_current_turn_kind() {
            TurnKind::Player => cmd.trigger(EntityTurnStart::player()),
            TurnKind::Enemy => cmd.trigger(EntityTurnStart::enemy()),
        }
    }
}

#[derive(EntityEvent, Clone, Debug)]
pub struct DrawFillHand {
    #[event_target]
    hand_pile: Entity,
    draw_pile: Entity,
    is_player: bool,
}

impl DrawFillHand {
    pub fn new(
        hand_pile: Instance<CardsPile>,
        draw_pile: Instance<CardsPile>,
        is_player: bool,
    ) -> Self {
        Self {
            hand_pile: hand_pile.entity(),
            draw_pile: draw_pile.entity(),
            is_player,
        }
    }
}

fn handle_turn_start(
    e: On<EntityTurnStart>,
    player_data: Res<PlayerData>,
    enemy_data: Res<EnemyData>,
    mut cmd: Commands,
) {
    let (hand_pile, draw_pile, is_player) = match e.turn_kind {
        TurnKind::Player => (
            player_data.hand_pile_entity,
            player_data.draw_pile_entity,
            true,
        ),
        TurnKind::Enemy => (
            enemy_data.hand_pile_entity,
            enemy_data.draw_pile_entity,
            false,
        ),
    };

    println!("COMPS LOG on CURR draw pile : ");
    cmd.entity(draw_pile.entity()).log_components();

    cmd.trigger(DrawFillHand::new(hand_pile, draw_pile, is_player));
}

pub fn handle_fill_hand(e: On<DrawFillHand>, q: Query<&CardsPile>, mut cmd: Commands) {
    let draw_pile = q
        .get(e.draw_pile)
        .expect("Should find 'CardsPile' Comp on pile to draw from entity");
    let hand_pile = q
        .get(e.hand_pile)
        .expect("Should find 'CardsPile' Comp on pile to draw from entity");

    let max_hand_size: usize = 3;

    if hand_pile.len() >= max_hand_size {
        return;
    }

    let left_to_draw_count = max_hand_size - hand_pile.len();

    for i in 0..left_to_draw_count {
        let Some(card) = draw_pile.0.get(i) else {
            return;
        };

        cmd.request_game_event(DrawCard {
            card: *card,
            is_player: e.is_player,
        });
    }

    cmd.request_event(EntityTurnEnd(e.hand_pile));
}

fn handle_turn_end(_: On<EntityTurnEnd>, mut battle_data: ResMut<BattleData>, mut cmd: Commands) {
    println!("TURN ENDED");
    match battle_data.get_trigger_next_turn_kind() {
        TurnKind::Player => cmd.trigger(EntityTurnStart::player()),
        TurnKind::Enemy => cmd.trigger(EntityTurnStart::enemy()),
    }
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum TurnKind {
    Player,
    Enemy,
}

#[derive(Clone, Event)]
pub struct EntityTurnStart {
    pub turn_kind: TurnKind,
}

impl EntityTurnStart {
    pub fn player() -> Self {
        Self {
            turn_kind: TurnKind::Player,
        }
    }

    pub fn enemy() -> Self {
        Self {
            turn_kind: TurnKind::Enemy,
        }
    }
}

#[derive(EntityEvent, Clone, Message)]
pub struct EntityTurnEnd(Entity);

#[derive(Event)]
pub struct GlobalTurnStart;

#[derive(Event)]
pub struct GlobalTurnEnd;

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
