use std::time::Duration;

use bevy::{
    app::{Plugin, Startup, Update},
    asset::{AssetServer, Assets, Handle, RenderAssetUsages, uuid::Uuid},
    camera::{Camera, Camera2d, ClearColorConfig, RenderTarget},
    color::{
        Alpha, Color, Srgba,
        palettes::css::{GREEN, GREY, RED, WHITE},
    },
    ecs::{
        children,
        component::Component,
        entity::Entity,
        error::Result,
        event::EntityEvent,
        lifecycle::RemovedComponents,
        message::{MessageReader, MessageWriter},
        observer::On,
        query::{Added, With, Without},
        resource::Resource,
        schedule::IntoScheduleConfigs,
        spawn::SpawnRelated,
        system::{Commands, Local, Query, Res, ResMut},
        world::World,
    },
    image::Image,
    input::{ButtonState, mouse::MouseButton},
    material::AlphaMode,
    math::{
        AspectRatio, Rot2, Vec2, Vec3,
        primitives::{InfinitePlane3d, Rectangle},
    },
    mesh::{Mesh, Mesh3d},
    pbr::{ExtendedMaterial, MeshMaterial3d, StandardMaterial},
    picking::{
        Pickable,
        backend::ray::RayMap,
        events::{Drag, DragEnd, DragStart, Pointer},
        mesh_picking::ray_cast::{MeshRayCast, MeshRayCastSettings, RayCastVisibility},
        pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
    },
    prelude::{Deref, DerefMut},
    render::{
        camera::NormalizedRenderTargetExt,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        texture::ManualTextureViews,
    },
    text::TextFont,
    time::{Time, Timer, TimerMode},
    transform::{
        commands::BuildChildrenTransformExt,
        components::{GlobalTransform, Transform},
    },
    ui::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, Display, FlexDirection,
        GlobalZIndex, IsDefaultUiCamera, JustifyContent, Node, Outline, PositionType, UiRect,
        UiTargetCamera, UiTransform, Val, ZIndex, percent, px,
        widget::{ImageNode, Text},
    },
    utils::default,
    window::{CursorIcon, PrimaryWindow, Window, WindowEvent},
};
use bevy_ecs::lifecycle::Add;
use bevy_tween::{
    DefaultTweenPlugins,
    bevy_time_runner::TimeRunner,
    prelude::{AnimationBuilderExt, EaseKind, TransformTargetStateExt},
    tween::IntoTarget,
};
use bevy_tweening::TweeningPlugin;

use crate::{
    ActiveCamera, CursorTarget, SkewMaterial, UiCardMarker,
    deck::deck_and_cards::{CardDrawn, SoulLife},
    game_flow::turns::CurrentDeckReference,
    visuals::cards::animation::{CardAnimatedBy, CardReleased, HighlightedTarget},
};

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(TweeningPlugin)
            .add_observer(spawn_card_drawn_notifer)
            .add_plugins(DefaultTweenPlugins::default())
            .insert_resource(DraggedCard::empty())
            .insert_resource(CardVisualAssets::default())
            .add_systems(
                Startup,
                |mut cmd: Commands, asset_server: Res<AssetServer>| {
                    cmd.insert_resource(CardVisualAssets::new(asset_server.load("card_base.png")));
                },
            )
            .add_systems(Update, tag_active_camera);
    }
}

#[derive(Clone, Debug)]
pub enum TextSection {
    Title(String),
    Subtitle(String),
    Description(String),
}

impl TextSection {
    fn text(&self) -> &str {
        match self {
            TextSection::Title(title_str) => title_str,
            TextSection::Subtitle(subtitle_str) => subtitle_str,
            TextSection::Description(desc_str) => desc_str,
        }
    }
}

#[derive(Component, Clone, Deref, DerefMut, Debug)]
pub struct CardUiTextContent {
    pub sections: Vec<TextSection>,
}

impl CardUiTextContent {
    pub fn copies_sections(&self) -> Vec<TextSection> {
        self.sections.clone()
    }
}

#[derive(Component)]
pub struct CardUiRoot;

#[derive(Resource, Clone, Debug)]
pub struct CardVisualAssets {
    pub background: Option<Handle<Image>>,
}

impl CardVisualAssets {
    pub fn new(background: Handle<Image>) -> Self {
        Self {
            background: Some(background.into()),
        }
    }
}

impl Default for CardVisualAssets {
    fn default() -> Self {
        Self { background: None }
    }
}

#[derive(Component)]
/// We need a marker for the camera, so the plugin knows which camera to perform position
/// calculations towards
pub struct UiCameraMarker;

#[derive(Component)]
struct DiegeticUiTarget;

#[derive(Component)]
pub struct CardTextureCamera;

#[derive(Component)]
pub struct DragStartWorldPos(Transform);

#[derive(EntityEvent)]
pub struct CardUiSpawned {
    pub entity: Entity,
    pub card_hand_index: u16,
    pub source_card_entity: Entity,
}

#[derive(Component, Clone)]
pub struct CardDrawnInHandNotifier {
    pub card_entity: Entity,
    pub card_hand_index: u16,
}

fn spawn_card_drawn_notifer(e: On<CardDrawn>, mut cmd: Commands) {
    println!("spawning card notifier for card entity : {:?}", e.entity);
    cmd.spawn(CardDrawnInHandNotifier {
        card_entity: e.entity,
        card_hand_index: e.card_hand_index,
    });
}

#[derive(Component)]
pub struct TweenAnimator;

#[derive(Component)]
pub struct TweenedBy(Entity);

#[derive(Component)]
pub struct CardDragData {
    pub timer: Timer,
    pub last_event_time: Duration,
    pub card_start_global_tf: GlobalTransform,
    pub intended_translation: Vec3,
    pub prev_global_pointer_pos: Option<Vec3>,
}

#[derive(Resource)]
pub struct DraggedCard {
    pub entity: Option<Entity>,
    pub distance: Vec2,
}

impl DraggedCard {
    pub fn new(entity: Entity, distance: Vec2) -> Self {
        Self {
            entity: Some(entity),
            distance,
        }
    }

    pub fn empty() -> Self {
        Self {
            entity: None,
            distance: Vec2::ZERO,
        }
    }

    pub fn reset(self: &mut Self) {
        self.entity = None;
        self.distance = Vec2::ZERO;
    }
}

pub fn dragstart_card_mesh(
    e: On<Pointer<DragStart>>,
    mut cmd: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &Transform, &GlobalTransform), With<UiCardMarker>>,
    mut dragged_card: ResMut<DraggedCard>,
) {
    let (mesh_ent, mesh_tf, mesh_global_tf) = q
        .get_mut(e.entity)
        .expect("didn't find all needed components");

    cmd.entity(mesh_ent).insert((
        DragStartWorldPos(mesh_tf.clone()),
        CardDragData {
            timer: Timer::from_seconds(0.1, TimerMode::Once),
            last_event_time: time.elapsed(),
            card_start_global_tf: mesh_global_tf.clone(),
            prev_global_pointer_pos: None,
            intended_translation: mesh_tf.translation,
        },
    ));

    dragged_card.entity = Some(mesh_ent);
}

pub fn drag_card_mesh(
    e: On<Pointer<Drag>>,
    mut cmd: Commands,
    time: Res<Time>,
    mut q: Query<(&Transform, &mut CardDragData), (With<UiCardMarker>)>,
    main_cam_q: Query<(&Camera, &GlobalTransform), (With<ActiveCamera>, With<Camera2d>)>,
    mut dragged_card: ResMut<DraggedCard>,
) {
    println!("in drag before checks");
    let (main_cam, main_cam_tf) = main_cam_q.single().expect("found more than one cam3d");

    let Ok((mesh_tf, mut drag_data)) = q.get_mut(e.entity) else {
        println!("nope");
        // the drag event might fire before DragStart ?
        return;
    };

    let last_evt_time = drag_data.last_event_time.clone();
    drag_data.timer.tick(time.elapsed() - last_evt_time);

    drag_data.last_event_time = time.elapsed();
    println!("in drag youuu");
    if !drag_data.timer.is_finished() {
        return;
    }

    println!("timer done");

    drag_data.timer.reset();
    drag_data.timer.unpause();

    let pointer_pos = e.pointer_location.position;
    let pointer_world_pos = main_cam
        .viewport_to_world(main_cam_tf, pointer_pos)
        .unwrap()
        .plane_intersection_point(
            drag_data.card_start_global_tf.translation(),
            InfinitePlane3d::new(drag_data.card_start_global_tf.forward()),
        )
        .unwrap();

    if drag_data.prev_global_pointer_pos.is_none() {
        let start_pointer_pos = e.pointer_location.position - e.distance;
        let start_pointer_world_pos = main_cam
            .viewport_to_world(main_cam_tf, start_pointer_pos)
            .unwrap()
            .plane_intersection_point(
                drag_data.card_start_global_tf.translation(),
                InfinitePlane3d::new(drag_data.card_start_global_tf.forward()),
            )
            .unwrap();

        drag_data.prev_global_pointer_pos = Some(start_pointer_world_pos);
    }

    let movement_since_last_anim = pointer_world_pos - drag_data.prev_global_pointer_pos.unwrap();
    drag_data.intended_translation += movement_since_last_anim;

    dragged_card.distance = e.distance;

    let target = e.entity.into_target();
    let mut tween_start = target.transform_state(*mesh_tf);
    let min_scale_factor: f32 = 0.5;
    println!("LEN = {:?}", e.distance.length());
    let scale_factor = (min_scale_factor
        - (((e.distance.y.abs() / 130.0).min(1.0)) * min_scale_factor))
        + min_scale_factor;
    let scale_end = Vec3::splat(scale_factor);
    let scale_tween = tween_start.scale_to(scale_end);
    let translation_tween = tween_start.translation_to(drag_data.intended_translation);
    println!("here");
    cmd.entity(e.entity).animation().insert_tween_here(
        Duration::from_millis(95),
        EaseKind::CubicOut,
        (scale_tween, translation_tween),
    );
    drag_data.prev_global_pointer_pos = Some(pointer_world_pos);
}

pub fn dragend_card_mesh(
    e: On<Pointer<DragEnd>>,
    mut cmd: Commands,
    mut q: Query<(Entity, &Transform, &DragStartWorldPos), (With<UiCardMarker>)>,
    mut highlighted_target: ResMut<HighlightedTarget>,
    mut dragged_card: ResMut<DraggedCard>,
    mut card_animated_by_q: Query<&mut CardAnimatedBy, With<UiCardMarker>>,
) {
    let Ok((mesh_ent, mesh_tf, mesh_drag_start_tf)) = q.get_mut(e.entity) else {
        return;
    };

    let ignore_tween = match dragged_card.entity {
        Some(card_entity) => {
            let mut ignore = false;
            if let Ok(mut animated_by) = card_animated_by_q.get_mut(card_entity) {
                animated_by.needs_rotation_setup = true;

                if highlighted_target.target_entity.is_some() {
                    ignore = true;
                    let mut mesh_ent_cmds = cmd.entity(mesh_ent);
                    mesh_ent_cmds.remove::<CardDragData>();
                    mesh_ent_cmds.remove::<DragStartWorldPos>();
                    dragged_card.reset();
                }
                println!("released card");
                cmd.trigger(CardReleased {
                    entity: card_entity,
                    selected_target: highlighted_target.target_entity,
                });
            }
            ignore
        }
        None => true,
    };

    highlighted_target.reset_and_remove_highlighter(&mut cmd);

    if ignore_tween {
        return;
    }

    let target = e.entity.into_target();
    let mut tween_start = target.transform_state(*mesh_tf);
    let scale_end = Vec3::splat(1.0);
    let scale_tween = tween_start.scale_to(scale_end);
    let translation_tween = tween_start.translation_to(mesh_drag_start_tf.0.translation);
    cmd.entity(e.entity).animation().insert_tween_here(
        Duration::from_millis(95),
        EaseKind::CubicOut,
        (scale_tween, translation_tween),
    );

    let mut mesh_ent_cmds = cmd.entity(mesh_ent);
    mesh_ent_cmds.remove::<CardDragData>();
    mesh_ent_cmds.remove::<DragStartWorldPos>();
    dragged_card.reset();
}

pub enum UiDataLevel {
    Title,
    Subtitle,
    Text,
}

pub struct UiData {
    pub level: UiDataLevel,
    pub text: &'static str,
}

pub trait UiDataContainer {
    fn get_ui_data(&self) -> Option<UiData>;
}

impl<T> UiDataContainer for T
where
    T: Component,
{
    fn get_ui_data(&self) -> Option<UiData> {
        None
    }
}

pub fn tag_active_camera(mut cmd: Commands, q: Query<Entity, (Added<ActiveCamera>, With<Camera>)>) {
    for cam_ent in &q {
        cmd.entity(cam_ent)
            .insert((UiCameraMarker, IsDefaultUiCamera));
    }
}
