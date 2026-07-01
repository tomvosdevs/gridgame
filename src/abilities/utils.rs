use bevy::app::{App, Plugin, Startup};
use bevy_diesel::{
    invoke::Ability,
    prelude::{InvokedBy, SpawnDieselSubstate, SpawnSubEffect},
    target::InvokerTarget,
};
use bevy_ecs::{
    component::Component,
    entity::Entity,
    hierarchy::ChildSpawnerCommands,
    message::MessageWriter,
    name::Name,
    observer::On,
    query::With,
    system::{Commands, Query},
};
use bevy_gearbox::{InitStateMachine, SpawnTransition};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::CartesianPosition;

use crate::{
    abilities::abilities_templates::AttachedToPlayer,
    game_flow::turns::{EntityTurnStart, PlayingEntity},
    grid_abilities_backend::{GridSpawnConfig, GridStartInvoke, GridTarget, GridTargetGenerator},
};

pub struct AbilityComposingPlugin;

impl Plugin for AbilityComposingPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(
            |e: On<EntityTurnStart>,
             q: Query<(Entity, &CartesianPosition), With<PlayingEntity>>,
             mut writer: MessageWriter<GridStartInvoke>,
             mut cmd: Commands| {
                let entity = e.entity;
                let player_grid_pos = q
                    .get(entity)
                    .expect("Starting turn player entity should have a CartesianPos")
                    .1;

                let (target_entity, target_pos) = q
                    .iter()
                    .find(|(e, _)| *e != entity)
                    .expect("should find at least one other player entity");

                cmd.entity(entity)
                    .insert(InvokerTarget::entity(target_entity, *target_pos));

                let ability_entity = create_base_ability_entity(&mut cmd, entity);

                basic_projectile_card(&mut cmd, ability_entity);
                writer.write(GridStartInvoke::new(
                    ability_entity,
                    GridTarget::entity(target_entity, *target_pos),
                ));
                println!("NEWARCH --- {:?}", player_grid_pos);
            },
        );
    }
}

/// Some types that allow you to insert or alter state machines by passing in functions.
pub type Payload = Box<dyn FnOnce(&mut ChildSpawnerCommands, Entity) + Send + Sync>;
pub type Control = Box<dyn FnOnce(&mut Commands, Entity, Payload) + Send + Sync>;
pub type Augment = Box<dyn FnOnce(&mut Commands, Entity) + Send + Sync>;

pub fn configure_projectile_spawn(
    parent: &mut ChildSpawnerCommands,
    entity: Entity,
    id: &'static str,
) {
    let ready = parent
        .spawn_diesel_substate(entity, Name::new("Ready"))
        .id();
    let invoke = parent
        .spawn_diesel_substate(entity, Name::new("Invoke"))
        .id();

    parent.spawn_subeffect(
        invoke,
        (
            Name::new("SpawnProjectile"),
            GridSpawnConfig::invoker(id)
                .with_target_generator(GridTargetGenerator::at_invoker_target()),
        ),
    );

    parent.spawn_transition::<GridStartInvoke>(ready, invoke);
    parent.spawn_transition_always(invoke, ready);

    let commands = parent.commands_mut();
    commands
        .entity(entity)
        .insert(Ability)
        .init_state_machine(ready);
}

pub fn configure_melee_spawn(parent: &mut ChildSpawnerCommands, entity: Entity, id: &'static str) {
    let ready = parent
        .spawn_diesel_substate(entity, Name::new("Ready"))
        .id();
    let invoke = parent
        .spawn_diesel_substate(entity, Name::new("Invoke"))
        .id();

    parent.spawn_subeffect(
        invoke,
        (
            Name::new("SpawnMelee"),
            GridSpawnConfig::invoker(id)
                .with_target_generator(GridTargetGenerator::at_invoker_target()),
        ),
    );

    parent.spawn_transition::<GridStartInvoke>(ready, invoke);
    parent.spawn_transition_always(invoke, ready);

    let commands = parent.commands_mut();
    commands
        .entity(entity)
        .insert(Ability)
        .init_state_machine(ready);
}

pub fn fire_and_cooldown_template(cmd: &mut Commands, entity: Entity, payload: Payload) {
    // TODO : add proper colldown logic here
    cmd.entity(entity)
        .with_children(|parent| (payload)(parent, entity));
}

/// Payload constructors
pub fn projectile(id: &'static str) -> Payload {
    println!("NEWARCH --- Spawning projectile payload");
    Box::new(move |parent, entity| configure_projectile_spawn(parent, entity, id))
}

pub fn melee(id: &'static str) -> Payload {
    Box::new(move |parent, entity| configure_projectile_spawn(parent, entity, id))
}

/// Control constructors
pub fn fire_and_cooldown() -> Control {
    Box::new(|cmd, entity, payload| {
        fire_and_cooldown_template(cmd, entity, payload);
    })
}

// pub fn ripple(count: u32) -> Control {
//     Box::new(move |cmd, entity, payload| {
//         ripple_template(cmd, entity, count, payload);
//     })
// }

#[derive(Component, Clone, Copy, Debug)]
pub struct SplashOnHit {
    pub radius: f32,
}

/// Augment constructor
pub fn splash(radius: f32) -> Augment {
    Box::new(move |cmd, entity| {
        cmd.entity(entity).insert(SplashOnHit { radius });
    })
}
/// Builder brings it all together:
#[derive(Component)]
pub struct AbilityBuilder {
    control: Control,
    payload: Payload,
    augments: Vec<Augment>,
}

impl AbilityBuilder {
    pub fn new(control: Control, payload: Payload) -> Self {
        Self {
            control,
            payload,
            augments: vec![],
        }
    }

    pub fn with(mut self, augment: Augment) -> Self {
        self.augments.push(augment);
        self
    }

    pub fn build(self, cmd: &mut Commands, entity: Entity) {
        (self.control)(cmd, entity, self.payload);
        for augment in self.augments.into_iter() {
            (augment)(cmd, entity);
        }
    }
}

pub fn create_base_ability_entity(cmd: &mut Commands, parent_entity: Entity) -> Entity {
    let child = cmd
        .spawn((
            InvokedBy(parent_entity),
            Ability,
            AttachedToPlayer(parent_entity),
        ))
        .id();
    cmd.entity(parent_entity).add_child(child);
    child
}

/// Some abilities
pub fn basic_projectile_card(cmd: &mut Commands, entity: Entity) {
    AbilityBuilder::new(fire_and_cooldown(), projectile("projectile")).build(cmd, entity);
}

pub fn basic_melee_card(cmd: &mut Commands, entity: Entity) {
    AbilityBuilder::new(fire_and_cooldown(), melee("melee")).build(cmd, entity);
}

// pub fn ripple_projectile(cmd: &mut Commands, entity: Entity, count: u32) {
//     AbilityBuilder::new(ripple(count), projectile("projectile")).build(cmd, entity);
// }

pub fn splash_projectile(cmd: &mut Commands, entity: Entity, radius: f32) {
    AbilityBuilder::new(fire_and_cooldown(), projectile("projectile"))
        .with(splash(radius))
        .build(cmd, entity);
}
