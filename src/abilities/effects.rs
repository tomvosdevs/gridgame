use std::{any::TypeId, collections::HashMap, marker::PhantomData};

use bevy::{app::Plugin, ui_widgets::observe};
use bevy_diesel::prelude::InvokedBy;
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    event::{EntityEvent, Event},
    hierarchy::ChildOf,
    message::{MessageReader, MessageWriter},
    observer::{Observer, On},
    query::With,
    related,
    relationship::RelationshipTarget,
    system::{Commands, Query, SystemId},
    world::EntityWorldMut,
};
use bevy_gauge::{
    attributes,
    expr::Expr,
    instant,
    prelude::{
        AttributeInitializer, AttributeQueries, Attributes, AttributesMut, InstantExt,
        InstantModifierSet, Modifier, ModifierSet,
    },
};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::CartesianPosition;

use crate::{
    abilities::abilities_templates::InvokingTriggerEffect,
    deck::{
        card_builders::{CardPool, CardPoolStatus, PoolSupplier},
        deck_and_cards::SoulLife,
    },
    game_flow::turns::{CurrentDeckReference, EntityTurnEnd, PlayingEntity},
    grid_abilities_backend::{AbilityHitEntity, DeckGoOff, DeckInvokerTarget, DeckStartInvoke},
    utils::IntoVec,
};

pub struct StatusEffectsPlugin;

impl Plugin for StatusEffectsPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_observer(tick_on::<EntityTurnEnd>);
    }
}

#[derive(Component)]
pub struct CasterHitEffect {
    caster: Entity,
}

impl CasterHitEffect {
    pub fn new(caster: Entity) -> Self {
        Self { caster }
    }
}

pub fn handle_invoke_subability_effect(
    mut reader: MessageReader<DeckGoOff>,
    q_effect: Query<&InvokingTriggerEffect>,
    mut writer: MessageWriter<DeckStartInvoke>,
) {
    for go_off in reader.read() {
        let Ok((template_entity, source_entity)) = q_effect
            .get(go_off.entity)
            .map(|v| (v.template_entity, v.source))
        else {
            continue;
        };

        println!("One check down");
        let target = go_off.target;
        writer.write(DeckStartInvoke::new(template_entity, target));
    }
}

#[derive(Component)]
pub struct SpawnEffect {
    pub action_root: Entity,
    pub casted: Entity,
}

impl SpawnEffect {
    pub fn new(action_root: Entity, casted: Entity) -> Self {
        Self {
            action_root,
            casted,
        }
    }
}

pub fn handle_spawn_effect(
    mut reader: MessageReader<DeckGoOff>,
    q_effect: Query<&SpawnEffect>,
    mut writer: MessageWriter<DeckStartInvoke>,
    mut cmd: Commands,
) {
    for go_off in reader.read() {
        let effect_entity = go_off.entity;
        println!("A GoOff was received for :");
        cmd.entity(effect_entity).log_components();
        let Ok(cast) = q_effect.get(effect_entity) else {
            continue;
        };
        let target = go_off.target;
        println!("target for invoke : {:?}", target);
        cmd.entity(cast.casted).insert((
            InvokedBy(cast.action_root),
            DeckInvokerTarget::entity(target.entity.unwrap(), target.position),
        ));
        writer.write(DeckStartInvoke::new(cast.casted, target));
    }
}

// pub trait EffectTrigger {}

// #[derive(Debug, Clone)]
// pub struct InvokeTrigger {}
// impl EffectTrigger for InvokeTrigger {}

// #[derive(Debug, Clone)]
// pub struct HitTrigger {}
// impl EffectTrigger for HitTrigger {}

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = StatusEffectOf, linked_spawn)]
pub struct StatusEffects(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = StatusEffects)]
pub struct StatusEffectOf(Entity);

// pub trait Applicable {}

// impl<C> Applicable for C where C: ElementalTag {}

// pub trait ElementalTag {}

// pub struct Poison {}
// impl ElementalTag for Poison {}
// impl PoolSupplier for Poison {
//     fn get_pools(&self) -> Vec<(CardPool, CardPoolStatus)> {
//         vec![(CardPool::Toxic, CardPoolStatus::Accepted)]
//     }
// }

// pub struct Fire {}
// impl ElementalTag for Fire {}
// impl PoolSupplier for Fire {
//     fn get_pools(&self) -> Vec<(CardPool, CardPoolStatus)> {
//         vec![
//             (CardPool::Heated, CardPoolStatus::Accepted),
//             (CardPool::Fire, CardPoolStatus::Accepted),
//         ]
//     }
// }
//

// pub fn tick_effects(e: On<EntityTurnEnd>, tickers_q: Query<&TickOn<EntityTurnEnd>>) {}

// fn poison(duration: f32) -> impl Bundle {
//     (
//         Poison,
//         StatusTimer(Timer::new(
//             Duration::from_secs_f32(duration),
//             TimerMode::Once,
//         )),
//         observe(tick_poison), // Note: requires `bevy_ui_widgets` feature
//     )
// }

#[derive(EntityEvent, Clone)]
pub struct Tick {
    #[event_target]
    pub status: Entity,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatusKind {
    Poison,
    Burn,
}

impl Into<&'static str> for StatusKind {
    fn into(self) -> &'static str {
        match self {
            StatusKind::Poison => "Poison",
            StatusKind::Burn => "Burn",
        }
    }
}

#[derive(Component, Debug, Clone)]
pub struct StatusEffectApplier {
    pub effect_kind: StatusKind,
    pub amount: u32,
}

impl StatusEffectApplier {
    pub fn new(effect_kind: StatusKind, amount: u32) -> Self {
        Self {
            effect_kind,
            amount,
        }
    }
}

pub fn status_effect(
    amount: u32,
    source: Entity,
    key_supplier: impl Into<&'static str>,
) -> impl Bundle {
    let mut status_default_mod = ModifierSet::new();
    let key = key_supplier.into();
    status_default_mod.add(key, amount as f32);
    println!("spawning sttaus");

    related!(
        StatusEffects[(
            EffectHandler {
                amount,
                effect_key: key,
                status_applied_by: Some(source),
            },
            TriggerOn::<EntityTurnEnd>::new(),
            Attributes::new(),
            AttributeInitializer::new(status_default_mod),
            observe(tick_status_effect),
        )]
    )
}

pub fn tick_on<T: EntityEvent + Clone>(
    e: On<T>,
    status_list_q: Query<&StatusEffects>,
    q: Query<Entity, (With<TriggerOn<T>>, With<StatusEffectOf>)>,
    mut cmd: Commands,
) {
    println!("suis la mon calisse MAIS pas tt a fait");
    let target_entity = e.event_target();
    cmd.entity(target_entity).log_components();
    let Ok(all_target_status) = status_list_q.get(target_entity) else {
        return;
    };

    for status in q.iter_many(all_target_status.iter()) {
        println!("suis la mon calisse");

        cmd.trigger(Tick { status });
    }
}

#[derive(Component, Clone)]
pub struct EffectHandler {
    amount: u32,
    effect_key: &'static str,
    status_applied_by: Option<Entity>,
}

fn tick_status_effect(
    tick: On<Tick>,
    effect: Query<(&StatusEffectOf, &EffectHandler, Entity)>,
    deck_ref_q: Query<&CurrentDeckReference>,
    mut attrs: AttributesMut,
) {
    let (StatusEffectOf(player), handler, effect_entity) = effect
        .get(tick.status)
        .expect("Needs status effect of + EffectHandler");

    let deck = deck_ref_q
        .get(*player)
        .expect("Player should have DeckRef")
        .0;

    let roles = match handler.status_applied_by {
        Some(e) => vec![("Applicator", e.clone()), ("Effect", effect_entity)],
        None => vec![("Effect", effect_entity)],
    };

    let health_mod = instant! {"SoulLife.current" -= format!("{}@Effect", handler.effect_key)};

    let evaluated_instant = attrs.evaluate_instant(&health_mod, &roles.as_slice(), deck);
    attrs.apply_evaluated_instant(&evaluated_instant, deck);
}

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = EvReactorOf, linked_spawn)]
pub struct EvReactors(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = EvReactors)]
pub struct EvReactorOf(pub Entity);

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = AbilityOfCaster, linked_spawn)]
pub struct CasterAbilities(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = CasterAbilities)]
pub struct AbilityOfCaster(pub Entity);

//
#[derive(EntityEvent, Clone)]
pub struct TriggerEffect<C: EntityEvent + Clone> {
    pub entity: Entity,
    pub cause: C,
}

#[derive(Component, Clone)]
pub struct TriggerOn<C: EntityEvent + Clone> {
    _data: PhantomData<C>,
}

impl<C: EntityEvent + Clone> TriggerOn<C> {
    pub fn new() -> Self {
        Self { _data: PhantomData }
    }
}
