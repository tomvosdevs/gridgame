use core::f32;
use std::any::Any;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

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
use bevy::core_pipeline::core_3d::graph::Node3d;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster};
use bevy::log::LogPlugin;

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_graph::RenderLabel;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages,
};
use bevy::shader::ShaderRef;
use bevy::state::app::StatesPlugin;
use bevy::ui_widgets::observe;
use bevy_diesel::DieselSet;
use bevy_diesel::events::HasDieselTarget;
use bevy_diesel::invoke::Ability;
use bevy_diesel::prelude::{ActiveState, SpatialBackend, SpawnDieselSubstate};
use bevy_diesel::target::Target;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::relationship::{OrderedRelationshipSourceCollection, Relationship};
use bevy_ecs::schedule::{MultiThreadedExecutor, ScheduleLabel};
use bevy_ecs::system::{IntoObserverSystem, SystemParam};
use bevy_ecs::world::{self, DeferredWorld};
use bevy_ecs_tilemap::prelude::*;
use bevy_flair::FlairPlugin;
use bevy_flair::style::StyleSheet;
use bevy_flair::style::components::{ClassList, NodeStyleSheet};
use bevy_gearbox::{
    AcceptAll, EnterState, GearboxMessage, GearboxSet, InitStateMachine, SpawnSubstate,
    SpawnTransition, StateComponent, StateMachine,
};
use bevy_ghx_grid::debug_plugin::view::DebugGridView;
use bevy_ghx_grid::debug_plugin::{DebugGridView3dBundle, GridDebugPlugin};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::{Cartesian3D, GridDelta};
use bevy_ghx_grid::ghx_grid::cartesian::grid::CartesianGrid;
use bevy_ghx_grid::ghx_grid::direction::Direction;
use bevy_ghx_grid::ghx_grid::grid::{Grid, GridIndex};
use bevy_ghx_proc_gen::GridNode;
use bevy_ghx_proc_gen::assets::{BundleInserter, ModelsAssets};
use bevy_ghx_proc_gen::proc_gen::generator::builder::GeneratorBuilder;
use bevy_ghx_proc_gen::proc_gen::generator::model::{
    ModelCollection, ModelInstance, ModelRotation,
};
use bevy_ghx_proc_gen::proc_gen::generator::rules::RulesBuilder;
use bevy_ghx_proc_gen::proc_gen::generator::socket::{SocketCollection, SocketsCartesian3D};
use bevy_ghx_proc_gen::simple_plugin::ProcGenSimplePlugins;
use bevy_ghx_proc_gen::spawner_plugin::NodesSpawner;
use bevy_immediate::Imm;
use bevy_immediate::attach::{BevyImmediateAttachPlugin, ImmediateAttach};
use bevy_immediate::ui::CapsUi;
use bevy_immediate::ui::look::ImmUiLook;
use bevy_immediate::ui::text::ImmUiText;
use bevy_mod_opacity::OpacityPlugin;
use bevy_northstar::nav::Nav;
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
    BattleData, CurrentDeckReference, EnemyBoardMarker, EnteredCombat, JustDrawn,
    PlayerBoardMarker, PlayingEntity, TurnsPlugin,
};
use crate::grid_abilities_backend::{BoardPos, DeckBackend};

use crate::network::{BattleTickUpdated, History, NetworkPlugin, SaveHistory};
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

    fn node_edges() -> Vec<bevy::render::render_graph::InternedRenderLabel> {
        vec![
            Node3d::Tonemapping.intern(),
            // The label is automatically generated from the name of the struct
            Self::node_label().intern(),
            Node3d::EndMainPassPostProcessing.intern(),
        ]
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
    pub draw_pile_entity: Instance<DrawPile>,
    pub hand_pile_entity: Instance<HandPile>,
}

#[derive(Resource)]
pub struct EnemyData {
    pub ui_entity: Entity,
    pub draw_pile_entity: Instance<DrawPile>,
    pub hand_pile_entity: Instance<HandPile>,
}

impl DeckDataSupplier for PlayerData {
    type CardComponent = PlayerCard;

    fn get_ui_entity(&self) -> Entity {
        self.ui_entity
    }

    fn get_deck_entity(&self) -> Entity {
        self.draw_pile_entity.entity()
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
        self.draw_pile_entity.entity()
    }

    fn is_player() -> bool {
        false
    }
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[require(CardsPile, Replicated)]
pub struct DrawPile;

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[require(CardsPile, Replicated)]
pub struct HandPile;

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[relationship_target(relationship = CardInPile, linked_spawn)]
#[require(Replicated)]
pub struct CardsPile(#[entities] Vec<Entity>);

impl Default for CardsPile {
    fn default() -> Self {
        Self::init()
    }
}

impl CardsPile {
    pub fn init() -> Self {
        Self(vec![])
    }
}

#[derive(Component, Debug, Clone, Serialize, Deserialize)]
#[relationship(relationship_target = CardsPile)]
#[require(Replicated)]
pub struct CardInPile(#[entities] Entity);

const CARD_WIDTH: i32 = 121;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeckKind {
    Draw,
    Hand,
}

#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[require(Replicated, SaveHistory)]
pub struct CardState(i32, DeckKind, BattleTick);

impl CardState {
    pub fn as_pos(&self) -> Vec2 {
        Vec2::new(((self.0 + 1) * CARD_WIDTH) as f32, 0.0)
    }

    pub fn is_in_hand(&self) -> bool {
        match self.1 {
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

pub fn on_battle_tick_init(
    _: On<Add, BattleTick>,
    s: Res<State<ClientState>>,
    q_subtick: Query<Entity, With<BattleSubTick>>,
    mut cmd: Commands,
) {
    match s.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }

    for e in q_subtick {
        cmd.entity(e).despawn();
    }

    cmd.spawn(BattleSubTick(0));
}

#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[require(Replicated)]
pub struct BattleTick {
    turn: i32,
    action: i32,
}

impl PartialOrd for BattleTick {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let self_total = self.turn as f32 + (self.action as f32 / 10.0);
        let other_total = other.turn as f32 + (other.action as f32 / 10.0);

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
        let self_total = self.turn as f32 + (self.action as f32 / 10.0);
        let other_total = other.turn as f32 + (other.action as f32 / 10.0);

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
    pub fn joined_tick(&self) -> (i32, i32) {
        (self.turn, self.action)
    }

    pub fn initial() -> Self {
        Self { turn: 0, action: 0 }
    }

    pub fn increment_next_turn(&mut self) {
        self.action = 0;
        self.turn += 1;
    }
}

#[derive(Component, Debug, Clone)]
pub struct BattleSubTick(pub i32);

pub const TICK_DURACTION_MS: u64 = 1000;
pub const MAX_ANIM_DURATION_MS: u64 = TICK_DURACTION_MS - 20;

pub fn handle_animate_tick(
    e: On<AnimateTick>,
    q_cards: Query<(Entity, &Viewable<Card>, &History<CardState>)>,
    q_world_pos: Query<&WorldPos>,
    mut cmd: Commands,
) {
    let tick = e.0;
    let min_action_tick = q_cards
        .iter()
        .map(|v| {
            v.2.0
                .iter()
                .filter(|t| t.0.turn as u32 == tick)
                .map(|t| t.0.action)
                .min()
                .unwrap_or(999)
        })
        .min()
        .unwrap();

    let max_action_tick = q_cards
        .iter()
        .map(|v| {
            v.2.0
                .iter()
                .filter(|t| t.0.turn as u32 == tick)
                .map(|t| t.0.action)
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap();

    println!("min {:?} max {:?}", min_action_tick, max_action_tick);

    let action_ticks_count = (max_action_tick - min_action_tick) + 1;

    for (card_entity, viewable, history) in q_cards.iter() {
        let view = viewable.view().entity();
        let maybe_world_pos = q_world_pos.get(view);

        if maybe_world_pos.is_ok() {
            let change_group_by_subtick: HashMap<i32, &Vec<CardState>> = history
                .0
                .iter()
                .filter_map(|(t, vals)| {
                    if t.turn != (tick as i32) || vals.is_empty() {
                        return None;
                    }

                    Some((t.action, vals))
                })
                .collect();

            let world_pos = maybe_world_pos.unwrap();

            println!("animating history : {:?}", history.0);
            let anim_duration_ms: f32 = (MAX_ANIM_DURATION_MS as f32) / (action_ticks_count as f32);
            let anim_done_delay: f32 = anim_duration_ms + 10.0;
            let easing = EaseFunction::CubicInOut;
            let duration = Duration::from_millis(anim_duration_ms.round() as u64);
            let mut last_loop_changes: Option<&Vec<CardState>> = None;

            for tick_nb in min_action_tick..=max_action_tick {
                let loop_i = tick_nb - min_action_tick;
                let loop_delay_ms = anim_done_delay * (loop_i as f32);
                println!("animating card index changes on tick : {:?}", tick_nb);

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
                    println!("animating world pos from card index : {:?}", card_idx.0);

                    let start_pos: Vec2 = match i == 0 {
                        true => match last_loop_changes {
                            None => world_pos.position,
                            Some(vals) => {
                                let prev = vals.iter().last().unwrap();
                                prev.as_pos()
                            }
                        },
                        false => {
                            let prev = changes.get(i - 1).unwrap();
                            prev.as_pos()
                        }
                    };
                    let end_pos = card_idx.as_pos();

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
                        .spawn((anim, AnimTarget::component::<WorldPos>(view)))
                        .id();

                    let mut anim = TweenAnim::new(tf_tween).with_destroy_on_completed(true);
                    anim.playback_state = PlaybackState::Paused;

                    let anim_b = cmd
                        .spawn((anim, AnimTarget::component::<WorldPos>(view)))
                        .id();

                    let delay_secs = loop_delay_ms / 1000.0;
                    cmd.spawn(Delayer::from_secs(delay_secs)).observe(
                        move |_: On<DelayCompleted>, mut q: Query<&mut TweenAnim>| {
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
pub struct CardDataView;

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

    cmd.entity(*view).insert((
        CardDataView,
        WorldPos {
            position: Vec2::ZERO,
            transform: UiTransform::IDENTITY,
        },
    ));
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

fn invert_indexes(mut cards: Vec<Mut<CardState>>) {
    let count = cards.len();
    if count <= 1 {
        return;
    }

    // sort the Vec in place by CardIndex value
    cards.sort_by_key(|val| val.0);

    let ordered_indexes: Vec<i32> = cards.iter().map(|val| val.0).collect();

    for (i, card_idx) in cards.iter_mut().enumerate() {
        let new = *ordered_indexes.get((count - 1) - i).unwrap();
        println!("invert from {:?} to {:?}", card_idx.0, new);
        card_idx.0 = new;
    }
}

fn test_pos_tr(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut player_cards_q: Query<&mut CardState, (With<PlayerCard>, With<InHand>, Without<EnemyCard>)>,
    mut enemy_cards_q: Query<&mut CardState, (With<EnemyCard>, With<InHand>, Without<PlayerCard>)>,
) {
    if !keyboard_input.just_pressed(KeyCode::KeyI) {
        return;
    }

    invert_indexes(player_cards_q.iter_mut().collect::<Vec<Mut<CardState>>>());
    invert_indexes(enemy_cards_q.iter_mut().collect::<Vec<Mut<CardState>>>());
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
        .insert((UiCardMarker, Card::new(), components));

    cmd.entity(card).with_children(|parent| {
        let in_draw = parent
            .spawn_substate(card, (StateComponent(InDrawPile)))
            .id();

        let being_drawn = parent
            .spawn_substate(card, (StateComponent(BeingDrawn)))
            .id();

        let in_hand = parent.spawn_substate(card, (StateComponent(InHand))).id();

        parent.spawn_transition::<DrawCard>(in_draw, being_drawn);

        let cmds = parent.commands_mut();
        cmds.entity(card).init_state_machine(in_draw);
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

impl HasDieselTarget<BoardPos> for DrawCard {
    fn diesel_target(&self) -> bevy_diesel::prelude::Target<BoardPos> {
        Target::position(BoardPos::new_on_enemy(1))
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

impl HasDieselTarget<BoardPos> for CardDrawn {
    fn diesel_target(&self) -> bevy_diesel::prelude::Target<BoardPos> {
        Target::position(BoardPos::new_on_enemy(1))
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
        source: &CardState,
        cards: &mut Vec<(Entity, &Card, &CardState)>,
        collection: &mut CardsPile,
    );
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
#[require(CardTargeting, Replicated)]
pub struct Magnetic {
    pub direction: CardDir,
    pub strength: i32,
}

impl Magnetic {
    pub fn new(direction: CardDir, strength: i32) -> Self {
        Self {
            direction,
            strength,
        }
    }
}

impl GeneratesCardTargeting for Magnetic {
    fn apply_effect(
        &self,
        source: &CardState,
        cards: &mut Vec<(Entity, &Card, &CardState)>,
        collection: &mut CardsPile,
    ) {
        if cards.len() < 2 {
            return;
        }

        cards.sort_by_key(|(_, _, card_idx)| card_idx.0);

        let curr = source.0;
        // Position of the source *within the `cards` slice* — NOT `curr` itself.
        let Some(curr_pos) = cards.iter().position(|(_, _, idx)| idx.0 == curr) else {
            return;
        };
        let curr_pos = curr_pos as i32;
        let len = cards.len() as i32;

        let right_count = (len - 1 - curr_pos).min(self.strength);
        let left_count = curr_pos.min(self.strength);

        let col_indexes: HashMap<Entity, i32> = collection
            .0
            .iter()
            .enumerate()
            .map(|(i, e)| (*e, i as i32))
            .collect();

        let modified: Vec<(Entity, i32)> = match self.direction {
            CardDir::Right => {
                if right_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos + 1) as usize;
                    let end = (curr_pos + 1 + right_count) as usize;
                    cards[start..end]
                        .iter()
                        .map(|(e, _, i)| (*e, i.0 - right_count))
                        .collect()
                }
            }
            CardDir::Left => {
                if left_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos - left_count) as usize;
                    let end = curr_pos as usize;
                    cards[start..end]
                        .iter()
                        // moving *toward* curr means increasing index, not decreasing
                        .map(|(e, _, i)| (*e, i.0 + left_count))
                        .collect()
                }
            }
            CardDir::Around => {
                let mut right: Vec<(Entity, i32)> = if right_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos + 1) as usize;
                    let end = (curr_pos + 1 + right_count) as usize;
                    cards[start..end]
                        .iter()
                        .map(|(e, _, _)| (*e, col_indexes.get(e).unwrap() - right_count))
                        .collect()
                };

                let mut left: Vec<(Entity, i32)> = if left_count <= 0 {
                    vec![]
                } else {
                    let start = (curr_pos - left_count) as usize;
                    let end = curr_pos as usize;
                    cards[start..end]
                        .iter()
                        .map(|(e, _, _)| (*e, col_indexes.get(e).unwrap() - left_count))
                        .collect()
                };

                right.append(&mut left);
                right
            }
        };

        for (e, new_i) in modified.into_iter() {
            let old_idx = cards.iter().find(|v| v.0 == e).unwrap().2.0;
            println!("switching card from {:?} to {:?}", old_idx, new_i.max(0));
            collection.0.place(e, new_i.max(0) as usize);
        }
    }
}

pub fn status_effect<E: EntityEvent + Clone, T: Component + Clone>(effect: T) -> impl Bundle {
    related!(StatusEffects[(TriggerOn::<E>::new(), effect, observe(tick_on::<E>))])
}

pub fn magnetic_effect(direction: CardDir, strength: i32) -> impl Bundle {
    status_effect::<DrawCard, Magnetic>(Magnetic::new(direction, strength))
}

pub fn burn_effect(direction: CardDir, strength: i32) -> impl Bundle {
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
    mut q_decks: Query<&mut CardsPile>,
    q_card_of: Query<&CardInPile>,
    // Instead use cardindex.stat
    cards: Query<(Entity, &Card, &CardState)>,
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

    let mut cards: Vec<(Entity, &Card, &CardState)> = cards.iter_many(&target_deck.0).collect();

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

pub fn input_linked_tests(
    mut cmd: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    maybe_battle_data: Option<Res<BattleData>>,
) {
    for key in keys.get_just_pressed() {
        match key {
            KeyCode::Space => {
                if maybe_battle_data.is_some() {
                    return;
                }
                cmd.insert_resource(BattleData::new());
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

fn check_update_cards_idx(
    q_decks: Query<(&CardsPile, Has<HandPile>), Changed<CardsPile>>,
    q_just_drawn: Query<(), With<JustDrawn>>,
    q_idx: Query<&CardState>,
    q_battle_tick: Query<&BattleTick>,
    mut writer: MessageWriter<DrawCard>,
    mut cmd: Commands,
    mut increment_action_tick: ResMut<IncrementActionTick>,
) {
    if q_decks.count() == 0 {
        return;
    }

    let battle_tick = q_battle_tick.single().unwrap();

    for (deck, is_hand) in q_decks.iter() {
        for (i, card) in deck.iter().enumerate() {
            let card_kind = match is_hand {
                true => DeckKind::Hand,
                false => DeckKind::Draw,
            };

            let after = CardState(i as i32, card_kind, battle_tick.clone());
            let Ok(before) = q_idx.get(card) else {
                // Initializes value the first time
                cmd.entity(card).insert(after);
                continue;
            };

            if before.0 == after.0 && before.1 == after.1 {
                continue;
            }

            cmd.entity(card).insert(after);

            if is_hand && q_just_drawn.contains(card) {
                println!("on draw mon calisse");
                cmd.entity(card).remove::<JustDrawn>();
                writer.write(DrawCard { card });
            }
        }
    }

    increment_action_tick.0 = true;
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

fn check_increment_action_tick(
    mut q: Query<&mut BattleTick>,
    mut increment_action_tick: Option<ResMut<IncrementActionTick>>,
    mut cmd: Commands,
) {
    let Some(mut increment_action_tick) = increment_action_tick else {
        cmd.insert_resource(IncrementActionTick(false));
        return;
    };

    if q.count() == 0 {
        return;
    }

    if !increment_action_tick.0 {
        return;
    }

    let mut battle_tick = q.single_mut().unwrap();
    battle_tick.action += 1;

    increment_action_tick.0 = false;
}

#[derive(Resource)]
struct ClientBattleAnimState {
    started: bool,
    timer: Timer,
    next_animated_tick: u32,
}

impl Default for ClientBattleAnimState {
    fn default() -> Self {
        Self {
            started: false,
            timer: Timer::from_seconds((TICK_DURACTION_MS as f32 / 1000.0), TimerMode::Repeating),
            next_animated_tick: 0,
        }
    }
}

fn handle_battle_tick_updated(
    e: On<BattleTickUpdated>,
    mut anim_state: ResMut<ClientBattleAnimState>,
    s: Res<State<ClientState>>,
) {
    match s.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }

    let tick = &e.0;

    if tick.turn != 0 || anim_state.started {
        return;
    }

    anim_state.started = true;
}

#[derive(Event)]
pub struct AnimateTick(pub u32);

fn handle_tick_battle_anim(
    mut anim_state: ResMut<ClientBattleAnimState>,
    s: Res<State<ClientState>>,
    time: Res<Time>,
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

    anim_state.timer.tick(time.delta());
    if !anim_state.timer.just_finished() {
        return;
    }

    println!(
        "triggering anim for tick : {:?}",
        anim_state.next_animated_tick
    );
    cmd.trigger(AnimateTick(anim_state.next_animated_tick));
    anim_state.next_animated_tick += 1;
}

#[derive(Resource)]
struct IncrementActionTick(bool);

#[derive(Message, Debug, Clone, PartialEq, Eq, Hash)]
struct TurnTick;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct TurnTickSet;

fn main() {
    let mut app = App::new();

    app.configure_sets(Update, TurnTickSet.run_if(on_message::<TurnTick>));
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
                filter:
                    "info,wgpu_core=error,wgpu_hal=error,ghx_proc_gen=debug,bevy_replicon=debug,renet=debug,bevy_renet=debug,bevy_replicon_renet=debug"
                        .into(),
                level: bevy::log::Level::DEBUG,
                ..default()
            }),
    ))
    .add_plugins(TilemapPlugin)
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
    .add_systems(FixedPreUpdate, check_increment_action_tick.run_if(in_state(ServerState::Running)))
    .add_observer(handle_animate_tick)
    // Observers
    .add_observer(propagate_effect_statuses::<DrawCard>)
    .add_observer(tick_effects)
    .register_viewable::<Card>()
    .add_observer(build_card_view)
    .add_systems(FixedUpdate, (|mut reader: MessageReader<DrawCard>, q: Query<(Entity, &CardState)>, mut cmd: Commands| {
        for e in reader.read() {
            if let Ok((ent, ci)) = q.get(e.card) {
                cmd.trigger_next_tick(e.clone());
            }
        }
    }).before(check_update_cards_idx))
    .add_systems(FixedUpdate, tick_delayers)
    .add_observer(on_battle_tick_init)
    .add_observer(handle_battle_tick_updated)
    .add_systems(FixedUpdate, (|q: Query<&History<CardState>, Changed<History<CardState>>>| {
        for hist in &q {
            for (tick, changes) in &hist.0 {
                println!("changes for tick {:?} are : {:?}", tick.joined_tick(), changes);
            }
        }
    }).run_if(in_state(ClientState::Connected)))
    .add_systems(
        FixedUpdate,
        check_update_cards_idx.run_if(in_state(ServerState::Running)),
    )
    .add_systems(FixedUpdate, check_increment_action_tick.after(check_update_cards_idx).run_if(in_state(ServerState::Running)))
    .add_systems(FixedUpdate, handle_tick_battle_anim)
    .run();
}
