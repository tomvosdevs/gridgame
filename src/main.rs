use std::collections::HashMap;
use std::f32::consts::PI;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use bevy::DefaultPlugins;
use bevy::app::{App, Startup};
use bevy::asset::{AssetServer, Handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::color::palettes::css::{BLUE, GREEN, PALE_TURQUOISE, PURPLE, RED, YELLOW};
use bevy::color::palettes::tailwind::{GRAY_300, ORANGE_400, RED_300, RED_800};
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
use bevy_ecs::relationship::Relationship;
use bevy_ecs::schedule::{MultiThreadedExecutor, ScheduleLabel};
use bevy_ecs::system::SystemParam;
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
use bevy_tween::BevyTweenRegisterSystems;
use bevy_tween::prelude::{AnimationBuilderExt, EaseKind, Interpolator};
use bevy_tween::tween::{ComponentTween, IntoTarget};
use bevy_tweening::lens::{
    UiPositionLens, UiTransformRotationLens, UiTransformScaleLens, UiTransformTranslationPxLens,
};
use bevy_tweening::{AnimTarget, CycleCompletedEvent, Lens, Tween, TweenAnim};
use moonshine_kind::GetInstanceCommands;
use moonshine_view::{RegisterViewable, Viewable, ViewableKind};
use pyri_state::setup::StatePlugin;
use rand::RngExt;
use rand::distr::uniform;

use crate::abilities::abilities_templates::AbilitiesTemplatePlugin;
use crate::abilities::effects::{
    StatusEffectOf, StatusEffects, StatusEffectsPlugin, Tick, TriggerEffect, TriggerOn,
};
use crate::creatures::generation::CreatureGenerationPlugin;
use crate::debug::ui::DebugUiPlugin;

use crate::deck::deck_and_cards::{Card, Deck, DeckAndCardsPlugin, InDeck, StatelessCard};
use crate::effects::{Burning, EffectsPlugin};

use crate::game_flow::turns::{CurrentDeckReference, EnteredCombat, PlayingEntity, TurnsPlugin};
use crate::grid_abilities_backend::{BoardPos, DeckBackend};

use crate::ui::{CardVisualAssets, GameUiPlugin};
use crate::visuals::cards::animation::DiegeticCardTweenPlugin;

pub mod abilities;
pub mod creatures;
pub mod debug;
pub mod deck;
pub mod effects;
pub mod game_flow;
pub mod grid_abilities_backend;

pub mod stats;

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

#[derive(Component)]
pub struct UiCardMarker;

#[derive(Component)]
pub struct CardHandContainer;

#[derive(Resource)]
pub struct PlayerHandContainer(pub Entity);

#[derive(Resource)]
pub struct EnemyHandContainer(Entity);

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = CardOf, linked_spawn)]
pub struct DeckOfCards(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = DeckOfCards)]
pub struct CardOf(Entity);

const CARD_WIDTH: i32 = 121;

#[derive(Component, Debug, Clone)]
pub struct CardIndex(i32);

impl CardIndex {
    pub fn as_pos(&self) -> Vec2 {
        Vec2::new(((self.0 + 1) * CARD_WIDTH) as f32, 0.0)
    }
}

#[derive(Component, Debug, Clone)]
pub struct PlayerCard;

#[derive(Component, Debug, Clone)]
pub struct EnemyCard;

// Styling constants
pub const CARDS_COL_GAP: i32 = 16;

pub fn handle_card_added(
    e: On<Add, Card>,
    player_cards_q: Query<Entity, With<PlayerCard>>,
    enemy_cards_q: Query<Entity, With<EnemyCard>>,
    mut cmd: Commands,
) {
    let new_index = match player_cards_q.contains(e.entity) {
        true => player_cards_q.count() - 1,
        false => enemy_cards_q.count() - 1,
    };

    cmd.entity(e.entity).insert(CardIndex(new_index as i32));
}

pub fn handle_cardindex_change(
    q_cards: Query<(&CardIndex, Ref<CardIndex>, &Viewable<Card>)>,
    q_world_pos: Query<&WorldPos>,
    mut cmd: Commands,
) {
    for (card_idx, _, viewable) in q_cards
        .iter()
        .filter(|(_, c_ref, _)| c_ref.is_changed() && !c_ref.is_added())
    {
        let view = viewable.view().entity();
        let world_pos = q_world_pos
            .get(view)
            .expect("Should find would pos for view");

        let easing = EaseFunction::CubicInOut;
        let duration = Duration::from_millis(550);

        let tween = Tween::new(
            easing,
            duration,
            WorldPosLens {
                pos: AnimMode::FromTo {
                    start: world_pos.position.unwrap(),
                    end: card_idx.as_pos(),
                },
                tf: AnimMode::NoAnim,
                translation: AnimMode::NoAnim,
            },
        );

        cmd.spawn((
            TweenAnim::new(tween).with_destroy_on_completed(true),
            AnimTarget::component::<WorldPos>(view),
        ));
    }
}

#[derive(Component, Clone)]
pub struct CardWidgetFor(Entity);

#[derive(SystemParam)]
pub struct CardWidgetParams<'w, 's> {
    query: Query<'w, 's, (Entity, &'static mut CardWidgetFor)>,
    q_card_viewable: Query<'w, 's, &'static Viewable<Card>>,
    q_world_cards: Query<'w, 's, &'static WorldPos>,
    visuals_assets: Res<'w, CardVisualAssets>,
}

#[derive(Component)]
pub struct ImageOf(Entity);

impl ImmediateAttach<CapsUi> for CardWidgetFor {
    type Params = CardWidgetParams<'static, 'static>;

    fn construct(ui: &mut Imm<CapsUi>, params: &mut CardWidgetParams) {
        let entity = ui.current_entity().unwrap();
        let (widget_entity, widget_for) = params.query.get_mut(entity).unwrap();
        let source_entity = widget_for.0;
        let view = params
            .q_card_viewable
            .get_mut(source_entity)
            .unwrap()
            .view()
            .entity();
        let world_card = params
            .q_world_cards
            .get(view)
            .expect("world card should exist");

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
                if let Some(pos) = world_card.position {
                    n.left = px(pos.x);
                    n.bottom = px(pos.y);
                };
            });
    }
}

#[derive(Component, Debug, Reflect, Clone)]
pub struct WorldPos {
    position: Option<Vec2>,
    transform: Option<UiTransform>,
}

impl WorldPos {
    pub fn hidden() -> Self {
        Self {
            position: None,
            transform: None,
        }
    }
}

impl ViewableKind for Card {
    fn view_bundle() -> impl Bundle {
        WorldPos::hidden()
    }
}

fn build_card_view(
    event: On<Add, Viewable<Card>>,
    query: Query<&Viewable<Card>>,
    mut commands: Commands,
) {
    let viewable = query.get(event.entity).unwrap();
    let view = viewable.view();
    println!("built view");
    commands.entity(*view).insert(WorldPos::hidden());
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
        if target.position.is_some() {
            match self.pos {
                AnimMode::FromTo { start, end } => {
                    target.position = Some(start.lerp(end, ratio));
                }
                AnimMode::NoAnim => {}
            }
        }
        if target.transform.is_some() {
            match self.tf {
                AnimMode::FromTo { start, end } => {
                    let new_translation = match self.translation {
                        AnimMode::FromTo { start, end } => {
                            let lerped = start.lerp(end, ratio);
                            Val2::px(lerped.x, lerped.y)
                        }
                        AnimMode::NoAnim => target.transform.unwrap().translation,
                    };

                    target.transform = Some(UiTransform {
                        translation: new_translation,
                        scale: start.scale.lerp(end.scale, ratio),
                        rotation: start.rotation.slerp(end.rotation, ratio),
                    });
                }
                AnimMode::NoAnim => {}
            }
        }
    }
}

fn handle_card_added_to_hand(
    event: On<Add, InHand>,
    query: Query<(&Viewable<Card>, &CardIndex)>,
    mut commands: Commands,
) {
    let (viewable, card_index) = query.get(event.entity).unwrap();
    let view = viewable.view().entity();
    println!("built view");

    commands.entity(view).insert(WorldPos {
        position: Some(Vec2::ZERO),
        transform: Some(UiTransform::default()),
    });

    let tween = Tween::new(
        EaseFunction::CubicOut,
        Duration::from_millis(400),
        WorldPosLens::position(Vec2::ZERO, card_index.as_pos()),
    );

    commands.spawn_empty().insert((
        AnimTarget::component::<WorldPos>(view),
        TweenAnim::new(tween).with_destroy_on_completed(true),
    ));
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

fn test_pos_tr(keyboard_input: Res<ButtonInput<KeyCode>>, mut q: Query<&mut CardIndex>) {
    if !keyboard_input.just_pressed(KeyCode::KeyI) {
        return;
    }

    let count = q.count();
    if count == 0 {
        return;
    }

    let max_index = count - 1;

    for (i, mut card_idx) in q
        .iter_mut()
        .sort_by_key::<&CardIndex, _>(|val| val.0)
        .enumerate()
    {
        println!(
            "INVERTING INDEX from {:?} to {:?}",
            card_idx.0,
            (max_index - i)
        );
        card_idx.0 = (max_index - i) as i32;
    }
}

#[derive(Clone, Component)]
pub struct TestMark;

fn spawn_card(
    components: impl Bundle,
    cmd: &mut Commands,
    parent: Entity,
    is_on_player: bool,
) -> Entity {
    let card = match is_on_player {
        true => cmd
            .spawn((PlayerCard, CardOf(parent), ChildOf(parent)))
            .id(),
        false => cmd.spawn((EnemyCard, CardOf(parent), ChildOf(parent))).id(),
    };

    cmd.entity(card)
        .insert((UiCardMarker, Card::new(), components));

    cmd.entity(card).with_children(|parent| {
        let in_draw = parent
            .spawn_substate(card, (StateComponent(InDrawPile), StateComponent(TestMark)))
            .id();
        let in_hand = parent
            .spawn_substate(card, (StateComponent(InHand), StateComponent(TestMark)))
            .id();

        parent.spawn_transition::<GameEvent<DrawCard, Requested>>(in_draw, in_hand);

        let cmds = parent.commands_mut();
        cmds.entity(card).init_state_machine(in_draw);
    });

    card
}

pub trait GameEvtState: Clone + Debug + Sync + Send + Reflect + 'static {}

#[derive(Clone, Debug, Reflect)]
pub struct Requested;
impl GameEvtState for Requested {}

#[derive(Clone, Debug, Reflect)]
pub struct Running;
impl GameEvtState for Running {}

#[derive(Clone, Debug, Reflect)]
pub struct Completed;
impl GameEvtState for Completed {}

#[derive(EntityEvent, Clone, Message, Debug, Reflect)]
pub struct GameEvent<E: EntityEvent + Clone, S: GameEvtState> {
    #[event_target]
    pub target: Entity,
    pub event: E,
    pub state: S,
}

impl<E: EntityEvent + Clone + Reflect + TypePath, S: GameEvtState + TypePath> GearboxMessage
    for GameEvent<E, S>
{
    type Validator = AcceptAll;

    fn target(&self) -> Entity {
        self.target
    }
}

pub trait BoardUtilsCommandsExt {
    fn request_game_event<E: EntityEvent + Clone>(&mut self, event: E);

    fn run_game_event<E: EntityEvent + Clone>(&mut self, event: &GameEvent<E, Requested>);

    fn complete_game_event<E: EntityEvent + Clone>(&mut self, event: GameEvent<E, Running>);

    fn send_and_trigger<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;
}

impl<'w, 's> BoardUtilsCommandsExt for Commands<'w, 's> {
    fn request_game_event<E: EntityEvent + Clone>(&mut self, event: E) {
        self.send_and_trigger(GameEvent {
            target: event.event_target(),
            event,
            state: Requested,
        });
    }

    fn run_game_event<E: EntityEvent + Clone>(&mut self, event: &GameEvent<E, Requested>) {
        let event = GameEvent {
            target: event.target,
            event: event.event.clone(),
            state: Running,
        };

        self.send_and_trigger(event);
    }

    fn complete_game_event<E: EntityEvent + Clone>(&mut self, event: GameEvent<E, Running>) {
        let event = GameEvent {
            target: event.target,
            event: event.event.clone(),
            state: Completed,
        };

        self.send_and_trigger(event);
    }

    fn send_and_trigger<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        self.write_message(event.clone());
        self.trigger(event);
    }
}

#[derive(EntityEvent, Clone, Message, Debug, Reflect)]
pub struct DrawCard {
    #[event_target]
    pub card: Entity,
    pub is_player: bool,
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
    pub fn on_player(card: Entity) -> Self {
        Self {
            card,
            is_player: true,
        }
    }

    pub fn on_enemy(card: Entity) -> Self {
        Self {
            card,
            is_player: false,
        }
    }
}

#[derive(Clone)]
pub enum TargetingAmount {
    Single,
    Fixed(i32),
    All,
}

#[derive(Clone)]
pub enum CardDir {
    Right,
    Left,
    Around,
}

// Card states marker
#[derive(Component, Clone, Debug, Reflect, Copy)]
pub struct InHand;
#[derive(Component, Clone, Debug, Reflect, Copy)]
pub struct InDrawPile;
#[derive(Component, Clone, Debug, Reflect, Copy)]
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
    fn get_valid_targets(
        &self,
        source: &CardIndex,
        cards: &mut Vec<(Entity, &Card, &CardIndex)>,
    ) -> Vec<Entity>;
}

#[derive(Component, Clone)]
#[require(CardTargeting)]
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
    fn get_valid_targets(
        &self,
        source: &CardIndex,
        cards: &mut Vec<(Entity, &Card, &CardIndex)>,
    ) -> Vec<Entity> {
        if cards.len() < 2 {
            return vec![];
        }

        println!("cards : {:?}", cards.len());

        cards.sort_by_key(|(_, _, card_idx)| card_idx.0);
        let index_values: Vec<i32> = cards.iter().map(|(_, _, idx)| idx.0).collect();
        let curr = source.0;
        let next_i = curr + 1;
        let prev_i = curr - 1;
        let max_i = *index_values.iter().max().unwrap();
        let min_i = *index_values.iter().min().unwrap();

        let right_offset = (max_i - curr).min(self.strength);
        let left_offset = (curr - min_i).min(self.strength);

        println!("min i {:?}, max i {:?}", min_i, max_i);
        println!("right off {:?}, left off {:?}", right_offset, left_offset);

        let valid = match self.direction {
            CardDir::Right => {
                if curr == max_i || right_offset <= 0 {
                    vec![]
                } else {
                    cards[(next_i) as usize..(next_i + right_offset) as usize].to_vec()
                }
            }
            CardDir::Left => {
                if curr == min_i || left_offset <= 0 {
                    vec![]
                } else {
                    cards[(curr) as usize..(prev_i - left_offset) as usize].to_vec()
                }
            }
            CardDir::Around => {
                println!("curr and max : {:?}, {:?}", curr, max_i);
                let mut right = match (curr == max_i || right_offset <= 0) {
                    true => vec![],
                    false => cards[(curr) as usize..]
                        .iter()
                        .take(right_offset as usize)
                        .map(|v| *v)
                        .collect::<Vec<_>>(),
                };

                println!("curr is : {:?}", curr);

                let mut left = match (curr == min_i || left_offset <= 0) {
                    true => vec![],
                    false => cards[..(curr) as usize]
                        .iter()
                        .take(left_offset as usize)
                        .map(|v| *v)
                        .collect::<Vec<_>>(),
                };

                right.append(&mut left);
                right
            }
        };

        let targets = valid.iter().map(|(e, _, _)| *e).collect::<Vec<Entity>>();

        let dir = match self.direction {
            CardDir::Right => "right",
            CardDir::Left => "left",
            CardDir::Around => "around",
        };
        println!(
            "selecting {:?} targets for i {:?} with strength {:?} and direction : {:?}",
            targets.len(),
            curr,
            self.strength,
            dir
        );

        targets
    }
}

pub fn status_effect<E: EntityEvent + Clone, T: Component + Clone>(effect: T) -> impl Bundle {
    related!(StatusEffects[(TriggerOn::<E>::new(), effect, observe(tick_on::<E>))])
}

pub fn magnetic_effect(direction: CardDir, strength: i32) -> impl Bundle {
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
    cards_q: Query<(Entity, &Card, &CardIndex)>,
    magnetic_effects: Query<&Magnetic>,
) {
    let Ok((effect_entity, effect_of)) = q.get(e.status) else {
        return;
    };

    let card_index = cards_q
        .get(effect_of.get())
        .expect("status parent should have a card index")
        .2;

    let mut cards: Vec<(Entity, &Card, &CardIndex)> = cards_q.iter().collect();

    let Ok(magnetic) = magnetic_effects.get(effect_entity) else {
        return;
    };

    for target_entity in magnetic.get_valid_targets(&card_index, &mut cards) {
        println!("affecting magnetic to an entity");
    }
}

pub fn handle_draw(
    e: On<GameEvent<DrawCard, Requested>>,
    q_widgets: Query<(Entity, &CardWidgetFor), With<Node>>,
    player_hand: Res<PlayerHandContainer>,
    enemy_hand: Res<EnemyHandContainer>,
    mut cmd: Commands,
) {
    let target_container = match e.event().event.is_player {
        true => player_hand.0,
        false => enemy_hand.0,
    };

    let ui_card = q_widgets
        .iter()
        .find(|(_, widget_for)| widget_for.0 == e.event().event.card)
        .expect("Widget for card shoudl exist")
        .0;

    println!("should work putain");

    cmd.entity(ui_card).insert(ChildOf(target_container));

    cmd.run_game_event(e.event());
}

// Change card into a reusable widget and use a

fn setup_base_scene(
    mut cmd: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    println!("tompere");

    cmd.spawn((Camera::default(), Camera2d::default(), ActiveCamera));

    cmd.spawn((
        Mesh2d(meshes.add(Rectangle::new(1000., 700.))),
        MeshMaterial2d(materials.add(Color::srgb(0.2, 0.2, 0.3))),
    ));

    cmd.trigger(EnteredCombat);

    // let player_hand = cmd.spawn((Node::default(), MainSceneUiRoot)).id();
    // cmd.insert_resource(PlayerHandContainer(player_hand));

    // let card_one = spawn_card(
    //     (magnetic_effect(CardDir::Around, 1), CardOf(player_hand)),
    //     &mut cmd,
    //     true,
    // );

    // cmd.entity(player_hand).add_child(card_one);

    // let new_card = spawn_card(
    //     (magnetic_effect(CardDir::Around, 1), CardOf(player_hand)),
    //     &mut cmd,
    //     true,
    // );

    // cmd.entity(player_hand).add_child(new_card);

    // let new_card_b = spawn_card(
    //     (magnetic_effect(CardDir::Around, 1), CardOf(player_hand)),
    //     &mut cmd,
    //     true,
    // );

    // cmd.entity(player_hand).add_child(new_card_b);
}

pub fn start_combat_test(
    mut cmd: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    q_deck_card: Query<
        Entity,
        (
            With<Card>,
            With<CardIndex>,
            Without<InHand>,
            With<PlayerCard>,
        ),
    >,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    let Some(card) = q_deck_card.iter().next() else {
        return;
    };

    println!("drawing this shii");
    cmd.entity(card).log_components();

    cmd.request_game_event(DrawCard {
        card,
        is_player: true,
    });
}

pub struct ImmeditateUiPlugin;

impl Plugin for ImmeditateUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            BevyImmediateAttachPlugin::<CapsUi, MainSceneUiRoot>::new(),
            BevyImmediateAttachPlugin::<CapsUi, CardWidgetFor>::new(),
            FlairPlugin,
        ));
    }
}

#[derive(Component)]
pub struct MainSceneUiRoot;

#[derive(SystemParam)]
pub struct HandUiParams<'w, 's> {
    pub q_cards: Query<
        'w,
        's,
        (
            Entity,
            &'static CardIndex,
            Option<&'static PlayerCard>,
            Option<&'static EnemyCard>,
        ),
    >,
    player_hand: Option<Res<'w, PlayerHandContainer>>,
    _enemy_hand: Option<Res<'w, EnemyHandContainer>>,
}

impl ImmediateAttach<CapsUi> for MainSceneUiRoot {
    type Params = HandUiParams<'static, 'static>;

    fn construct(ui: &mut Imm<CapsUi>, params: &mut HandUiParams) {
        // Grab this once, up front — don't call ui.current_entity() again later.
        let current_entity = ui.current_entity();

        // Collect just the Entity ids you need (Copy), not the borrowed tuples.
        let cards: Vec<Entity> = match current_entity {
            Some(entity) => match &params.player_hand {
                Some(player_hand) => {
                    let want_player = player_hand.0 == entity;
                    params
                        .q_cards
                        .iter()
                        .filter(|(_, _, maybe_player_card, maybe_enemy_card)| {
                            if want_player {
                                maybe_player_card.is_some()
                            } else {
                                maybe_enemy_card.is_some()
                            }
                        })
                        .map(|(e, _, _, _)| e)
                        .collect()
                }
                None => vec![],
            },
            None => vec![],
        };

        // Precompute the boolean the node_mut closure needs, so it doesn't touch params/ui.
        let is_enemy_hand = current_entity
            .zip(params._enemy_hand.as_ref())
            .map(|(e, res)| res.0 == e)
            .unwrap_or(false);

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
                if is_enemy_hand {
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

#[derive(Message, Debug, Clone, PartialEq, Eq, Hash)]
struct TurnTick;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
struct TurnTickSet;

#[derive(Resource)]
pub struct Stylesheets {
    pub hand: Handle<StyleSheet>,
}

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
                filter: "info,wgpu_core=error,wgpu_hal=error,ghx_proc_gen=debug".into(),
                level: bevy::log::Level::DEBUG,
                ..default()
            })
            .disable::<StatesPlugin>(),
        StatePlugin,
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
        DebugUiPlugin,
        (CreatureGenerationPlugin, StatusEffectsPlugin),
    ))
    .add_plugins(ImmeditateUiPlugin)
    .add_plugins(AbilitiesTemplatePlugin)
    .add_plugins(DeckBackend::plugin())
    .add_plugins(OpacityPlugin)
    // Resources
    .insert_resource(DirectionalLightShadowMap { size: 4096 })
    // Startup
    .add_systems(Startup, setup_base_scene.after(GearboxSet))
    .add_systems(
        Startup,
        |mut cmd: Commands, asset_server: Res<AssetServer>| {
            cmd.insert_resource(Stylesheets {
                hand: asset_server.load("styles/main.css"),
            });
        },
    )
    // Update
    .add_systems(Update, start_combat_test)
    .add_systems(Update, test_pos_tr)
    .add_systems(Update, handle_cardindex_change.after(test_pos_tr))
    // Observers
    .add_observer(handle_draw)
    .add_observer(propagate_effect_statuses::<DrawCard>)
    .add_observer(tick_effects)
    .add_observer(handle_card_added)
    // .add_observer(handle_card_widget_added)
    .add_observer(relay_event_as_message::<DrawCard>)
    .register_viewable::<Card>()
    .add_observer(build_card_view)
    .add_observer(handle_card_added_to_hand)
    .run();
}
