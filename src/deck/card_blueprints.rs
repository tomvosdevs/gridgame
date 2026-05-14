use bevy::app::{App, Plugin, Startup};
use bevy_diesel::spawn::TemplateRegistry;
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    schedule::IntoScheduleConfigs,
    system::{Commands, Res},
};
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    abilities::{
        abilities_templates::{AbilityHandler, AbilityHandlerBuilder, BaseAbility},
        definitions::register_abilities,
        effects::{AbilityEffectKind, AbilityEffects, DamageEffect, EffectTrigger},
    },
    deck::{
        card_builders::{CardPool, CardPoolStatus, RarityCond, RarityPicker},
        deck_and_cards::Card,
    },
};

pub struct CardBlueprintPlugin;

impl Plugin for CardBlueprintPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_blueprints.after(register_abilities));
    }
}

// pub enum PoolMatch

#[derive(Debug)]
pub enum NamePicker {
    Fixed(&'static str),
    OneOf(Vec<&'static str>),
}

impl NamePicker {
    pub fn pick(&mut self, rng: &mut WyRand) -> &'static str {
        match self {
            NamePicker::Fixed(val) => val,
            NamePicker::OneOf(items) => {
                let rand_idx = rng.random_range(0..items.len());
                items.get(rand_idx).unwrap()
            }
        }
    }
}

#[derive(Component, Debug)]
pub struct CardBlueprint {
    templates: Vec<&'static str>,
    base_entity: Option<Entity>,
    name_picker: NamePicker,
    matches_pools: Vec<(CardPool, CardPoolStatus)>,
    matches_rarity: Option<RarityCond>,
}

impl CardBlueprint {
    pub fn new(base_template: &'static str) -> Self {
        Self {
            templates: vec![base_template],
            base_entity: None,
            name_picker: NamePicker::Fixed("Missing name picker"),
            matches_pools: vec![],
            matches_rarity: None,
        }
    }

    fn get_base_entity_instance(&self, cmd: &mut Commands) -> Option<Entity> {
        let Some(base) = self.base_entity else {
            return None;
        };
        let instance = cmd.spawn_empty().id();
        cmd.entity(base).clone_with_opt_out(instance, |_| {});
        Some(instance)
    }

    pub fn set_name_picker(&mut self, picker: NamePicker) {
        self.name_picker = picker
    }

    pub fn create_base_entity(mut self, cmd: &mut Commands, effects: impl Bundle) -> Self {
        let entity = cmd.spawn(effects).id();
        self.base_entity = Some(entity);
        self
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

    pub fn generate(
        &self,
        cmd: &mut Commands,
        templates: &Res<TemplateRegistry>,
        rng: &mut WyRand,
        rarity: RarityPicker,
    ) -> impl Bundle {
        let mut ability_entity: Option<Entity> = None;
        for id in self.templates.iter() {
            let t_func = templates
                .get(id)
                .expect("should have found template for id");

            ability_entity = Some(t_func(cmd, ability_entity));
        }

        let handler = AbilityHandlerBuilder::from_ability_entity(ability_entity.unwrap())
            .add_modifiers(vec![])
            .pass_base_entity(self.get_base_entity_instance(cmd))
            .build(cmd);

        (Card::new(handler), rarity.pick(rng))
    }
}

type AE<T> = AbilityEffects<T>;
type E = AbilityEffectKind;

pub fn register_blueprints(mut cmd: Commands) {
    let projectile_tid = BaseAbility::Projectile.as_str();
    let melee_tid = BaseAbility::Melee.as_str();

    let basic_projectile_blueprint = CardBlueprint::new(projectile_tid)
        .create_base_entity(&mut cmd, AE::hit(E::flat_damage(2.0)))
        .add_required_pool(CardPool::Ranged);
    cmd.spawn(basic_projectile_blueprint);

    let bomb_blueprint = CardBlueprint::new(projectile_tid)
        .create_base_entity(&mut cmd, AE::hit(E::flat_damage(4.0)))
        .add_required_pool(CardPool::Ranged);
    cmd.spawn(bomb_blueprint);

    // let _basic_melee_blueprint = cmd
    //     .spawn(
    //         CardBlueprint::new(melee_tid)
    //             .add_required_pool(CardPool::Melee)
    //             .add_required_pool(CardPool::Clawed),
    //     )
    //     .id();
}
