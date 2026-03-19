use bevy::{
    app::{App, Plugin, Startup, Update},
    ecs::{
        entity::Entity,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Query, ResMut},
    },
    state::condition::in_state,
};

use crate::{
    actions::{
        Action, ActionPoints, Electric, Fire, Melee, MovementPoints, Physical, Piercing, Range,
        Ranged, Water,
    },
    startup_3d,
    states::CombatState,
};

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (set_selected_action__test).after(startup_3d))
            .add_systems(
                Update,
                print_on_determine_play_order.run_if(in_state(CombatState::DeterminePlayOrder)),
            )
            .insert_resource(SelectedAction(None));
    }
}

#[derive(Resource)]
pub struct SelectedAction(pub Option<Entity>);

pub fn set_selected_action__test(
    mut selected_action: ResMut<SelectedAction>,
    actions_q: Query<(
        Entity,
        &Action,
        &Range,
        &ActionPoints,
        Option<&MovementPoints>,
        Option<&Physical>,
        Option<&Ranged>,
        Option<&Melee>,
        Option<&Piercing>,
        Option<&Fire>,
        Option<&Electric>,
        Option<&Water>,
    )>,
) {
    // for (
    //     _,
    //     range,
    //     ap_cost,
    //     mp_cost,
    //     physical_dmg,
    //     ranged_dmg,
    //     melee_dmg,
    //     piercing_dmg,
    //     fire_dmg,
    //     electric_dmg,
    //     water_dmg,
    // ) in &attacks_q
    if let Some(action) = actions_q.iter().next() {
        println!("found registered attack : {:?}", action);
        selected_action.0 = Some(action.0);
    }
}

fn print_on_determine_play_order() {
    println!("it's player's turn");
}
