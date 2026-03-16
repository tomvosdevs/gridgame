use bevy::{
    app::{Plugin, Update},
    camera::Camera,
    color::palettes::css::WHITE,
    ecs::{
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
    transform::components::Transform,
    ui::{
        BorderColor, BorderRadius, IsDefaultUiCamera, Node, Outline, UiRect, Val, px, widget::Text,
    },
};
use bevy_ui_anchor::{AnchorPoint, AnchorUiConfig, AnchorUiNode, AnchorUiPlugin, AnchoredUiNodes};

use crate::{ActiveCamera, CursorTarget};

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
        Entity,
        (
            With<CursorTarget>,
            With<Transform>,
            Without<AnchoredUiNodes>,
        ),
    >,
) {
    for ent in &q {
        cmd.entity(ent).insert(AnchoredUiNodes::spawn_one((
            RemoveOnCursorTargetChange(ent),
            AnchorUiConfig {
                anchorpoint: AnchorPoint::middle(),
                offset: Some(Vec3::new(0.0, 0.5, 0.0)),
                ..Default::default()
            },
            Node {
                border: UiRect::all(Val::Px(2.)),
                border_radius: BorderRadius::all(px(3)),
                ..Default::default()
            },
            BorderColor::all(WHITE),
            Outline::default(),
            Children::spawn_one(
                // text
                Text("HP goes here".into()),
            ),
        )));
    }
}

pub fn remove_cursor_target_health_ui(
    mut cmd: Commands,
    q: Query<(Entity, &RemoveOnCursorTargetChange)>,
    cursor_target_q: Query<Entity, With<CursorTarget>>,
) {
    let cursor_targets: Vec<Entity> = cursor_target_q.iter().collect();
    for (ent, remove_on) in &q {
        if cursor_targets.contains(&remove_on.0) {
            continue;
        }
        cmd.entity(ent).despawn();
    }
}
