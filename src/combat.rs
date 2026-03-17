use bevy::{
    app::{App, Plugin, Update},
    ecs::schedule::IntoScheduleConfigs,
    state::condition::in_state,
};

use crate::states::CombatState;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            print_on_determine_play_order.run_if(in_state(CombatState::DeterminePlayOrder)),
        );
    }
}

fn print_on_determine_play_order() {
    println!("it's player's turn");
}
