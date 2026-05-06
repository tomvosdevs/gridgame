use bevy::app::{App, Plugin, Startup};
use bevy_diesel::spawn::TemplateRegistry;
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    schedule::IntoScheduleConfigs,
    system::{Commands, Res, ResMut},
};

use crate::{
    abilities::definitions::register_abilities,
    deck::card_builders::{CardPool, CardPoolStatus, RarityCond},
};

pub struct CardBlueprintPlugin;

impl Plugin for CardBlueprintPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_blueprints.after(register_abilities));
    }
}

// pub enum PoolMatch

#[derive(Component, Debug)]
pub struct CardBlueprint {
    templates: Vec<&'static str>,
    matches_pools: Vec<(CardPool, CardPoolStatus)>,
    matches_rarity: Option<RarityCond>,
}

impl CardBlueprint {
    pub fn new(base_template: &'static str) -> Self {
        Self {
            templates: vec![base_template],
            matches_pools: vec![],
            matches_rarity: None,
        }
    }

    pub fn chain_template(mut self, template: &'static str) -> Self {
        self.templates.push(template);
        self
    }

    pub fn add_required_pool(mut self, pool: CardPool) -> Self {
        self.matches_pools.push((pool, CardPoolStatus::Required));
        self
    }

    pub fn add_accepted_pool(mut self, pool: CardPool) -> Self {
        self.matches_pools.push((pool, CardPoolStatus::Accepted));
        self
    }

    pub fn add_forbidden_pool(mut self, pool: CardPool) -> Self {
        self.matches_pools.push((pool, CardPoolStatus::Forbidden));
        self
    }

    pub fn add_rarity_condition(mut self, cond: RarityCond) -> Self {
        self.matches_rarity = Some(cond);
        self
    }

    pub fn does_match(&self, pools: &Vec<(CardPool, CardPoolStatus)>) -> bool {
        pools.iter().all(|(exp_pool, exp_status)| match exp_status {
            CardPoolStatus::Required => self
                .matches_pools
                .iter()
                .find(|(p, s)| p == exp_pool && s == exp_status)
                .is_some(),
            CardPoolStatus::Accepted => true,
            CardPoolStatus::Forbidden => self
                .matches_pools
                .iter()
                .filter(|(p, _)| p == exp_pool)
                .all(|(_, s)| *s != CardPoolStatus::Forbidden),
        })
    }

    pub fn build_ability_entity(
        &self,
        cmd: &mut Commands,
        templates: &Res<TemplateRegistry>,
    ) -> Entity {
        let mut entity: Option<Entity> = None;
        for id in self.templates.iter() {
            let t_func = templates
                .get(id)
                .expect("should have found template for id");

            entity = Some(t_func(cmd, entity));
        }
        entity.expect("Entity should have been spawned by now")
    }
}

pub fn register_blueprints(mut cmd: Commands) {
    let projectile_tid = "base_projectile";
    let melee_tid = "base_melee";

    let basic_projectile_blueprint = cmd
        .spawn(CardBlueprint::new(projectile_tid).add_required_pool(CardPool::Ranged))
        .id();

    let basic_melee_blueprint = cmd
        .spawn(CardBlueprint::new(melee_tid).add_required_pool(CardPool::Melee))
        .id();
}
