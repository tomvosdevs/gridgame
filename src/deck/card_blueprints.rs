use std::vec::IntoIter;

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
    message::MessageWriter,
    observer::{self, Observer, On},
    query::With,
    related,
    relationship::RelationshipTarget,
    schedule::IntoScheduleConfigs,
    system::{Commands, EntityCommands, IntoSystem, Query, Res},
    world::{EntityWorldMut, World},
};
use bevy_gauge::{
    instant,
    prelude::{AttributesMut, InstantExt, InstantModifierSet, ModifierSet},
};
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    abilities::{
        abilities_templates::{
            AbilityHandler, AbilityHandlerBuilder, ActionCastData, BaseAbility,
            basic_projectile_ability, melee_template, projectile_template, ripple_invoking,
        },
        definitions::register_abilities,
        effects::{
            EffectMod, EvReactorOf, EvReactors, OneShotEffect, SpawnFn, StatusEffectApplier,
            StatusEffectOf, StatusEffects, StatusKind, TriggerEffect, TriggerOn, status_effect,
        },
    },
    deck::{
        card_builders::{CardPool, CardPoolStatus, RarityCond, RarityPicker},
        deck_and_cards::Card,
    },
    game_flow::turns::CurrentDeckReference,
    grid_abilities_backend::{
        AbilityHitEntity, GridGoOffConfig, GridStartInvoke, GridTarget, GridTargetGenerator,
        GridTargetMutator, HitReceived, HitTargetKind,
    },
};

pub struct CardBlueprintPlugin;

impl Plugin for CardBlueprintPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, register_blueprints.after(register_abilities))
            .add_observer(handle_matching_reactors::<NotifyActionHit>)
            .add_observer(create_hit_observer)
            .add_observer(log_notified);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbilityConstructor {
    Projectile,
    Melee,
    BundleAsEntity(Entity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbilityMod {
    Ripple(u32),
}

impl AbilityMod {
    pub fn build(&self, cmd: &mut Commands, entity: Entity, sub_template: Entity) -> Entity {
        match self {
            AbilityMod::Ripple(count) => ripple_invoking(cmd, Some(entity), *count, sub_template),
        }
    }
}

impl AbilityConstructor {
    pub fn build(&self, entity: Entity, created_entities: &mut Vec<Entity>, cmd: &mut Commands) {
        let ability_container = cmd.spawn(InvokedBy(entity)).id();
        created_entities.push(ability_container);
        match self {
            AbilityConstructor::Projectile => {
                projectile_template(cmd, Some(ability_container));
            }
            AbilityConstructor::Melee => {
                melee_template(cmd, Some(ability_container));
            }
            AbilityConstructor::BundleAsEntity(bundle_entity) => {
                println!("cloning bundle as entity, bundle has : ");
                cmd.entity(*bundle_entity).log_components();
                cmd.entity(*bundle_entity)
                    .clone_with_opt_out(entity, |builder| {
                        builder.linked_cloning(true);
                    });
            }
        }
    }
}

impl Into<AbilityNode> for AbilityConstructor {
    fn into(self) -> AbilityNode {
        AbilityNode::End(self)
    }
}

#[derive(Debug, Clone)]
pub enum AbilityNode {
    End(AbilityConstructor),
    Nested(Vec<AbilityNode>),
}

impl IntoIterator for AbilityNode {
    type Item = AbilityNode;

    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            AbilityNode::End(ability_constructor) => {
                vec![AbilityNode::End(ability_constructor)].into_iter()
            }
            AbilityNode::Nested(ability_nodes) => ability_nodes.into_iter(),
        }
    }
}

impl AbilityNode {
    pub fn build_and_spawn(
        &self,
        entity: Entity,
        mut created_entities: &mut Vec<Entity>,
        cmd: &mut Commands,
    ) {
        match self {
            AbilityNode::End(ability_constructor) => {
                ability_constructor.build(entity, &mut created_entities, cmd)
            }
            AbilityNode::Nested(ability_nodes) => {
                for node in ability_nodes.iter() {
                    node.build_and_spawn(entity, &mut created_entities, cmd);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum InvokingHandlerKind {
    Single,
    Ripple(u32),
}

impl InvokingHandlerKind {
    pub fn start_invoke_on(
        &self,
        cmd: &mut Commands,
        template_entity: Entity,
        mut writer: MessageWriter<GridStartInvoke>,
        target: GridTarget,
        invoker_bundle: impl Bundle,
    ) {
        let invoked = match self {
            InvokingHandlerKind::Single => template_entity,
            InvokingHandlerKind::Ripple(ripple_count) => {
                let invoked = cmd.spawn(invoker_bundle).id();
                let invoker = ripple_invoking(cmd, Some(invoked), *ripple_count, template_entity);
                cmd.entity(invoker).insert(InvokedBy(invoked));
                invoked
            }
        };
        writer.write(GridStartInvoke::new(invoked, target));
    }
}

#[derive(Component, Debug)]
pub struct CardBlueprint {
    invoking_handler_kind: InvokingHandlerKind,
    nodes: AbilityNode,
    base_entity: Option<Entity>,
    name_picker: NamePicker,
    matches_pools: Vec<(CardPool, CardPoolStatus)>,
    matches_rarity: Option<RarityCond>,
}

impl CardBlueprint {
    pub fn new(nodes: AbilityNode, invoking_handler_kind: InvokingHandlerKind) -> Self {
        Self {
            invoking_handler_kind,
            nodes,
            base_entity: None,
            name_picker: NamePicker::Fixed("Missing name picker"),
            matches_pools: vec![],
            matches_rarity: None,
        }
    }

    pub fn set_name_picker(&mut self, picker: NamePicker) {
        self.name_picker = picker
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

    pub fn generate(&self, rng: &mut WyRand, rarity: RarityPicker) -> impl Bundle {
        (
            Card::new(self.nodes.clone(), self.invoking_handler_kind.clone()),
            rarity.pick(rng),
        )
    }
}

type E = EffectMod;
type OSE = OneShotEffect;

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
    let Some(effect) = q.get(e.entity).ok().map(|v| v.effect.clone()) else {
        return;
    };

    println!("creating obs");
    cmd.entity(e.entity).observe(
        move |e: On<TriggerEffect<NotifyActionHit>>,
              q: Query<&CurrentDeckReference>,
              mut attributes: AttributesMut,
              mut obs_cmd: Commands| {
            println!("CCCC = 1 inside on hit eggect");
            let target = e.cause.target.entity.unwrap();
            let attacker = e.cause.cast_data.source_playing_entity;

            if !e.cause.target_kind.is_player() {
                return;
            }

            let roles = [("Attacker", attacker)];
            let target_deck = q.get(target).expect("Target player should have deck").0;
            println!("applying damage");

            match &effect {
                EffectMod::OneShot(one_shot) => match one_shot {
                    OneShotEffect::Mod(modifier_set) => {
                        modifier_set
                            .try_apply(target_deck, &mut attributes)
                            .expect("Failed to apply modifier set");
                    }
                    OneShotEffect::Instant(instant_modifier_set) => {
                        let evaluated_instant =
                            attributes.evaluate_instant(&instant_modifier_set, &roles, target_deck);
                        attributes.apply_evaluated_instant(&evaluated_instant, target_deck);
                    }
                },
                EffectMod::SpawnTickable(effect_applier) => {
                    println!("spawning status effect");
                    obs_cmd.entity(target).log_components();
                    obs_cmd.entity(target).insert(status_effect(
                        effect_applier.amount,
                        attacker,
                        effect_applier.effect_kind.clone(),
                    ));
                }
            };
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
#[entity_event(propagate = &'static InvokedBy, auto_propagate)]
pub struct NotifyActionHit {
    #[event_target]
    pub sub_ability_entity: Entity,
    pub cast_data: ActionCastData,
    pub target: GridTarget,
    pub target_kind: HitTargetKind,
}

fn log_notified(e: On<NotifyActionHit>, mut cmd: Commands) {
    println!("Hit notified on : {:?}", e.event_target());
    cmd.entity(e.event_target()).log_components();
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

pub fn one_shot_instant(effect: InstantModifierSet) -> EffectMod {
    EffectMod::OneShot(OSE::Instant(effect))
}

pub fn one_shot_mod(effect: ModifierSet) -> EffectMod {
    EffectMod::OneShot(OSE::Mod(effect))
}

pub fn status_effect_applier(effect_applier: StatusEffectApplier) -> EffectMod {
    EffectMod::SpawnTickable(effect_applier)
}

type EA = StatusEffectApplier;
type EK = StatusKind;

pub fn register_blueprints(mut cmd: Commands) {
    let projectile_tid = BaseAbility::Projectile.as_str();
    let melee_tid = BaseAbility::Melee.as_str();

    let other_projectile_blueprint = CardBlueprint::new(
        AbilityNode::Nested(vec![
            AbilityConstructor::Projectile.into(),
            AbilityConstructor::BundleAsEntity(
                cmd.spawn(related!(
                    EvReactors[
                        on_hit_effect(one_shot_instant(
                            instant! {"SoulLife.current" -= "Strength@Attacker"},
                        )),
                        on_hit_effect(status_effect_applier(EA::new(EK::Poison, 3))), // More here
                    ]
                ))
                .id(),
            )
            .into(),
        ]),
        InvokingHandlerKind::Ripple(3),
    )
    .add_required_pool(CardPool::Ranged);
    cmd.spawn(other_projectile_blueprint);

    // let shield_blueprint = CardBlueprint::new(AbilityNode::Nested(vec![
    //     AbilityConstructor::Projectile.into(),
    //     AbilityConstructor::Projectile.into(),
    //     AbilityConstructor::BundleAsEntity(
    //         cmd.spawn(related!(
    //             EvReactors[
    //                 on_hit_effect(one_shot_instant(
    //                     instant! {"SoulLife.current" -= "Strength@Attacker"},
    //                 )),
    //                 on_hit_effect(status_effect_applier(EA::new(EK::Poison, 3))), // More here
    //             ]
    //         ))
    //         .id(),
    //     )
    //     .into(),
    // ]))
    // .add_required_pool(CardPool::Ranged);
    // cmd.spawn(other_projectile_blueprint);

    // let basic_projectile_blueprint = CardBlueprint::new(projectile_tid)
    //     .create_base_entity(
    //         &mut cmd,
    //         (effects(
    //             on_hit_effect(E::flat_damage(1.0)),
    //             // More here
    //         )),
    //     )
    //     .add_required_pool(CardPool::Ranged);
    // cmd.spawn(basic_projectile_blueprint);

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
