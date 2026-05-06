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
    prelude::{DelayedDespawn, InvokedBy, SpawnDieselSubstate, SpawnSubEffect, template_repeater},
    print::PrintLn,
    spawn::TemplateRegistry,
};
use bevy_ecs::{
    event::EntityEvent,
    message::MessageWriter,
    observer::On,
    system::{Res, Single},
};
use bevy_gauge::instant;
use bevy_gearbox::{Active, GearboxMessage, InitStateMachine, SpawnTransition, StateComponent};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    GridCell,
    abilities::effects::DamageEffect,
    deck::card_builders::{CardPool, CardPoolStatus},
    game_flow::turns::{CurrentPlayingEntity, PlayingEntity},
    grid_abilities_backend::{
        AbilityHitEntity, GridGoOffConfig, GridInvokerTarget, GridSpawnConfig, GridStartInvoke,
        GridTarget, GridTargetGenerator, GridTargetMutator,
    },
    melee::MeleeEffect,
    projectiles::ProjectileEffect,
};

pub struct AbilitiesTemplatePlugin;

impl Plugin for AbilitiesTemplatePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Startup, register_templates)
            .add_observer(handle_cast);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CastInvokedBy {
    CurrentlyPlaying,
    Specific(Entity),
}

#[derive(EntityEvent)]
pub struct CastAbility {
    #[event_target]
    ability: Entity,
    target: Entity,
    invoked_by: CastInvokedBy,
}

impl CastAbility {
    pub fn new(ability: Entity, target: Entity, invoked_by: CastInvokedBy) -> Self {
        Self {
            ability,
            target,
            invoked_by,
        }
    }

    pub fn on_active_player(ability: Entity, target: Entity) -> Self {
        Self {
            ability,
            target,
            invoked_by: CastInvokedBy::CurrentlyPlaying,
        }
    }
}

fn handle_cast(
    e: On<CastAbility>,
    mut cmd: Commands,
    mut writer: MessageWriter<GridStartInvoke>,
    currently_playing: Res<CurrentPlayingEntity>,
    cells_q: Query<&GridNode, With<GridCell>>,
    playing_q: Query<&CartesianPosition, With<PlayingEntity>>,
    grid: Single<&mut CartesianGrid<Cartesian3D>>,
) {
    let invoker = match e.invoked_by {
        CastInvokedBy::CurrentlyPlaying => currently_playing.0,
        CastInvokedBy::Specific(entity) => entity,
    };
    let ability = e.ability;
    let target = e.target;

    let target_position = cells_q.get(target).map_or_else(
        |_| {
            *playing_q
                .get(target)
                .expect("Target should be a GridCell or PlayingEntity")
        },
        |node| grid.pos_from_index(node.0),
    );

    cmd.entity(ability).insert(InvokedBy(invoker));
    cmd.entity(invoker)
        .insert(GridInvokerTarget::entity(target, target_position));

    let grid_target = GridTarget::entity(target, target_position);
    writer.write(GridStartInvoke::new(ability, grid_target));
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

fn register_templates(mut registry: ResMut<TemplateRegistry>) {
    registry.register("projectile", projectile_template);
    registry.register("melee", melee_template);
    registry.register("base_projectile", basic_projectile_ability);
    registry.register("base_melee", basic_melee_ability);
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
