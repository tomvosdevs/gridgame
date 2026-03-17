use bevy::{
    app::{Plugin, Update},
    camera::Camera,
    color::{
        Color,
        palettes::css::{GREY, RED, WHITE},
    },
    ecs::{
        children,
        component::Component,
        entity::Entity,
        hierarchy::Children,
        lifecycle::RemovedComponents,
        query::{Added, With, Without},
        schedule::IntoScheduleConfigs,
        spawn::SpawnRelated,
        system::{Commands, Query},
    },
    math::Vec3,
    picking::Pickable,
    transform::components::Transform,
    ui::{
        BackgroundColor, BorderColor, BorderRadius, IsDefaultUiCamera, Node, Outline, PositionType,
        UiRect, Val, ZIndex, px, widget::Text,
    },
};
use bevy_ui_anchor::{AnchorPoint, AnchorUiConfig, AnchorUiNode, AnchorUiPlugin, AnchoredUiNodes};

use crate::{ActiveCamera, CursorTarget, Health, MaxHealth};

pub struct GameUiPlugin;

#[derive(Component)]
/// We need a marker for the camera, so the plugin knows which camera to perform position
/// calculations towards
pub struct UiCameraMarker;

impl Plugin for GameUiPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(AnchorUiPlugin::<UiCameraMarker>::new())
            .add_systems(Update, tag_active_camera)
            .add_systems(Update, draw_cursor_target_health_ui)
            .add_systems(
                Update,
                remove_cursor_target_health_ui.after(draw_cursor_target_health_ui),
            );
    }
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
