use std::f32::consts::PI;

use bevy::{
    app::{First, Plugin, Startup, Update},
    asset::{Assets, RenderAssetUsages, uuid::Uuid},
    camera::{Camera, Camera2d, Camera3d, RenderTarget},
    color::{
        Color,
        palettes::css::{BLUE, GRAY, GREY, RED, WHITE},
    },
    ecs::{
        children,
        component::Component,
        entity::Entity,
        error::Result,
        hierarchy::Children,
        lifecycle::RemovedComponents,
        message::{MessageReader, MessageWriter},
        observer::On,
        query::{Added, With, Without},
        schedule::IntoScheduleConfigs,
        spawn::SpawnRelated,
        system::{Commands, Local, Query, Res, ResMut},
    },
    image::Image,
    input::{ButtonState, mouse::MouseButton},
    math::{
        Dir3, Quat, Vec2, Vec3,
        primitives::{Cuboid, Plane3d, Rectangle},
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
    render::{
        camera::NormalizedRenderTargetExt,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        texture::ManualTextureViews,
    },
    text::{TextColor, TextFont},
    transform::components::{GlobalTransform, Transform},
    ui::{
        AlignItems, BackgroundColor, BorderColor, BorderRadius, ComputedNode, FlexDirection,
        IsDefaultUiCamera, JustifyContent, Node, Outline, PositionType, UiRect, UiTargetCamera,
        Val, ZIndex, percent, px, widget::Text,
    },
    utils::default,
    window::{PrimaryWindow, Window, WindowEvent},
};
use bevy_ui_anchor::{AnchorPoint, AnchorUiConfig, AnchorUiNode, AnchorUiPlugin, AnchoredUiNodes};

use crate::{ActiveCamera, CursorTarget, Health, MaxHealth, startup_3d};

const CUBE_POINTER_ID: PointerId = PointerId::Custom(Uuid::from_u128(90870987));

pub struct GameUiPlugin;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(AnchorUiPlugin::<UiCameraMarker>::new())
            .add_systems(Startup, setup_diegetic_ui.after(startup_3d))
            .add_systems(First, drive_diegetic_pointer.in_set(PickingSystems::Input))
            .add_systems(Update, tag_active_camera)
            .add_systems(Update, draw_cursor_target_health_ui)
            .add_systems(
                Update,
                remove_cursor_target_health_ui.after(draw_cursor_target_health_ui),
            );
    }
}

#[derive(Component)]
/// We need a marker for the camera, so the plugin knows which camera to perform position
/// calculations towards
pub struct UiCameraMarker;

#[derive(Component)]
struct DiegeticUiTarget;

fn setup_diegetic_ui(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    q: Query<&Transform, (With<Camera3d>, With<ActiveCamera>)>,
) {
    let main_cam_tf = q
        .single()
        .expect("found more than one Cam3d with 'ActiveCamera'");

    let size = Extent3d {
        width: 400,
        height: 1000,
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
            Camera {
                // render before the "main pass" camera
                order: -1,
                ..default()
            },
            RenderTarget::Image(image_handle.clone().into()),
        ))
        .id();

    commands
        .spawn((
            Node {
                // Cover the whole image
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(GRAY.into()),
            UiTargetCamera(texture_camera),
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Auto,
                        height: Val::Auto,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(20.)),
                        border_radius: BorderRadius::all(Val::Px(10.)),
                        ..default()
                    },
                    BackgroundColor(BLUE.into()),
                ))
                .observe(
                    |drag: On<Pointer<Drag>>, mut nodes: Query<(&mut Node, &ComputedNode)>| {
                        let (mut node, computed) = nodes.get_mut(drag.entity).unwrap();
                        node.left =
                            Val::Px(drag.pointer_location.position.x - computed.size.x / 2.0);
                        node.top = Val::Px(drag.pointer_location.position.y - 50.0);
                    },
                )
                .observe(
                    |over: On<Pointer<Over>>, mut colors: Query<&mut BackgroundColor>| {
                        colors.get_mut(over.entity).unwrap().0 = RED.into();
                    },
                )
                .observe(
                    |out: On<Pointer<Out>>, mut colors: Query<&mut BackgroundColor>| {
                        colors.get_mut(out.entity).unwrap().0 = BLUE.into();
                    },
                )
                .with_children(|parent| {
                    parent.spawn((
                        Text::new("Drag Me!"),
                        TextFont {
                            font_size: 40.0,
                            ..default()
                        },
                        TextColor::WHITE,
                    ));
                });
        });

    let mesh_handle = meshes.add(Rectangle::new(20.0, 50.0));

    // This material has the texture that has been rendered.
    let material_handle = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle),
        reflectance: 0.02,
        unlit: false,
        ..default()
    });

    let cam_forward = main_cam_tf.forward();

    // Cube with material containing the rendered UI texture.
    commands.spawn((
        Mesh3d(mesh_handle.clone()),
        MeshMaterial3d(material_handle),
        Transform::from_xyz(5.0, 10.0, 0.0).looking_to(cam_forward, Vec3::Y),
        DiegeticUiTarget,
    ));

    // Main camera is spawned elsewhere
    //
    commands.spawn(CUBE_POINTER_ID);
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
    ui_camera: Query<&RenderTarget, With<Camera2d>>,
    primary_window: Query<Entity, With<PrimaryWindow>>,
    windows: Query<(Entity, &Window)>,
    images: Res<Assets<Image>>,
    manual_texture_views: Res<ManualTextureViews>,
    mut window_events: MessageReader<WindowEvent>,
    mut pointer_inputs: MessageWriter<PointerInput>,
) -> Result {
    // Get the size of the texture, so we can convert from dimensionless UV coordinates that span
    // from 0 to 1, to pixel coordinates.
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
