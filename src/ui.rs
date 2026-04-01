use std::{collections::HashMap, f32::consts::PI, ops::Mul, time::Duration};

use bevy::{
    app::{First, Plugin, Startup, Update},
    asset::{Assets, RenderAssetUsages, uuid::Uuid},
    audio::Sample,
    camera::{Camera, Camera2d, Camera3d, ClearColorConfig, RenderTarget},
    color::{
        Color, Srgba,
        palettes::css::{BLUE, GRAY, GREY, RED, WHITE},
    },
    ecs::{
        children,
        component::Component,
        entity::Entity,
        error::Result,
        event::EntityEvent,
        hierarchy::Children,
        lifecycle::RemovedComponents,
        message::{MessageReader, MessageWriter},
        observer::On,
        query::{Added, With, Without},
        schedule::IntoScheduleConfigs,
        spawn::SpawnRelated,
        system::{Commands, Local, Query, Res, ResMut},
        world::World,
    },
    image::Image,
    input::{ButtonState, mouse::MouseButton},
    math::{
        AspectRatio, Dir3, Quat, Vec2, Vec3,
        curve::EaseFunction,
        primitives::{Cuboid, InfinitePlane3d, Plane3d, Rectangle},
    },
    mesh::{Mesh, Mesh3d},
    pbr::{MeshMaterial3d, StandardMaterial},
    picking::{
        Pickable, PickingSystems,
        backend::ray::RayMap,
        events::{Drag, Out, Over, Pointer},
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
    text::{TextColor, TextFont},
    transform::components::{GlobalTransform, Transform},
    ui::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, ComputedNode, Display,
        FlexDirection, IsDefaultUiCamera, JustifyContent, JustifyItems, Node, Outline,
        PositionType, UiRect, UiTargetCamera, Val, ZIndex, percent, px, widget::Text,
    },
    utils::default,
    window::{CursorIcon, PrimaryWindow, Window, WindowEvent},
};
use bevy_tween::{
    DefaultTweenPlugins,
    bevy_time_runner::TimeRunner,
    interpolate::{translation, translation_by},
    prelude::{AnimationBuilderExt, EaseKind, TimeDirection, TransformTargetStateExt},
    tween::{AnimationTarget, IntoTarget},
};
use bevy_tweening::{EntityCommandsTweeningExtensions, TweeningPlugin};
use bevy_ui_anchor::{AnchorPoint, AnchorUiConfig, AnchorUiNode, AnchorUiPlugin, AnchoredUiNodes};
use haalka::{
    HaalkaPlugin,
    align::{Align, Alignable},
    jonmo::signal,
    prelude::{
        BuilderPassThrough, Column, Cursorable, El, Element, LazyEntity, Row, SignalExt, Spawnable,
        deref_copied,
    },
};

use crate::{
    ActiveCamera, CursorTarget, Health, MaxHealth,
    deck_and_cards::{Card, CardDrawn},
    startup_3d,
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
            .add_systems(Update, tag_active_camera)
            .add_systems(Update, draw_cursor_target_health_ui)
            .add_systems(
                Update,
                remove_cursor_target_health_ui.after(draw_cursor_target_health_ui),
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
        .insert(Pickable::default())
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
pub struct CardUiTargetMesh;

#[derive(EntityEvent)]
pub struct CardUiSpawned {
    pub entity: Entity,
    pub card_hand_index: u16,
}

#[derive(Component, Clone)]
pub struct CardDrawnInHandNotifier {
    pub card_entity: Entity,
    pub card_hand_index: u16,
}

fn spawn_card_drawn_notifer(e: On<CardDrawn>, mut cmd: Commands) {
    println!("spawning card notifier");
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
        });
        world.commands().entity(notifier_ent).despawn();
    }
}

fn spawn_card_diegetic_ui(
    e: On<CardUiSpawned>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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

    // This material has the texture that has been rendered.
    let material_handle = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle),
        perceptual_roughness: 1.0,
        cull_mode: None,
        alpha_mode: AlphaMode::Blend,
        // emissive_exposure_weight: 0.8,
        // #TODO: See how to make the card react to light
        // a bit while still being lit enough to be readable
        unlit: true,
        ..default()
    });

    let cam_forward = main_cam_tf.forward();
    let cam_left = main_cam_tf.left();
    let cam_right = main_cam_tf.right();
    let cam_down = main_cam_tf.down();
    let cam_up = main_cam_tf.up();

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
            CardUiTargetMesh,
            Mesh3d(mesh_handle.clone()),
            MeshMaterial3d(material_handle),
            Transform::from_translation(card_translation).looking_to(cam_forward, Vec3::Y),
            Pickable::default(),
            DiegeticUiTarget,
        ))
        .observe(drag_card_mesh);
    // .observe(over_moveup_card_mesh)
    // .observe(leave_moveback_card_mesh);

    // Main camera is spawned elsewhere
    //
    commands.spawn(CUBE_POINTER_ID);
}

#[derive(Component)]
pub struct TweenAnimator;

pub fn drag_card_mesh(
    e: On<Pointer<Drag>>,
    mut cmd: Commands,
    mut q: Query<(Entity, &GlobalTransform), (With<Mesh3d>, With<CardUiTargetMesh>)>,
    main_cam_q: Query<(&Camera, &GlobalTransform), (With<ActiveCamera>, With<Camera3d>)>,
) {
    let (main_cam, main_cam_tf) = main_cam_q.single().expect("found more than one cam3d");
    let Ok((mesh_ent, mesh_global_tf)) = q.get_mut(e.entity) else {
        return;
    };

    let move_amount = e.delta.x + e.delta.y;
    let pointer_pos = e.pointer_location.position;
    let pointer_world_pos = main_cam
        .viewport_to_world(main_cam_tf, pointer_pos)
        .unwrap()
        .plane_intersection_point(
            mesh_global_tf.translation(),
            InfinitePlane3d::new(mesh_global_tf.forward()),
        )
        .unwrap();
    println!("pointer world pos : {:?} : ", pointer_world_pos);

    let duration_ms = if move_amount < 50.0 { 30 } else { 15 };
    // TODO : Change to use a reusable TweenAnim and update that
    cmd.entity(mesh_ent).move_to(
        pointer_world_pos,
        Duration::from_millis(duration_ms),
        EaseFunction::CubicIn,
    );
}

pub fn over_moveup_card_mesh(
    mut e: On<Pointer<Over>>,
    mut cmd: Commands,
    mut q: Query<&mut Transform, (With<Mesh3d>, With<CardUiTargetMesh>)>,
) {
    let Ok(mut mesh_tf) = q.get_mut(e.entity) else {
        return;
    };

    let end = mesh_tf.translation + Vec3::new(0., 3., 0.);
    let target = e.entity.into_target();
    let mut transform_state = target.transform_state(*mesh_tf);
    let tween = transform_state.translation_to(end);
    cmd.entity(e.entity).animation().insert_tween_here(
        Duration::from_millis(500),
        EaseKind::QuadraticIn,
        tween,
    );
}

pub fn leave_moveback_card_mesh(
    mut e: On<Pointer<Out>>,
    mut cmd: Commands,
    mut q: Query<(&mut Transform, &mut TimeRunner), (With<Mesh3d>, With<CardUiTargetMesh>)>,
) {
    let Ok((mut mesh_tf, mut time_runner)) = q.get_mut(e.entity) else {
        return;
    };

    println!("Over leave");
    time_runner.set_direction(TimeDirection::Backward);
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

pub fn draw_cursor_target_health_ui(
    mut cmd: Commands,
    q: Query<
        (Entity, &Health, &MaxHealth),
        (
            Added<CursorTarget>,
            With<Transform>,
            Without<AnchoredUiNodes>,
        ),
    >,
) {
    for (ent, health, max_health) in &q {
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
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(10)),
                ..Default::default()
            },
            BorderColor::all(WHITE),
            Outline::default(),
            children![
                (
                    Pickable::IGNORE,
                    // text
                    Node {
                        width: px(health.0),
                        height: px(10),
                        border_radius: BorderRadius::all(px(10)),
                        position_type: PositionType::Absolute,
                        ..Default::default()
                    },
                    BackgroundColor(Color::Srgba(RED)),
                    ZIndex(1),
                ),
                (
                    Pickable::IGNORE,
                    // text
                    Node {
                        width: px(max_health.0),
                        height: px(10),
                        border_radius: BorderRadius::all(px(10)),
                        position_type: PositionType::Relative,
                        ..Default::default()
                    },
                    BackgroundColor(Color::Srgba(GREY)),
                )
            ],
        )));
    }
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
