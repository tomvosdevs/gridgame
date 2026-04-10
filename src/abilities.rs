use std::{any::type_name, collections::HashMap, marker::PhantomData};

use bevy::{
    app::{Plugin, Startup},
    camera::visibility::Visibility,
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        name::Name,
        system::{Commands, EntityCommands, ResMut},
    },
};
use bevy_diesel::{
    effect::GoOffConfig,
    invoke::Ability,
    prelude::{
        DelayedDespawn, RequiresStatsOf, SpawnDieselSubstate, SpawnSubEffect, template_single_shot,
    },
    print::PrintLn,
    spawn::TemplateRegistry,
};
use bevy_gearbox::{GearboxMessage, InitStateMachine, SpawnTransition, StateComponent};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::CartesianPosition;

use crate::{
    grid_abilities_backend::{
        AbilityHitEntity, GridGoOffConfig, GridPosOffset, GridSpawnConfig, GridStartInvoke,
        GridTargetGenerator, GridTargetMutator,
    },
    projectiles::ProjectileEffect,
    states::TeamHitFilter,
};

pub struct AbilitiesTemplatePlugin;

impl Plugin for AbilitiesTemplatePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Startup, register_templates);
    }
}

// UTIL => Either retrieves the entity if this isn't the first template or creates one
pub trait AbilitiesCommands {
    fn get_or_spawn_new(self: &mut Self, entity: Option<Entity>) -> Entity;

    fn existing_or_new_cmds(self: &mut Self, entity: Option<Entity>) -> EntityCommands<'_>;
}

impl<'w, 's> AbilitiesCommands for Commands<'w, 's> {
    fn get_or_spawn_new(self: &mut Self, entity: Option<Entity>) -> Entity {
        entity.unwrap_or_else(|| self.spawn_empty().id())
    }

    fn existing_or_new_cmds(self: &mut Self, entity: Option<Entity>) -> EntityCommands<'_> {
        let entity = entity.unwrap_or_else(|| self.spawn_empty().id());
        self.entity(entity)
    }
}

pub trait AbilityBuilderState {}

pub struct CommandsMissing;
impl AbilityBuilderState for CommandsMissing {}

pub struct CommandsPassed;
impl AbilityBuilderState for CommandsPassed {}

pub struct AbilityTemplateBuilder<'w, 's, 'c, T: AbilityBuilderState> {
    commands: Option<&'c mut Commands<'w, 's>>,
    root_name: &'static str,
    root_entity: Entity,
    entities_hierarchy: HashMap<&'static str, Entity>,
    _marker: PhantomData<T>,
}

impl<'w, 's, 'c> AbilityTemplateBuilder<'w, 's, 'c, CommandsMissing> {
    pub fn new(
        commands: &'c mut Commands<'w, 's>,
        root_name: &'static str,
        root_entity: Option<Entity>,
    ) -> AbilityTemplateBuilder<'w, 's, 'c, CommandsPassed> {
        let mut hierarchy = HashMap::new();
        let root = root_entity.unwrap_or_else(|| commands.spawn_empty().id());
        hierarchy.insert(root_name, root);
        AbilityTemplateBuilder {
            commands: Some(commands),
            root_name,
            root_entity: root,
            entities_hierarchy: hierarchy,
            _marker: PhantomData,
        }
    }
}

impl<'w, 's, 'c> AbilityTemplateBuilder<'w, 's, 'c, CommandsPassed> {
    pub fn with_substate(mut self: Self, path: &'static str, bundle: impl Bundle) -> Self {
        let parts: Vec<&str> = path.split("/").take(2).collect();
        let (parent, name) = match parts.as_slice() {
            [name] => (self.root_name, *name),
            [parent, name] => (*parent, *name),
            _ => unreachable!(),
        };

        println!(
            "name : {:?} - parent : {:?} - curr: {:?}",
            name, parent, self.entities_hierarchy
        );

        let entry_entity = *self.entities_hierarchy.get(parent).unwrap();

        let mut new_state_entity = Entity::PLACEHOLDER;

        self.commands
            .as_mut()
            .unwrap()
            .entity(self.root_entity)
            .with_children(|children| {
                new_state_entity = children
                    .spawn_diesel_substate(entry_entity, (Name::new(name), bundle))
                    .id();
            });

        self.entities_hierarchy.insert(name, new_state_entity);

        self
    }

    pub fn with_subeffect(mut self: Self, path: &'static str, bundle: impl Bundle) -> Self {
        let parts: Vec<&str> = path.split("/").take(2).collect();
        let (parent, name) = match parts.as_slice() {
            [name] => (self.root_name, *name),
            [parent, name] => (*parent, *name),
            _ => unreachable!(),
        };

        let entry_entity = *self.entities_hierarchy.get(parent).unwrap();

        let mut new_effect_entity = Entity::PLACEHOLDER;
        self.commands
            .as_mut()
            .unwrap()
            .entity(self.root_entity)
            .with_children(|children| {
                new_effect_entity = children
                    .spawn_subeffect(entry_entity, (Name::new(name), bundle))
                    .id();
            });

        // println!("inserted effect with comps =>");
        // self.commands
        //     .as_mut()
        //     .unwrap()
        //     .entity(new_effect_entity)
        //     .log_components();

        self.entities_hierarchy.insert(name, new_effect_entity);

        self
    }

    pub fn with_state_transition<M: GearboxMessage>(
        mut self: Self,
        from: &'static str,
        to: &'static str,
    ) -> Self {
        let from_entity = *self.entities_hierarchy.get(from).unwrap();
        let to_entity = *self.entities_hierarchy.get(to).unwrap();

        self.commands
            .as_mut()
            .unwrap()
            .entity(self.root_entity)
            .with_children(|children| {
                children.spawn_transition::<M>(from_entity, to_entity);
            });

        self
    }

    pub fn with_state_transition_always(
        mut self: Self,
        from: &'static str,
        to: &'static str,
    ) -> Self {
        let from_entity = *self.entities_hierarchy.get(from).unwrap();
        let to_entity = *self.entities_hierarchy.get(to).unwrap();

        self.commands
            .as_mut()
            .unwrap()
            .entity(self.root_entity)
            .with_children(|children| {
                children.spawn_transition_always(from_entity, to_entity);
            });

        self
    }

    pub fn with_initial_root(
        mut self: Self,
        bundle: impl Bundle,
        state_name: &'static str,
    ) -> Self {
        let state_entity = *self.entities_hierarchy.get(state_name).unwrap();

        self.commands
            .as_mut()
            .unwrap()
            .entity(self.root_entity)
            .insert(bundle)
            .init_state_machine(state_entity);

        self
    }

    pub fn get_root_entity(self: Self) -> Entity {
        self.root_entity
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

fn register_templates(mut registry: ResMut<TemplateRegistry>) {
    registry.register("projectile_ability", basic_projectile_ability);
    registry.register("projectile", projectile_template);
    registry.register("projectile", projectile_template);
}

pub fn projectile_template(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let flying = parent
            .spawn_diesel_substate(entity, Name::new("Flying"))
            .id();

        // let hit = parent.spawn_diesel_substate(entity, Name::new("Hit")).id();
        //
        let done = parent
            .spawn_diesel_substate(
                entity,
                (Name::new("Done"), StateComponent(DelayedDespawn::now())),
            )
            .id();

        parent.spawn_transition::<AbilityHitEntity>(flying, done);

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

    println!("projectile components :  -->");
    // commands.entity(entity).log_components();

    entity
}

pub fn test_some_ability(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let active = parent
            .spawn_diesel_substate(entity, Name::new("Active"))
            .id();

        let spawn = parent
            .spawn_diesel_substate(
                entity,
                (Name::new("Spawn"), GridGoOffConfig::invoker_target()),
            )
            .id();

        parent.spawn_subeffect(
            spawn,
            (
                Name::new("EffectSpawn"),
                GridSpawnConfig::passed("projectile"),
            ),
        );

        let done = parent.spawn_diesel_substate(entity, Name::new("Done")).id();

        parent.spawn_transition::<GridStartInvoke>(active, spawn);
        parent.spawn_transition::<GridStartInvoke>(spawn, done);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert(Ability)
            .init_state_machine(active);
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
            .spawn_diesel_substate(
                entity,
                (
                    Name::new("Invoke"),
                    GridGoOffConfig::invoker_target(),
                    PrintLn::new("Invoke GoOff:"),
                ),
            )
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
        parent.spawn_transition_always(invoke, ready);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert(Ability)
            .init_state_machine(ready);
    });

    entity
}
