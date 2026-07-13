use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;

use bevy::DefaultPlugins;
use bevy::app::{App, Startup};
use bevy::asset::{AssetServer, Handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::color::palettes::css::{PALE_TURQUOISE, RED};
use bevy::color::palettes::tailwind::{GRAY_300, ORANGE_400, RED_300, RED_800};
use bevy::core_pipeline::core_3d::graph::Node3d;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster};
use bevy::log::LogPlugin;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
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
use bevy_diesel::prelude::SpatialBackend;
use bevy_ecs::relationship::Relationship;
use bevy_ecs::system::SystemParam;
use bevy_ecs_tilemap::prelude::*;
use bevy_gearbox::{
    AcceptAll, GearboxMessage, InitStateMachine, SpawnSubstate, SpawnTransition, StateComponent,
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
use bevy_immediate::ui::text::ImmUiText;
use bevy_northstar::nav::Nav;
use bevy_tween::BevyTweenRegisterSystems;
use bevy_tween::prelude::Interpolator;
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

use crate::game_flow::turns::{CombatStart, CurrentDeckReference, PlayingEntity, TurnsPlugin};
use crate::grid_abilities_backend::DeckBackend;

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
    );
}

#[derive(Component)]
pub struct UiCardMarker;

#[derive(Component)]
pub struct CardHandContainer;

#[derive(Resource)]
pub struct PlayerHandContainer(pub Entity);

#[derive(Resource)]
pub struct EnemyHandContainer(pub Option<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = CardOf, linked_spawn)]
pub struct DeckOfCards(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = DeckOfCards)]
pub struct CardOf(Entity);

impl EnemyHandContainer {
    pub fn no_hand() -> Self {
        Self(None)
    }
}

#[derive(Message, Clone, Reflect)]
struct MoveToHand {
    machine: Entity,
}

impl GearboxMessage for MoveToHand {
    type Validator = AcceptAll;

    fn target(&self) -> Entity {
        self.machine
    }
}

#[derive(Component, Debug, Clone)]
pub struct CardIndex(i32);

#[derive(Component, Debug, Clone)]
pub struct PlayerCard;

#[derive(Component, Debug, Clone)]
pub struct EnemyCard;

pub fn handle_card_added(
    e: On<Add, Card>,
    player_cards_q: Query<Entity, With<PlayerCard>>,
    enemy_cards_q: Query<Entity, With<EnemyCard>>,
    mut cmd: Commands,
) {
    let new_index = match player_cards_q.contains(e.entity) {
        true => player_cards_q.count(),
        false => enemy_cards_q.count(),
    };

    cmd.entity(e.entity).insert(CardIndex(new_index as i32));
}

#[derive(Component)]
pub struct CardWidget;

#[derive(SystemParam)]
pub struct CardWidgetParams<'w, 's> {
    query: Query<'w, 's, &'static mut CardWidget>,
    visuals_assets: Res<'w, CardVisualAssets>,
}

impl ImmediateAttach<CapsUi> for CardWidget {
    type Params = CardWidgetParams<'static, 'static>;

    fn construct(ui: &mut Imm<CapsUi>, params: &mut CardWidgetParams) {
        let entity = ui.current_entity().unwrap();

        ui.ch().on_spawn_insert(|| {
            (
                Node {
                    width: px(121),
                    height: px(151),
                    ..default()
                },
                children![(
                    ImageNode::new(params.visuals_assets.background.clone()),
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    }
                )],
            )
        });
    }
}

fn spawn_card(
    asset_server: &Res<AssetServer>,
    components: impl Bundle,
    cmd: &mut Commands,
    is_on_player: bool,
) -> Entity {
    let card = cmd
        .spawn((CardWidget, UiCardMarker, Card::new(), components))
        .id();

    cmd.entity(card).with_children(|parent| {
        let in_draw = parent
            .spawn_substate(card, (Name::new("InDrawPile"), StateComponent(InDrawPile)))
            .id();
        let in_hand = parent
            .spawn_substate(card, (Name::new("InHand"), StateComponent(InHand)))
            .id();

        parent.spawn_transition::<MoveToHand>(in_draw, in_hand);

        let cmds = parent.commands_mut();
        cmds.entity(card).init_state_machine(in_draw);
    });

    if is_on_player {
        cmd.entity(card).insert(PlayerCard);
    } else {
        cmd.entity(card).insert(EnemyCard);
    }

    card
}

#[derive(EntityEvent, Clone)]
pub struct DrawCard {
    #[event_target]
    pub card: Entity,
    pub is_player: bool,
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
#[derive(Component, Clone)]
pub struct InHand;
#[derive(Component, Clone)]
pub struct InDrawPile;
#[derive(Component, Clone)]
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
        let curr = source.0;
        let max_i = (cards.len() - 1) as i32;

        let valid = match self.direction {
            CardDir::Right => {
                if curr == max_i {
                    vec![]
                } else {
                    cards[(curr + 1) as usize..(curr + 1 + self.strength) as usize].to_vec()
                }
            }
            CardDir::Left => {
                if curr == 0 {
                    vec![]
                } else {
                    cards[(curr - 1) as usize..(curr - 1 - self.strength) as usize].to_vec()
                }
            }
            CardDir::Around => {
                let mut right = match curr == max_i {
                    true => vec![],
                    false => {
                        cards[(curr + 1) as usize..(curr + 1 + self.strength) as usize].to_vec()
                    }
                };

                let mut left = match curr == 0 {
                    true => vec![],
                    false => {
                        cards[(curr - 1) as usize..(curr - 1 - self.strength) as usize].to_vec()
                    }
                };

                right.append(&mut left);
                right
            }
        };

        valid.iter().map(|(e, _, _)| *e).collect::<Vec<Entity>>()
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
    e: On<DrawCard>,
    player_hand: Res<PlayerHandContainer>,
    enemy_hand: Res<EnemyHandContainer>,
    mut cmd: Commands,
) {
    let target_container = match e.is_player {
        true => player_hand.0,
        false => enemy_hand
            .0
            .expect("Enemy hand should exist when drawing a card"),
    };

    cmd.entity(e.card).insert(ChildOf(target_container));
}

// Change card into a reusable widget and use a

fn setup_base_scene(
    mut cmd: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    asset_server: Res<AssetServer>,
) {
    println!("tompere");

    cmd.spawn((Camera::default(), Camera2d::default(), ActiveCamera));

    cmd.spawn((
        Mesh2d(meshes.add(Rectangle::new(1000., 700.))),
        MeshMaterial2d(materials.add(Color::srgb(0.2, 0.2, 0.3))),
    ));

    let player_hand = cmd.spawn((Node::default(), MainSceneUiRoot)).id();

    let card_one = spawn_card(
        &asset_server,
        (magnetic_effect(CardDir::Around, 1), CardOf(player_hand)),
        &mut cmd,
        true,
    );

    cmd.entity(player_hand).add_child(card_one);

    cmd.insert_resource(PlayerHandContainer(player_hand));

    let new_card = spawn_card(
        &asset_server,
        (magnetic_effect(CardDir::Around, 1), CardOf(player_hand)),
        &mut cmd,
        true,
    );

    cmd.entity(player_hand).add_child(new_card);

    let new_card_b = spawn_card(
        &asset_server,
        (magnetic_effect(CardDir::Around, 1), CardOf(player_hand)),
        &mut cmd,
        true,
    );

    cmd.entity(player_hand).add_child(new_card_b);

    cmd.trigger(DrawCard {
        card: new_card,
        is_player: true,
    });
}

pub fn start_combat_test(mut cmd: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    let default_deck = cmd.spawn(Deck).id();

    cmd.spawn((Card::new(), InDeck(default_deck), StatelessCard::new()));

    // You
    cmd.spawn((PlayingEntity, CurrentDeckReference(default_deck)));
    // The ennemy
    cmd.spawn((PlayingEntity, CurrentDeckReference(default_deck)));

    cmd.trigger(CombatStart);
}

pub struct ImmeditateUiPlugin;

impl Plugin for ImmeditateUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BevyImmediateAttachPlugin::<CapsUi, MainSceneUiRoot>::new());
    }
}

#[derive(Component)]
pub struct MainSceneUiRoot;

#[derive(SystemParam)]
pub struct HandUiParams<'w, 's> {
    pub query: Query<'w, 's, &'static CardIndex>,
}

impl ImmediateAttach<CapsUi> for MainSceneUiRoot {
    // Allows to access world data
    type Params = HandUiParams<'static, 'static>;

    fn construct(ui: &mut Imm<CapsUi>, params: &mut HandUiParams) {
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
            .add(|ui| {
                for c in &params.query {
                    ui.ch().text(format!("here is a card!!!!"));
                }
            });
    }
}

fn main() {
    App::new()
        .add_plugins((
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
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .add_systems(Startup, setup_base_scene)
        .add_systems(Update, start_combat_test)
        .insert_resource(EnemyHandContainer::no_hand())
        .add_observer(handle_draw)
        .add_observer(propagate_effect_statuses::<DrawCard>)
        .add_observer(tick_effects)
        .add_observer(handle_card_added)
        .add_systems(Startup, |mut cmd: Commands| {
            cmd.spawn((Node::default(), MainSceneUiRoot));
        })
        .run();
}
