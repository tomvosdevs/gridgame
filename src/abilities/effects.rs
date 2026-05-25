use std::{collections::HashMap, marker::PhantomData};

use bevy_diesel::prelude::InvokedBy;
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    event::EntityEvent,
    hierarchy::ChildOf,
    message::{MessageReader, MessageWriter},
    observer::On,
    query::With,
    system::{Commands, Query},
};
use bevy_gauge::{
    expr::Expr,
    instant,
    prelude::{
        AttributeQueries, AttributesMut, InstantExt, InstantModifierSet, Modifier, ModifierSet,
    },
};
use bevy_ghx_grid::ghx_grid::cartesian::coordinates::CartesianPosition;

use crate::{
    abilities::abilities_templates::{CasterAbilityCasted, CasterHitReceived},
    deck::card_blueprints::SubAbilityOf,
    game_flow::turns::{CurrentDeckReference, PlayingEntity},
    grid_abilities_backend::{
        AbilityHitEntity, CasterAbilityHit, GridGoOff, GridInvokerTarget, GridStartInvoke,
    },
    utils::IntoVec,
};

#[derive(Component)]
pub struct JustCastedEffect {
    caster: Entity,
}

impl JustCastedEffect {
    pub fn new(caster: Entity) -> Self {
        Self { caster }
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

pub fn handle_just_casted_effect(
    mut reader: MessageReader<GridGoOff>,
    mut cmd: Commands,
    q_effect: Query<&JustCastedEffect>,
) {
    for go_off in reader.read() {
        let Ok(effect) = q_effect.get(go_off.entity) else {
            continue;
        };

        cmd.trigger(CasterAbilityCasted(effect.caster));
    }
}

pub fn propag_caster_hit(mut reader: MessageReader<CasterAbilityHit>, mut cmd: Commands) {
    for e in reader.read() {
        println!("trig sent BOSS");

        cmd.trigger(CasterHitReceived(e.entity));
    }
}

#[derive(Component)]
pub struct SpawnEffect {
    pub caster: Entity,
    pub casted: Entity,
}

impl SpawnEffect {
    pub fn new(caster: Entity, casted: Entity) -> Self {
        Self { caster, casted }
    }
}

pub fn handle_spawn_effect(
    mut reader: MessageReader<GridGoOff>,
    q_effect: Query<&SpawnEffect>,
    mut writer: MessageWriter<GridStartInvoke>,
    mut cmd: Commands,
) {
    for go_off in reader.read() {
        let effect_entity = go_off.entity;
        let Ok(cast) = q_effect.get(effect_entity) else {
            continue;
        };
        let target = go_off.target;
        println!("target for invoke : {:?}", target);
        cmd.entity(cast.casted).insert((
            InvokedBy(cast.caster),
            SubAbilityOf(cast.caster),
            GridInvokerTarget::entity(target.entity.unwrap(), target.position),
        ));
        writer.write(GridStartInvoke::new(cast.casted, target));
    }
}

pub enum PosDirection {
    East,
    West,
    North,
    South,
    NorthEast,
    SouthEast,
    NorthWest,
    SouthWest,
}

pub enum NeighborMatch {
    OneOf(Vec<PosDirection>),
    All,
}

pub enum ContextFilter {
    All,
    PlayersOnly,
    SameTeamOnly,
    TilesOnly,
}

pub enum GameContext {
    Neighboring(NeighborMatch),
}

// TODO: Implement this, will allow to add auto updated aliases for some context
// and use these as role in attributes or mod expressions
#[derive(Component)]
pub struct ContextRoleAlias {
    pub applies_to: HashMap<GameContext, Vec<ContextFilter>>,
    pub to: Entity,
    pub alias: &'static str,
}

#[derive(Debug, Clone)]
pub enum AbilityEffectKind {
    Mod(ModifierSet),
    Instant(InstantModifierSet),
}

impl AbilityEffectKind {
    pub fn flat_damage(damage: f32) -> Self {
        let damage: &'static str = Box::leak(format!("{}", damage).into_boxed_str());
        println!("will apply dmg : {:?}", damage);
        Self::Instant(instant! {"SoulLife.current" -= damage})
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

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = EvReactorOf, linked_spawn)]
pub struct EvReactors(Vec<Entity>);

impl EvReactors {
    pub fn get_all(&self) -> &Vec<Entity> {
        &self.0
    }
}

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = EvReactors)]
pub struct EvReactorOf(pub Entity);

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = AbilityOfCaster, linked_spawn)]
pub struct CasterAbilities(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = CasterAbilities)]
pub struct AbilityOfCaster(pub Entity);

// impl StatusEffects<HitTrigger> {
//     pub fn hit(effects: impl IntoVec<AbilityEffectKind>) -> Self {
//         Self {
//             effects: effects.into_vec(),
//             _data: PhantomData,
//         }
//     }
// }

// impl StatusEffects<InvokeTrigger> {
//     pub fn invoked(effects: Vec<AbilityEffectKind>) -> Self {
//         Self {
//             effects: effects,
//             _data: PhantomData,
//         }
//     }
// }

// #[derive(Component)]
// pub struct DamageEffect(pub &'static str);
//
#[derive(EntityEvent)]
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

pub fn observe_effects(
    e: On<AbilityHitEntity>,
    status_effects_q: Query<&StatusEffects>,
    effect_reactors_q: Query<Entity, With<TriggerOn<CasterHitReceived>>>,
    // invoked_by_q: Query<&InvokedBy>,
    // player_target_q: Query<&GridInvokerTarget, With<PlayingEntity>>,
    // mut attributes: AttributesMut,
    // curr_deck_refs_q: Query<&CurrentDeckReference>,
    mut cmd: Commands,
) {
    println!("c'est la chefton");
    // let ability_entity = e.entity;
    // let Ok(status_effects) = status_effects_q.get(ability_entity) else {
    //     return;
    // };

    // // println!("received caster hit");

    // // let attacker = invoked_by_q
    // //     .get(caster)
    // //     .expect("should have found invoked by")
    // //     .0;

    // // let target_entity = player_target_q
    // //     .get(attacker)
    // //     .expect("InvokerTarget should be set")
    // //     .entity
    // //     .unwrap();

    // // let targeted_deck_entity = curr_deck_refs_q
    // //     .get(target_entity)
    // //     .expect("Attacks should target an entity with a deck")
    // //     .0;

    // for effect_target in effect_reactors_q.iter_many(&status_effects.0) {
    //     cmd.trigger(TriggerEffect {
    //         entity: effect_target,
    //         cause: e.event().clone(),
    //     });
    // }

    // // let roles = [("Attacker", attacker)];
    // // for effect in hit_effects_q.iter_many(status_effects.0) {
    // //     match effect {
    // //         // TODO :
    // //         cmd
    // //         AbilityEffectKind::Mod(modifier_set) => {
    // //             modifier_set
    // //                 .try_apply(targeted_deck_entity, &mut attributes)
    // //                 .expect("Failed to apply modifier set");
    // //         }
    // //         AbilityEffectKind::Instant(instant_modifier_set) => {
    // //             let evaluated_instant = attributes.evaluate_instant(
    // //                 &instant_modifier_set,
    // //                 &roles,
    // //                 targeted_deck_entity,
    // //             );
    // //             attributes.apply_evaluated_instant(&evaluated_instant, targeted_deck_entity);
    // //         }
    // //     }
    // // }
}

// pub fn handle_event_trigger(e: On<TriggerEffect>) {}
