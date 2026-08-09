use std::{any::type_name, io::Read, marker::PhantomData};

use bevy::{
    app::{Plugin, Startup, Update},
    camera::visibility::Visibility,
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        name::Name,
        query::With,
        system::{Commands, Query, ResMut},
    },
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::{
    invoke::Ability,
    prelude::{
        DelayedDespawn, InvokedBy, RequiresStatsOf, SpawnBranch, SpawnDieselSubstate,
        SpawnSubEffect,
    },
    print::PrintLn,
    spawn::TemplateRegistry,
};
use bevy_ecs::{
    event::EntityEvent,
    lifecycle::Add,
    message::{MessageReader, MessageWriter},
    observer::{Observer, On},
    schedule::IntoScheduleConfigs,
    system::{Res, Single},
};
use bevy_gauge::{attributes, instant, prelude::Attributes, requires};
use bevy_gearbox::{GearboxSet, InitStateMachine, SpawnSubstate, SpawnTransition, StateComponent};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    abilities::{
        effects::{AbilityOfCaster, CasterHitEffect, SpawnEffect},
        utils::AbilityComposingPlugin,
    },
    grid_abilities_backend::{
        AbilityHitEntity, BoardFilter, BoardGatherer, CastEnd, DeckGoOff, DeckGoOffConfig,
        DeckInvokerTarget, DeckSpawnConfig, DeckStartInvoke, DeckTarget, DeckTargetGenerator,
        DeckTargetMutator, EntityGatheringFilter, GridCheckShape, NumberType,
    },
    stats::players::Speed,
    utils::{CombatGridQ, IntoVec},
};

pub struct AbilitiesTemplatePlugin;

impl Plugin for AbilitiesTemplatePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(AbilityComposingPlugin)
            .add_systems(Startup, register_templates);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CastInvokedBy {
    CurrentlyPlaying,
    Specific(Entity),
}

#[derive(Component, Debug, Clone)]
pub struct ActionCastData {
    pub source_playing_entity: Entity,
    pub source_caster_entity: Entity,
}

impl ActionCastData {
    pub fn new(source_playing_entity: Entity, source_caster_entity: Entity) -> Self {
        Self {
            source_playing_entity,
            source_caster_entity,
        }
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

pub const PROJECTILE_ABILITY: &str = "projectile_ability";

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum AbilityKind {
    Projectile,
    Melee,
}

fn register_templates(mut registry: ResMut<TemplateRegistry>) {
    // TODO remove
    // registry.register("projectile", projectile_template);
    // registry.register("melee", melee_template);
    // registry.register(BaseAbility::Projectile.as_str(), basic_projectile_ability);
}

#[derive(EntityEvent)]
pub struct AbilityCastRequested {
    #[event_target]
    card_entity: Entity,
    invoked_by: CastInvokedBy,
    target: Entity,
}

impl AbilityCastRequested {
    pub fn new(card_entity: Entity, invoked_by: CastInvokedBy, target: Entity) -> Self {
        Self {
            card_entity,
            invoked_by,
            target,
        }
    }
}

#[derive(Component)]
pub struct AbilityInitialized;

#[derive(Component)]
pub struct InvokingTriggerEffect {
    pub template_entity: Entity,
    pub source: Entity,
}

impl InvokingTriggerEffect {
    pub fn new(template_entity: Entity, source: Entity) -> Self {
        Self {
            template_entity,
            source,
        }
    }
}

#[derive(Component, Debug, Clone)]
pub struct AbilityTemplate(pub Entity);

// pub fn projectile_template(commands: &mut Commands, entity: Option<Entity>) -> Entity {
//     let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

//     commands.entity(entity).with_children(|parent| {
//         let ready = parent
//             .spawn_diesel_substate(entity, (Name::new("Ready")))
//             .id();

//         let active = parent
//             .spawn_diesel_substate(entity, (Name::new("Flying")))
//             .id();

//         let hit_and_done = parent
//             .spawn_diesel_substate(
//                 entity,
//                 (Name::new("Hit"), StateComponent(DelayedDespawn::now())),
//             )
//             .id();

//         parent.spawn_transition::<AbilityHitEntity>(active, hit_and_done);

//         let commands = parent.commands_mut();
//         commands
//             .entity(entity)
//             .insert((
//                 Ability,
//                 Name::new("BaseProjectile"),
//                 Speed::new(8),
//                 AbilityInitialized,
//                 Marker::<Projectile>::new(),
//                 ProjectileEffect::new(8.0),
//                 Visibility::Inherited,
//                 GridGoOffConfig::invoker_target(),
//             ))
//             .init_state_machine(active);
//     });

//     entity
// }

// pub fn basic_projectile_ability(commands: &mut Commands, entity: Option<Entity>) -> Entity {
//     let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

//     commands.entity(entity).with_children(|parent| {
//         let ready = parent
//             .spawn_diesel_substate(entity, Name::new("Ready"))
//             .id();
//         let invoke = parent
//             .spawn_diesel_substate(entity, (Name::new("Invoke"), PrintLn::new("Invoke GoOff:")))
//             .id();

//         parent.spawn_subeffect(
//             invoke,
//             (
//                 Name::new("SpawnProjectile"),
//                 PrintLn::new("Spawning GoOff:"),
//                 GridSpawnConfig::invoker("projectile")
//                     .with_target_generator(GridTargetGenerator::at_invoker_target()),
//             ),
//         );

//         parent.spawn_transition::<GridStartInvoke>(ready, invoke);
//         parent.spawn_transition_always(invoke, ready);

//         let commands = parent.commands_mut();
//         commands
//             .entity(entity)
//             .insert(Ability)
//             .init_state_machine(ready);
//     });

//     entity
// }

// pub fn basic_melee_ability(commands: &mut Commands, entity: Option<Entity>) -> Entity {
//     let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

//     commands.entity(entity).with_children(|parent| {
//         let ready = parent
//             .spawn_diesel_substate(entity, Name::new("Ready"))
//             .id();
//         let invoke = parent
//             .spawn_diesel_substate(entity, (Name::new("Invoke")))
//             .id();

//         parent.spawn_subeffect(
//             invoke,
//             (
//                 Name::new("SpawnMelee"),
//                 PrintLn::new("Spawning Melee GoOff:"),
//                 GridSpawnConfig::invoker("melee")
//                     .with_target_generator(GridTargetGenerator::at_invoker_target()),
//             ),
//         );

//         parent.spawn_transition::<GridStartInvoke>(ready, invoke);

//         let commands = parent.commands_mut();
//         commands
//             .entity(entity)
//             .insert(Ability)
//             .init_state_machine(ready);
//     });

//     entity
// }
