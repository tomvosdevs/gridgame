use std::{f32::consts::TAU, fmt::Debug, i32, marker::PhantomData, u32};

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
    DieselSet,
    effect::{GoOff, GoOffConfig, GoOffOrigin, SubEffects, go_off_on_entry},
    events::{self, HasDieselTarget, OnRepeat, StartInvoke, StopInvoke, go_off_side_effect},
    gauge::AttributeResolvable,
    gearbox::{EdgeTimer, GearboxSchedule},
    invoke::Ability,
    prelude::{
        GearboxPhase, InvokedBy, Source, SubstateOf, SustainedModifierSet, generate_targets,
        resolve_invoker, resolve_root,
    },
    print::print_effect,
    spawn::{self, OnSpawnInvoker, OnSpawnOrigin, OnSpawnTarget, SpawnConfig, spawn_system},
    target::{self, InvokerTarget, Scope, Target, TargetGenerator, TargetMutator, TargetType},
};
use bevy_diesel::{
    gauge_ext::modifiers::sustained_modifier_apply,
    gearbox::{AcceptAll, GearboxMessage, GearboxSet, RegistrationAppExt},
};
use bevy_diesel::{pipeline::propagate_system, prelude::SpatialBackend};
use bevy_ecs::{
    hierarchy::Children,
    lifecycle::Add,
    observer::On,
    system::{Commands, Res},
};

use bevy_prng::WyRand;
use bevy_rand::{plugin::EntropyPlugin, prelude::GlobalRng};
use rand::{Rng, RngExt, SeedableRng};

use crate::{
    CardCast, CardInPile, CastTicksRequirement, DrawCard, EnemyData, InDrawPile, InHand,
    PlayerData, PosInDeck, Ticking, TicksSinceCast,
    abilities::{
        abilities_templates::ActionCastData,
        effects::{AbilityOfCaster, handle_invoke_subability_effect, handle_spawn_effect},
    },
    deck::deck_and_cards::Card,
    game_flow::turns::PlayingEntity,
};

// Vec3 type aliases
pub type DeckInvokerTarget = InvokerTarget<PosInDeck>;
pub type DeckTarget = Target<PosInDeck>;
pub type DeckGoOff = GoOff<PosInDeck>;
pub type DeckStartInvoke = StartInvoke<PosInDeck>;
pub type DeckStopInvoke = StopInvoke<PosInDeck>;
pub type DeckOnRepeat = OnRepeat<PosInDeck>;
pub type DeckOnSpawnOrigin = OnSpawnOrigin<PosInDeck>;
pub type DeckOnSpawnTarget = OnSpawnTarget<PosInDeck>;
pub type DeckOnSpawnInvoker = OnSpawnInvoker<PosInDeck>;
pub type DeckTargetType = TargetType<PosInDeck>;
pub type DeckTargetGenerator = TargetGenerator<DeckBackend>;
pub type DeckTargetMutator = TargetMutator<DeckBackend>;
pub type DeckSpawnConfig = SpawnConfig<DeckBackend>;
pub type DeckGoOffConfig = GoOffConfig<DeckBackend>;
pub type DeckGoOffOrigin = GoOffOrigin<DeckBackend>;

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

impl HasDieselTarget<PosInDeck> for CastEnd {
    fn diesel_target(&self) -> Target<PosInDeck> {
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

impl HasDieselTarget<PosInDeck> for AbilityHitEntity {
    fn diesel_target(&self) -> DeckTarget {
        self.target
    }
}

impl HasDieselTarget<PosInDeck> for AbilityHitPosition {
    fn diesel_target(&self) -> DeckTarget {
        self.target
    }
}

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
    grid_playing_q: Query<&PosInDeck>,
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

        use bevy_diesel::gauge::prelude::AttributesAppExt;
        app.register_attribute_derived::<DeckSpawnConfig>();
        app.register_attribute_derived::<DeckTargetMutator>();

        app.add_systems(
            GearboxSchedule,
            (
                go_off_on_entry::<DeckBackend>,
                propagate_system::<DeckBackend>,
            )
                .chain()
                .in_set(DieselSet::Propagation),
        );

        // Leaf effect systems: read GoOff
        app.add_systems(
            GearboxSchedule,
            (
                spawn_system::<DeckBackend>,
                print_effect::<PosInDeck>,
                handle_spawn_effect,
                handle_invoke_subability_effect,
            )
                .in_set(DieselSet::Effects),
        );

        // Sustained modifier apply — monomorphized here because the generic
        // fn needs B::Context which can only resolve with a concrete backend.
        app.add_systems(
            Update,
            sustained_modifier_apply::<DeckBackend>.in_set(SustainedModifierSet),
        );

        app.add_observer(log_test_ticker_setup);

        // #TODO: Will need to do something similar
        // // Collision types + system (unfiltered - entities with Collides marker)
        app.register_transition::<DrawCard>();
        app.register_transition::<CardCast>();
        app.register_transition::<AbilityHitEntity>();
        app.register_transition::<AbilityHitPosition>();
        app.register_transition::<StartCast>();
        app.register_transition::<CastEnd>();
        app.register_transition::<DeckStartInvoke>();

        app.register_state_component::<InHand>();
        app.register_state_component::<InDrawPile>();

        // app.add_systems(
        //     GearboxSchedule,
        //     (
        //         go_off_side_effect::<AbilityHitEntity, PosInDeck>
        //             .in_set(GearboxPhase::SideEffectPhase),
        //         go_off_side_effect::<AbilityHitPosition, PosInDeck>
        //             .in_set(GearboxPhase::SideEffectPhase),
        //         go_off_side_effect::<CastEnd, PosInDeck>.in_set(GearboxPhase::SideEffectPhase),
        //     ),
        // );

        // app.add_plugins(HitHandlingPlugin);
    }
}

pub struct TickEdgeTimer {
    current: u32,
    // Point this to the CardCast ? Or use attributes ?
    cast_source: Entity,
}

fn log_test_ticker_setup(
    e: On<Add, Ticking>,
    q: Query<(&CastTicksRequirement, &TicksSinceCast), (With<Ticking>, With<Ability>)>,
    q_abilities: Query<(), With<Ability>>,
    mut cmd: Commands,
) {
    // Todo : Later, change requirement to relationship with children,
    // each representing a requirement, and check if one with TickAmount
    // exists AND / OR use gauge attributes / condition check
    let (tick_requirement, ticks_since_cast) = q.get(e.entity).expect(
        "Any 'Ticking' component should only be
            added to entities that also have Ability,
            CastTicksRequirement and TicksSinceCast",
    );
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

#[derive(Debug, Clone, PartialEq, Eq, AttributeResolvable)]
pub enum GatherMode {
    RandomPick,
    FromStart,
    FromEnd,
}
#[derive(Debug, Clone, PartialEq, Eq, AttributeResolvable)]
pub enum PileType {
    Hand,
    Draw,
    Both,
}
#[derive(Debug, Clone, PartialEq, Eq, AttributeResolvable)]
pub enum PlayerType {
    Player,
    Enemy,
    Both,
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum BoardGatherer {
    Around(u32),
    OnRight(u32),
    OnLeft(u32),
    Piles {
        piles: PileType,
        players: PlayerType,
        amount: u32,
        mode: GatherMode,
    },
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
    pub player_data: Option<Res<'w, PlayerData>>,
    pub enemy_data: Option<Res<'w, EnemyData>>,
    pub cards: Query<'w, 's, (Entity, &'static Card, &'static PosInDeck)>,
    global_transforms: Query<'w, 's, &'static GlobalTransform>,
    rng: Single<'w, 's, &'static mut WyRand, With<GlobalRng>>,
}

fn rand_u32(mut rng: Single<&mut WyRand, With<GlobalRng>>) -> u32 {
    rng.next_u32() / u32::MAX
}

fn rand_u32_range(rng: Single<&mut WyRand, With<GlobalRng>>, min: u32, max: u32) -> u32 {
    min + rand_u32(rng) * (max - min)
}

#[derive(Clone, Copy, Default)]
pub struct DeckBackend;

// #[derive(Reflect, Debug, Default, Clone, Copy, AttributeResolvable, Component)]
// pub struct PosInDeck {
//     is_player: bool,
//     hand_index: i32,
// }

// impl PosInDeck {
//     pub fn new_on_player(hand_index: i32) -> Self {
//         Self {
//             is_player: true,
//             hand_index,
//         }
//     }

//     pub fn new_on_enemy(hand_index: i32) -> Self {
//         Self {
//             is_player: false,
//             hand_index,
//         }
//     }
// }

impl SpatialBackend for DeckBackend {
    type Pos = PosInDeck;

    type Offset = PosInDeck;

    type Gatherer = BoardGatherer;

    type Filter = BoardFilter;

    type Context<'w, 's> = BoardContext<'w, 's>;

    fn apply_offset(
        ctx: &mut Self::Context<'_, '_>,
        pos: Self::Pos,
        offset: &Self::Offset,
    ) -> Self::Pos {
        let mut new = pos.clone();
        new.index += offset.index;
        new
    }

    fn distance(a: &Self::Pos, b: &Self::Pos) -> f32 {
        (a.index - b.index) as f32
    }

    fn position_of(ctx: &Self::Context<'_, '_>, entity: Entity) -> Option<Self::Pos> {
        ctx.cards.get(entity).ok().map(|c| *c.2)
    }

    fn gather(
        ctx: &mut Self::Context<'_, '_>,
        origin: Self::Pos,
        gatherer: &Self::Gatherer,
        exclude: Entity,
    ) -> Vec<(Target<Self::Pos>, Scope)> {
        let cards = match gatherer {
            BoardGatherer::Around(amount)
            | BoardGatherer::OnRight(amount)
            | BoardGatherer::OnLeft(amount) => {
                let mut start = origin.index - amount;
                let mut end = origin.index - amount;
                match gatherer {
                    BoardGatherer::OnRight(_) => start = origin.index,
                    BoardGatherer::OnLeft(_) => end = origin.index,
                    _ => {}
                }

                let gather_range = start..end;

                let mut res = ctx
                    .cards
                    .iter()
                    .filter(|(e, _, pos)| *e != exclude && pos.deck == origin.deck)
                    .find(|(_, _, pos)| gather_range.contains(&pos.index))
                    .map_or(vec![], |(card, _, pos)| {
                        let dist_from_origin = origin.index.abs_diff(pos.index);
                        let scope = vec![
                            ("OriginDistance@scope", dist_from_origin as f32),
                            ("Radius@scope", *amount as f32),
                            ("Rank@scope", dist_from_origin as f32),
                        ];
                        vec![(DeckTarget::entity(card, *pos), scope)]
                    });

                let total = res.len() as f32;
                for c in &mut res {
                    c.1.push(("GatherCount@scope", total));
                }
                res
            }
            BoardGatherer::Piles {
                piles,
                players,
                amount,
                mode,
            } => {
                todo!()
            }
        };

        cards
    }

    fn apply_filter(
        ctx: &mut Self::Context<'_, '_>,
        targets: Vec<(Target<Self::Pos>, Scope)>,
        filter: &Self::Filter,
        _invoker: bevy::ecs::entity::Entity,
        _origin: Self::Pos,
    ) -> Vec<(Target<Self::Pos>, Scope)> {
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
