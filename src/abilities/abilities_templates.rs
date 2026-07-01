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
    GridCell,
    abilities::{
        effects::{AbilityOfCaster, CasterHitEffect, EffectMod, JustCastedEffect, SpawnEffect},
        utils::AbilityComposingPlugin,
    },
    deck::{card_blueprints::AbilityNode, deck_and_cards::Card},
    game_flow::turns::{
        CurrentDeckReference, CurrentPlayingEntity, EntityTurnStart, PlayingEntity,
    },
    grid_abilities_backend::{
        AbilityHitEntity, CastEnd, EntityGatheringFilter, Grid3DFilter, Grid3DGatherer,
        GridCheckShape, GridGoOff, GridGoOffConfig, GridInvokerTarget, GridSpawnConfig,
        GridStartInvoke, GridTarget, GridTargetGenerator, GridTargetMutator, NumberType,
    },
    melee::MeleeEffect,
    projectiles::ProjectileEffect,
    stats::players::Speed,
    utils::{CombatGridQ, IntoVec},
};

pub struct AbilitiesTemplatePlugin;

impl Plugin for AbilitiesTemplatePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(AbilityComposingPlugin)
            .add_systems(Startup, register_templates)
            .add_observer(handle_action_cast);
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

// fn handle_cast(
//     e: On<AbilityCastRequested>,
//     mut cmd: Commands,
//     mut writer: MessageWriter<GridStartInvoke>,
//     card_q: Query<&Card>,
//     currently_playing: Res<CurrentPlayingEntity>,
//     cells_q: Query<&GridNode, With<GridCell>>,
//     playing_q: Query<(&CartesianPosition), With<PlayingEntity>>,
//     player_pos_q: Query<&GlobalTransform, With<PlayingEntity>>,
//     grid: Single<&mut CartesianGrid<Cartesian3D>>,
// ) {
//     let card = card_q
//         .get(e.card_entity)
//         .expect("Card should exist in the card entity from cast event");
//     let attacking_player = match e.invoked_by {
//         CastInvokedBy::CurrentlyPlaying => currently_playing.0,
//         CastInvokedBy::Specific(entity) => entity,
//     };
//     let target = e.target;

//     let attacking_pos = playing_q
//         .get(attacking_player)
//         .expect("this man should have a cartesian pos");

//     let target_position = cells_q.get(target).map_or_else(
//         |_| {
//             *playing_q
//                 .get(target)
//                 .expect("Target should be a GridCell or PlayingEntity")
//         },
//         |node| grid.pos_from_index(node.0),
//     );

//     println!("on the player : ");
//     cmd.entity(attacking_player).log_components();

//     let invoker = cmd
//         .spawn((
//             attacking_pos.clone(),
//             Name::new("Da invokery"),
//             GridInvokerTarget::entity(target, target_position),
//             ActionCastData {
//                 source_playing_entity: attacking_player,
//                 source_caster_entity: e.card_entity,
//             },
//         ))
//         .id();

//     cmd.entity(attacking_player)
//         .insert(FromCaster::new(card.ability_builder.caster_entity));
//     cmd.entity(card.ability_builder.caster_entity).insert((
//         InvokedBy(invoker),
//         attacking_pos.clone(),
//         GridInvokerTarget::entity(target, target_position),
//     ));

//     let grid_target = GridTarget::entity(target, target_position);
//     writer.write(GridStartInvoke::new(
//         card.ability_builder.caster_entity,
//         grid_target,
//     ));
// }

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
    pub nodes: AbilityNode,
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

impl AbilityHandlerBuilder<ABSInitial> {
    pub fn from_nodes(nodes: AbilityNode) -> AbilityHandlerBuilder<ABSAbilityPassed> {
        AbilityHandlerBuilder::<ABSAbilityPassed> {
            nodes,
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
            nodes: self.nodes,
            base_entity: None,
            modifiers: ability_modifiers.into_vec(),
            _data: PhantomData,
        }
    }
}

#[derive(Component)]
pub struct CasterEntity;

#[derive(EntityEvent, Clone)]
pub struct CasterAbilityCasted(pub Entity);

#[derive(EntityEvent, Clone)]
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
        let mut e_cmds = cmd.entity(entity);

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
                    // SpawnEffect::new(entity, self.ability_entity),
                    JustCastedEffect::new(entity),
                ),
            );

            parent.spawn_transition::<GridStartInvoke>(s__ready, s__cast);
            parent.spawn_transition::<CastEnd>(s__cast, s__hit);
            parent.spawn_transition_always(s__hit, s__ready);

            let _cmd = parent.commands_mut();
            _cmd.entity(entity)
                .insert((Name::new("Abilities caster"), Ability, CasterEntity))
                .init_state_machine(s__ready);
        });

        AbilityHandler {
            caster_entity: entity,
            builder: self,
        }
    }
}

fn register_templates(mut registry: ResMut<TemplateRegistry>) {
    // TODO remove
    registry.register("projectile", projectile_template);
    registry.register("melee", melee_template);
    // registry.register(BaseAbility::Projectile.as_str(), basic_projectile_ability);
    registry.register(BaseAbility::Melee.as_str(), basic_melee_ability);
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

pub fn action_base(cmd: &mut Commands, builder: &AbilityNode) -> (Entity, Vec<Entity>) {
    let initial = cmd.spawn_empty().id();
    let mut created_entities: Vec<Entity> = Vec::new();

    builder.build_and_spawn(initial, &mut created_entities, cmd);
    (initial, created_entities)
}

#[derive(Component, Debug, Clone)]
pub struct AttachedToPlayer(pub Entity);

#[derive(Component, Debug, Clone)]
pub struct HasRootInvoker(pub Entity);

pub fn handle_action_cast(
    e: On<AbilityCastRequested>,
    cards_q: Query<&Card>,
    cells_q: Query<&GridNode, With<GridCell>>,
    playing_q: Query<(&CartesianPosition, &Transform), With<PlayingEntity>>,
    currently_playing: Res<CurrentPlayingEntity>,
    grid: CombatGridQ,
    mut cmd: Commands,
    mut writer: MessageWriter<GridStartInvoke>,
) {
    let (attacking_player, (origin_grid_pos, origin_tf)) = match e.invoked_by {
        CastInvokedBy::CurrentlyPlaying => (
            currently_playing.0,
            playing_q
                .get(currently_playing.0)
                .expect("Player should have cartesian pos"),
        ),
        CastInvokedBy::Specific(entity) => (
            entity,
            playing_q
                .get(currently_playing.0)
                .expect("Player should have cartesian pos"),
        ),
    };

    let target_entity = e.target;
    let target_position = cells_q.get(target_entity).map_or_else(
        |_| {
            *playing_q
                .get(target_entity)
                .expect("Target should be a GridCell or PlayingEntity")
                .0
        },
        |node| grid.pos_from_index(node.0),
    );
    let target = GridInvokerTarget::entity(e.target, target_position);
    let grid_target = GridTarget::entity(target.entity.unwrap(), target.position);

    // Maybe define a list of states that are required by any action and return them for event triggers ?
    // let action_entity = cmd
    //     .spawn((
    //         *origin_grid_pos,
    //         Transform::from_translation(origin_tf.translation),
    //     ))
    //     .id();

    let card_entity = e.card_entity;
    let card = cards_q
        .get(card_entity)
        .expect("Passed card entity does not have the 'Card' entity");

    let (action_entity, created_entities) = action_base(&mut cmd, &card.ability_builder);

    cmd.entity(action_entity).insert((
        *origin_grid_pos,
        Transform::from_translation(origin_tf.translation),
        GridInvokerTarget::entity(grid_target.entity.unwrap(), grid_target.position),
        InvokedBy(attacking_player),
        AttachedToPlayer(attacking_player),
    ));

    for e in created_entities.iter() {
        cmd.entity(*e).insert((
            AttachedToPlayer(attacking_player),
            HasRootInvoker(action_entity),
            GridInvokerTarget::entity(grid_target.entity.unwrap(), grid_target.position),
            *origin_grid_pos,
        ));
    }

    let invoker_test = cmd
        .spawn((
            target,
            AttachedToPlayer(attacking_player),
            HasRootInvoker(action_entity),
            ActionCastData::new(attacking_player, action_entity),
            InvokedBy(attacking_player),
            Ability,
        ))
        .id();

    cmd.entity(invoker_test).with_children(|parent| {
        let ready = parent
            .spawn_substate(action_entity, Name::new("ActionReady"))
            .id();
        let invoke = parent
            .spawn_substate(action_entity, Name::new("ActionInvoke"))
            .id();

        parent.spawn_subeffect(invoke, (SpawnEffect::new(action_entity, action_entity)));

        parent.spawn_transition::<GridStartInvoke>(ready, invoke);

        parent
            .commands_mut()
            .entity(action_entity)
            .insert((
                target,
                AttachedToPlayer(attacking_player),
                HasRootInvoker(action_entity),
                ActionCastData::new(attacking_player, action_entity),
                Ability,
            ))
            .init_state_machine(ready);
    });

    cmd.entity(attacking_player).insert((
        GridInvokerTarget::entity(grid_target.entity.unwrap(), grid_target.position),
        ActionCastData::new(attacking_player, invoker_test),
    ));

    writer.write(GridStartInvoke::new(invoker_test, grid_target));
    // card.invoking_kind.start_invoke_on(
    //     &mut cmd,
    //     action_entity,
    //     writer,
    //     grid_target,
    //     (
    //         InvokedBy(action_entity),
    //         AttachedToPlayer(attacking_player),
    //         HasRootInvoker(action_entity),
    //         ActionCastData::new(attacking_player, action_entity),
    //         Ability,
    //         grid_target,
    //     ),
    // );
}

#[derive(Component, Clone)]
pub struct MarkerTest;

pub fn init_action(cmd: &mut Commands, target: GridTarget, invoker: Entity) -> Entity {
    cmd.spawn((
        target,
        InvokedBy(invoker),
        GridGoOffConfig::invoker_target(),
    ))
    .id()
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

pub fn ripple_invoking(
    commands: &mut Commands,
    entity: Option<Entity>,
    ripple_count: u32,
    template_entity: Entity,
) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());
    println!("spawn ripple");

    commands.entity(entity).with_children(|parent| {
        let initial_invoke = parent
            .spawn_diesel_substate(
                entity,
                (
                    Name::new("Initial spawn"),
                    InvokingTriggerEffect::new(template_entity, entity),
                    GridGoOffConfig::invoker_target(),
                ),
            )
            .id();

        let ready = parent
            .spawn_diesel_substate(entity, Name::new("Ready"))
            .id();

        let invoke = parent
            .spawn_diesel_substate(
                entity,
                (
                    Name::new("Invoke"),
                    InvokingTriggerEffect::new(template_entity, entity),
                    GridGoOffConfig::invoker_target()
                        .with_gatherer(Grid3DGatherer::EntitiesInShape {
                            shape: GridCheckShape::Sphere(4.0),
                            gathering_filter: EntityGatheringFilter::Playing,
                            sort_by_nearest: true,
                        })
                        .with_filter(Grid3DFilter::new(NumberType::Fixed(1))),
                ),
            )
            .id();

        let hit = parent.spawn_diesel_substate(entity, Name::new("Hit")).id();
        parent.spawn_subeffect(hit, instant! {"RippleCount" -= 1.0});

        let done = parent
            .spawn_diesel_substate(
                entity,
                (Name::new("Hit"), StateComponent(DelayedDespawn::now())),
            )
            .id();

        parent.spawn_transition::<AbilityHitEntity>(initial_invoke, hit);
        parent.spawn_transition_always(ready, invoke);
        parent.spawn_transition::<AbilityHitEntity>(invoke, hit);
        parent.spawn_branch::<AbilityHitEntity>(hit, |b| {
            b.when(done, move |t| {
                t.insert(requires! {"RippleCount <= 0"})
                    .insert(RequiresStatsOf(entity));
            });
            b.otherwise(ready);
        });

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert((
                Ability,
                Name::new("Ripple"),
                Visibility::Inherited,
                Attributes::new(),
                attributes! {
                    "RippleCount" => ripple_count as f32
                },
            ))
            .init_state_machine(initial_invoke);
    });

    entity
}

#[derive(Component, Debug, Clone)]
pub struct AbilityTemplate(pub Entity);

pub struct StateBundleEntity(Entity);

pub fn projectile_template(commands: &mut Commands, entity: Option<Entity>) -> Entity {
    let entity = entity.unwrap_or_else(|| commands.spawn_empty().id());

    commands.entity(entity).with_children(|parent| {
        let ready = parent
            .spawn_diesel_substate(entity, (Name::new("Ready")))
            .id();

        let active = parent
            .spawn_diesel_substate(entity, (Name::new("Flying")))
            .id();

        let hit_and_done = parent
            .spawn_diesel_substate(
                entity,
                (Name::new("Hit"), StateComponent(DelayedDespawn::now())),
            )
            .id();

        parent.spawn_transition::<AbilityHitEntity>(active, hit_and_done);

        let commands = parent.commands_mut();
        commands
            .entity(entity)
            .insert((
                Ability,
                Name::new("BaseProjectile"),
                Speed::new(8),
                AbilityInitialized,
                Marker::<Projectile>::new(),
                ProjectileEffect::new(8.0),
                Visibility::Inherited,
                GridGoOffConfig::invoker_target(),
            ))
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
