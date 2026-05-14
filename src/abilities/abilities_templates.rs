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
    observer::On,
    schedule::IntoScheduleConfigs,
    system::{Res, Single},
};
use bevy_gauge::{
    attributes, instant,
    prelude::{AttributesMut, Modifier},
    requires,
};
use bevy_gearbox::{GearboxSet, InitStateMachine, SpawnTransition, StateComponent};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    GridCell,
    abilities::effects::{
        AbilityEffectKind, AbilityEffects, CasterHitEffect, DamageEffect, HitTrigger,
        JustCastedEffect, SpawnEffect, observe_effects, propag_caster_hit,
    },
    deck::deck_and_cards::Card,
    game_flow::turns::{CurrentDeckReference, CurrentPlayingEntity, PlayingEntity},
    grid_abilities_backend::{
        AbilityHitEntity, CasterAbilityHit, EntityGatheringFilter, Grid3DFilter, Grid3DGatherer,
        GridCheckShape, GridGoOff, GridGoOffConfig, GridInvokerTarget, GridSpawnConfig,
        GridStartInvoke, GridTarget, GridTargetGenerator, NumberType,
    },
    melee::MeleeEffect,
    projectiles::ProjectileEffect,
    utils::IntoVec,
};

pub struct AbilitiesTemplatePlugin;

impl Plugin for AbilitiesTemplatePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Startup, register_templates)
            .add_systems(Update, propag_caster_hit.before(GearboxSet))
            .add_observer(handle_cast);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CastInvokedBy {
    CurrentlyPlaying,
    Specific(Entity),
}

fn handle_cast(
    e: On<AbilityCastRequested>,
    mut cmd: Commands,
    mut writer: MessageWriter<GridStartInvoke>,
    card_q: Query<&Card>,
    currently_playing: Res<CurrentPlayingEntity>,
    cells_q: Query<&GridNode, With<GridCell>>,
    playing_q: Query<&CartesianPosition, With<PlayingEntity>>,
    grid: Single<&mut CartesianGrid<Cartesian3D>>,
) {
    let card = card_q
        .get(e.card_entity)
        .expect("Card should exist in the card entity from cast event");
    let invoker = match e.invoked_by {
        CastInvokedBy::CurrentlyPlaying => currently_playing.0,
        CastInvokedBy::Specific(entity) => entity,
    };
    let target = e.target;

    let target_position = cells_q.get(target).map_or_else(
        |_| {
            *playing_q
                .get(target)
                .expect("Target should be a GridCell or PlayingEntity")
        },
        |node| grid.pos_from_index(node.0),
    );

    cmd.entity(invoker)
        .insert(FromCaster::new(card.ability_handler.caster_entity));
    cmd.entity(card.ability_handler.caster_entity)
        .insert(InvokedBy(invoker));
    cmd.entity(invoker)
        .insert(GridInvokerTarget::entity(target, target_position));

    let grid_target = GridTarget::entity(target, target_position);
    writer.write(GridStartInvoke::new(
        card.ability_handler.caster_entity,
        grid_target,
    ));
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

pub enum BaseAbility {
    Projectile,
    Melee,
}

#[derive(Clone)]
pub enum AbilityModifier {
    Ripple,
}

impl BaseAbility {
    pub fn as_str(&self) -> &str {
        match self {
            BaseAbility::Projectile => "base_projectile",
            BaseAbility::Melee => "base_melee",
        }
    }
}

pub trait AbilityBuilderState {}

#[derive(Clone)]
pub struct ABSInitial {}
impl AbilityBuilderState for ABSInitial {}

#[derive(Clone)]
pub struct ABSAbilityPassed {}
impl AbilityBuilderState for ABSAbilityPassed {}

#[derive(Clone)]
pub struct ABSReady {}
impl AbilityBuilderState for ABSReady {}

#[derive(Clone)]
pub struct AbilityHandlerBuilder<S>
where
    S: AbilityBuilderState,
{
    pub ability_entity: Entity,
    pub base_entity: Option<Entity>,
    pub modifiers: Vec<AbilityModifier>,
    _data: PhantomData<S>,
}

#[derive(Clone)]
pub struct AbilityHandler {
    pub caster_entity: Entity,
    pub builder: AbilityHandlerBuilder<ABSReady>,
}

impl AbilityHandler {
    pub fn reset(mut self, cmd: &mut Commands) {
        // Remove existing ability entity
        cmd.entity(self.caster_entity).despawn();
        // Generate new one
        self = self.builder.build(cmd);
    }
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

impl AbilityHandlerBuilder<ABSInitial> {
    pub fn from_ability_entity(ability_entity: Entity) -> AbilityHandlerBuilder<ABSAbilityPassed> {
        AbilityHandlerBuilder::<ABSAbilityPassed> {
            ability_entity,
            base_entity: None,
            modifiers: vec![],
            _data: PhantomData,
        }
    }

    pub fn from_template(
        template: BaseAbility,
        templates: &Res<TemplateRegistry>,
        cmd: &mut Commands,
    ) -> AbilityHandlerBuilder<ABSAbilityPassed> {
        let spawn_fn = templates
            .get(template.as_str())
            .expect("Failed to find template");
        let ability_entity = spawn_fn(cmd, None);

        AbilityHandlerBuilder::<ABSAbilityPassed> {
            ability_entity,
            base_entity: None,
            modifiers: vec![],
            _data: PhantomData,
        }
    }
}

impl AbilityHandlerBuilder<ABSAbilityPassed> {
    pub fn add_modifiers(
        self,
        ability_modifiers: impl IntoVec<AbilityModifier>,
    ) -> AbilityHandlerBuilder<ABSReady> {
        AbilityHandlerBuilder::<ABSReady> {
            ability_entity: self.ability_entity,
            base_entity: None,
            modifiers: ability_modifiers.into_vec(),
            _data: PhantomData,
        }
    }
}

#[derive(Component)]
pub struct CasterEntity;

#[derive(EntityEvent)]
pub struct CasterAbilityCasted(pub Entity);

#[derive(EntityEvent)]
pub struct CasterHitReceived(pub Entity);

#[derive(Component)]
pub struct FromCaster {
    pub entity: Entity,
}

impl FromCaster {
    pub fn new(entity: Entity) -> Self {
        Self { entity }
    }
}

impl AbilityHandlerBuilder<ABSReady> {
    pub fn pass_base_entity(mut self, base_entity: Option<Entity>) -> Self {
        self.base_entity = base_entity;
        self
    }

    pub fn build(self, cmd: &mut Commands) -> AbilityHandler {
        let entity = self.base_entity.unwrap_or_else(|| cmd.spawn_empty().id());
        cmd.entity(self.ability_entity)
            .insert(FromCaster::new(entity));
        let mut e_cmds = cmd.entity(entity);
        println!("starting observer on : {:?}", entity);
        e_cmds.observe(observe_effects);

        e_cmds.with_children(|parent| {
            let s__ready = parent
                .spawn_diesel_substate(entity, Name::new("Ready"))
                .id();

            let s__cast = parent
                .spawn_diesel_substate(entity, (Name::new("HandlerCast")))
                .id();

            let s__hit = parent.spawn_diesel_substate(entity, Name::new("Hit")).id();

            parent.spawn_subeffect(
                s__cast,
                (
                    SpawnEffect::new(entity, self.ability_entity),
                    JustCastedEffect::new(entity),
                ),
            );

            parent.spawn_transition::<GridStartInvoke>(s__ready, s__cast);
            parent.spawn_transition::<CasterAbilityHit>(s__cast, s__hit);
            parent.spawn_transition_always(s__hit, s__ready);

            let _cmd = parent.commands_mut();
            _cmd.entity(entity)
                .insert((Name::new("ABE"), Ability, CasterEntity))
                .init_state_machine(s__ready);
        });

        AbilityHandler {
            caster_entity: entity,
            builder: self,
        }
    }
}

fn register_templates(mut registry: ResMut<TemplateRegistry>) {
    registry.register("projectile", projectile_template);
    registry.register("melee", melee_template);
    registry.register(BaseAbility::Projectile.as_str(), basic_projectile_ability);
    registry.register(BaseAbility::Melee.as_str(), basic_melee_ability);
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
                (
                    Name::new("Hit"),
                    StateComponent(DelayedDespawn::now()), // This repeats the template
                                                           // GridSpawnConfig::target("projectile").with_target_generator(
                                                           //     GridTargetGenerator::default()
                                                           //         .with_gatherer(Grid3DGatherer::EntitiesInShape {
                                                           //             shape: GridCheckShape::Sphere(12.0),
                                                           //             gathering_filter: EntityGatheringFilter::All,
                                                           //             sort_by_nearest: false,
                                                           //         })
                                                           //         .with_filter(Grid3DFilter::new(NumberType::Fixed(1))),
                                                           // ),
                ),
            )
            .id();

        // parent.spawn_subeffect(hit_and_done, DamageEffect("Strength@Attacker * 2.0"));

        parent.spawn_transition::<AbilityHitEntity>(flying, hit_and_done);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert((
                Name::new("BaseProjectile"),
                Marker::<Projectile>::new(),
                ProjectileEffect::new(10.0),
                Visibility::Inherited,
                Ability,
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
        parent.spawn_transition_always(invoke, ready);

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
