use std::{f32::consts::TAU, fmt::Debug, i32, marker::PhantomData, ops::Deref};

use bevy::{
    app::{App, Plugin, Update},
    ecs::{
        component::Component,
        entity::Entity,
        event::EntityEvent,
        hierarchy::ChildOf,
        message::{Message, MessageReader, MessageWriter},
        query::{With, Without},
        system::{Commands, Local, Query, Res, Single, SystemParam},
    },
    math::{Dir3, I16Vec3, ShapeSample, U16Vec3, UVec3, Vec2, Vec3, primitives::Sphere},
    prelude::IntoScheduleConfigs,
    reflect::Reflect,
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::{
    effect::{GoOff, GoOffOrigin, SubEffects},
    events::{HasDieselTarget, PosBound},
    invoke::Ability,
    prelude::{InvokedBy, generate_targets, resolve_invoker, resolve_root},
    spawn::{OnSpawnInvoker, OnSpawnOrigin, OnSpawnTarget, SpawnConfig, TemplateRegistry},
    target::{InvokerTarget, TargetGenerator, TargetMutator, TargetType},
};
use bevy_diesel::{pipeline::propagate_system, prelude::SpatialBackend, target::Target};
use bevy_gauge::{
    AttributeResolvable,
    prelude::{AttributesAppExt, AttributesMut},
};
use bevy_gearbox::{AcceptAll, GearboxMessage, RegistrationAppExt};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_prng::WyRand;
use bevy_rand::{
    plugin::EntropyPlugin,
    prelude::GlobalRng,
    traits::{ForkableAsRng, ForkableRng},
};
use rand::{Rng, RngExt, SeedableRng};

use crate::{
    GridCell,
    projectiles::ProjectilePlugin,
    states::{FromGrid, PlayingEntity, TeamHitFilter, ToWorldPos},
};

// Vec3 type aliases
pub type GridInvokerTarget = bevy_diesel::target::InvokerTarget<CartesianPosition>;
pub type GridTarget = bevy_diesel::target::Target<CartesianPosition>;
pub type GridGoOff = bevy_diesel::effect::GoOff<CartesianPosition>;
pub type GridStartInvoke = bevy_diesel::events::StartInvoke<CartesianPosition>;
pub type GridStopInvoke = bevy_diesel::events::StopInvoke<CartesianPosition>;
pub type GridOnRepeat = bevy_diesel::events::OnRepeat<CartesianPosition>;
pub type GridOnSpawnOrigin = bevy_diesel::spawn::OnSpawnOrigin<CartesianPosition>;
pub type GridOnSpawnTarget = bevy_diesel::spawn::OnSpawnTarget<CartesianPosition>;
pub type GridOnSpawnInvoker = bevy_diesel::spawn::OnSpawnInvoker<CartesianPosition>;
pub type GridTargetType = bevy_diesel::target::TargetType<CartesianPosition>;
pub type GridTargetGenerator = bevy_diesel::target::TargetGenerator<Grid3DBackend>;
pub type GridTargetMutator = bevy_diesel::target::TargetMutator<Grid3DBackend>;
pub type GridSpawnConfig = bevy_diesel::spawn::SpawnConfig<Grid3DBackend>;
pub type GridGoOffConfig = bevy_diesel::effect::GoOffConfig<Grid3DBackend>;

#[derive(Debug, Clone, Reflect, PartialEq)]
pub enum HitTargetKind {
    Playing,
    Cell,
}

/// Collision with an entity target.
#[derive(Message, Clone, Debug, Reflect)]
pub struct AbilityHitEntity {
    pub entity: Entity,
    pub target: GridTarget,
    pub target_kind: HitTargetKind,
}

impl GearboxMessage for AbilityHitEntity {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

impl AbilityHitEntity {
    pub fn new(entity: Entity, target: GridTarget, target_kind: HitTargetKind) -> Self {
        Self {
            entity,
            target,
            target_kind,
        }
    }
}

/// Collision with a contact point position.
#[derive(Message, Clone, Debug, Reflect)]
pub struct AbilityHitPosition {
    pub entity: Entity,
    pub target: GridTarget,
}

impl GearboxMessage for AbilityHitPosition {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

impl AbilityHitPosition {
    pub fn new(entity: Entity, target: GridTarget) -> Self {
        Self { entity, target }
    }
}

impl HasDieselTarget<CartesianPosition> for AbilityHitEntity {
    fn diesel_target(&self) -> GridTarget {
        self.target
    }
}

impl HasDieselTarget<CartesianPosition> for AbilityHitPosition {
    fn diesel_target(&self) -> GridTarget {
        self.target
    }
}

pub trait HitFilter: Component + Clone + Debug + Send + Sync + 'static {
    /// Component queried on invoker and target entities.
    type Lookup: Component;

    /// Return `true` if the ability should affect this target.
    fn can_target(
        &self,
        invoker_data: Option<&Self::Lookup>,
        target_data: Option<&Self::Lookup>,
    ) -> bool;
}

pub struct HitFilterPlugin<F: HitFilter> {
    _marker: PhantomData<F>,
}

impl<F: HitFilter> Default for HitFilterPlugin<F> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<F: HitFilter> Plugin for HitFilterPlugin<F> {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_hit_system::<F>);
    }
}

#[derive(EntityEvent, Message)]
pub struct HitReceived {
    entity: Entity,
    hit_by: Entity,
}

pub fn handle_unfiltered_hit_system<F: HitFilter>(
    mut hit_events: MessageReader<HitReceived>,
    invoker_q: Query<&InvokedBy>,
    grid: Single<&CartesianGrid<Cartesian3D>>,
    grid_cells_q: Query<&GridNode, With<GridCell>>,
    grid_playing_q: Query<&CartesianPosition, With<PlayingEntity>>,
    mut entity_writer: MessageWriter<AbilityHitEntity>,
    mut position_writer: MessageWriter<AbilityHitPosition>,
) {
    let grid = grid.deref();

    for hit in hit_events.read() {
        if let Ok(cell) = grid_cells_q.get(hit.entity) {
            let pos = grid.pos_from_index(cell.0);
            entity_writer.write(AbilityHitEntity::new(
                hit.entity,
                GridTarget::entity(hit.hit_by, pos),
                HitTargetKind::Cell,
            ));
        } else {
            if let Ok(playing_pos) = grid_playing_q.get(hit.entity) {
                entity_writer.write(AbilityHitEntity::new(
                    hit.entity,
                    GridTarget::entity(hit.hit_by, *playing_pos),
                    HitTargetKind::Playing,
                ));
            }
        }
    }
}

pub fn handle_hit_system<F: HitFilter>(
    mut hit_events: MessageReader<HitReceived>,
    invoker_q: Query<&InvokedBy>,
    grid: Single<&CartesianGrid<Cartesian3D>>,
    grid_cells_q: Query<&GridNode, With<GridCell>>,
    grid_playing_q: Query<&CartesianPosition, With<PlayingEntity>>,
    filters_q: Query<&F>,
    filter_lookup_q: Query<&F::Lookup>,
    mut entity_writer: MessageWriter<AbilityHitEntity>,
    mut position_writer: MessageWriter<AbilityHitPosition>,
) {
    let grid = grid.deref();

    for hit in hit_events.read() {
        match filters_q.get(hit.entity) {
            Ok(filter) => {
                let invoker_data = filter_lookup_q.get(hit.hit_by).ok();
                let target_data = filter_lookup_q.get(hit.entity).ok();
                if !filter.can_target(invoker_data, target_data) {
                    continue;
                }
            }
            Err(_) => {}
        };

        if let Ok(cell) = grid_cells_q.get(hit.entity) {
            let pos = grid.pos_from_index(cell.0);
            entity_writer.write(AbilityHitEntity::new(
                hit.entity,
                GridTarget::entity(hit.hit_by, pos),
                HitTargetKind::Cell,
            ));
        } else {
            if let Ok(playing_pos) = grid_playing_q.get(hit.entity) {
                entity_writer.write(AbilityHitEntity::new(
                    hit.entity,
                    GridTarget::entity(hit.hit_by, *playing_pos),
                    HitTargetKind::Playing,
                ));
            }
        }
    }
}

pub fn propag_log(mut reader: MessageReader<GoOffOrigin<CartesianPosition>>) {
    for m in reader.read() {
        println!("found go off origin msg");
    }
}

pub fn propag_log_b<B: SpatialBackend>(mut reader: MessageReader<GoOffOrigin<B::Pos>>) {
    for m in reader.read() {
        println!("found go off origin msg GENERIC");
    }
}

pub fn debug_propagate_system(
    mut reader: MessageReader<GoOffOrigin<CartesianPosition>>,
    mut ctx: Grid3DContext<'_, '_>,
    q_sub_effects: Query<&SubEffects>,
    q_target_mutator: Query<Option<&TargetMutator<Grid3DBackend>>>,
    q_invoker: Query<&InvokedBy>,
    q_child_of: Query<&ChildOf>,
    q_invoker_target: Query<&InvokerTarget<CartesianPosition>>,
    mut writer: MessageWriter<GoOff<CartesianPosition>>,
) {
    println!("hey mate");
    for origin in reader.read() {
        println!("inside debug propage");
        let root_entity = origin.entity;
        let passed_target = origin.target;

        let invoker = resolve_invoker(&q_invoker, root_entity);
        let root = resolve_root(&q_child_of, root_entity);
        let invoker_target: Target<CartesianPosition> = q_invoker_target
            .get(invoker)
            .copied()
            .map(Target::from)
            .unwrap_or_default();

        println!("got here == 1");

        // Resolve the root's own target list — apply its TargetMutator if
        // present, otherwise use the passed target verbatim. This matches
        // the behavior applied to children below.
        let root_targets: Vec<Target<CartesianPosition>> =
            if let Ok(Some(mutator)) = q_target_mutator.get(root_entity) {
                println!("got here == 2");
                let mut targets = generate_targets::<Grid3DBackend>(
                    &mutator.generator,
                    &mut ctx,
                    invoker,
                    invoker_target,
                    root,
                    CartesianPosition::default(),
                    passed_target,
                );
                targets = Grid3DBackend::apply_filter(
                    &mut ctx,
                    targets,
                    &mutator.generator.filter,
                    invoker,
                    passed_target.position,
                );
                targets
            } else {
                vec![passed_target]
            };

        // Fire GoOff on the root entity itself (one per resolved target).
        for &target in &root_targets {
            println!(
                "SENDING GO_OFF => target entity, position: {:?}, {:?}",
                target.entity, target.position
            );
            // writer.write(GoOff::new(root_entity, target));
        }

        // Walk the tree: (entity, targets_for_this_entity)
        let mut stack: Vec<(Entity, Vec<Target<CartesianPosition>>)> =
            vec![(root_entity, root_targets)];

        while let Some((parent, in_targets)) = stack.pop() {
            println!("got here == 3");
            let Ok(subs) = q_sub_effects.get(parent) else {
                continue;
            };

            for &child in subs.into_iter() {
                println!("got here == 4");
                let out_targets = if let Ok(Some(mutator)) = q_target_mutator.get(child) {
                    println!("got here == 5");
                    let mut aggregated = Vec::new();
                    for passed in in_targets.iter() {
                        println!("got here == 6");
                        let mut targets = generate_targets::<Grid3DBackend>(
                            &mutator.generator,
                            &mut ctx,
                            invoker,
                            invoker_target,
                            root,
                            CartesianPosition::default(),
                            *passed,
                        );
                        let origin_pos = passed.position;
                        targets = Grid3DBackend::apply_filter(
                            &mut ctx,
                            targets,
                            &mutator.generator.filter,
                            invoker,
                            origin_pos,
                        );
                        aggregated.append(&mut targets);
                    }
                    aggregated
                } else {
                    in_targets.clone()
                };

                // Write one GoOff per target (batch messages instead of Vec)
                for &target in &out_targets {
                    println!("GO_OFF bis => {:?}", target.position);
                    // writer.write(GoOff::new(child, target));
                }
                stack.push((child, out_targets));
            }
        }
    }
}

pub struct Grid3dDieselPlugin;

impl Plugin for Grid3dDieselPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        // app.add_message::<HitReceived>();
        // app.add_message::<AbilityHitEntity>();
        // app.add_message::<AbilityHitPosition>();
        // app.add_message::<GridStartInvoke>();

        app.add_plugins(EntropyPlugin::<bevy_prng::WyRand>::default());
        // app.add_plugins(HitFilterPlugin::<TeamHitFilter>::default());
        app.add_plugins(Grid3DBackend::plugin_core());

        use bevy_diesel::bevy_gauge::prelude::AttributesAppExt;
        app.register_attribute_derived::<GridSpawnConfig>();
        app.register_attribute_derived::<GridTargetMutator>();

        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::effect::go_off_on_entry::<Grid3DBackend>,
                propagate_system::<Grid3DBackend>,
            )
                .chain()
                .in_set(bevy_diesel::DieselSet::Propagation),
        );

        // Leaf effect systems: read GoOff
        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::spawn::spawn_system::<Grid3DBackend>,
                bevy_diesel::print::print_effect::<CartesianPosition>,
            )
                .in_set(bevy_diesel::DieselSet::Effects),
        );

        // Sustained modifier apply — monomorphized here because the generic
        // fn needs B::Context which can only resolve with a concrete backend.
        app.add_systems(
            Update,
            bevy_diesel::gauge::modifiers::sustained_modifier_apply::<Grid3DBackend>
                .in_set(bevy_diesel::gauge::SustainedModifierSet),
        );

        app.add_plugins(ProjectilePlugin);

        // #TODO: Will need to do something similar
        // // Collision types + system (unfiltered - entities with Collides marker)
        app.register_transition::<AbilityHitEntity>();
        app.register_transition::<AbilityHitPosition>();
        app.register_transition::<GridStartInvoke>();

        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::events::go_off_side_effect::<AbilityHitEntity, CartesianPosition>
                    .in_set(bevy_diesel::bevy_gearbox::GearboxPhase::SideEffectPhase),
                bevy_diesel::events::go_off_side_effect::<AbilityHitPosition, CartesianPosition>
                    .in_set(bevy_diesel::bevy_gearbox::GearboxPhase::SideEffectPhase),
            ),
        );
    }
}

fn find_ability(
    entity: Entity,
    q_invoker: &Query<&InvokedBy>,
    q_ability: &Query<(), With<Ability>>,
) -> Option<Entity> {
    let mut current = entity;
    loop {
        if q_ability.get(current).is_ok() {
            return Some(current);
        }
        let Ok(invoked_by) = q_invoker.get(current) else {
            return None;
        };
        current = invoked_by.0;
    }
}

fn test_generate_targets_with_spawn_positions<B: SpatialBackend>(
    generator: &TargetGenerator<B>,
    spawn_targets: &[Target<B::Pos>],
    in_targets: &[Target<B::Pos>],
    invoker: Entity,
    invoker_target: Target<B::Pos>,
    root: Entity,
    ctx: &mut B::Context<'_, '_>,
) -> Vec<Target<B::Pos>> {
    if matches!(generator.target_type, TargetType::Spawn) {
        let mut all_targets = Vec::new();
        for (idx, spawn_target) in spawn_targets.iter().enumerate() {
            let passed_target = in_targets
                .get(idx % in_targets.len())
                .copied()
                .unwrap_or(*spawn_target);
            let mut targets = generate_targets::<B>(
                generator,
                ctx,
                invoker,
                invoker_target,
                root,
                spawn_target.position,
                passed_target,
            );
            all_targets.append(&mut targets);
        }
        all_targets
    } else {
        let mut all_targets = Vec::new();
        for passed_target in in_targets.iter() {
            let mut t = generate_targets::<B>(
                generator,
                ctx,
                invoker,
                invoker_target,
                root,
                B::Pos::default(),
                *passed_target,
            );
            all_targets.append(&mut t);
        }
        all_targets
    }
}

pub fn test_check_go_off(mut reader: MessageReader<GridGoOff>) {
    for go in reader.read() {
        println!("found go off!");
    }
}

pub fn test_spawn_system<B: SpatialBackend>(
    mut reader: MessageReader<GoOff<B::Pos>>,
    q_effect: Query<&SpawnConfig<B>>,
    q_invoker: Query<&InvokedBy>,
    q_child_of: Query<&ChildOf>,
    q_invoker_target: Query<&InvokerTarget<B::Pos>>,
    q_ability: Query<(), With<Ability>>,
    template_registry: Res<TemplateRegistry>,
    mut ctx: B::Context<'_, '_>,
    mut commands: Commands,
    mut attributes: AttributesMut,
    mut spawn_target_writer: MessageWriter<OnSpawnTarget<B::Pos>>,
    mut spawn_origin_writer: MessageWriter<OnSpawnOrigin<B::Pos>>,
    mut spawn_invoker_writer: MessageWriter<OnSpawnInvoker<B::Pos>>,
) {
    println!("hey there");
    for go_off in reader.read() {
        let effect_entity = go_off.entity;
        let passed = go_off.target;

        let invoker = q_invoker.root_ancestor(effect_entity);
        let invoker_target: Target<B::Pos> = q_invoker_target
            .get(invoker)
            .copied()
            .map(Target::from)
            .unwrap_or_default();
        let Ok(spawn_config) = q_effect.get(effect_entity) else {
            continue;
        };
        let root = q_child_of.root_ancestor(effect_entity);

        let mut spawn_targets = generate_targets::<B>(
            &spawn_config.spawn_position_generator,
            &mut ctx,
            invoker,
            invoker_target,
            root,
            B::Pos::default(),
            passed,
        );

        if spawn_targets.is_empty() {
            continue;
        }

        let target_targets = if let Some(target_generator) = &spawn_config.spawn_target_generator {
            let targets = test_generate_targets_with_spawn_positions::<B>(
                target_generator,
                &spawn_targets,
                &[passed],
                invoker,
                invoker_target,
                root,
                &mut ctx,
            );
            if targets.is_empty() {
                continue;
            }
            Some(targets)
        } else {
            None
        };

        let parent_entity = if let Some(parent_target_type) = &spawn_config.as_child_of {
            let parent = match parent_target_type {
                TargetType::Invoker => Some(invoker),
                TargetType::InvokerTarget => invoker_target.entity,
                TargetType::Root => Some(root),
                TargetType::Passed => passed.entity,
                TargetType::Spawn => None,
                TargetType::Position(_) => None,
            };
            if parent.is_none() {
                continue;
            }
            parent
        } else {
            None
        };

        for (i, spawn_target) in spawn_targets.iter().enumerate() {
            let spawned_entity = commands.spawn(InvokedBy(invoker)).id();
            B::insert_position(
                &mut commands.entity(spawned_entity),
                &ctx,
                spawn_target.position,
                parent_entity,
            );
            template_registry.get(&spawn_config.template_id).unwrap()(
                &mut commands,
                Some(spawned_entity),
            );

            // Register gauge sources for cross-entity attribute expressions.
            // The aliases are stored in the DependencyGraph immediately; when
            // Attributes + modifiers are applied later (during command flush),
            // expressions like "Damage@root" or "Cooldown@ability" will resolve.
            attributes.register_source(spawned_entity, "root", root);
            if let Some(ability) = find_ability(effect_entity, &q_invoker, &q_ability) {
                attributes.register_source(spawned_entity, "ability", ability);
            }

            match (&target_targets, parent_entity) {
                (Some(targets), Some(parent)) => {
                    let target_target = targets
                        .get(i % targets.len())
                        .copied()
                        .unwrap_or(*spawn_target);
                    commands
                        .entity(spawned_entity)
                        .insert((target_target, ChildOf(parent)));
                    spawn_target_writer.write(OnSpawnTarget::new(spawned_entity, target_target));
                }
                (Some(targets), None) => {
                    let target_target = targets
                        .get(i % targets.len())
                        .copied()
                        .unwrap_or(*spawn_target);
                    commands.entity(spawned_entity).insert(target_target);
                    spawn_target_writer.write(OnSpawnTarget::new(spawned_entity, target_target));
                }
                (None, Some(parent)) => {
                    commands.entity(spawned_entity).insert(ChildOf(parent));
                }
                (None, None) => {}
            }

            spawn_origin_writer.write(OnSpawnOrigin::new(
                spawned_entity,
                Target::entity(spawned_entity, spawn_target.position),
            ));

            let invoker_position = B::position_of(&ctx, invoker).unwrap_or_default();
            spawn_invoker_writer.write(OnSpawnInvoker::new(
                spawned_entity,
                Target::entity(invoker, invoker_position),
            ));
        }
    }
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum NumberType {
    All,
    Fixed(usize),
    Random { min: usize, max: usize },
}

impl Default for NumberType {
    fn default() -> Self {
        Self::All
    }
}

impl NumberType {
    /// Resolve to a concrete count. Panics on `All`. Use only for gatherers
    /// where a count is always required.
    pub fn resolve_count(&self, mut rng: WyRand) -> usize {
        match self {
            NumberType::All => panic!("NumberType::All has no concrete count"),
            NumberType::Fixed(n) => *n,
            NumberType::Random { min, max } => {
                if min >= max {
                    return *min;
                }
                let range = max - min + 1;
                let r = (rng.next_u64() as usize) % range;
                min + r
            }
        }
    }

    /// Resolve to a concrete count, or `None` for unlimited.
    fn resolve_limit(&self, rng: WyRand) -> Option<usize> {
        match self {
            NumberType::All => None,
            _ => Some(self.resolve_count(rng)),
        }
    }
}

#[derive(Debug, Clone, AttributeResolvable, PartialEq, Eq)]
pub enum EntityGatheringFilter {
    All,
    Playing,
    Cells,
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum GridCheckShape {
    Circle(f32),
    Sphere(f32),
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum Grid3DGatherer {
    // Position generators - produce N random points around origin
    Sphere {
        radius: f32,
        count: NumberType,
    },
    Circle {
        radius: f32,
        count: NumberType,
    },
    // Box {
    //     #[skip]
    //     half_extents: CartesianPosition,
    //     count: NumberType,
    // },
    // Line {
    //     #[skip]
    //     direction: Dir3,
    //     length: f32,
    //     count: NumberType,
    // },
    /// All entities in an shape.
    EntitiesInShape {
        shape: GridCheckShape,
        gathering_filter: EntityGatheringFilter,
        sort_by_nearest: bool,
    },
}

#[derive(Clone, Debug, AttributeResolvable)]
pub struct Grid3DFilter {
    /// Max target count. `NumberType::All` passes everything through.
    pub count: NumberType,
    /// Require line-of-sight (TODO).
    #[skip]
    pub line_of_sight: bool,
}

impl Default for Grid3DFilter {
    fn default() -> Self {
        Self {
            count: NumberType::All,
            line_of_sight: false,
        }
    }
}

#[derive(SystemParam)]
pub struct Grid3DContext<'w, 's> {
    pub grid: Single<'w, 's, &'static CartesianGrid<Cartesian3D>>,
    pub grid_tf: Single<'w, 's, &'static Transform, With<CartesianGrid<Cartesian3D>>>,
    pub transforms: Query<'w, 's, &'static Transform, Without<CartesianGrid<Cartesian3D>>>,
    pub grid_cells:
        Query<'w, 's, (Entity, &'static GridNode), (With<GridCell>, Without<PlayingEntity>)>,
    pub playing: Query<'w, 's, (Entity, &'static CartesianPosition), With<PlayingEntity>>,
    global_transforms: Query<'w, 's, &'static GlobalTransform>,
    rng: Single<'w, 's, &'static mut WyRand, With<GlobalRng>>,
}

fn rand_u32(mut rng: Single<&mut WyRand, With<GlobalRng>>) -> u32 {
    rng.next_u32() / u32::MAX
}

fn rand_u32_range(rng: Single<&mut WyRand, With<GlobalRng>>, min: u32, max: u32) -> u32 {
    min + rand_u32(rng) * (max - min)
}

pub enum SmoothingShape {
    Circle(u32),
    // Diamond,
    // RoundedSquare,
    // Square,
    // Superellipse(i32), // custom n exponent
    Annulus { inner: f32, outer: f32 },
    Gaussian { std_dev_ratio: f32, radius: f32 }, // ratio of r, e.g. 0.33 = r/3
}

impl SmoothingShape {
    fn exponent(&self) -> Option<i32> {
        match self {
            SmoothingShape::Circle(_) => Some(2),
            // SmoothingShape::Diamond => Some(1),
            // SmoothingShape::RoundedSquare => Some(4),
            // SmoothingShape::Square => Some(i32::MAX),
            // SmoothingShape::Superellipse(n) => Some(*n),
            _ => None,
        }
    }
}

pub fn random_in_shape(mut rng: WyRand, shape: &SmoothingShape) -> (i32, i32) {
    match shape {
        SmoothingShape::Circle(r) => {
            let angle = rng.random_range(0.0..TAU);
            let radius = (*r as f32) * rng.random::<f32>().sqrt();
            ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
        }

        SmoothingShape::Annulus { inner, outer } => {
            let angle = rng.random_range(0.0..TAU);
            let t = rng.random::<f32>();
            let radius: f32 = (inner * inner + t * (outer * outer - inner * inner)).sqrt();
            ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
        }

        SmoothingShape::Gaussian {
            std_dev_ratio,
            radius,
        } => {
            let std_dev = radius * std_dev_ratio;
            let u1: f32 = rng.random();
            let u2: f32 = rng.random();
            let mag = std_dev * (-2.0 * u1.ln()).sqrt();
            (
                (mag * (TAU * u2).cos()) as i32,
                (mag * (TAU * u2).sin()) as i32,
            )
        }
    }
}

// pub fn random_in_shape_specific(
//     mut rng: Single<&mut WyRand, With<GlobalRng>>,
//     shape: &SmoothingShape,
//     rad: f32,
// ) -> (i32, i32) {
//     match shape {
//         SmoothingShape::Circle(r) => {
//             let angle = rng.random_range(0.0..TAU);
//             let radius = r * rng.random::<f32>().sqrt();
//             ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
//         }

//         SmoothingShape::Annulus { inner, outer } => {
//             let angle = rng.random_range(0.0..TAU);
//             let t = rng.random::<f32>();
//             let radius: f32 = (inner * inner + t * (outer * outer - inner * inner)).sqrt();
//             ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
//         }

//         SmoothingShape::Gaussian {
//             std_dev_ratio,
//             radius,
//         } => {
//             let std_dev = radius * std_dev_ratio;
//             let u1: f32 = rng.random();
//             let u2: f32 = rng.random();
//             let mag = std_dev * (-2.0 * u1.ln()).sqrt();
//             (
//                 (mag * (TAU * u2).cos()) as i32,
//                 (mag * (TAU * u2).sin()) as i32,
//             )
//         }

//         shape => {
//             let n = shape.exponent().unwrap();
//             let rf = rad;
//             if n == f32::INFINITY {
//                 let r = rad as i32;
//                 return (rng.random_range(-r..=r), rng.random_range(-r..=r));
//             }
//             let r_i = rad as i32;
//             loop {
//                 let x = rng.random_range(-r_i..=r_i);
//                 let y = rng.random_range(-r_i..=r_i);
//                 let xf = x as f32 / rf;
//                 let yf = y as f32 / rf;
//                 if xf.abs().powf(n) + yf.abs().powf(n) <= 1.0 {
//                     return (x, y);
//                 }
//             }
//         }
//     }
// }

#[derive(Clone, Debug, AttributeResolvable)]
pub struct GridDirectionOffset {
    #[skip]
    pub dir: Dir3,
    pub min_dist: i32,
    pub max_dist: i32,
}

#[derive(Clone, Debug, AttributeResolvable)]
pub struct GridFixedOffset {
    x: u32,
    y: u32,
    z: u32,
}

impl GridFixedOffset {
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    pub fn from_cartesian_pos(pos: CartesianPosition) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum GridPosOffset {
    None,
    Fixed(GridFixedOffset),
    RandomInDir(GridDirectionOffset),
    RandomInCircle(u32),
    RandomInSphere(u32),
}

impl GridPosOffset {
    pub fn apply_to(
        self: &Self,
        target: CartesianPosition,
        mut rng: WyRand,
        grid: &CartesianGrid<Cartesian3D>,
    ) -> CartesianPosition {
        match self {
            GridPosOffset::None => target,
            GridPosOffset::Fixed(grid_fixed_offset) => CartesianPosition {
                x: target.x + grid_fixed_offset.x,
                y: target.y + grid_fixed_offset.y,
                z: target.z + grid_fixed_offset.z,
            }
            .grid_clamped(grid),
            GridPosOffset::RandomInDir(grid_direction_offset) => {
                let random_dist = rng
                    .random_range(grid_direction_offset.min_dist..grid_direction_offset.max_dist);

                let random_offset = random_dist as f32 * grid_direction_offset.dir;
                CartesianPosition {
                    x: (target.x as f32 + random_offset.x.trunc()).max(0.) as u32,
                    y: (target.y as f32 + random_offset.y.trunc()).max(0.) as u32,
                    z: (target.z as f32 + random_offset.z.trunc()).max(0.) as u32,
                }
            }
            GridPosOffset::RandomInCircle(radius) => {
                let (x, z) = random_in_shape(rng, &SmoothingShape::Circle(*radius));
                CartesianPosition {
                    x: (target.x as i32 + x).max(0) as u32,
                    y: target.y,
                    z: (target.z as i32 + z).max(0) as u32,
                }
                .grid_clamped(grid)
            }
            GridPosOffset::RandomInSphere(radius) => {
                let sphere = Sphere::new(*radius as f32);
                let rand_offset = sphere.sample_interior(&mut rng);
                CartesianPosition::new(
                    (target.x as f32 + rand_offset.x).max(0.) as u32,
                    (target.y as f32 + rand_offset.y).max(0.) as u32,
                    (target.z as f32 + rand_offset.z).max(0.) as u32,
                )
                .grid_clamped(grid)
            }
        }
    }
}

pub trait InGridBoundaries {
    fn grid_clamped(self: Self, grid: &CartesianGrid<Cartesian3D>) -> Self;
}

impl InGridBoundaries for CartesianPosition {
    fn grid_clamped(mut self: Self, grid: &CartesianGrid<Cartesian3D>) -> Self {
        self.x = self.x.clamp(0, grid.size_x());
        self.y = self.y.clamp(0, grid.size_y());
        self.z = self.z.clamp(0, grid.size_z());
        self
    }
}

impl Default for GridPosOffset {
    fn default() -> Self {
        GridPosOffset::None
    }
}

pub fn get_entity_targets_in_shape(
    origin: CartesianPosition,
    potential_targets: &mut Vec<(Entity, CartesianPosition)>,
    shape: &GridCheckShape,
    sort_by_nearest: bool,
) -> Vec<GridTarget> {
    let radius = match shape {
        GridCheckShape::Circle(r) => *r,
        GridCheckShape::Sphere(r) => *r,
    };

    let mut unsorted: Vec<(f32, GridTarget)> = potential_targets
        .iter()
        .filter_map(|(e, node_pos)| {
            match shape {
                GridCheckShape::Circle(_) => {
                    if node_pos.y != origin.y {
                        return None;
                    }
                }
                _ => {}
            }

            let distance = Vec3::new(node_pos.x as f32, node_pos.y as f32, node_pos.z as f32)
                .distance(Vec3::new(origin.x as f32, origin.y as f32, origin.z as f32));
            if distance <= radius {
                Some((distance, GridTarget::entity(*e, *node_pos)))
            } else {
                None
            }
        })
        .collect();

    if !sort_by_nearest {
        return unsorted.iter().map(|(_, t)| *t).collect();
    }

    unsorted.sort_by(|(dist_a, _), (dist_b, _)| dist_a.total_cmp(&dist_b));
    unsorted.iter().map(|(_, t)| *t).collect()
}

pub struct Grid3DBackend;

impl SpatialBackend for Grid3DBackend {
    type Pos = CartesianPosition;

    type Offset = GridPosOffset;

    type Gatherer = Grid3DGatherer;

    type Filter = Grid3DFilter;

    type Context<'w, 's> = Grid3DContext<'w, 's>;

    fn apply_offset(
        ctx: &mut Self::Context<'_, '_>,
        pos: Self::Pos,
        offset: &Self::Offset,
    ) -> Self::Pos {
        offset.apply_to(pos, ctx.rng.fork(), &ctx.grid.deref())
    }

    fn distance(a: &Self::Pos, b: &Self::Pos) -> f32 {
        a.manhattan_distance(b) as f32
    }

    fn position_of(ctx: &Self::Context<'_, '_>, entity: Entity) -> Option<Self::Pos> {
        ctx.grid_cells
            .get(entity)
            .ok()
            .map(|c| ctx.grid.pos_from_index(c.1.0))
            // The grid_cells from the context seems to exclude the player, so I added this extra lookup.
            .or_else(|| ctx.playing.get(entity).ok().map(|(_, pos)| *pos))
    }

    fn gather(
        ctx: &mut Self::Context<'_, '_>,
        origin: Self::Pos,
        gatherer: &Self::Gatherer,
        exclude: Entity,
    ) -> Vec<bevy_diesel::prelude::Target<Self::Pos>> {
        match gatherer {
            Grid3DGatherer::Sphere { radius, count } => {
                let rng = ctx.rng.fork();
                let n = count.resolve_count(rng.clone());
                (0..n)
                    .map(|_| {
                        let (rand_x, rand_z) =
                            random_in_shape(rng.clone(), &SmoothingShape::Circle(*radius as u32));
                        let pos = CartesianPosition::new(
                            (origin.x as i32 + rand_x) as u32,
                            origin.y,
                            (origin.z as i32 + rand_z) as u32,
                        );
                        Target::position(pos)
                    })
                    .collect()
            }
            Grid3DGatherer::Circle { radius, count } => {
                let rng = ctx.rng.fork();
                let n = count.resolve_count(rng.clone());
                (0..n)
                    .map(|_| {
                        let (rand_x, rand_z) =
                            random_in_shape(rng.clone(), &SmoothingShape::Circle(*radius as u32));
                        let pos = CartesianPosition::new(
                            (origin.x as i32 + rand_x) as u32,
                            origin.y,
                            (origin.z as i32 + rand_z) as u32,
                        );
                        Target::position(pos)
                    })
                    .collect()
            }
            // Grid3DGatherer::Box {
            //     half_extents,
            //     count,
            // } => todo!(),
            // Grid3DGatherer::Line {
            //     direction,
            //     length,
            //     count,
            // } => todo!(),
            Grid3DGatherer::EntitiesInShape {
                shape,
                gathering_filter,
                sort_by_nearest,
            } => {
                let grid = ctx.grid.deref();
                let mut found_entities: Vec<Target<Self::Pos>> = vec![];

                if *gathering_filter == EntityGatheringFilter::Cells
                    || *gathering_filter == EntityGatheringFilter::All
                {
                    let mut potential_targets: Vec<(Entity, CartesianPosition)> = ctx
                        .grid_cells
                        .iter()
                        .filter_map(|(e, grid_node)| {
                            if e == exclude {
                                return None;
                            }

                            let node_index = grid_node.0;
                            let node_pos = grid.pos_from_index(node_index);
                            Some((e, node_pos))
                        })
                        .collect();

                    let mut matching_targets = get_entity_targets_in_shape(
                        origin,
                        &mut potential_targets,
                        shape,
                        *sort_by_nearest,
                    );

                    found_entities.append(&mut matching_targets);
                }

                if *gathering_filter == EntityGatheringFilter::Playing
                    || *gathering_filter == EntityGatheringFilter::All
                {
                    let mut potential_targets: Vec<(Entity, CartesianPosition)> = ctx
                        .playing
                        .iter()
                        .filter_map(|(e, cartesian_pos)| {
                            if e == exclude {
                                return None;
                            }
                            Some((e, *cartesian_pos))
                        })
                        .collect();

                    let mut matching_targets = get_entity_targets_in_shape(
                        origin,
                        &mut potential_targets,
                        shape,
                        *sort_by_nearest,
                    );

                    found_entities.append(&mut matching_targets);
                }

                found_entities
            }
        }
    }

    fn apply_filter(
        ctx: &mut Self::Context<'_, '_>,
        targets: Vec<bevy_diesel::prelude::Target<Self::Pos>>,
        filter: &Self::Filter,
        invoker: bevy::ecs::entity::Entity,
        origin: Self::Pos,
    ) -> Vec<bevy_diesel::prelude::Target<Self::Pos>> {
        let resolved_count = match filter.count {
            NumberType::All => usize::MAX,
            NumberType::Fixed(n) => n,
            NumberType::Random { min, max } => ctx.rng.random_range(min..max),
        };

        targets.into_iter().take(resolved_count).collect()
    }

    fn insert_position(
        commands: &mut bevy::ecs::system::EntityCommands,
        ctx: &Self::Context<'_, '_>,
        pos: Self::Pos,
        parent: Option<bevy::ecs::entity::Entity>,
    ) {
        let world_pos = pos.as_world_pos(ctx.grid_tf.translation);

        let transform = if let Some(parent_entity) = parent {
            if let Ok(parent_gt) = ctx.global_transforms.get(parent_entity) {
                let local_pos = parent_gt.affine().inverse().transform_point3(world_pos);
                Transform::from_translation(local_pos)
            } else {
                Transform::from_translation(world_pos)
            }
        } else {
            Transform::from_translation(world_pos)
        };
        commands.insert(transform);
    }

    fn plugin() -> impl Plugin {
        Grid3dDieselPlugin
    }
}
