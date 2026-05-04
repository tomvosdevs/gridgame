use std::{any::type_name, collections::HashMap, marker::PhantomData};

use bevy::{
    app::{Plugin, Startup, Update},
    camera::visibility::Visibility,
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        name::Name,
        query::With,
        resource::Resource,
        system::{Commands, EntityCommands, Query, ResMut},
    },
};
use bevy_diesel::{
    invoke::Ability,
    prelude::{DelayedDespawn, SpawnDieselSubstate, SpawnSubEffect, template_repeater},
    print::PrintLn,
    spawn::TemplateRegistry,
};
use bevy_gauge::instant;
use bevy_gearbox::{Active, GearboxMessage, InitStateMachine, SpawnTransition, StateComponent};
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    abilities::effects::DamageEffect,
    deck::card_builders::{CardPool, CardPoolStatus},
    grid_abilities_backend::{
        AbilityHitEntity, GridGoOffConfig, GridSpawnConfig, GridStartInvoke, GridTargetGenerator,
        GridTargetMutator,
    },
    melee::MeleeEffect,
    projectiles::ProjectileEffect,
};

pub struct AbilitiesTemplatePlugin;

impl Plugin for AbilitiesTemplatePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(AbilityTemplateRegistry::default())
            .add_systems(Startup, register_templates);
    }
}

pub trait ComponentMarker {
    fn bundle() -> impl Bundle;
}

pub struct Projectile;
impl ComponentMarker for Projectile {
    fn bundle() -> impl Bundle {
        (
            Marker::<Projectile>::new(),
            Name::new(type_name::<Self>()),
            Ability,
        )
    }
}

#[derive(Component)]
pub struct Marker<T>
where
    T: ComponentMarker,
{
    marker: PhantomData<T>,
}

impl<T: ComponentMarker> Marker<T> {
    pub fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

// ==================
// ==== TEMPLATES ====
// ==================
//
pub enum AbilityTemplateKey {
    BasicProjectile,
}

pub const PROJECTILE_ABILITY: &str = "projectile_ability";

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum AbilityKind {
    Projectile,
    Melee,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum AbilityModifierKind {
    Multicast(u32),
}

impl AbilityModifierKind {
    pub fn apply(
        self: &Self,
        base_spawner_fn: &dyn Fn(&mut Commands, Option<Entity>) -> Entity,
        cmd: &mut Commands,
    ) -> Entity {
        match self {
            AbilityModifierKind::Multicast(n) => {
                let root = cmd.spawn_empty().id();
                for _ in 0_u32..*n {
                    let inner = base_spawner_fn(cmd, None);
                    cmd.entity(root).add_child(inner);
                }
                root
            }
        }
    }
}

#[derive(Resource)]
pub struct AbilityTemplateRegistry {
    templates:
        HashMap<AbilityKind, Box<dyn Fn(&mut Commands, Option<Entity>) -> Entity + Send + Sync>>,
}

impl Default for AbilityTemplateRegistry {
    fn default() -> Self {
        let mut instance = Self {
            templates: Default::default(),
        };

        // TODO : This NEEDS to have an entry for each enum entry, if it crashes this might be the reason
        instance.register_kind(AbilityKind::Projectile, basic_projectile_ability);
        instance.register_kind(AbilityKind::Melee, basic_melee_ability);

        instance
    }
}

impl AbilityTemplateRegistry {
    pub fn register_kind<F>(&mut self, kind: AbilityKind, template: F)
    where
        F: Fn(&mut Commands, Option<Entity>) -> Entity + Send + Sync + 'static,
    {
        self.templates.insert(kind, Box::new(template));
    }

    pub fn build_ability(
        &self,
        cmd: &mut Commands,
        pools: &Vec<(CardPool, CardPoolStatus)>,
        rng: &mut WyRand,
        modifiers: Vec<AbilityModifierKind>,
    ) -> Entity {
        let ability_spawner_fn = self.get_valid_ability(pools, rng);
        let mut entity: Option<Entity> = None;
        for modifier in modifiers.iter() {
            entity = Some(modifier.apply(ability_spawner_fn, cmd));
        }

        entity.unwrap_or_else(|| ability_spawner_fn(cmd, None))
    }

    pub fn get_valid_ability(
        &self,
        pools: &Vec<(CardPool, CardPoolStatus)>,
        rng: &mut WyRand,
    ) -> &Box<dyn Fn(&mut Commands, Option<Entity>) -> Entity + Send + Sync> {
        let can_melee = pools.contains(&(CardPool::Melee, CardPoolStatus::Required));
        let can_shoot = pools.contains(&(CardPool::Ranged, CardPoolStatus::Required));
        let kind = match (can_melee, can_shoot) {
            (true, true) => {
                if rng.random_bool(1.0 / 2.0) {
                    AbilityKind::Melee
                } else {
                    AbilityKind::Projectile
                }
            }
            (true, false) => AbilityKind::Melee,
            (false, true) => AbilityKind::Projectile,
            (false, false) => AbilityKind::Melee,
        };

        println!("getting ability for k : {:?}", kind);
        self.templates.get(&kind).unwrap()
    }
}

fn register_templates(mut registry: ResMut<TemplateRegistry>) {
    registry.register("projectile", projectile_template);
    registry.register("melee", melee_template);
}

pub fn projectile_template(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let flying = parent
            .spawn_diesel_substate(entity, Name::new("Flying"))
            .id();

        let hit_and_done = parent
            .spawn_diesel_substate(
                entity,
                (Name::new("Hit"), StateComponent(DelayedDespawn::now())),
            )
            .id();

        parent.spawn_subeffect(hit_and_done, DamageEffect(-3.0));

        parent.spawn_transition::<AbilityHitEntity>(flying, hit_and_done);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert((
                Name::new("BaseProjectile"),
                Marker::<Projectile>::new(),
                ProjectileEffect::new(10.0),
                Visibility::Inherited,
            ))
            .init_state_machine(flying);
    });

    entity
}

pub fn basic_projectile_ability(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let ready = parent
            .spawn_diesel_substate(entity, Name::new("Ready"))
            .id();
        let invoke = parent
            .spawn_diesel_substate(entity, (Name::new("Invoke"), PrintLn::new("Invoke GoOff:")))
            .id();

        parent.spawn_subeffect(
            invoke,
            (
                Name::new("SpawnProjectile"),
                PrintLn::new("Spawning GoOff:"),
                GridSpawnConfig::invoker("projectile")
                    .with_target_generator(GridTargetGenerator::at_invoker_target()),
            ),
        );

        parent.spawn_transition::<GridStartInvoke>(ready, invoke);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert(Ability)
            .init_state_machine(ready);
    });

    entity
}

pub fn melee_template(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let attacking = parent
            .spawn_diesel_substate(entity, Name::new("Attacking"))
            .id();

        let done = parent
            .spawn_diesel_substate(
                entity,
                (Name::new("Done"), StateComponent(DelayedDespawn::now())),
            )
            .id();

        parent.spawn_transition::<AbilityHitEntity>(attacking, done);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert((Name::new("MeleeAtk"), Visibility::Inherited, MeleeEffect))
            .init_state_machine(attacking);
    });

    entity
}

pub fn basic_melee_ability(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let ready = parent
            .spawn_diesel_substate(entity, Name::new("Ready"))
            .id();
        let invoke = parent
            .spawn_diesel_substate(entity, (Name::new("Invoke")))
            .id();

        parent.spawn_subeffect(
            invoke,
            (
                Name::new("SpawnMelee"),
                PrintLn::new("Spawning Melee GoOff:"),
                GridSpawnConfig::invoker("melee")
                    .with_target_generator(GridTargetGenerator::at_invoker_target()),
            ),
        );

        parent.spawn_transition::<GridStartInvoke>(ready, invoke);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert(Ability)
            .init_state_machine(ready);
    });

    entity
}
