use bevy::{
    app::{App, Plugin, Startup},
    ui_widgets::observe,
};
use bevy_diesel::{prelude::InvokedBy, spawn::TemplateRegistry};
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    event::EntityEvent,
    lifecycle::Add,
    observer::{self, Observer, On},
    query::With,
    related,
    relationship::RelationshipTarget,
    schedule::IntoScheduleConfigs,
    system::{Commands, Query, Res},
};
use bevy_gauge::{
    instant,
    prelude::{AttributesMut, InstantExt},
};
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    abilities::{
        abilities_templates::{AbilityHandler, AbilityHandlerBuilder, ActionCastData, BaseAbility},
        definitions::register_abilities,
        effects::{
            AbilityEffectKind, EvReactorOf, EvReactors, StatusEffectOf, StatusEffects,
            TriggerEffect, TriggerOn,
        },
    },
    deck::{
        card_builders::{CardPool, CardPoolStatus, RarityCond, RarityPicker},
        deck_and_cards::Card,
    },
    game_flow::turns::CurrentDeckReference,
    grid_abilities_backend::{AbilityHitEntity, GridTarget, HitReceived, HitTargetKind},
};

pub struct CardBlueprintPlugin;

impl Plugin for CardBlueprintPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_blueprints.after(register_abilities))
            .add_observer(handle_matching_reactors::<NotifyActionHit>)
            .add_observer(create_hit_observer);
    }
}

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = SubAbilityOf)]
pub struct SubAbilities(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = SubAbilities)]
pub struct SubAbilityOf(pub Entity);

pub fn read_notified_hit(e: On<NotifyActionHit>) {}

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
        cmd.entity(base).clone_with_opt_out(instance, |builder| {
            builder.linked_cloning(true);
        });
        Some(instance)
    }

    pub fn set_name_picker(&mut self, picker: NamePicker) {
        self.name_picker = picker
    }

    pub fn create_base_entity(mut self, cmd: &mut Commands, bundle: impl Bundle) -> Self {
        let entity = cmd.spawn(bundle).id();
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

type E = AbilityEffectKind;

#[derive(EntityEvent)]
pub struct GotHit {
    pub entity: Entity,
    pub attacking_player: Entity,
    pub effect: E,
}

impl GotHit {
    pub fn new(entity: Entity, attacking_player: Entity, effect: E) -> Self {
        Self {
            entity,
            attacking_player,
            effect,
        }
    }
}

#[derive(Component, Clone)]
pub struct ObservesReaction {
    pub effect: E,
}

fn create_hit_observer(
    e: On<Add, ObservesReaction>,
    q: Query<&ObservesReaction, With<TriggerOn<NotifyActionHit>>>,
    mut cmd: Commands,
) {
    let effect = q.get(e.entity).expect("It should be here").effect.clone();
    println!("creating obs");
    cmd.entity(e.entity).observe(
        move |e: On<TriggerEffect<NotifyActionHit>>,
              q: Query<&CurrentDeckReference>,
              mut attributes: AttributesMut| {
            println!("CCCC = 1 inside on hit eggect");
            let target = e.cause.target.entity.unwrap();
            let attacker = e.cause.cast_data.source_playing_entity;

            if !e.cause.target_kind.is_player() {
                return;
            }

            let roles = [("Attacker", attacker)];
            let target_deck = q.get(target).expect("Target player should have deck").0;
            println!("applying damage");

            match effect.clone() {
                AbilityEffectKind::Mod(modifier_set) => {
                    modifier_set
                        .try_apply(target_deck, &mut attributes)
                        .expect("Failed to apply modifier set");
                }
                AbilityEffectKind::Instant(instant_modifier_set) => {
                    let evaluated_instant =
                        attributes.evaluate_instant(&instant_modifier_set, &roles, target_deck);
                    attributes.apply_evaluated_instant(&evaluated_instant, target_deck);
                }
            }
        },
    );
}

fn on_hit_effect(effect: E) -> impl Bundle {
    (
        ObservesReaction { effect },
        TriggerOn::<NotifyActionHit>::new(),
    )
}

#[derive(EntityEvent, Clone)]
#[entity_event(propagate = &'static SubAbilityOf, auto_propagate)]
pub struct NotifyActionHit {
    #[event_target]
    pub sub_ability_entity: Entity,
    pub cast_data: ActionCastData,
    pub target: GridTarget,
    pub target_kind: HitTargetKind,
}

fn handle_matching_reactors<T: EntityEvent + Clone>(
    e: On<T>,
    reactors_q: Query<&EvReactors>,
    effects_q: Query<Entity, With<TriggerOn<T>>>,
    mut cmd: Commands,
) {
    // Instead react to an event like HitReceived with all the data and entities and collect the reactors?
    let evt_receiver = e.event_target();
    let Ok(reactors) = reactors_q.get(evt_receiver) else {
        return;
    };
    println!("inside handle m react : {:?}", reactors.iter().len());

    for effect in effects_q.iter_many(reactors.iter()) {
        println!("heres ONE");
        cmd.entity(effect).log_components();
        cmd.trigger(TriggerEffect {
            entity: effect,
            cause: e.event().clone(),
        });
    }
}

pub fn effects(inner: impl Bundle) -> impl Bundle {
    related!(EvReactors[inner])
}

pub fn register_blueprints(mut cmd: Commands) {
    let projectile_tid = BaseAbility::Projectile.as_str();
    let melee_tid = BaseAbility::Melee.as_str();

    let other_projectile_blueprint = CardBlueprint::new(projectile_tid)
        .create_base_entity(
            &mut cmd,
            (effects(
                on_hit_effect(E::Instant(
                    instant! {"SoulLife.current" -= "Strength@Attacker"},
                )),
                // More here
            )),
        )
        .add_required_pool(CardPool::Ranged);
    cmd.spawn(other_projectile_blueprint);

    let basic_projectile_blueprint = CardBlueprint::new(projectile_tid)
        .create_base_entity(
            &mut cmd,
            (effects(
                on_hit_effect(E::flat_damage(1.0)),
                // More here
            )),
        )
        .add_required_pool(CardPool::Ranged);
    cmd.spawn(basic_projectile_blueprint);

    // let bomb_blueprint = CardBlueprint::new(projectile_tid)
    //     .create_base_entity(&mut cmd, ())
    //     .add_required_pool(CardPool::Ranged);
    // cmd.spawn(bomb_blueprint);

    // let _basic_melee_blueprint = cmd
    //     .spawn(
    //         CardBlueprint::new(melee_tid)
    //             .add_required_pool(CardPool::Melee)
    //             .add_required_pool(CardPool::Clawed),
    //     )
    //     .id();
}
