use std::{any::Any, marker::PhantomData};

use bevy::{
    app::{Plugin, Update},
    ecs::{
        bundle::Bundle, component::Component, entity::Entity, event::EntityEvent, name::Name, observer::On, query::{QueryData, QueryFilter, QueryState, ReadOnlyQueryData, With}, system::{Commands, EntityCommand, EntityCommands, Query}, world::World
    },
};

use crate::{Health, HealthState};

pub struct ActionPlugin;

impl Plugin for ActionPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Update, apply_pending_effects);
    }
}

pub struct Confusion;
#[derive(Component, Debug, Default)]
pub struct Range(i32);

pub trait AsAny: Any {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Component + DamageKind> AsAny for T {
    fn as_any(&self) -> &dyn Any { self }
}

pub trait DamageKind: Any + Component {
    fn apply_damage(self) -> impl Bundle;
}

#[derive(Component, Debug, Default)]
pub struct Physical;

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Ranged(i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Melee(pub i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Piercing(i32);

#[derive(Component, Debug, Default)]
pub struct Fire(i32);

#[derive(Component, Debug, Default)]
pub struct Electric(i32);

#[derive(Component, Debug, Default)]
pub struct Water(i32);

impl DamageKind for Physical {
    fn apply_damage(self) -> impl Bundle {
        (PendingEffectTowards())
    }
}
impl DamageKind for Ranged {}
impl DamageKind for Melee {}
impl DamageKind for Piercing {}
impl DamageKind for Fire {}
impl DamageKind for Electric {}
impl DamageKind for Water {}

#[derive(Component)]
#[relationship(relationship_target = PendingEffects)]
pub struct PendingEffectTowards(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = PendingEffectTowards, linked_spawn)]
pub struct PendingEffects(Vec<Entity>);

#[derive(Component)]
pub struct Damage(pub i32);

pub fn apply_pending_effects(
    mut cmd: Commands,
    mut target_q: Query<(Entity, &PendingEffects, &mut Health)>,
    pending: Query<&Damage, With<PendingEffectTowards>>,
) {
    for (target_ent, effects, mut target_health) in &mut target_q {
        for &source_ent in &effects.0 {
            if let Ok(damage) = pending.get(source_ent) {
                println!("applygin dmg");
                if HealthState::Dead == target_health.apply_damage(damage.0) {
                    cmd.entity(target_ent).despawn();
                }
                cmd.entity(source_ent).despawn();
            }
        }
    }
}

#[derive(Component, Debug)]
pub struct MovementPoints(pub i32);

#[derive(Component, Debug, Default)]
pub struct ActionPoints(pub i32);

pub trait ActionBuilderState {}

pub struct ActionCreationStart;
impl ActionBuilderState for ActionCreationStart {}
pub struct ActionCreated;
impl ActionBuilderState for ActionCreated {}
pub struct ActionNameDefined;
impl ActionBuilderState for ActionNameDefined {}
pub struct ActionRangeDefined;
impl ActionBuilderState for ActionRangeDefined {}
pub struct ActionPointsDefined;
impl ActionBuilderState for ActionPointsDefined {}
pub struct ActionDamageDefined;
impl ActionBuilderState for ActionDamageDefined {}

pub struct ActionBuilder<'a, S: ActionBuilderState = ActionCreated> {
    _state: PhantomData<S>,
    entity: Entity,
    entity_commands: EntityCommands<'a>,
}

impl<'a, S: ActionBuilderState> ActionBuilder<'a, S> {
    pub fn get_entity(self: Self) -> Entity {
        self.entity
    }
}

impl<'a> ActionBuilder<'a, ActionCreationStart> {
    pub fn empty(commands: &'a mut Commands) -> ActionBuilder<'a, ActionCreated> {
        let entity_commands = commands.spawn(Action);
        ActionBuilder {
            _state: PhantomData,
            entity: entity_commands.id(),
            entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionCreated> {
    pub fn with_name(mut self: Self, name: &'static str) -> ActionBuilder<'a, ActionNameDefined> {
        self.entity_commands.insert(Name::new(name));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionNameDefined> {
    pub fn with_range(mut self: Self, range: i32) -> ActionBuilder<'a, ActionRangeDefined> {
        self.entity_commands.insert(Range(range));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionRangeDefined> {
    pub fn with_mp_cost(mut self: Self, movement_points: i32) -> Self {
        self.entity_commands.insert(MovementPoints(movement_points));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }

    pub fn with_ap_cost(
        mut self: Self,
        action_points: i32,
    ) -> ActionBuilder<'a, ActionPointsDefined> {
        self.entity_commands.insert(ActionPoints(action_points));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionPointsDefined> {
    pub fn with_melee_damage(
        mut self: Self,
        damage: i32,
    ) -> ActionBuilder<'a, ActionDamageDefined> {
        self.entity_commands.insert(Melee(damage));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }

    pub fn with_ranged_damage(
        mut self: Self,
        damage: i32,
    ) -> ActionBuilder<'a, ActionDamageDefined> {
        self.entity_commands.insert(Ranged(damage));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }

    pub fn with_damage_of_kind<T: DamageKind + Component>(mut self, damage_kind: T) -> Self {
        self.entity_commands.insert(damage_kind);
        self
    }
}

#[derive(Component, Debug)]
#[require(ActionPoints, Range)]
pub struct Action;

impl<'a> Action {
    pub fn spawn_empty(commands: &'a mut Commands) -> ActionBuilder<'a, ActionCreated> {
        ActionBuilder::empty(commands)
    }
}

impl Action {
    pub fn attack_entity(
        entity_cmds: &mut EntityCommands,
        action: (Entity, &Action, &Range, &ActionPoints, Option<&MovementPoints>, Option<&Physical>, Option<&Ranged>, Option<&Melee>, Option<&Piercing>, Option<&Fire>, Option<&Electric>, Option<&Water>)
    ) {
        let (action_entity, action_c, range, ap_cost, mp_cost, physical_dmg, ranged_dmg, melee_dmg, piercing_dmg, fire_dmg, electric_dmg, water_dmg) = action;


        if let Some(physical_dmg) = physical_dmg {
            physical_dmg
        }
        entity_cmds.
    }
}

#[derive(EntityEvent)]
struct ActionCast {
    entity: Entity,
}

#[derive(EntityEvent)]
struct Split {
    entity: Entity,
}

pub struct Predicate {
    validate_fn: Box<dyn FnMut(&mut World, Entity) -> bool + Send + Sync + 'static>,
}

impl Predicate {
    pub fn validate(&mut self, world: &mut World, entity: Entity) -> bool {
        (self.validate_fn)(world, entity)
    }

    pub fn new<D, F>(predicate: impl Fn(D::Item<'_, '_>) -> bool + Send + Sync + 'static) -> Self
    where
        D: ReadOnlyQueryData + 'static,
        F: QueryFilter + 'static,
    {
        let mut query_state: Option<QueryState<D, F>> = None;

        Self {
            validate_fn: Box::new(move |world: &mut World, entity: Entity| -> bool {
                let qs = query_state.get_or_insert_with(|| QueryState::<D, F>::new(world));
                qs.update_archetypes(world);
                match qs.get(world, entity) {
                    Ok(data) => predicate(data),
                    Err(_) => false,
                }
            }),
        }
    }
}

struct Reaction {
    trigger_events: Vec<ActionEffect>,
    predicates: Vec<Predicate>
}

impl Reaction {
    pub fn validate_all(&mut self, world: &mut World, entity: Entity) -> bool {
        self.predicates.iter_mut().all(|p| p.validate(world, entity))
    }

    pub fn trigger_effects(&mut self, entity: Entity, world: &mut World) {
        if self.validate_all(world, entity) {
            for effect in &self.trigger_events {
                effect.trigger_world(entity, world);
            }
        }
    }
}

pub enum ActionEffect {
    Split,
    Infuse
}

impl ActionEffect {
    pub fn trigger(self: &Self, entity: Entity, cmd: &mut Commands) {
        match self {
            ActionEffect::Split => cmd.trigger(Split {entity}),
            ActionEffect::Infuse => cmd.trigger(Split {entity}),
        }
    }

    pub fn trigger_world(self: &Self, entity: Entity, world: &mut World) {
        match self {
            ActionEffect::Split => world.trigger(Split {entity}),
            ActionEffect::Infuse => world.trigger(Split {entity}),
        }
    }
}

#[derive(Component)]
pub struct ActionEffects(Vec<ActionEffect>);

#[derive(Component)]
pub struct ActionTargets(Vec<Entity>);

pub fn setup_ex(world: &mut World) {
    world.add_observer(|e: On<ActionCast>, mut cmd: Commands, q: Query<(&ActionTargets, &ActionEffects)>| {
        let Ok((targets, effects)) = q.get(e.entity) else {
            return
        };

        for target_ent in targets.0.iter() {
            for effect in effects.0.iter() {
                effect.trigger(*target_ent, &mut cmd);
            }
        }
    });

    world.add_observer(|e: On<Split>, mut world: &mut World| {
        let p = Predicate<>;
    });
}
