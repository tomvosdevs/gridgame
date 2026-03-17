use std::marker::PhantomData;

use bevy::{
    app::{Plugin, Update},
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        name::Name,
        query::With,
        system::{Commands, EntityCommand, EntityCommands, Query},
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

pub trait DamageKind {}

#[derive(Component, Debug, Default)]
pub struct Physical(i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Ranged(i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Melee(i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Piercing(i32);

#[derive(Component, Debug, Default)]
pub struct Fire(i32);

#[derive(Component, Debug, Default)]
pub struct Electric(i32);

#[derive(Component, Debug, Default)]
pub struct Water(i32);

impl DamageKind for Physical {}
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
