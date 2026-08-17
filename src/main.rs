use core::f32;
use std::any::Any;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use std::u32;

use bevy::DefaultPlugins;
use bevy::app::{App, Startup};
use bevy::asset::{AssetServer, Handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::color::palettes::css::{BLUE, GREEN, PALE_TURQUOISE, PURPLE, RED, YELLOW};
use bevy::color::palettes::tailwind::{
    BLUE_600, BLUE_800, GRAY_300, ORANGE_400, RED_300, RED_800, RED_900, SLATE_600, YELLOW_800,
};
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster};
use bevy::log::LogPlugin;

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages,
};
use bevy::shader::ShaderRef;
use bevy::state::app::StatesPlugin;
use bevy::ui_widgets::observe;
use bevy_diesel::DieselSet;
use bevy_diesel::events::{HasDieselTarget, PosBound};
use bevy_diesel::gauge::{AttributeResolvable, register_derived, requires};
use bevy_diesel::gearbox::{
    AcceptAll, EnterState, GearboxMessage, GearboxSet, InitStateMachine, SpawnSubstate,
    SpawnTransition, StateComponent, StateMachine,
};
use bevy_diesel::invoke::Ability;
use bevy_diesel::prelude::{
    ActiveState, AttributeDerived, RequiresStatsOf, SpatialBackend, SpawnBranch,
    SpawnDieselSubstate, SpawnSubEffect, WriteBack, state_component,
};
use bevy_diesel::target::Target;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::relationship::{OrderedRelationshipSourceCollection, Relationship};
use bevy_ecs::schedule::{MultiThreadedExecutor, ScheduleLabel};
use bevy_ecs::system::{IntoObserverSystem, SystemParam};
use bevy_ecs::world::{self, DeferredWorld};

use bevy_flair::FlairPlugin;
use bevy_flair::style::StyleSheet;
use bevy_flair::style::components::{ClassList, Styled};

use bevy_immediate::Imm;
use bevy_immediate::attach::{BevyImmediateAttachPlugin, ImmediateAttach};
use bevy_immediate::ui::CapsUi;
use bevy_immediate::ui::look::ImmUiLook;
use bevy_immediate::ui::text::ImmUiText;
use bevy_mod_opacity::OpacityPlugin;

use bevy_replicon::client::server_mutate_ticks::ServerMutateTicks;
use bevy_replicon::prelude::{ClientState, Replicated, ServerState};
use bevy_replicon::server::server_tick::ServerTick;
use bevy_tween::BevyTweenRegisterSystems;
use bevy_tween::prelude::{AnimationBuilderExt, EaseKind, Interpolator};
use bevy_tween::tween::{ComponentTween, IntoTarget};
use bevy_tweening::lens::{
    UiPositionLens, UiTransformRotationLens, UiTransformScaleLens, UiTransformTranslationPxLens,
};
use bevy_tweening::{
    AnimCompletedEvent, AnimTarget, CycleCompletedEvent, Lens, PlaybackState, Tween, TweenAnim,
};
use moonshine_kind::{GetInstanceCommands, Instance, Kind};
use moonshine_view::{RegisterViewable, Viewable, ViewableKind};

use rand::RngExt;
use rand::distr::uniform;
use serde::{Deserialize, Serialize};

use crate::abilities::abilities_templates::AbilitiesTemplatePlugin;
use crate::abilities::effects::{
    StatusEffectOf, StatusEffects, StatusEffectsPlugin, Tick, TriggerEffect, TriggerOn,
};
use crate::creatures::generation::CreatureGenerationPlugin;
use crate::debug::ui::DebugUiPlugin;

use crate::deck::deck_and_cards::{Card, DeckAndCardsPlugin, InDeck, StatelessCard};
use crate::effects::{Burning, EffectsPlugin};

use crate::game_flow::turns::{
    BattleGlobalState, CurrentDeckReference, EnemyBoardMarker, EnteredCombat, JustDrawn,
    PlayerBoardMarker, PlayingEntity, TurnsPlugin, inside_battle, should_update_subtick,
};

use crate::grid_abilities_backend::DeckBackend;
use crate::network::{BattleTickingJustStarted, History, NetworkPlugin, SaveHistory};
use crate::ui::{CardVisualAssets, GameUiPlugin};
use crate::utils::IntoVec;
use crate::visuals::cards::animation::DiegeticCardTweenPlugin;

pub mod abilities;
pub mod creatures;
pub mod debug;
pub mod deck;
pub mod effects;
pub mod game_flow;
pub mod grid_abilities_backend;

pub mod stats;

pub mod network;
pub mod ui;
pub mod utils;
pub mod visuals;

#[derive(Resource)]
pub struct CursorPos(Vec2);
impl Default for CursorPos {
    fn default() -> Self {
        // Initialize the cursor pos at some far away place. It will get updated
        // correctly when the cursor moves.
        Self(Vec2::new(-1000.0, -1000.0))
    }
}

// We need to keep the cursor position updated based on any `CursorMoved` events.
pub fn update_cursor_pos(
    camera_q: Query<(&GlobalTransform, &Camera)>,
    mut cursor_moved_events: MessageReader<CursorMoved>,
    mut cursor_pos: ResMut<CursorPos>,
) {
    for cursor_moved in cursor_moved_events.read() {
        // To get the mouse's world position, we have to transform its window position by
        // any transforms on the camera. This is done by projecting the cursor position into
        // camera space (world space).
        for (cam_t, cam) in camera_q.iter() {
            if let Ok(pos) = cam.viewport_to_world_2d(cam_t, cursor_moved.position) {
                *cursor_pos = CursorPos(pos);
            }
        }
    }
}

#[derive(Component, Debug)]
pub struct ActiveCamera;

#[derive(Component, Debug)]
pub struct CursorTarget(pub Entity);

// 1. Isolate the data into a dedicated ShaderType
#[derive(ShaderType, Debug, Clone)]
pub struct TrainMaterialUniforms {
    pub scale: f32,
    pub mesh_vp_min: Vec2,
    pub mesh_vp_max: Vec2,
}

// 2. Bind the struct as a single uniform
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TrainMaterial {
    // Bind the isolated struct directly to 0
    #[uniform(0)]
    pub uniforms: TrainMaterialUniforms,

    #[texture(1)]
    #[sampler(2)]
    pub mask_image: Option<Handle<Image>>,
}

impl Material for TrainMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/train.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/train.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
struct FullscreenEffect {
    intensity: f32,
}

impl FullscreenMaterial for FullscreenEffect {
    fn fragment_shader() -> ShaderRef {
        "shaders/fullscreen.wgsl".into()
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SkewMaterial {
    #[uniform(100)]
    pub skew_amount: f32,
    #[uniform(100)]
    pub _pad0: f32,
    #[uniform(100)]
    pub offset: Vec3,
    #[uniform(100)]
    pub flatten: f32,
}

impl MaterialExtension for SkewMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/skew_material.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/skew_material.wgsl".into()
    }
    fn deferred_vertex_shader() -> ShaderRef {
        "shaders/skew_material.wgsl".into()
    }
}

impl Default for SkewMaterial {
    fn default() -> Self {
        Self {
            skew_amount: 0.,
            _pad0: 0.,
            offset: Vec3::ZERO,
            flatten: 1.0,
        }
    }
}

pub trait NodeInterpolatable: Send + Sync + 'static {
    fn apply(&self, node: &mut Node, value: f32);
}

pub struct NodeLeft;
impl NodeInterpolatable for NodeLeft {
    fn apply(&self, node: &mut Node, value: f32) {
        node.left = px(value);
    }
}

pub struct NodeBottom;
impl NodeInterpolatable for NodeBottom {
    fn apply(&self, node: &mut Node, value: f32) {
        node.bottom = px(value);
    }
}

pub struct InterpolateNode<T: NodeInterpolatable> {
    pub interpolator: T,
    pub start: f32,
    pub end: f32,
}

impl InterpolateNode<NodeLeft> {
    pub fn left(start: f32, end: f32) -> InterpolateNode<NodeLeft> {
        InterpolateNode::<NodeLeft> {
            interpolator: NodeLeft,
            start,
            end,
        }
    }
}

impl InterpolateNode<NodeBottom> {
    pub fn bottom(start: f32, end: f32) -> InterpolateNode<NodeBottom> {
        InterpolateNode {
            interpolator: NodeBottom,
            start,
            end,
        }
    }
}

impl<T: NodeInterpolatable> Interpolator for InterpolateNode<T> {
    type Item = Node;

    fn interpolate(
        &self,
        item: &mut Self::Item,
        value: bevy_tween::interpolate::CurrentValue,
        _previous_value: bevy_tween::interpolate::PreviousValue,
    ) {
        let lerped = self.start.lerp(self.end, value);
        self.interpolator.apply(item, lerped);
    }
}

pub struct InterpolateSkew {
    pub start: f32,
    pub end: f32,
}

impl Interpolator for InterpolateSkew {
    type Item = ExtendedMaterial<bevy::prelude::StandardMaterial, SkewMaterial>;

    fn interpolate(
        &self,
        item: &mut Self::Item,
        value: bevy_tween::interpolate::CurrentValue,
        _previous_value: bevy_tween::interpolate::PreviousValue,
    ) {
        item.extension.skew_amount = self.start.lerp(self.end, value);
    }
}

pub fn custom_interpolators_plugin(app: &mut App) {
    app.add_tween_systems(
        PostUpdate,
        bevy_tween::asset_tween_system::<InterpolateSkew, ()>(),
    )
    .add_tween_systems(
        PostUpdate,
        bevy_tween::component_tween_system::<InterpolateNode<NodeLeft>>(),
    )
    .add_tween_systems(
        PostUpdate,
        bevy_tween::component_tween_system::<InterpolateNode<NodeBottom>>(),
    );
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct UiCardMarker;

#[derive(Component)]
pub struct CardHandContainer;

pub trait DeckDataSupplier: Resource {
    type CardComponent: Component;
    fn get_ui_entity(&self) -> Entity;
    fn get_deck_entity(&self) -> Entity;
    fn is_player() -> bool;
}

#[derive(Resource)]
pub struct PlayerData {
    pub ui_entity: Entity,
    pub draw_pile: Instance<DrawPile>,
    pub hand_pile: Instance<HandPile>,
}

#[derive(Resource)]
pub struct EnemyData {
    pub ui_entity: Entity,
    pub draw_pile: Instance<DrawPile>,
    pub hand_pile: Instance<HandPile>,
}

impl DeckDataSupplier for PlayerData {
    type CardComponent = PlayerCard;

    fn get_ui_entity(&self) -> Entity {
        self.ui_entity
    }

    fn get_deck_entity(&self) -> Entity {
        self.draw_pile.entity()
    }

    fn is_player() -> bool {
        true
    }
}

impl DeckDataSupplier for EnemyData {
    type CardComponent = EnemyCard;

    fn get_ui_entity(&self) -> Entity {
        self.ui_entity
    }

    fn get_deck_entity(&self) -> Entity {
        self.draw_pile.entity()
    }

    fn is_player() -> bool {
        false
    }
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[require(PileWithCards, Replicated)]
pub struct DrawPile;

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[require(PileWithCards, Replicated)]
pub struct HandPile;

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[relationship_target(relationship = CardInPile, linked_spawn)]
#[require(Replicated)]
pub struct PileWithCards(#[entities] Vec<Entity>);

impl Default for PileWithCards {
    fn default() -> Self {
        Self::init()
    }
}

impl PileWithCards {
    pub fn init() -> Self {
        Self(vec![])
    }
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[relationship(relationship_target = PileWithCards)]
#[require(Replicated)]
pub struct CardInPile(#[entities] Entity);

#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[require(Replicated)]
pub struct TicksSinceCast {
    value: u32,
    last_change_tick: BattleTick,
}

impl Default for TicksSinceCast {
    fn default() -> Self {
        Self {
            value: 0,
            last_change_tick: BattleTick::initial(),
        }
    }
}

impl AttributeDerived for TicksSinceCast {
    fn should_update(&self, attrs: &bevy_diesel::prelude::Attributes) -> bool {
        let attr_val = attrs.value("TicksSinceCast");
        self.value as f32 != attr_val
    }

    fn update_from_attributes(&mut self, attrs: &bevy_diesel::prelude::Attributes) {
        self.value = attrs.value("CastTicksRequirement").round() as u32;
    }
}

register_derived!(TicksSinceCast);

#[derive(Component, Clone, Debug)]
pub struct RequiresBattleTickSync;

fn handle_sync_new_battle_entity(e: On<Add, RequiresBattleTickSync>) {}

#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[require(Replicated)]
pub struct CastTicksRequirement {
    value: u32,
}

impl CastTicksRequirement {
    pub fn new(ticks: u32) -> Self {
        Self { value: ticks }
    }

    pub fn tick(&mut self, curr_tick: &BattleTick) {
        self.value += 1;
    }
}

impl AttributeDerived for CastTicksRequirement {
    fn should_update(&self, attrs: &bevy_diesel::prelude::Attributes) -> bool {
        let attr_val = attrs.value("CastTicksRequirement");
        self.value as f32 != attr_val
    }

    fn update_from_attributes(&mut self, attrs: &bevy_diesel::prelude::Attributes) {
        self.value = attrs.value("CastTicksRequirement").round() as u32;
    }
}

register_derived!(CastTicksRequirement);

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CastState {
    Pending,
    Triggered,
}

#[state_component]
#[derive(Component, Clone, Debug, FromTemplate)]
pub struct Ticking;

pub fn cast_data(cast_ticks_requirement: u32) -> impl Bundle {
    (
        CastTicksRequirement::new(cast_ticks_requirement),
        TicksSinceCast::default(),
    )
}

const CARD_WIDTH: u32 = 121;

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Reflect, Copy, Default, AttributeResolvable,
)]
pub enum DeckKind {
    #[default]
    Draw,
    Hand,
}

#[derive(
    Component,
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Reflect,
    Copy,
    Default,
    AttributeResolvable,
)]
// (i32, DeckKind, BattleTick)
#[require(Replicated, SaveHistory)]
pub struct PosInDeck {
    index: u32,
    deck: DeckKind,
}

impl PosInDeck {
    pub fn new(index: u32, deck: DeckKind) -> Self {
        Self { index, deck }
    }

    pub fn as_world_pos(&self) -> Vec2 {
        Vec2::new(((self.index + 1) * CARD_WIDTH) as f32, 0.0)
    }

    pub fn is_in_hand(&self) -> bool {
        match self.deck {
            DeckKind::Draw => false,
            DeckKind::Hand => true,
        }
    }
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[require(Replicated)]
pub struct PlayerCard;

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[require(Replicated)]
pub struct EnemyCard;

// Styling constants
pub const CARDS_COL_GAP: i32 = 16;

#[derive(Resource, Clone, Serialize, Deserialize, Debug)]
pub enum BattleData {
    NoBattle,
    InCombat { battle_tick: BattleTick },
}

#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Hash,
    Reflect,
    Copy,
    Default,
    AttributeResolvable,
)]
pub struct BattleTick {
    turn: u32,
    subtick: u32,
}

impl PartialOrd for BattleTick {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let self_total = self.turn as f32 + (self.subtick as f32 / 10.0);
        let other_total = other.turn as f32 + (other.subtick as f32 / 10.0);

        if self_total > other_total {
            return Some(std::cmp::Ordering::Greater);
        } else if self_total < other_total {
            return Some(std::cmp::Ordering::Less);
        } else {
            return Some(std::cmp::Ordering::Equal);
        }
    }

    fn lt(&self, other: &Self) -> bool {
        self.partial_cmp(other)
            .is_some_and(std::cmp::Ordering::is_lt)
    }

    fn le(&self, other: &Self) -> bool {
        self.partial_cmp(other)
            .is_some_and(std::cmp::Ordering::is_le)
    }

    fn gt(&self, other: &Self) -> bool {
        self.partial_cmp(other)
            .is_some_and(std::cmp::Ordering::is_gt)
    }

    fn ge(&self, other: &Self) -> bool {
        self.partial_cmp(other)
            .is_some_and(std::cmp::Ordering::is_ge)
    }
}

impl Ord for BattleTick {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let self_total = self.turn as f32 + (self.subtick as f32 / 10.0);
        let other_total = other.turn as f32 + (other.subtick as f32 / 10.0);

        if self_total > other_total {
            return std::cmp::Ordering::Greater;
        } else if self_total < other_total {
            return std::cmp::Ordering::Less;
        } else {
            return std::cmp::Ordering::Equal;
        }
    }
}

impl BattleTick {
    pub fn joined_tick(&self) -> (u32, u32) {
        (self.turn, self.subtick)
    }

    pub fn initial() -> Self {
        Self {
            turn: 0,
            subtick: 0,
        }
    }

    pub fn increment_next_turn(&mut self) {
        self.subtick = 0;
        self.turn += 1;
    }

    pub fn null() -> Self {
        Self {
            turn: u32::MAX,
            subtick: u32::MAX,
        }
    }
}

#[derive(Component, Debug, Clone)]
pub struct BattleSubTick(pub i32);

pub const TICK_DURACTION_MS: u64 = 1000;
pub const MAX_ANIM_DURATION_MS: u64 = TICK_DURACTION_MS - 20;

#[derive(Component, Debug, Clone)]
pub struct TurnAnimator {
    turn: u32,
}

impl TurnAnimator {
    pub fn from_turn_tick(turn: u32) -> Self {
        Self { turn }
    }
}

pub fn handle_animate_tick(
    e: On<AnimateTick>,
    q_cards: Query<(Entity, &Viewable<Card>, &History<PosInDeck>)>,
    q_world_pos: Query<&WorldPos>,
    mut cmd: Commands,
) {
    let turn_tick = e.0;
    let min_action_tick = q_cards
        .iter()
        .map(|v| {
            v.2.changes
                .iter()
                .filter(|t| t.0.turn as u32 == turn_tick)
                .map(|t| t.0.subtick)
                .min()
                .unwrap_or(999)
        })
        .min()
        .unwrap();

    let max_action_tick = q_cards
        .iter()
        .map(|v| {
            v.2.changes
                .iter()
                .filter(|t| t.0.turn as u32 == turn_tick)
                .map(|t| {
                    println!("animating one with subtick : {:?}", t.0.subtick);
                    t.0.subtick
                })
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap();

    println!("min {:?} max {:?}", min_action_tick, max_action_tick);

    let action_ticks_count = (max_action_tick as i32 - min_action_tick as i32) + 1;

    for (card_entity, viewable, history) in q_cards.iter() {
        let view = viewable.view().entity();
        let maybe_world_pos = q_world_pos.get(view);

        if maybe_world_pos.is_ok() {
            let change_group_by_subtick: HashMap<u32, &Vec<PosInDeck>> = history
                .changes
                .iter()
                .filter_map(|(t, vals)| {
                    if t.turn != turn_tick || vals.is_empty() {
                        return None;
                    }

                    Some((t.subtick, vals))
                })
                .collect();

            let world_pos = maybe_world_pos.unwrap();

            let anim_duration_ms: f32 = (MAX_ANIM_DURATION_MS as f32) / (action_ticks_count as f32);
            let anim_done_delay: f32 = anim_duration_ms + 10.0;
            let easing = EaseFunction::CubicInOut;
            let duration = Duration::from_millis(anim_duration_ms.round() as u64);
            let mut last_loop_changes: Option<&Vec<PosInDeck>> = None;

            for tick_nb in min_action_tick..=max_action_tick {
                let loop_i = tick_nb - min_action_tick;
                let loop_delay_ms = anim_done_delay * (loop_i as f32);

                let Some(changes) = change_group_by_subtick.get(&tick_nb) else {
                    continue;
                };

                if changes.is_empty() {
                    continue;
                }

                for (i, card_idx) in changes.iter().enumerate() {
                    if !card_idx.is_in_hand() {
                        continue;
                    }
                    let cloned_idx = card_idx.clone();

                    let start_pos: Vec2 = match i == 0 {
                        true => match last_loop_changes {
                            None => world_pos.position,
                            Some(vals) => {
                                let prev = vals.iter().last().unwrap();
                                prev.as_world_pos()
                            }
                        },
                        false => {
                            let prev = changes.get(i - 1).unwrap();
                            prev.as_world_pos()
                        }
                    };
                    let end_pos = card_idx.as_world_pos();

                    let move_dir_multiplier: f32 = if end_pos.x > start_pos.x { 1.0 } else { -1.0 };

                    let pos_tween = Tween::new(
                        easing,
                        duration,
                        WorldPosLens {
                            pos: AnimMode::FromTo {
                                start: start_pos,
                                end: end_pos,
                            },
                            tf: AnimMode::NoAnim,
                            translation: AnimMode::NoAnim,
                        },
                    );

                    let degs_offset = 20.0 * move_dir_multiplier;
                    let start_tf = world_pos.transform.clone();

                    let tf_tween = Tween::new(
                        EaseFunction::SmoothStepIn,
                        duration / 2,
                        WorldPosLens {
                            pos: AnimMode::NoAnim,
                            tf: AnimMode::FromTo {
                                start: UiTransform::from_rotation(start_tf.rotation),
                                end: UiTransform::from_rotation(Rot2::degrees(degs_offset)),
                            },
                            translation: AnimMode::NoAnim,
                        },
                    )
                    .then(Tween::new(
                        EaseFunction::CubicOut,
                        duration / 2,
                        WorldPosLens {
                            pos: AnimMode::NoAnim,
                            tf: AnimMode::FromTo {
                                start: UiTransform::from_rotation(Rot2::degrees(degs_offset)),
                                end: UiTransform::from_rotation(Rot2::degrees(0.0)),
                            },
                            translation: AnimMode::NoAnim,
                        },
                    ));

                    let mut anim = TweenAnim::new(pos_tween).with_destroy_on_completed(true);
                    anim.playback_state = PlaybackState::Paused;

                    let anim_a = cmd
                        .spawn((
                            anim,
                            TurnAnimator::from_turn_tick(turn_tick),
                            AnimTarget::component::<WorldPos>(view),
                        ))
                        .id();

                    let mut anim = TweenAnim::new(tf_tween).with_destroy_on_completed(true);
                    anim.playback_state = PlaybackState::Paused;

                    let anim_b = cmd
                        .spawn((
                            anim,
                            TurnAnimator::from_turn_tick(turn_tick),
                            AnimTarget::component::<WorldPos>(view),
                        ))
                        .id();

                    let delay_secs = loop_delay_ms / 1000.0;
                    cmd.spawn(Delayer::from_secs(delay_secs)).observe(
                        move |_: On<DelayCompleted>, mut q: Query<&mut TweenAnim>| {
                            println!(
                                "animating world pos, following a delay of {:?} seconds from card index : {:?}",
                                delay_secs.clone(),
                                cloned_idx.clone()
                            );
                            let [mut tween_a, mut tween_b] =
                                q.get_many_mut([anim_a, anim_b]).unwrap();

                            tween_a.playback_state = PlaybackState::Playing;
                            tween_b.playback_state = PlaybackState::Playing;
                        },
                    );
                }

                last_loop_changes = Some(*changes);
            }
        } else if maybe_world_pos.is_err() {
            cmd.entity(view).insert(WorldPos {
                position: Vec2::ZERO,
                transform: UiTransform::default(),
            });
        }
    }
}

#[derive(Component, Clone, Serialize, Deserialize)]
pub struct CardWidgetFor(#[entities] Entity);

#[derive(SystemParam)]
pub struct CardWidgetParams<'w, 's> {
    query: Query<'w, 's, (Entity, &'static mut CardWidgetFor)>,
    q_card_viewable: Query<'w, 's, &'static Viewable<Card>>,
    q_world_cards: Query<'w, 's, &'static WorldPos>,
    visuals_assets: Res<'w, CardVisualAssets>,
    q_effects_list: Query<'w, 's, &'static StatusEffects>,
    // Switch to using auto registered attributes for easier global access ?
    q_effects: Query<'w, 's, (&'static StatusEffectOf, Option<&'static Magnetic>)>,
}

#[derive(Component)]
pub struct ImageOf(pub Entity);

impl ImmediateAttach<CapsUi> for CardWidgetFor {
    type Params = CardWidgetParams<'static, 'static>;

    fn construct(ui: &mut Imm<CapsUi>, params: &mut CardWidgetParams) {
        let entity = ui.current_entity().unwrap();
        let (widget_entity, widget_for) = params.query.get_mut(entity).unwrap();
        let source_entity = widget_for.0;
        let Ok(viewable) = params.q_card_viewable.get_mut(source_entity) else {
            return;
        };

        let view = viewable.view().entity();

        let effects: Vec<_> =
            params
                .q_effects_list
                .get(source_entity)
                .map_or(vec![], |card_effects_list| {
                    let descendant_effects: Vec<Entity> = card_effects_list.iter().collect();
                    params
                        .q_effects
                        .iter_many(descendant_effects)
                        .collect::<Vec<_>>()
                });
        let maybe_world_card = params.q_world_cards.get(view);

        ui.ch()
            .on_spawn_insert(|| {
                (
                    Node {
                        width: px(121),
                        height: px(151),
                        position_type: PositionType::Absolute,
                        bottom: px(0),
                        left: px(0),
                        display: Display::Flex,
                        align_items: AlignItems::FlexEnd,
                        justify_content: JustifyContent::Center,
                        align_content: AlignContent::Center,
                        ..default()
                    },
                    UiTransform::default(),
                    ClassList::new("card"),
                    InheritedVisibility::VISIBLE,
                    Hovered::default(),
                    children![(
                        ImageOf(widget_entity),
                        ImageNode::new(
                            params
                                .visuals_assets
                                .background
                                .clone()
                                .expect("bg should be set")
                        ),
                        Node {
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                    )],
                )
            })
            .node_mut(|n| {
                if let Ok(world_card) = maybe_world_card {
                    n.left = px(world_card.position.x);
                    n.bottom = px(world_card.position.y);
                };
            })
            .at_this_moment_apply_commands(|cmds| {
                if let Ok(world_card) = maybe_world_card {
                    cmds.insert(world_card.transform.clone());
                };
            })
            .add(move |ui| {
                for effect in effects.iter() {
                    let (color, text_content): (Color, String) = match effect.1 {
                        Some(magnetic) => (SLATE_600.into(), magnetic.strength.to_string()),
                        None => (RED_900.into(), "".to_string()),
                    };
                    ui.ch().on_spawn_insert(|| {
                        (
                            Node {
                                width: px(42),
                                height: px(42),
                                top: px(0),
                                left: px(0),
                                display: Display::Flex,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                position_type: PositionType::Absolute,
                                border_radius: BorderRadius::all(px(200)),
                                ..default()
                            },
                            UiTransform {
                                translation: Val2 {
                                    x: percent(-25.0),
                                    y: percent(-25.0),
                                },
                                ..default()
                            },
                            BackgroundColor(color),
                            children![Text::new(text_content)],
                        )
                    });
                }
            });
    }
}

#[derive(Component, Debug, Reflect, Clone)]
pub struct WorldPos {
    position: Vec2,
    transform: UiTransform,
}

impl ViewableKind for Card {
    fn view_bundle() -> impl Bundle {
        ()
    }
}

fn build_card_view(
    event: On<Add, Viewable<Card>>,
    query: Query<&Viewable<Card>>,
    client_state: Res<State<ClientState>>,
    mut cmd: Commands,
) {
    match client_state.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }

    println!("BUILT VIEW");

    let viewable = query.get(event.entity).unwrap();
    let view = viewable.view();

    cmd.entity(*view).insert((WorldPos {
        position: Vec2::ZERO,
        transform: UiTransform::IDENTITY,
    },));
}

pub enum AnimMode<T: Clone + Send + 'static> {
    FromTo { start: T, end: T },
    NoAnim,
}

pub struct WorldPosLens {
    pos: AnimMode<Vec2>,
    tf: AnimMode<UiTransform>,
    translation: AnimMode<Vec2>,
}

impl WorldPosLens {
    pub fn position(start: Vec2, end: Vec2) -> Self {
        Self {
            pos: AnimMode::FromTo { start, end },
            tf: AnimMode::NoAnim,
            translation: AnimMode::NoAnim,
        }
    }
}

impl Lens<WorldPos> for WorldPosLens {
    fn lerp(&mut self, mut target: Mut<WorldPos>, ratio: f32) {
        match self.pos {
            AnimMode::FromTo { start, end } => {
                target.position = start.lerp(end, ratio);
            }
            AnimMode::NoAnim => {}
        }
        match self.tf {
            AnimMode::FromTo { start, end } => {
                let new_translation = match self.translation {
                    AnimMode::FromTo { start, end } => {
                        let lerped = start.lerp(end, ratio);
                        Val2::px(lerped.x, lerped.y)
                    }
                    AnimMode::NoAnim => target.transform.translation,
                };

                target.transform = UiTransform {
                    translation: new_translation,
                    scale: start.scale.lerp(end.scale, ratio),
                    rotation: start.rotation.slerp(end.rotation, ratio),
                };
            }
            AnimMode::NoAnim => {}
        }
    }
}

pub fn relay_event_as_message<T: EntityEvent + GearboxMessage>(
    e: On<T>,
    mut writer: MessageWriter<T>,
) {
    writer.write(e.event().clone());
}

#[derive(EntityEvent)]
pub struct CardUiDrawn {
    entity: Entity,
}

#[derive(Clone, Component)]
pub struct TestMark;

fn spawn_card(
    components: impl Bundle,
    cmd: &mut Commands,
    draw_pile: Instance<DrawPile>,
    hand_pile: Instance<HandPile>,
    is_on_player: bool,
) -> Entity {
    let draw_pile_entity = draw_pile.entity();
    let hand_pile_entity = hand_pile.entity();
    let card = match is_on_player {
        true => cmd.spawn((PlayerCard, CardInPile(draw_pile_entity))).id(),
        false => cmd.spawn((EnemyCard, CardInPile(draw_pile_entity))).id(),
    };

    cmd.entity(card)
        .insert((UiCardMarker, Card::new(), cast_data(10), components));

    cmd.entity(card).with_children(|parent| {
        let ticking = parent.spawn_substate(card, Name::new("Ticking")).id();

        let cast = parent.spawn_substate(card, Name::new("Cast")).id();

        parent.spawn_transition::<CardCast>(ticking, cast);
        parent.spawn_transition_always(cast, ticking);

        let cmds = parent.commands_mut();
        cmds.entity(card).init_state_machine(ticking);
    });

    card
}

pub trait BoardUtilsCommandsExt {
    fn send_and_trigger<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn trigger_next_tick<E>(&mut self, event: E)
    where
        E: EntityEvent + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn trigger_delayed<E>(&mut self, event: E, delay_ms: f32)
    where
        E: Event + Clone,
        for<'a> E::Trigger<'a>: Default;
}

impl<'w, 's> BoardUtilsCommandsExt for Commands<'w, 's> {
    fn send_and_trigger<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        self.write_message(event.clone());
        self.trigger(event);
    }

    fn trigger_next_tick<E>(&mut self, event: E)
    where
        E: EntityEvent + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        let event = event.clone();
        self.spawn(Delayer::next_tick()).observe(
            move |_: On<DelayCompleted>, mut obs_cmd: Commands| {
                obs_cmd.trigger(event.clone());
            },
        );
    }

    fn trigger_delayed<E>(&mut self, event: E, delay_secs: f32)
    where
        E: Event + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        let event = event.clone();
        self.spawn(Delayer::from_secs(delay_secs)).observe(
            move |_: On<DelayCompleted>, mut obs_cmd: Commands| {
                obs_cmd.trigger(event.clone());
            },
        );
    }
}

#[derive(EntityEvent, Clone, Message, Debug, Reflect)]
pub struct CardCast {
    #[event_target]
    pub card: Entity,
}

impl GearboxMessage for CardCast {
    type Validator = AcceptAll;

    fn target(&self) -> Entity {
        self.card
    }
}

#[derive(EntityEvent, Clone, Message, Debug, Reflect)]
pub struct DrawCard {
    #[event_target]
    pub card: Entity,
}

impl GearboxMessage for DrawCard {
    type Validator = AcceptAll;

    fn target(&self) -> Entity {
        self.card
    }
}

#[derive(EntityEvent, Clone, Message, Debug, Reflect)]
pub struct CardDrawn {
    #[event_target]
    pub card: Entity,
    pub is_player: bool,
}

impl GearboxMessage for CardDrawn {
    type Validator = AcceptAll;

    fn target(&self) -> Entity {
        self.card
    }
}

pub fn propagate_effect_statuses<T: EntityEvent + Clone>(
    e: On<T>,
    q: Query<&StatusEffects>,
    effects: Query<Entity, With<TriggerOn<T>>>,
    mut cmd: Commands,
) {
    println!("LLL - trying to propagate");
    let entity = e.event_target();
    let Ok(statuses) = q.get(entity) else {
        return;
    };

    for effect in effects.iter_many(statuses.iter()) {
        cmd.trigger(TriggerEffect {
            entity: effect,
            cause: e.event().clone(),
        });
    }
}

impl DrawCard {
    pub fn new(card: Entity) -> Self {
        Self { card }
    }
}

#[derive(Clone)]
pub enum TargetingAmount {
    Single,
    Fixed(i32),
    All,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CardDir {
    Right,
    Left,
    Around,
}

// Card states marker
#[derive(Component, Clone, Debug, Reflect, Copy, Serialize, Deserialize)]
pub struct InHand;
#[derive(Component, Clone, Debug, Reflect, Copy, Serialize, Deserialize)]
pub struct BeingDrawn;
#[derive(Component, Clone, Debug, Reflect, Copy, Serialize, Deserialize)]
pub struct InDrawPile;
#[derive(Component, Clone, Debug, Reflect, Copy, Serialize, Deserialize)]
pub struct InDiscard;

#[derive(Component)]
pub enum CardTargeting {
    ByDir(CardDir, TargetingAmount),
}

impl Default for CardTargeting {
    fn default() -> Self {
        Self::ByDir(CardDir::Around, TargetingAmount::Single)
    }
}

pub trait GeneratesCardTargeting {
    fn apply_effect(
        &self,
        source: &PosInDeck,
        cards: &mut Vec<(Entity, &Card, &PosInDeck)>,
        collection: &mut PileWithCards,
    );
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
#[require(CardTargeting, Replicated)]
pub struct Magnetic {
    pub direction: CardDir,
    pub strength: u32,
}

impl Magnetic {
    pub fn new(direction: CardDir, strength: u32) -> Self {
        Self {
            direction,
            strength,
        }
    }
}

impl GeneratesCardTargeting for Magnetic {
    fn apply_effect(
        &self,
        source: &PosInDeck,
        cards: &mut Vec<(Entity, &Card, &PosInDeck)>,
        collection: &mut PileWithCards,
    ) {
        if cards.len() < 2 {
            return;
        }

        cards.sort_by_key(|(_, _, card_idx)| card_idx.index);

        let curr = source.index;
        // Position of the source *within the `cards` slice* — NOT `curr` itself.
        let Some(curr_pos) = cards.iter().position(|(_, _, idx)| idx.index == curr) else {
            return;
        };
        let curr_pos = curr_pos as u32;
        let len = cards.len() as u32;

        let right_count = (len - 1 - curr_pos).min(self.strength) as u32;
        let left_count = curr_pos.min(self.strength);

        let col_indexes: HashMap<Entity, u32> = collection
            .0
            .iter()
            .enumerate()
            .map(|(i, e)| (*e, i as u32))
            .collect();

        let modified: Vec<(Entity, u32)> = match self.direction {
            CardDir::Right => {
                if right_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos + 1) as usize;
                    let end = (curr_pos + 1 + right_count) as usize;
                    cards[start..end]
                        .iter()
                        .map(|(e, _, i)| (*e, i.index.saturating_sub(right_count)))
                        .collect()
                }
            }
            CardDir::Left => {
                if left_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos.saturating_sub(left_count)) as usize;
                    let end = curr_pos as usize;
                    cards[start..end]
                        .iter()
                        // moving *toward* curr means increasing index, not decreasing
                        .map(|(e, _, i)| (*e, i.index + left_count))
                        .collect()
                }
            }
            CardDir::Around => {
                let mut right: Vec<(Entity, u32)> = if right_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos + 1) as usize;
                    let end = (curr_pos + 1 + right_count) as usize;
                    cards[start..end]
                        .iter()
                        .map(|(e, _, _)| {
                            (*e, col_indexes.get(e).unwrap().wrapping_sub(right_count))
                        })
                        .collect()
                };

                let mut left: Vec<(Entity, u32)> = if left_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos.saturating_sub(left_count)) as usize;
                    let end = curr_pos as usize;
                    cards[start..end]
                        .iter()
                        .map(|(e, _, _)| {
                            (*e, col_indexes.get(e).unwrap().saturating_sub(left_count))
                        })
                        .collect()
                };

                right.append(&mut left);
                right
            }
        };

        for (e, new_i) in modified.into_iter() {
            let old_idx = cards.iter().find(|v| v.0 == e).unwrap().2.index;
            println!("switching card from {:?} to {:?}", old_idx, new_i.max(0));
            collection.0.place(e, new_i.max(0) as usize);
        }
    }
}

pub fn status_effect<E: EntityEvent + Clone, T: Component + Clone>(effect: T) -> impl Bundle {
    related!(StatusEffects[(TriggerOn::<E>::new(), effect, observe(tick_on::<E>))])
}

pub fn magnetic_effect(direction: CardDir, strength: u32) -> impl Bundle {
    status_effect::<DrawCard, Magnetic>(Magnetic::new(direction, strength))
}

pub fn burn_effect(direction: CardDir, strength: u32) -> impl Bundle {
    status_effect::<DrawCard, Magnetic>(Magnetic::new(direction, strength))
}

pub fn tick_on<T: EntityEvent + Clone>(
    e: On<TriggerEffect<T>>,
    q: Query<Entity, (With<TriggerOn<T>>, With<StatusEffectOf>)>,
    mut cmd: Commands,
) {
    println!("LLL - suis la mon calisse check attend..");
    if !q.contains(e.entity) {
        return;
    }
    println!("LLL - mon calisse OUAIS C BON");
    cmd.trigger(Tick { status: e.entity });
}

pub fn tick_effects(
    e: On<Tick>,
    q: Query<(Entity, &StatusEffectOf)>,
    mut q_decks: Query<&mut PileWithCards>,
    q_card_of: Query<&CardInPile>,
    // Instead use cardindex.stat
    cards: Query<(Entity, &Card, &PosInDeck)>,
    magnetic_effects: Query<&Magnetic>,
    mut cmd: Commands,
) {
    let Ok((effect_entity, effect_of)) = q.get(e.status) else {
        return;
    };

    let target_card = effect_of.get();

    let deck_entity = q_card_of.get(target_card).unwrap().0;
    let target_deck = q_decks
        .get_mut(deck_entity)
        .expect("should find associated deck for effect card parent")
        .into_inner();

    println!("ticking effects for deck : ");
    cmd.entity(deck_entity).log_components();

    println!("log comps");
    cmd.entity(effect_of.get()).log_components();

    let applier_index_c = cards
        .get(effect_of.get())
        .expect("status parent should have a card index and in hand")
        .2;

    let mut cards: Vec<(Entity, &Card, &PosInDeck)> = cards.iter_many(&target_deck.0).collect();

    let Ok(magnetic) = magnetic_effects.get(effect_entity) else {
        return;
    };

    magnetic.apply_effect(&applier_index_c, &mut cards, target_deck);
}

// Change card into a reusable widget and use a

fn setup_base_scene(
    mut cmd: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    cmd.spawn((Camera::default(), Camera2d::default(), ActiveCamera));

    cmd.spawn((
        Mesh2d(meshes.add(Rectangle::new(1000., 700.))),
        MeshMaterial2d(materials.add(Color::srgb(0.2, 0.2, 0.3))),
    ));
}

pub fn input_linked_tests(mut cmd: Commands, keys: Res<ButtonInput<KeyCode>>) {
    for key in keys.get_just_pressed() {
        match key {
            KeyCode::Space => {
                //
            }
            _ => {}
        }
    }
}

pub struct ImmeditateUiPlugin;

impl Plugin for ImmeditateUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            BevyImmediateAttachPlugin::<CapsUi, MainSceneUiRoot<PlayerData>>::new(),
            BevyImmediateAttachPlugin::<CapsUi, MainSceneUiRoot<EnemyData>>::new(),
            BevyImmediateAttachPlugin::<CapsUi, CardWidgetFor>::new(),
            FlairPlugin,
        ));
    }
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
pub struct MainSceneUiRoot<D: DeckDataSupplier> {
    _phantom: PhantomData<D>,
}

impl<D: DeckDataSupplier> MainSceneUiRoot<D> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

#[derive(SystemParam)]
pub struct HandUiParams<'w, 's, D: DeckDataSupplier> {
    pub q_cards: Query<
        'w,
        's,
        (
            Entity,
            // &'static CardIndex,
            &'static <D as DeckDataSupplier>::CardComponent,
        ),
    >,
}

impl<'w, 's, D: DeckDataSupplier> HandUiParams<'w, 's, D> {
    pub fn is_player(&self) -> bool {
        D::is_player()
    }
}

impl<D: DeckDataSupplier> ImmediateAttach<CapsUi> for MainSceneUiRoot<D> {
    type Params = HandUiParams<'static, 'static, D>;

    fn construct(ui: &mut Imm<CapsUi>, params: &mut HandUiParams<D>) {
        // Grab this once, up front — don't call ui.current_entity() again later.
        let current_entity = ui.current_entity();

        // Collect just the Entity ids you need (Copy), not the borrowed tuples.
        let cards: Vec<Entity> = match current_entity {
            Some(entity) => params.q_cards.iter().map(|vals| vals.0).collect(),
            None => vec![],
        };

        // Precompute the boolean the node_mut closure needs, so it doesn't touch params/ui.
        let is_enemy_deck = !params.is_player();

        ui.ch()
            .on_spawn_insert(|| {
                (
                    Node {
                        width: percent(100.0),
                        height: percent(100.0),
                        align_items: AlignItems::FlexEnd,
                        justify_content: JustifyContent::Start,
                        padding: UiRect::all(percent(10.0)),
                        ..default()
                    },
                    InheritedVisibility::VISIBLE,
                    CardHandContainer,
                )
            })
            .node_mut(move |n| {
                if is_enemy_deck {
                    n.bottom = Val::Vh(50.0);
                }
            })
            .add(move |ui| {
                for e in cards.iter().copied() {
                    ui.ch()
                        .on_spawn_insert(|| Node::DEFAULT)
                        .on_spawn_insert(move || CardWidgetFor(e));
                }
            });
    }
}

#[derive(Component, Clone)]
pub struct PendingDraw;

fn check_update_cards_idx(
    q_decks: Query<(&PileWithCards, Has<HandPile>), Changed<PileWithCards>>,
    q_just_drawn: Query<(), With<JustDrawn>>,
    q_idx: Query<&PosInDeck>,
    mut cmd: Commands,
    // mut increment_action_tick: ResMut<IncrementActionTick>,
) {
    if q_decks.count() == 0 {
        return;
    }

    for (deck, is_hand) in q_decks.iter() {
        for (i, card) in deck.iter().enumerate() {
            let card_kind = match is_hand {
                true => DeckKind::Hand,
                false => DeckKind::Draw,
            };

            let after = PosInDeck::new(i as u32, card_kind);
            let Ok(before) = q_idx.get(card) else {
                // Initializes value the first time
                cmd.entity(card).insert(after);
                continue;
            };

            if before.index == after.index && before.deck == after.deck {
                continue;
            }

            cmd.entity(card).insert(after);

            if is_hand && q_just_drawn.contains(card) {
                println!("on draw mon calisse");
                cmd.entity(card).remove::<JustDrawn>();
                cmd.entity(card).insert(PendingDraw);
            }
        }
    }

    // increment_action_tick.0 = true;
}

#[derive(Clone, Debug)]
pub enum DelayerMode {
    NextTick { ticks_left: i32 },
    Timer(Timer),
}

#[derive(Component, Clone, Debug)]
struct Delayer(DelayerMode);

impl Delayer {
    fn from_secs(secs: f32) -> Self {
        Self(DelayerMode::Timer(Timer::from_seconds(
            secs,
            TimerMode::Once,
        )))
    }

    fn next_tick() -> Self {
        Self(DelayerMode::NextTick { ticks_left: 1 })
    }
}

#[derive(EntityEvent)]
struct DelayCompleted(Entity);

fn tick_delayers(mut q: Query<(Entity, &mut Delayer)>, time: Res<Time>, mut cmd: Commands) {
    for (ent, mut delayer) in &mut q {
        match &mut delayer.0 {
            DelayerMode::Timer(timer) => {
                timer.tick(time.delta());
                if timer.just_finished() {
                    cmd.entity(ent).remove::<Delayer>();
                    cmd.trigger(DelayCompleted(ent));
                    cmd.entity(ent).despawn();
                }
            }
            DelayerMode::NextTick { ticks_left } => {
                if *ticks_left >= 1 {
                    delayer.0 = DelayerMode::NextTick { ticks_left: 0 }
                } else {
                    cmd.entity(ent).remove::<Delayer>();
                    cmd.trigger(DelayCompleted(ent));
                    cmd.entity(ent).despawn();
                }
            }
        }
    }
}

#[derive(Resource)]
pub struct ClientBattleAnimState {
    started: bool,
    next_animated_tick: u32,
}

impl ClientBattleAnimState {
    pub fn get_current_animating_turn_tick(&self) -> Option<u32> {
        if !self.started {
            return None;
        }

        Some(self.next_animated_tick.saturating_sub(1))
    }
}

impl Default for ClientBattleAnimState {
    fn default() -> Self {
        Self {
            started: false,
            next_animated_tick: 0,
        }
    }
}

fn check_battle_data_fully_received(
    server_mutate_ticks: Res<ServerMutateTicks>,
    mut anim_state: ResMut<ClientBattleAnimState>,
    mut cmd: Commands,
    q: Query<&History<PosInDeck>>,
) {
    if anim_state.started {
        return;
    }

    let Some(_) = server_mutate_ticks.last_confirmed_tick() else {
        return;
    };

    let any_changes_registered = q.iter().any(|his| his.changes.len() >= 1);

    if !any_changes_registered {
        return;
    }

    println!("READY ! All ticks for first turn receive -> START ANIMATING");
    anim_state.started = true;
    cmd.trigger_delayed(ReqNextTurnAnim { turn: 0 }, 0.1);
}

#[derive(Event)]
pub struct AnimateTick(pub u32);

fn start_next_turn_anim(
    _: On<ReqNextTurnAnim>,
    mut anim_state: ResMut<ClientBattleAnimState>,
    s: Res<State<ClientState>>,
    mut cmd: Commands,
) {
    match s.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }

    if !anim_state.started {
        return;
    }

    cmd.trigger(AnimateTick(anim_state.next_animated_tick));
    anim_state.next_animated_tick += 1;
}

#[derive(Event, Clone, Debug)]
pub struct ReqNextTurnAnim {
    turn: u32,
}

fn handle_turn_animator_added(
    e: On<Add, TurnAnimator>,
    q: Query<&TurnAnimator, With<TweenAnim>>,
    mut cmd: Commands,
) {
    let target = e.entity;
    let turn = q
        .get(target)
        .expect("TurnAnimator should always be put on an entity with TweenAnim")
        .turn;

    cmd.entity(target).observe(
        move |evt: On<AnimCompletedEvent>,
              q: Query<(Entity, &TurnAnimator)>,
              mut obs_cmd: Commands,
              anim_state: Res<ClientBattleAnimState>| {
            obs_cmd.entity(evt.anim_entity).despawn();
            let turn_has_pending_animators = q
                .iter()
                .any(|(ent, t)| ent != evt.anim_entity && t.turn == turn.clone());

            let next_expected_turn = turn.clone() + 1;
            if turn_has_pending_animators || anim_state.next_animated_tick != next_expected_turn {
                return;
            }

            println!("STARTING NEXT TURN ANIM");
            obs_cmd.trigger(ReqNextTurnAnim {
                turn: next_expected_turn,
            });
        },
    );
}

fn update_subtick(mut battle_data: ResMut<BattleData>) {
    match &mut battle_data.into_inner() {
        BattleData::NoBattle => {
            println!("XXXXXX == NO BATTLE DATAA");
            return;
        }
        BattleData::InCombat { battle_tick } => {
            println!("OOOOOO == FOUND BATTLE DATAA : {:?}", battle_tick);
            battle_tick.subtick += 1;
        }
    }
}

fn main() {
    let mut app = App::new();
    app.add_plugins((
        MeshPickingPlugin,
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: String::from("Drill"),
                    ..Default::default()
                }),
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            .set(LogPlugin {
                filter: "info,wgpu_core=error,wgpu_hal=error".into(),
                level: bevy::log::Level::DEBUG,
                ..default()
            }),
    ))
    .add_plugins((
        MaterialPlugin::<ExtendedMaterial<StandardMaterial, SkewMaterial>>::default(),
        MaterialPlugin::<TrainMaterial>::default(),
        FullscreenMaterialPlugin::<FullscreenEffect>::default(),
        custom_interpolators_plugin,
        EffectsPlugin,
        DiegeticCardTweenPlugin,
        GameUiPlugin,
        TurnsPlugin,
        DeckAndCardsPlugin,
        (CreatureGenerationPlugin, StatusEffectsPlugin),
    ))
    .add_plugins(ImmeditateUiPlugin)
    .add_plugins(AbilitiesTemplatePlugin)
    .add_plugins(DeckBackend::plugin())
    .add_plugins(OpacityPlugin)
    .add_plugins(NetworkPlugin)
    // Resources
    .insert_resource(DirectionalLightShadowMap { size: 4096 })
    .insert_resource(ClientBattleAnimState::default())
    // Startup
    .add_systems(Startup, setup_base_scene.after(GearboxSet))
    .add_systems(Update, input_linked_tests)
    .add_observer(handle_animate_tick)
    // Observers
    .add_observer(propagate_effect_statuses::<DrawCard>)
    .add_observer(tick_effects)
    .register_viewable::<Card>()
    .add_observer(build_card_view)
    .add_observer(|e: On<Remove, PileWithCards>, mut cmd: Commands| {
        cmd.entity(e.entity).insert(PileWithCards::init());
    })
    .add_systems(
        Update,
        (|mut reader: MessageReader<DrawCard>,
          q: Query<(Entity, &PosInDeck)>,
          mut cmd: Commands| {
            for e in reader.read() {
                if let Ok((ent, ci)) = q.get(e.card) {
                    cmd.trigger(e.clone());
                }
            }
        }),
    )
    .add_systems(FixedUpdate, tick_delayers)
    .add_systems(
        FixedUpdate,
        check_battle_data_fully_received.run_if(in_state(ClientState::Connected)),
    )
    .add_systems(
        FixedUpdate,
        check_update_cards_idx.run_if(in_state(ServerState::Running)),
    )
    .add_systems(
        FixedPostUpdate,
        update_subtick.run_if(should_update_subtick),
    )
    .add_observer(start_next_turn_anim)
    .add_observer(handle_turn_animator_added)
    .run();
}
