use std::{f32::consts::TAU, fmt::Debug, i32, marker::PhantomData, ops::Deref, u32};

use bevy::{
    app::{App, Plugin, Update},
    ecs::{
        component::Component,
        entity::Entity,
        event::EntityEvent,
        hierarchy::ChildOf,
        message::{Message, MessageReader, MessageWriter},
        query::{With, Without},
        system::{Query, Single, SystemParam},
    },
    math::{Dir3, ShapeSample, Vec3, primitives::Sphere},
    prelude::IntoScheduleConfigs,
    reflect::Reflect,
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::{
    effect::{GoOff, GoOffOrigin, SubEffects},
    events::HasDieselTarget,
    prelude::{InvokedBy, generate_targets, resolve_invoker, resolve_root},
    target::{InvokerTarget, TargetMutator},
};
use bevy_diesel::{pipeline::propagate_system, prelude::SpatialBackend, target::Target};
use bevy_ecs::{hierarchy::Children, system::Commands};
use bevy_gauge::AttributeResolvable;
use bevy_gearbox::{AcceptAll, GearboxMessage, GearboxSet, RegistrationAppExt};
use bevy_ghx_proc_gen::GridNode;
use bevy_prng::WyRand;
use bevy_rand::{plugin::EntropyPlugin, prelude::GlobalRng};
use rand::{Rng, RngExt, SeedableRng};

use crate::{
    CardInPile, DrawCard, InDrawPile, InHand,
    abilities::{
        abilities_templates::ActionCastData,
        effects::{AbilityOfCaster, handle_invoke_subability_effect, handle_spawn_effect},
    },
    deck::deck_and_cards::Card,
    game_flow::turns::PlayingEntity,
};

// Vec3 type aliases
pub type DeckInvokerTarget = bevy_diesel::target::InvokerTarget<BoardPos>;
pub type DeckTarget = bevy_diesel::target::Target<BoardPos>;
pub type DeckGoOff = bevy_diesel::effect::GoOff<BoardPos>;
pub type DeckStartInvoke = bevy_diesel::events::StartInvoke<BoardPos>;
pub type DeckStopInvoke = bevy_diesel::events::StopInvoke<BoardPos>;
pub type DeckOnRepeat = bevy_diesel::events::OnRepeat<BoardPos>;
pub type DeckOnSpawnOrigin = bevy_diesel::spawn::OnSpawnOrigin<BoardPos>;
pub type DeckOnSpawnTarget = bevy_diesel::spawn::OnSpawnTarget<BoardPos>;
pub type DeckOnSpawnInvoker = bevy_diesel::spawn::OnSpawnInvoker<BoardPos>;
pub type DeckTargetType = bevy_diesel::target::TargetType<BoardPos>;
pub type DeckTargetGenerator = bevy_diesel::target::TargetGenerator<DeckBackend>;
pub type DeckTargetMutator = bevy_diesel::target::TargetMutator<DeckBackend>;
pub type DeckSpawnConfig = bevy_diesel::spawn::SpawnConfig<DeckBackend>;
pub type DeckGoOffConfig = bevy_diesel::effect::GoOffConfig<DeckBackend>;
pub type DeckGoOffOrigin = bevy_diesel::effect::GoOffOrigin<DeckBackend>;

#[derive(Debug, Clone, Reflect, PartialEq)]
pub enum HitTargetKind {
    Playing,
    Cell,
}

impl HitTargetKind {
    pub fn is_player(&self) -> bool {
        match self {
            HitTargetKind::Playing => true,
            _ => false,
        }
    }
}

/// Collision with an entity target.
#[derive(Message, Clone, Debug, Reflect, EntityEvent)]
#[entity_event(propagate = &'static InvokedBy, auto_propagate)]
pub struct AbilityHitEntity {
    pub entity: Entity,
    pub attacking_player: Entity,
    pub target: DeckTarget,
    pub target_kind: HitTargetKind,
}

impl GearboxMessage for AbilityHitEntity {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

impl AbilityHitEntity {
    pub fn new(
        entity: Entity,
        attacking_player: Entity,
        target: DeckTarget,
        target_kind: HitTargetKind,
    ) -> Self {
        Self {
            entity,
            attacking_player,
            target,
            target_kind,
        }
    }
}

/// Parent caster was ended
#[derive(Message, Clone, Debug, Reflect, EntityEvent)]
pub struct CastEnd {
    pub entity: Entity,
    pub target: DeckTarget,
}

impl CastEnd {
    pub fn new(entity: Entity, target: DeckTarget) -> Self {
        Self { entity, target }
    }
}

impl GearboxMessage for CastEnd {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

impl HasDieselTarget<BoardPos> for CastEnd {
    fn diesel_target(&self) -> Target<BoardPos> {
        self.target
    }
}

/// Parent caster was invoked
#[derive(Message, Clone, Debug, Reflect)]
pub struct StartCast {
    pub entity: Entity,
}

impl StartCast {
    pub fn new(entity: Entity) -> Self {
        Self { entity }
    }
}

impl GearboxMessage for StartCast {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

/// Collision with a contact point position.
#[derive(Message, Clone, Debug, Reflect)]
pub struct AbilityHitPosition {
    pub entity: Entity,
    pub target: DeckTarget,
}

impl GearboxMessage for AbilityHitPosition {
    type Validator = AcceptAll;
    fn target(&self) -> Entity {
        self.entity
    }
}

impl AbilityHitPosition {
    pub fn new(entity: Entity, target: DeckTarget) -> Self {
        Self { entity, target }
    }
}

impl HasDieselTarget<BoardPos> for AbilityHitEntity {
    fn diesel_target(&self) -> DeckTarget {
        self.target
    }
}

impl HasDieselTarget<BoardPos> for AbilityHitPosition {
    fn diesel_target(&self) -> DeckTarget {
        self.target
    }
}

// pub trait HitFilter: Component + Clone + Debug + Send + Sync + 'static {
//     /// Component queried on invoker and target entities.
//     type Lookup: Component;

//     /// Return `true` if the ability should affect this target.
//     fn can_target(
//         &self,
//         invoker_data: Option<&Self::Lookup>,
//         target_data: Option<&Self::Lookup>,
//     ) -> bool;
// }

// struct HitFilterPlugin<F: HitFilter> {
//     _marker: PhantomData<F>,
// }

// impl<F: HitFilter> Default for HitFilterPlugin<F> {
//     fn default() -> Self {
//         Self {
//             _marker: PhantomData,
//         }
//     }
// }

// impl<F: HitFilter> Plugin for HitFilterPlugin<F> {
//     fn build(&self, app: &mut App) {
//         app.add_systems(Update, handle_hit_system::<F>);
//     }
// }

// pub struct HitHandlingPlugin;

// impl Plugin for HitHandlingPlugin {
//     fn build(&self, app: &mut App) {
//         app.add_plugins(HitFilterPlugin::<TeamHitFilter>::default())
//             .add_systems(Update, handle_unfiltered_hit_system.before(GearboxSet));
//     }
// }

#[derive(EntityEvent, Message)]
pub struct HitReceived {
    #[event_target]
    pub hit_player: Entity,
    pub ability_entity: Entity,
    pub cast_data: ActionCastData,
}

impl HitReceived {
    pub fn new(hit_player: Entity, ability_entity: Entity, cast_data: ActionCastData) -> Self {
        Self {
            hit_player,
            ability_entity,
            cast_data,
        }
    }
}

pub fn handle_unfiltered_hit_system(
    mut hit_events: MessageReader<HitReceived>,
    mut cmd: Commands,
    grid_playing_q: Query<&BoardPos>,
    invoked_q: Query<&InvokedBy>,
    mut entity_writer: MessageWriter<AbilityHitEntity>,
    mut cast_end_writer: MessageWriter<CastEnd>,
    _position_writer: MessageWriter<AbilityHitPosition>,
) {
    println!("needs rewrite");
}

// pub fn handle_hit_system<F: HitFilter>(
//     mut hit_events: MessageReader<HitReceived>,
//     mut cmd: Commands,
//     _invoker_q: Query<&InvokedBy>,
//     grid: Single<&CartesianGrid<Cartesian3D>>,
//     cards: Query<&Card>,
//     filters_q: Query<&F>,
//     filter_lookup_q: Query<&F::Lookup>,
//     mut entity_writer: MessageWriter<AbilityHitEntity>,
//     _position_writer: MessageWriter<AbilityHitPosition>,
// ) {
//     let grid = grid.deref();

//     for hit in hit_events.read() {
//         match filters_q.get(hit.hit_player) {
//             Ok(filter) => {
//                 let invoker_data = filter_lookup_q.get(hit.ability_entity).ok();
//                 let target_data = filter_lookup_q.get(hit.hit_player).ok();
//                 if !filter.can_target(invoker_data, target_data) {
//                     continue;
//                 }
//             }
//             Err(_) => {}
//         };

//         if let Ok(cell) = grid_cells_q.get(hit.hit_player) {
//             let pos = grid.pos_from_index(cell.0);
//             let evt = AbilityHitEntity::new(
//                 hit.hit_player,
//                 hit.cast_data.source_playing_entity,
//                 GridTarget::entity(hit.ability_entity, pos),
//                 HitTargetKind::Cell,
//             );
//             entity_writer.write(evt.clone());
//             cmd.trigger(evt);
//         } else {
//             if let Ok(playing_pos) = grid_playing_q.get(hit.hit_player) {
//                 let evt = AbilityHitEntity::new(
//                     hit.hit_player,
//                     hit.cast_data.source_playing_entity,
//                     GridTarget::entity(hit.ability_entity, *playing_pos),
//                     HitTargetKind::Playing,
//                 );
//                 entity_writer.write(evt.clone());
//                 cmd.trigger(evt);
//             }
//         }
//     }
// }

pub struct BoardDieselPlugin;

impl Plugin for BoardDieselPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_message::<HitReceived>();

        app.add_plugins(EntropyPlugin::<bevy_prng::WyRand>::default());
        app.add_plugins(DeckBackend::plugin_core());

        use bevy_diesel::bevy_gauge::prelude::AttributesAppExt;
        app.register_attribute_derived::<DeckSpawnConfig>();
        app.register_attribute_derived::<DeckTargetMutator>();

        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::effect::go_off_on_entry::<DeckBackend>,
                propagate_system::<DeckBackend>,
            )
                .chain()
                .in_set(bevy_diesel::DieselSet::Propagation),
        );

        // Leaf effect systems: read GoOff
        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::spawn::spawn_system::<DeckBackend>,
                bevy_diesel::print::print_effect::<BoardPos>,
                handle_spawn_effect,
                handle_invoke_subability_effect,
            )
                .in_set(bevy_diesel::DieselSet::Effects),
        );

        // Sustained modifier apply — monomorphized here because the generic
        // fn needs B::Context which can only resolve with a concrete backend.
        app.add_systems(
            Update,
            bevy_diesel::gauge::modifiers::sustained_modifier_apply::<DeckBackend>
                .in_set(bevy_diesel::gauge::SustainedModifierSet),
        );

        // #TODO: Will need to do something similar
        // // Collision types + system (unfiltered - entities with Collides marker)
        app.register_transition::<DrawCard>();
        app.register_transition::<AbilityHitEntity>();
        app.register_transition::<AbilityHitPosition>();
        app.register_transition::<StartCast>();
        app.register_transition::<CastEnd>();
        app.register_transition::<DeckStartInvoke>();

        app.register_state_component::<InHand>();
        app.register_state_component::<InDrawPile>();

        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::events::go_off_side_effect::<AbilityHitEntity, BoardPos>
                    .in_set(bevy_diesel::bevy_gearbox::GearboxPhase::SideEffectPhase),
                bevy_diesel::events::go_off_side_effect::<AbilityHitPosition, BoardPos>
                    .in_set(bevy_diesel::bevy_gearbox::GearboxPhase::SideEffectPhase),
                bevy_diesel::events::go_off_side_effect::<CastEnd, BoardPos>
                    .in_set(bevy_diesel::bevy_gearbox::GearboxPhase::SideEffectPhase),
            ),
        );

        // app.add_plugins(HitHandlingPlugin);
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
pub enum BoardGatherer {
    NextCard,
    PrevCard,
    OffsetBy(i32),
}

#[derive(Clone, Debug, AttributeResolvable)]
pub struct BoardFilter {
    /// Max target count. `NumberType::All` passes everything through.
    pub count: NumberType,
}

impl BoardFilter {
    pub fn new(count: NumberType) -> Self {
        Self { count }
    }
}

impl Default for BoardFilter {
    fn default() -> Self {
        Self {
            count: NumberType::All,
        }
    }
}

#[derive(SystemParam)]
pub struct BoardContext<'w, 's> {
    pub playing: Query<'w, 's, (Entity, &'static BoardPos), With<PlayingEntity>>,
    pub cards: Query<'w, 's, (Entity, &'static Card, &'static BoardPos)>,
    global_transforms: Query<'w, 's, &'static GlobalTransform>,
    rng: Single<'w, 's, &'static mut WyRand, With<GlobalRng>>,
}

fn rand_u32(mut rng: Single<&mut WyRand, With<GlobalRng>>) -> u32 {
    rng.next_u32() / u32::MAX
}

fn rand_u32_range(rng: Single<&mut WyRand, With<GlobalRng>>, min: u32, max: u32) -> u32 {
    min + rand_u32(rng) * (max - min)
}

pub struct DeckBackend;

#[derive(Reflect, Debug, Default, Clone, Copy, AttributeResolvable, Component)]
pub struct BoardPos {
    is_player: bool,
    hand_index: i32,
}

impl BoardPos {
    pub fn new_on_player(hand_index: i32) -> Self {
        Self {
            is_player: true,
            hand_index,
        }
    }

    pub fn new_on_enemy(hand_index: i32) -> Self {
        Self {
            is_player: false,
            hand_index,
        }
    }
}

impl SpatialBackend for DeckBackend {
    type Pos = BoardPos;

    type Offset = BoardPos;

    type Gatherer = BoardGatherer;

    type Filter = BoardFilter;

    type Context<'w, 's> = BoardContext<'w, 's>;

    fn apply_offset(
        ctx: &mut Self::Context<'_, '_>,
        pos: Self::Pos,
        offset: &Self::Offset,
    ) -> Self::Pos {
        BoardPos::new_on_player(pos.hand_index + offset.hand_index)
    }

    fn distance(a: &Self::Pos, b: &Self::Pos) -> f32 {
        (a.hand_index - b.hand_index) as f32
    }

    fn position_of(ctx: &Self::Context<'_, '_>, entity: Entity) -> Option<Self::Pos> {
        ctx.cards.get(entity).ok().map(|c| *c.2)
    }

    fn gather(
        ctx: &mut Self::Context<'_, '_>,
        origin: Self::Pos,
        gatherer: &Self::Gatherer,
        exclude: Entity,
    ) -> Vec<bevy_diesel::prelude::Target<Self::Pos>> {
        let index_offset = match gatherer {
            BoardGatherer::NextCard => 1,
            BoardGatherer::PrevCard => -1,
            BoardGatherer::OffsetBy(offset) => *offset,
        };

        ctx.cards
            .iter()
            .find(|(_, _, pos)| pos.hand_index == (origin.hand_index + index_offset))
            .map_or(vec![], |(card, _, pos)| {
                vec![DeckTarget::entity(card, *pos)]
            })
    }

    fn apply_filter(
        ctx: &mut Self::Context<'_, '_>,
        targets: Vec<bevy_diesel::prelude::Target<Self::Pos>>,
        filter: &Self::Filter,
        _invoker: bevy::ecs::entity::Entity,
        _origin: Self::Pos,
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
        commands.insert(pos.clone());
    }

    fn plugin() -> impl Plugin {
        BoardDieselPlugin
    }
}
