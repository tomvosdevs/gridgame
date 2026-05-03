use std::time::Duration;

use bevy::{
    app::{Plugin, Update},
    asset::{Assets, RenderAssetUsages, uuid::Uuid},
    camera::{Camera, Camera2d, Camera3d, ClearColorConfig, RenderTarget},
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
        alpha::AlphaMode,
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
        UiTargetCamera, UiTransform, Val, ZIndex, px, widget::Text,
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
use bevy_ui_anchor::{AnchorPoint, AnchorUiConfig, AnchorUiPlugin, AnchoredUiNodes};
use haalka::{
    HaalkaPlugin,
    align::{Align, Alignable},
    jonmo::signal,
    prelude::{BuilderPassThrough, Column, Cursorable, El, Element, Row, SignalExt, Spawnable},
};

use crate::{
    ActiveCamera, CursorTarget, Health, MaxHealth, SkewMaterial,
    deck::deck_and_cards::{CardDrawn, SoulLife},
    game_flow::turns::CurrentDeckReference,
    visuals::cards::animation::{CardAnimatedBy, CardReleased, HighlightedTarget},
};

const CUBE_POINTER_ID: PointerId = PointerId::Custom(Uuid::from_u128(90870987));

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(HaalkaPlugin::new())
            .add_plugins(TweeningPlugin)
            .add_observer(spawn_card_drawn_notifer)
            .add_observer(spawn_card_diegetic_ui)
            .add_plugins(AnchorUiPlugin::<UiCameraMarker>::new())
            .add_plugins(DefaultTweenPlugins::default())
            // .add_systems(
            //     Startup,
            //     (|world: &mut World| {
            //         card_ui_root().spawn(world);
            //     })
            //     .before(setup_diegetic_ui),
            // )
            // .add_systems(Startup, setup_diegetic_ui.after(startup_3d))
            // TODO: Add this back for input handling
            // .add_systems(First, drive_diegetic_pointer.in_set(PickingSystems::Input))
            .insert_resource(DraggedCard::empty())
            .add_systems(Update, tag_active_camera)
            .add_systems(Update, draw_cursor_target_health_ui)
            .add_systems(
                Update,
                remove_cursor_target_health_ui.after(draw_cursor_target_health_ui),
            )
            .add_systems(
                Update,
                spawn_healthbar_ui
                    .after(draw_cursor_target_health_ui)
                    .before(remove_cursor_target_health_ui),
            )
            .add_systems(Update, handle_card_drawn_notifiers);
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

pub fn card_ui(card_text_entity: Entity) -> impl Element {
    El::<Node>::new()
        .with_node(|mut n| {
            n.height = Val::Percent(100.);
            n.width = Val::Percent(100.);
            n.padding = UiRect::all(Val::Px(10.0));
            n.position_type = PositionType::Relative;
        })
        .insert(Pickable {
            should_block_lower: false,
            is_hoverable: true,
        })
        .insert(BackgroundColor::from(Color::hsla(0., 0., 0., 0.)))
        .insert(CardUiRoot)
        .cursor(CursorIcon::default())
        .align_content(Align::new().left().top())
        .child(
            El::<Node>::new()
                .with_node(|mut n| {
                    n.position_type = PositionType::Absolute;
                    n.top = Val::Px(0.0);
                    n.left = Val::Px(0.0);
                    n.width = Val::Px(80.0);
                    n.height = Val::Px(80.0);
                    n.border_radius = BorderRadius::all(Val::Px(100.0));
                    n.display = Display::Flex;
                    n.align_items = AlignItems::Center;
                    n.justify_content = JustifyContent::Center;
                })
                .insert(BackgroundColor::from(Srgba::hex("#B01807").unwrap()))
                .insert(ZIndex(10))
                .child(
                    El::<Text>::new()
                        .text(Text::new("2"))
                        .text_font(TextFont::from_font_size(48.0)),
                ),
        )
        .child(
            El::<Node>::new()
                .with_node(|mut n| {
                    n.height = Val::Percent(100.);
                    n.width = Val::Percent(100.);
                    n.padding = UiRect::all(Val::Percent(8.0));
                    n.border_radius = BorderRadius::all(Val::Px(18.0));
                })
                .insert(BackgroundColor::from(Srgba::hex("#F7F5F3").unwrap()))
                .child_signal(
                    signal::from_component_changed::<CardUiTextContent>(card_text_entity).map_in(
                        |text_sections| {
                            let sections = text_sections.sections.to_vec();
                            Column::<Node>::new().items(
                                sections
                                    .into_iter()
                                    .map(|section| match section {
                                        TextSection::Title(s) => El::<Text>::new()
                                            .text(Text::new(s.clone()))
                                            .text_font(TextFont::from_font_size(64.0)),
                                        TextSection::Subtitle(s) => El::<Text>::new()
                                            .text(Text::new(s.clone()))
                                            .text_font(TextFont::from_font_size(48.0)),
                                        TextSection::Description(s) => El::<Text>::new()
                                            .text(Text::new(s.clone()))
                                            .text_font(TextFont::from_font_size(26.0)),
                                    })
                                    .into_iter(),
                            )
                        },
                    ),
                ),
        )
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
#[require(Mesh3d)]
pub struct CardUiTargetMesh {
    pub source_card: Entity,
}

impl CardUiTargetMesh {
    pub fn new(source_card: Entity) -> Self {
        Self { source_card }
    }
}

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

fn handle_card_drawn_notifiers(world: &mut World) {
    let mut q = world.query::<(Entity, &CardDrawnInHandNotifier)>();
    let notifiers_data: Vec<(Entity, CardDrawnInHandNotifier)> =
        q.iter(world).map(|(e, n)| (e, n.clone())).collect();

    let test_card_data_ent = world
        .spawn(CardUiTextContent {
            sections: vec![
                TextSection::Title("Hey".to_string()),
                TextSection::Subtitle("Some sub".to_string()),
            ],
        })
        .id();

    for (notifier_ent, notifier) in notifiers_data {
        let card_ui_root = card_ui(test_card_data_ent).spawn(world);
        world.trigger(CardUiSpawned {
            entity: card_ui_root,
            card_hand_index: notifier.card_hand_index,
            source_card_entity: notifier.card_entity,
        });
        world.commands().entity(notifier_ent).despawn();
    }
}

fn spawn_card_diegetic_ui(
    e: On<CardUiSpawned>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, SkewMaterial>>>,
    mut images: ResMut<Assets<Image>>,
    main_3d_cam_q: Query<&Transform, (With<Camera3d>, With<ActiveCamera>)>,
) {
    let main_cam_tf = main_3d_cam_q
        .single()
        .expect("found more than one Cam3d with 'ActiveCamera'");

    println!("in spawn card diegetic ui");

    let card_ui_root_ent = e.entity;
    let card_hand_index = e.card_hand_index;

    let card_aspect_ratio = AspectRatio::try_new(4.0, 5.0).unwrap();
    let card_max_side_length = 512.0;
    let card_mesh_max_side_length = 7.5;
    let aspect_ratio_multiplier = match card_aspect_ratio.is_portrait() {
        true => Vec2::new(card_aspect_ratio.ratio(), 1.0),
        false => Vec2::new(1.0, card_aspect_ratio.ratio()),
    };

    println!("size mul = {:?}", aspect_ratio_multiplier);

    let size = Extent3d {
        width: (card_max_side_length * aspect_ratio_multiplier.x).round() as u32,
        height: (card_max_side_length * aspect_ratio_multiplier.y).round() as u32,
        ..default()
    };

    // This is the texture that will be rendered to.
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // You need to set these texture usage flags in order to use the image as a render target
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;

    let image_handle = images.add(image);

    let texture_camera = commands
        .spawn((
            Camera2d,
            CardTextureCamera,
            Camera {
                // render before the "main pass" camera
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image_handle.clone().into()),
        ))
        .id();

    commands
        .entity(card_ui_root_ent)
        .insert(UiTargetCamera(texture_camera));

    let card_mesh_width = card_mesh_max_side_length * aspect_ratio_multiplier.x;
    let card_mesh_height = card_mesh_max_side_length * aspect_ratio_multiplier.y;
    let mesh_handle = meshes.add(Rectangle::new(card_mesh_width, card_mesh_height));

    let cam_forward = main_cam_tf.forward();
    let cam_left = main_cam_tf.left();
    let cam_right = main_cam_tf.right();
    let cam_down = main_cam_tf.down();
    let cam_up = main_cam_tf.up();

    // This material has the texture that has been rendered.
    let material_handle = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color_texture: Some(image_handle),
            perceptual_roughness: 1.0,
            cull_mode: None,
            alpha_mode: AlphaMode::Blend,
            // emissive_exposure_weight: 0.8,
            // #TODO: See how to make the card react to light
            // a bit while still being lit enough to be readable
            unlit: true,
            ..default()
        },
        extension: SkewMaterial::default(),
    });

    let gap = 1.0;
    let margin = 0.7;
    let offset_step = gap + card_mesh_width;
    let card_x_offset = card_hand_index as f32 * offset_step;

    let card_translation = main_cam_tf.translation
        + *cam_forward * 3.0
        + *cam_left * 21.2
        + *cam_right * card_x_offset // spread cards left by index
        + *cam_down * 11.8
        // Correct position to account for card size
        + *cam_up * ((card_mesh_height/2.0) + margin)
        + *cam_right * ((card_mesh_width/2.0) + margin);

    // Cube with material containing the rendered UI texture.
    commands
        .spawn((
            CardUiTargetMesh::new(e.source_card_entity),
            Mesh3d(mesh_handle.clone()),
            MeshMaterial3d(material_handle),
            Transform::from_translation(card_translation).looking_to(cam_forward, Vec3::Y),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            DiegeticUiTarget,
        ))
        .observe(dragstart_card_mesh)
        .observe(drag_card_mesh)
        .observe(dragend_card_mesh);
    // .observe(over_moveup_card_mesh)
    // .observe(leave_moveback_card_mesh);

    // Main camera is spawned elsewhere
    //
    commands.spawn(CUBE_POINTER_ID);
}

#[derive(Component)]
pub struct TweenAnimator;

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
    mut q: Query<(Entity, &Transform, &GlobalTransform), With<CardUiTargetMesh>>,
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
    mut q: Query<(&Transform, &mut CardDragData), (With<Mesh3d>, With<CardUiTargetMesh>)>,
    main_cam_q: Query<(&Camera, &GlobalTransform), (With<ActiveCamera>, With<Camera3d>)>,
    mut dragged_card: ResMut<DraggedCard>,
) {
    println!("in drag before checks");
    let (main_cam, main_cam_tf) = main_cam_q.single().expect("found more than one cam3d");
    // cmd.entity(e.entity).log_components();
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
    mut q: Query<(Entity, &Transform, &DragStartWorldPos), (With<Mesh3d>, With<CardUiTargetMesh>)>,
    mut highlighted_target: ResMut<HighlightedTarget>,
    mut dragged_card: ResMut<DraggedCard>,
    mut card_animated_by_q: Query<&mut CardAnimatedBy, With<CardUiTargetMesh>>,
    _time_runners_q: Query<&mut TimeRunner>,
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

#[derive(Component)]
pub struct RemoveOnCursorTargetChange(Entity);

#[derive(Component)]
pub struct HealthUiRootJustSpawned(Entity);

#[derive(Component)]
pub struct HealthUiRoot(Entity);

pub fn draw_cursor_target_health_ui(
    mut cmd: Commands,
    q: Query<
        (Entity, &CurrentDeckReference),
        (
            Added<CursorTarget>,
            With<Transform>,
            Without<AnchoredUiNodes>,
        ),
    >,
    soul_life_q: Query<Entity, With<SoulLife>>,
) {
    for (ent, deck_ref) in &q {
        println!("start draw health ui");
        let soul_life_ent = soul_life_q
            .get(deck_ref.0)
            .expect("Deck should have soullife");
        cmd.entity(ent).insert(AnchoredUiNodes::spawn_one((
            RemoveOnCursorTargetChange(ent),
            // TODO: Check if this works ? is it overriden ? Needed ?
            Pickable::IGNORE,
            AnchorUiConfig {
                anchorpoint: AnchorPoint::middle(),
                offset: Some(Vec3::new(0.0, 0.5, 0.0)),
                ..Default::default()
            },
            Node {
                ..Default::default()
            },
            HealthUiRoot(soul_life_ent),
            HealthUiRootJustSpawned(soul_life_ent),
            // Outline::default(),
        )));
    }
}

pub fn spawn_healthbar_ui(world: &mut World) {
    let entities: Vec<(Entity, Entity)> = world
        .query::<(Entity, &HealthUiRootJustSpawned)>()
        .iter(world)
        .map(|(entity, root_data)| (entity, root_data.0))
        .collect();

    for (entity, health_source) in entities {
        println!("herezzz");
        let ui_ent = healthbar_ui(health_source).spawn(world);
        world.commands().entity(ui_ent).set_parent_in_place(entity);
        world.entity_mut(entity).remove::<HealthUiRootJustSpawned>();
    }
}

pub fn healthbar_ui(health_source: Entity) -> impl Element {
    El::<Node>::new().child_signal(
        signal::from_component_changed::<SoulLife>(health_source).map_in_ref(|soul_life| {
            let curr_val = soul_life.current.clone();
            let max_val = soul_life.max.clone();
            El::<Node>::new()
                .with_node(move |mut node| {
                    node.height = Val::Px(10.0);
                    node.position_type = PositionType::Relative;
                })
                .child(
                    Column::<Node>::new()
                        .with_node(move |mut node| {
                            node.height = Val::Percent(100.0);
                            node.position_type = PositionType::Relative;
                            node.display = Display::Flex;
                            node.flex_direction = FlexDirection::Row;
                        })
                        .insert(ZIndex(1))
                        .items(
                            (0..(max_val as usize))
                                .map(|i| {
                                    let el = El::<Node>::new()
                                        .with_node(move |mut node| {
                                            node.height = Val::Px(10.0);
                                            node.width = Val::Px(10.0);
                                            node.border_radius = BorderRadius::all(Val::Px(2.0));
                                            node.right = Val::Px((i as f32) * 3.0)
                                        })
                                        .insert(UiTransform::from_rotation(Rot2::degrees(45.0)))
                                        .insert(ZIndex(100))
                                        .insert(GlobalZIndex(100))
                                        .insert(Outline::new(
                                            Val::Px(2.0),
                                            Val::ZERO,
                                            WHITE.with_alpha(0.2).into(),
                                        ))
                                        .child(
                                            El::<Node>::new()
                                                .with_node(|mut node| {
                                                    node.width = Val::Percent(100.0);
                                                    node.height = Val::Percent(100.0);
                                                    node.border_radius =
                                                        BorderRadius::all(Val::Px(2.0));
                                                })
                                                .insert(Outline::new(
                                                    Val::Px(2.0),
                                                    Val::ZERO,
                                                    WHITE.into(),
                                                ))
                                                .insert(GlobalZIndex(10)),
                                        );

                                    if i < (curr_val as usize) {
                                        el.insert(BackgroundColor(Color::Srgba(RED)))
                                    } else {
                                        el.insert(BackgroundColor(Color::Srgba(GREY)))
                                    }
                                })
                                .collect::<Vec<El<Node>>>(),
                        ),
                )
        }),
    )
}

/// Because bevy has no way to know how to map a mouse input to the UI texture, we need to write a
/// system that tells it there is a pointer on the UI texture. We cast a ray into the scene and find
/// the UV (2D texture) coordinates of the raycast hit. This UV coordinate is effectively the same
/// as a pointer coordinate on a 2D UI rect.
fn drive_diegetic_pointer(
    mut cursor_last: Local<Vec2>,
    mut raycast: MeshRayCast,
    rays: Res<RayMap>,
    cubes: Query<&Mesh3d, With<DiegeticUiTarget>>,
    ui_camera: Query<&RenderTarget, (With<Camera2d>, With<CardTextureCamera>)>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    windows: Query<(Entity, &Window)>,
    images: Res<Assets<Image>>,
    manual_texture_views: Res<ManualTextureViews>,
    mut window_events: MessageReader<WindowEvent>,
    mut pointer_inputs: MessageWriter<PointerInput>,
) -> Result {
    // Get the size of the texture, so we can convert from dimensionless UV coordinates that span
    // from 0 to 1, to pixel coordinates.
    // TODO: Switch to remove single and allow multiple
    let target = ui_camera
        .single()?
        .normalize(primary_window.single().ok())
        .unwrap();
    let target_info = target
        .get_render_target_info(windows, &images, &manual_texture_views)
        .unwrap();
    let size = target_info.physical_size.as_vec2();

    // Find raycast hits and update the virtual pointer.
    let raycast_settings = MeshRayCastSettings {
        visibility: RayCastVisibility::VisibleInView,
        filter: &|entity| cubes.contains(entity),
        early_exit_test: &|_| false,
    };
    for (_id, ray) in rays.iter() {
        for (_cube, hit) in raycast.cast_ray(*ray, &raycast_settings) {
            let position = size * hit.uv.unwrap();
            if position != *cursor_last {
                pointer_inputs.write(PointerInput::new(
                    CUBE_POINTER_ID,
                    Location {
                        target: target.clone(),
                        position,
                    },
                    PointerAction::Move {
                        delta: position - *cursor_last,
                    },
                ));
                *cursor_last = position;
            }
        }
    }

    // Pipe pointer button presses to the virtual pointer on the UI texture.
    for window_event in window_events.read() {
        if let WindowEvent::MouseButtonInput(input) = window_event {
            let button = match input.button {
                MouseButton::Left => PointerButton::Primary,
                MouseButton::Right => PointerButton::Secondary,
                MouseButton::Middle => PointerButton::Middle,
                _ => continue,
            };
            let action = match input.state {
                ButtonState::Pressed => PointerAction::Press(button),
                ButtonState::Released => PointerAction::Release(button),
            };
            pointer_inputs.write(PointerInput::new(
                CUBE_POINTER_ID,
                Location {
                    target: target.clone(),
                    position: *cursor_last,
                },
                action,
            ));
        }
    }

    Ok(())
}

pub fn remove_cursor_target_health_ui(
    mut cmd: Commands,
    mut removed: RemovedComponents<CursorTarget>,
    ui_q: Query<(Entity, &RemoveOnCursorTargetChange)>,
) {
    for removed_entity in removed.read() {
        for (ui_ent, marker) in &ui_q {
            if marker.0 == removed_entity {
                cmd.entity(ui_ent).despawn();
            }
        }
    }
}
