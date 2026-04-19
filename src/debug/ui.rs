use bevy::{
    app::{Plugin, Startup},
    ecs::{
        entity::Entity,
        observer::On,
        resource::Resource,
        system::{Commands, Query, Res, ResMut},
    },
    text::TextFont,
    ui::{Node, Val, widget::Text},
    utils::default,
};

use crate::game_flow::turns::{CombatStart, EntityTurnStart, GlobalTurnStart};

pub struct DebugUiPlugin;

impl Plugin for DebugUiPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Startup, setup_debug_ui)
            .add_observer(
                |_: On<CombatStart>, d: Res<DebugUiData>, mut q: Query<&mut Text>| {
                    if let Ok(mut t) = q.get_mut(d.0) {
                        let new_line = match t.0.is_empty() {
                            true => "Combat started",
                            false => "\nCombat started",
                        };
                        t.0 += new_line;
                    }
                },
            )
            .add_observer(
                |_: On<GlobalTurnStart>, d: Res<DebugUiData>, mut q: Query<&mut Text>| {
                    if let Ok(mut t) = q.get_mut(d.0) {
                        let new_line = "\nNew [Global] turn started";
                        t.0 += new_line;
                    }
                },
            )
            .add_observer(
                |_: On<EntityTurnStart>, d: Res<DebugUiData>, mut q: Query<&mut Text>| {
                    if let Ok(mut t) = q.get_mut(d.0) {
                        let new_line = "\nNew [Entity] turn started";
                        t.0 += new_line;
                    }
                },
            );
    }
}

#[derive(Resource)]
struct DebugUiData(Entity);

fn setup_debug_ui(mut cmd: Commands) {
    let debug_entity = cmd
        .spawn((
            Node {
                top: Val::Percent(10.),
                left: Val::Percent(10.),
                ..default()
            },
            Text::new(""),
            TextFont::from_font_size(30.),
        ))
        .id();

    cmd.insert_resource(DebugUiData(debug_entity));
}
