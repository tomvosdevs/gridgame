use std::{collections::HashMap, marker::PhantomData};

use bevy_diesel::prelude::InvokedBy;
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
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
    game_flow::turns::{CurrentDeckReference, PlayingEntity},
    grid_abilities_backend::{CasterAbilityHit, GridGoOff, GridInvokerTarget, GridStartInvoke},
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

pub trait EffectTrigger {}

#[derive(Debug, Clone)]
pub struct InvokeTrigger {}
impl EffectTrigger for InvokeTrigger {}

#[derive(Debug, Clone)]
pub struct HitTrigger {}
impl EffectTrigger for HitTrigger {}

#[derive(Component, Debug, Clone)]
pub struct AbilityEffects<T>
where
    T: EffectTrigger + Clone,
{
    pub effects: Vec<AbilityEffectKind>,
    _data: PhantomData<T>,
}

impl AbilityEffects<HitTrigger> {
    pub fn hit(effects: impl IntoVec<AbilityEffectKind>) -> Self {
        Self {
            effects: effects.into_vec(),
            _data: PhantomData,
        }
    }
}

impl AbilityEffects<InvokeTrigger> {
    pub fn invoked(effects: Vec<AbilityEffectKind>) -> Self {
        Self {
            effects: effects,
            _data: PhantomData,
        }
    }
}

#[derive(Component)]
pub struct DamageEffect(pub &'static str);

pub fn observe_effects(
    e: On<CasterHitReceived>,
    effect_q: Query<&AbilityEffects<HitTrigger>>,
    invoked_by_q: Query<&InvokedBy>,
    player_target_q: Query<&GridInvokerTarget, With<PlayingEntity>>,
    mut attributes: AttributesMut,
    curr_deck_refs_q: Query<&CurrentDeckReference>,
) {
    let caster = e.0;
    let Ok(effects) = effect_q.get(caster) else {
        return;
    };
    println!("received caster hit");

    let attacker = invoked_by_q
        .get(caster)
        .expect("should have found invoked by")
        .0;

    let target_entity = player_target_q
        .get(attacker)
        .expect("InvokerTarget should be set")
        .entity
        .unwrap();

    let targeted_deck_entity = curr_deck_refs_q
        .get(target_entity)
        .expect("Attacks should target an entity with a deck")
        .0;

    let roles = [("Attacker", attacker)];

    for effect in effects.effects.iter() {
        // TODO / IDEA : Currently this applies everything on the deck, it's probably
        // a good idea to add an arg to specify what is the target between these two
        match effect {
            // TODO :
            AbilityEffectKind::Mod(modifier_set) => {
                modifier_set
                    .try_apply(targeted_deck_entity, &mut attributes)
                    .expect("Failed to apply modifier set");
            }
            AbilityEffectKind::Instant(instant_modifier_set) => {
                let evaluated_instant = attributes.evaluate_instant(
                    &instant_modifier_set,
                    &roles,
                    targeted_deck_entity,
                );
                attributes.apply_evaluated_instant(&evaluated_instant, targeted_deck_entity);
            }
        }
    }
}
