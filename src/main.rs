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
    BLUE_600, BLUE_800, GRAY_300, ORANGE_400, RED_300, RED_800, RED_900, SLATE_600,
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
use bevy_ecs::world::DeferredWorld;
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
use bevy_tweening::{AnimCompletedEvent, AnimTarget, CycleCompletedEvent, Lens, Tween, TweenAnim};
use moonshine_kind::{GetInstanceCommands, Instance};
use moonshine_view::{RegisterViewable, Viewable, ViewableKind};

use rand::RngExt;
use rand::distr::uniform;

use crate::abilities::abilities_templates::AbilitiesTemplatePlugin;
use crate::abilities::effects::{
    StatusEffectOf, StatusEffects, StatusEffectsPlugin, Tick, TriggerEffect, TriggerOn,
};
use crate::creatures::generation::CreatureGenerationPlugin;
use crate::debug::ui::DebugUiPlugin;

use crate::deck::deck_and_cards::{Card, DeckAndCardsPlugin, InDeck, StatelessCard};
use crate::effects::{Burning, EffectsPlugin};

use crate::game_flow::turns::{
    BattleData, CurrentDeckReference, EnteredCombat, PlayingEntity, TurnsPlugin,
};
use crate::grid_abilities_backend::{BoardPos, DeckBackend};

use crate::network::NetworkPlugin;
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

#[derive(Component)]
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
    pub draw_pile_entity: Instance<CardsPile>,
    pub hand_pile_entity: Instance<CardsPile>,
}

#[derive(Resource)]
pub struct EnemyData {
    pub ui_entity: Entity,
    pub draw_pile_entity: Instance<CardsPile>,
    pub hand_pile_entity: Instance<CardsPile>,
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

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = CardInPile, linked_spawn)]
pub struct CardsPile(Vec<Entity>);

impl CardsPile {
    pub fn init() -> Self {
        Self(vec![])
    }
}

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = CardsPile)]
pub struct CardInPile(Entity);

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

pub fn handle_cardindex_change(
    q_cards: Query<(
        Entity,
        &CardIndex,
        Ref<CardIndex>,
        &Viewable<Card>,
        Has<InHand>,
        Has<PlayerCard>,
    )>,
    q_world_pos: Query<&WorldPos>,
    mut cmd: Commands,
) {
    for (card_entity, card_idx, card_idx_ref, viewable, is_in_hand, is_player_card) in
        q_cards.iter()
    {
        let view = viewable.view().entity();

        if card_idx_ref.is_changed() && !card_idx_ref.is_added() {
            if !is_in_hand {
                continue;
            }

            // Only keep entries with a position
            let world_pos = q_world_pos
                .get(view)
                .expect("should have world pos initialized");

            let just_drawn = world_pos.position == Vec2::ZERO;
            // No need to animate a card when it's position is actually the same
            if card_idx.as_pos().x == world_pos.position.x {
                continue;
            }

            let easing = EaseFunction::CubicInOut;
            let duration = Duration::from_millis(550);

            let move_dir_multiplier: f32 = if card_idx.as_pos().x > world_pos.position.x {
                1.0
            } else {
                -1.0
            };

            let tween = Tween::new(
                easing,
                duration,
                WorldPosLens {
                    pos: AnimMode::FromTo {
                        start: world_pos.position,
                        end: card_idx.as_pos(),
                    },
                    tf: AnimMode::NoAnim,
                    translation: AnimMode::NoAnim,
                },
            );

            let degs_offset = 20.0 * move_dir_multiplier;
            let start_tf = world_pos.transform.clone();

            let tween_b = Tween::new(
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

            let animator = cmd
                .spawn((
                    TweenAnim::new(tween).with_destroy_on_completed(true),
                    AnimTarget::component::<WorldPos>(view),
                ))
                .id();

            if just_drawn {
                cmd.entity(animator).observe(
                    move |_: On<AnimCompletedEvent>, mut obs_cmd: Commands| {
                        obs_cmd.complete_game_event(GameEvent {
                            target: card_entity,
                            event: DrawCard {
                                card: card_entity,
                                is_player: is_player_card,
                            },
                            state: Running,
                        });
                    },
                );
            }

            cmd.spawn((
                TweenAnim::new(tween_b).with_destroy_on_completed(true),
                AnimTarget::component::<WorldPos>(view),
            ));
        } else if card_idx_ref.is_added() {
            cmd.entity(view).insert(WorldPos {
                position: Vec2::ZERO,
                transform: UiTransform::default(),
            });
        }
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
    mut cmd: Commands,
) {
    let viewable = query.get(event.entity).unwrap();
    let view = viewable.view();
    println!("built view");

    cmd.entity(*view).insert(CardDataView);
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

fn invert_indexes(mut cards: Vec<Mut<CardIndex>>) {
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
    mut player_cards_q: Query<&mut CardIndex, (With<PlayerCard>, With<InHand>, Without<EnemyCard>)>,
    mut enemy_cards_q: Query<&mut CardIndex, (With<EnemyCard>, With<InHand>, Without<PlayerCard>)>,
) {
    if !keyboard_input.just_pressed(KeyCode::KeyI) {
        return;
    }

    invert_indexes(player_cards_q.iter_mut().collect::<Vec<Mut<CardIndex>>>());
    invert_indexes(enemy_cards_q.iter_mut().collect::<Vec<Mut<CardIndex>>>());
}

#[derive(Clone, Component)]
pub struct TestMark;

fn spawn_card(
    components: impl Bundle,
    cmd: &mut Commands,
    draw_pile: Instance<CardsPile>,
    hand_pile: Instance<CardsPile>,
    is_on_player: bool,
) -> Entity {
    let draw_pile_entity = draw_pile.entity();
    let hand_pile_entity = hand_pile.entity();
    let card = match is_on_player {
        true => cmd
            .spawn((
                PlayerCard,
                CardInPile(draw_pile_entity),
                ChildOf(draw_pile_entity),
            ))
            .id(),
        false => cmd
            .spawn((
                EnemyCard,
                CardInPile(draw_pile_entity),
                ChildOf(draw_pile_entity),
            ))
            .id(),
    };

    cmd.entity(card)
        .insert((UiCardMarker, Card::new(), components));

    cmd.entity(card).with_children(|parent| {
        let in_draw = parent
            .spawn_substate(card, (StateComponent(InDrawPile)))
            .id();

        let in_hand = parent.spawn_substate(card, (StateComponent(InHand))).id();

        parent.spawn_transition::<GameEvent<DrawCard, Init>>(in_draw, in_hand);

        let cmds = parent.commands_mut();
        cmds.entity(card).init_state_machine(in_draw);
        cmds.entity(card)
            .observe(move |_: On<Add, InHand>, mut obs_cmd: Commands| {
                obs_cmd.entity(card).insert(CardInPile(hand_pile_entity));
            });
        cmds.entity(card)
            .observe(move |_: On<Add, InDrawPile>, mut obs_cmd: Commands| {
                obs_cmd.entity(card).insert(CardInPile(draw_pile_entity));
            });
    });

    card
}

pub trait GameEvtState: Clone + Debug + Sync + Send + Reflect + 'static {}

#[derive(Clone, Debug, Reflect)]
pub struct Requested;
impl GameEvtState for Requested {}

#[derive(Clone, Debug, Reflect)]
pub struct Init;
impl GameEvtState for Init {}

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
    fn request_event<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn request_game_event<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn init_game_event<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn run_game_event<E>(&mut self, event: &GameEvent<E, Init>)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn complete_game_event<E>(&mut self, event: GameEvent<E, Running>)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;

    fn send_and_trigger<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default;
}

pub trait DynEntityEvent: Send + Sync + 'static {
    fn target(&self) -> Entity;
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn DynEntityEvent>;
    fn trigger(&self, commands: &mut Commands, mode: EventHandleMode);
}

impl<E> DynEntityEvent for E
where
    E: EntityEvent + Send + Sync + Clone + Message + 'static,
    for<'a> E::Trigger<'a>: Default,
{
    fn target(&self) -> Entity {
        self.event_target()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_box(&self) -> Box<dyn DynEntityEvent> {
        Box::new(self.clone())
    }
    fn trigger(&self, commands: &mut Commands, mode: EventHandleMode) {
        match mode {
            EventHandleMode::AsGameEvent => commands.init_game_event(self.clone()),
            EventHandleMode::AsEvent => commands.trigger(self.clone()),
        }
    }
}

impl Clone for Box<dyn DynEntityEvent> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Event)]
pub struct CurrentGameEventComplete;

#[derive(Event)]
pub struct ReceivedGameEventReq {
    pub game_evt: Box<dyn DynEntityEvent>,
    pub mode: EventHandleMode,
}

#[derive(Debug, Clone)]
pub enum EventHandleMode {
    AsGameEvent,
    AsEvent,
}

#[derive(Resource)]
pub struct PendingEventRequests {
    pub requests: Vec<(Box<dyn DynEntityEvent>, EventHandleMode)>,
    pub running: bool,
}

impl PendingEventRequests {
    pub fn empty() -> Self {
        Self {
            requests: vec![],
            running: false,
        }
    }
}

pub fn handle_game_event_request(
    e: On<ReceivedGameEventReq>,
    mut pending_reqs: ResMut<PendingEventRequests>,
) {
    println!("a game event was requested");
    pending_reqs
        .requests
        .push((e.game_evt.clone(), e.mode.clone()));
}

pub fn check_pending_game_evt_req_changed(
    mut pending_reqs: ResMut<PendingEventRequests>,
    mut cmd: Commands,
) {
    if !pending_reqs.is_changed() || pending_reqs.running {
        return;
    }

    let Some((req, mode)) = pending_reqs.requests.first() else {
        return;
    };
    req.trigger(&mut cmd, mode.clone());
    pending_reqs.running = true;
}

pub fn handle_current_game_event_completed(
    _: On<CurrentGameEventComplete>,
    mut pending_reqs: ResMut<PendingEventRequests>,
) {
    println!("a game event was COMPLETED");
    pending_reqs.running = false;
    pending_reqs.requests.remove(0);
}

impl<'w, 's> BoardUtilsCommandsExt for Commands<'w, 's> {
    fn request_event<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        self.trigger(ReceivedGameEventReq {
            game_evt: Box::new(event),
            mode: EventHandleMode::AsEvent,
        });
    }

    fn request_game_event<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        self.trigger(ReceivedGameEventReq {
            game_evt: Box::new(event),
            mode: EventHandleMode::AsGameEvent,
        });
    }

    fn init_game_event<E>(&mut self, event: E)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        println!("a game event was initiliazed");
        self.send_and_trigger(GameEvent {
            target: event.event_target(),
            event,
            state: Init,
        });
    }

    fn run_game_event<E>(&mut self, event: &GameEvent<E, Init>)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        let event = GameEvent {
            target: event.target,
            event: event.event.clone(),
            state: Running,
        };

        self.send_and_trigger(event);
    }

    fn complete_game_event<E>(&mut self, event: GameEvent<E, Running>)
    where
        E: EntityEvent + Message + Clone,
        for<'a> E::Trigger<'a>: Default,
    {
        let event = GameEvent {
            target: event.target,
            event: event.event.clone(),
            state: Completed,
        };

        self.trigger(CurrentGameEventComplete);

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
    fn apply_effect(
        &self,
        source: &CardIndex,
        cards: &mut Vec<(Entity, &Card, &CardIndex)>,
        collection: &mut CardsPile,
    );
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
    fn apply_effect(
        &self,
        source: &CardIndex,
        cards: &mut Vec<(Entity, &Card, &CardIndex)>,
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
    related!(
        StatusEffects[(
            TriggerOn::<GameEvent<E, Completed>>::new(),
            effect,
            observe(tick_on::<GameEvent<E, Completed>>)
        )]
    )
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
    hand_cards_q: Query<(Entity, &Card, &CardIndex), With<InHand>>,
    magnetic_effects: Query<&Magnetic>,
) {
    let Ok((effect_entity, effect_of)) = q.get(e.status) else {
        return;
    };

    let target_card = q_card_of.get(effect_of.get()).unwrap().0;
    let target_deck = q_decks
        .get_mut(target_card)
        .expect("should find associated deck for effect card parent")
        .into_inner();

    println!("ticking effects");

    let applier_index_c = hand_cards_q
        .get(effect_of.get())
        .expect("status parent should have a card index")
        .2;

    let mut cards: Vec<(Entity, &Card, &CardIndex)> =
        hand_cards_q.iter_many(&target_deck.0).collect();

    let Ok(magnetic) = magnetic_effects.get(effect_entity) else {
        return;
    };

    magnetic.apply_effect(&applier_index_c, &mut cards, target_deck);
}

pub fn handle_draw_card(
    e: On<GameEvent<DrawCard, Init>>,
    q_widgets: Query<(Entity, &CardWidgetFor), With<Node>>,
    player_deck: Res<PlayerData>,
    enemy_deck: Res<EnemyData>,
    mut cmd: Commands,
) {
    let target_container = match e.event().event.is_player {
        true => player_deck.ui_entity,
        false => enemy_deck.ui_entity,
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

    // let player_deck = cmd.spawn((Node::default(), MainSceneUiRoot)).id();
    // cmd.insert_resource(PlayerHandContainer(player_deck));

    // let card_one = spawn_card(
    //     (magnetic_effect(CardDir::Around, 1), CardOf(player_deck)),
    //     &mut cmd,
    //     true,
    // );

    // cmd.entity(player_deck).add_child(card_one);

    // let new_card = spawn_card(
    //     (magnetic_effect(CardDir::Around, 1), CardOf(player_deck)),
    //     &mut cmd,
    //     true,
    // );

    // cmd.entity(player_deck).add_child(new_card);

    // let new_card_b = spawn_card(
    //     (magnetic_effect(CardDir::Around, 1), CardOf(player_deck)),
    //     &mut cmd,
    //     true,
    // );

    // cmd.entity(player_deck).add_child(new_card_b);
}

pub fn input_linked_tests(
    mut cmd: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    q_deck_card: Query<
        (Entity, Option<&PlayerCard>, Option<&EnemyCard>),
        (With<Card>, With<CardIndex>, Without<InHand>),
    >,
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
            // KeyCode::KeyP => {
            //     let Some(card) = q_deck_card
            //         .iter()
            //         .find_map(|(ent, _, maybe_enemy)| maybe_enemy.map(|_| ent))
            //     else {
            //         return;
            //     };
            //     cmd.request_game_event(DrawCard {
            //         card,
            //         is_player: false,
            //     });
            // }
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

#[derive(Component)]
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
            &'static CardIndex,
            &'static <D as DeckDataSupplier>::CardComponent,
        ),
    >,
    target_deck: Res<'w, D>,
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
    .insert_resource(PendingEventRequests::empty())
    // Startup
    .add_systems(Startup, setup_base_scene.after(GearboxSet))
    .add_systems(Update, input_linked_tests)
    .add_systems(Update, test_pos_tr)
    .add_systems(Update, handle_cardindex_change.after(test_pos_tr))
    .add_systems(Update, check_pending_game_evt_req_changed)
    // Observers
    .add_observer(handle_draw_card)
    .add_observer(propagate_effect_statuses::<DrawCard>)
    .add_observer(propagate_effect_statuses::<GameEvent<DrawCard, Init>>)
    .add_observer(propagate_effect_statuses::<GameEvent<DrawCard, Running>>)
    .add_observer(propagate_effect_statuses::<GameEvent<DrawCard, Completed>>)
    .add_observer(handle_game_event_request)
    .add_observer(handle_current_game_event_completed)
    .add_observer(tick_effects)
    // .add_observer(handle_card_widget_added)
    // .add_observer(relay_event_as_message::<DrawCard>)
    .register_viewable::<Card>()
    .add_observer(build_card_view)
    // .add_observer(handle_card_added_to_hand)
    .add_systems(
        Update,
        |q_decks: Query<&CardsPile, Changed<CardsPile>>, mut cmd: Commands| {
            if q_decks.count() == 0 {
                return;
            }

            for deck in q_decks.iter() {
                for (i, card) in deck.iter().enumerate() {
                    cmd.entity(card).insert(CardIndex(i as i32));
                }
            }
        },
    )
    .run();
}
