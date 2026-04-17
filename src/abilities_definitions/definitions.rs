use std::collections::HashMap;

use bevy::{
    app::{Plugin, Update},
    ecs::{bundle::Bundle, component::Component, name::Name, resource::Resource, system::Commands},
};

pub struct AbilitiesDefinitionsPlugin;

impl Plugin for AbilitiesDefinitionsPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Update, register_abilities);
    }
}

#[derive(Component, Debug)]
pub struct AbilityEntry {
    pub register_name: String,
}

#[derive(Component)]
pub struct TemplateAbility;

impl AbilityEntry {
    pub fn new(name: &'static str) -> Self {
        Self {
            register_name: name.to_string(),
        }
    }

    pub fn new_template(ability_name: &'static str) -> impl Bundle {
        (
            AbilityEntry::new(ability_name),
            TemplateAbility,
            Name::new(format!("template for : {:?}", ability_name)),
        )
    }
}

pub fn register_abilities(mut cmd: Commands) {
    cmd.spawn(AbilityEntry::new_template("Projectile"));
}
