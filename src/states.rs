use bevy::{
    app::{App, Plugin, Startup},
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        event::{EntityEvent, Event},
        observer::On,
        query::With,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query},
    },
    state::state::States,
};

pub struct TurnsPlugin;

impl Plugin for TurnsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(
            |_: On<StartCombat>,
             mut cmd: Commands,
             q: Query<(Entity, &Speed), With<PlayingEntity>>| {
                let entities_by_turn_order: Vec<Entity> = q
                    .iter()
                    .sort_by::<&Speed>(|val1, val2| val2.0.cmp(&val1.0))
                    .map(|(ent, _)| ent)
                    .collect();

                for (idx, ent) in entities_by_turn_order.iter().enumerate() {
                    if idx == 0 {
                        cmd.trigger(StartTurn { entity: *ent });
                    }
                    cmd.entity(*ent).insert(TurnOrder(idx as i32));
                }
            },
        )
        .add_observer(|e: On<StartTurn>, mut cmd: Commands| {
            cmd.insert_resource(CurrentPlayingEntity(e.entity));
            println!("turn started on : {:?}", e.entity);
        })
        .add_systems(
            Startup,
            (spawn_test_playing_entities, start_combat_test).chain(),
        );
    }
}

#[derive(Resource)]
pub struct CurrentPlayingEntity(pub Entity);

#[derive(Event)]
pub struct StartCombat;

#[derive(EntityEvent)]
pub struct StartTurn {
    entity: Entity,
}

#[derive(EntityEvent)]
pub struct EndTurn {
    entity: Entity,
}

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

#[derive(Component, Default)]
pub struct PlayingEntity;

impl PlayingEntity {
    pub fn new_ally() -> impl Bundle {
        (PlayingEntity, MemberOf::<0>)
    }

    pub fn new_ennemy() -> impl Bundle {
        (PlayingEntity, MemberOf::<1>)
    }

    pub fn new_environmental() -> impl Bundle {
        (PlayingEntity, MemberOf::<2>)
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

pub fn spawn_test_playing_entities(mut cmd: Commands) {
    cmd.spawn((PlayingEntity::new_ally(), Creature::from_stats(6, 3, 1)));
    cmd.spawn((PlayingEntity::new_ally(), Creature::from_stats(1, 3, 1)));
    cmd.spawn((PlayingEntity::new_ennemy(), Creature::from_stats(5, 4, 1)));
}

pub fn start_combat_test(mut cmd: Commands) {
    cmd.trigger(StartCombat);
}
