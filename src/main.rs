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
use bevy::color::palettes::tailwind::{GRAY_300, ORANGE_400, RED_300};
use bevy::core_pipeline::core_3d::graph::Node3d;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::light::{DirectionalLightShadowMap, NotShadowCaster};
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
use bevy_diesel::prelude::SpatialBackend;
use bevy_ecs_tilemap::prelude::*;
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
use bevy_northstar::nav::Nav;
use bevy_tween::BevyTweenRegisterSystems;
use bevy_tween::prelude::Interpolator;
use pyri_state::setup::StatePlugin;
use rand::RngExt;
use rand::distr::uniform;

use crate::abilities::abilities_templates::AbilitiesTemplatePlugin;
use crate::abilities::effects::StatusEffectsPlugin;
use crate::creatures::generation::CreatureGenerationPlugin;
use crate::debug::ui::DebugUiPlugin;
use crate::deck::card_blueprints::CardBlueprintPlugin;
use crate::deck::deck_and_cards::DeckAndCardsPlugin;
use crate::effects::{Burning, EffectsPlugin};

use crate::game_flow::turns::TurnsPlugin;
use crate::grid_abilities_backend::Grid3DBackend;

use crate::ui::GameUiPlugin;
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
            (
                CreatureGenerationPlugin,
                StatusEffectsPlugin,
                CardBlueprintPlugin,
            ),
        ))
        .add_plugins(AbilitiesTemplatePlugin)
        .add_plugins(Grid3DBackend::plugin())
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        // .add_systems(
        //     Startup, startup_3d, // setup_object_masking).chain()
        // )
        .run();
}
