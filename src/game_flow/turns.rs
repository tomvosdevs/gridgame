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
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::prelude::Invokes;
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
use pyri_state::{
    access::NextMut,
    pattern::StatePattern,
    prelude::{State, StateFlush},
    setup::AppExtState,
};
use rand::RngExt;

use crate::{
    abilities::abilities_templates::{Marker, Projectile},
    creatures::{
        definitions::{Creature, CreatureKind},
        generation::CreatureGenerationRequested,
    },
    deck::deck_and_cards::{
        ActiveDeck, CardPile, CardState, Deck, DrawHand, InDrawPile, StatelessCard,
        UnassignedDeckState,
    },
    stats::players::{MeleeRange, Speed, Strength},
    utils::AsFlippedUVec3,
};

pub struct TurnsPlugin;

impl Plugin for TurnsPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .init_state::<CombatState>()
            .add_observer(handle_playing_gen_req)
            // .add_observer(spawn_combat_playing_entities)
            .add_observer(handle_combat_start)
            .add_observer(handle_turn_start)
            .add_observer(handle_turn_end)
            .add_systems(
                StateFlush,
                GameState::InCombat.on_enter(request_test_playing_gen),
            )
            .add_systems(Startup, spawn_dev_text)
            .add_systems(Update, draw_dev_text)
            .add_systems(
                Update,
                GameState::InCombat.on_update((keyboard_update_turn_test, start_combat_test)),
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
    playing_q: Query<Entity, With<PlayingEntity>>,
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

    // cmd.insert_resource(CombatData::init_new_combat(&entities_by_turn_order));
    for (idx, ent) in playing_q.iter().enumerate() {
        cmd.entity(ent).insert(TurnOrder(idx as i32));
        if idx == 0 {
            cmd.trigger(EntityTurnStart { entity: ent });
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

#[derive(EntityEvent, Clone)]
pub struct EntityTurnStart {
    pub entity: Entity,
}

#[derive(EntityEvent, Clone)]
pub struct EntityTurnEnd {
    pub entity: Entity,
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
