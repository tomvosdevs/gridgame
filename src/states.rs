use bevy::state::state::States;

#[derive(States, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CombatState {
    DeterminePlayOrder,
    PlayerTurn(i32),
    EnemyTurn(i32),
    EnvironmentTurn(i32),
}
