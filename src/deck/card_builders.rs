use std::range::Range;

use bevy::ecs::{
    bundle::Bundle,
    component::Component,
    observer::On,
    query::With,
    system::{Commands, Query, Res, Single},
};
use bevy_diesel::spawn::TemplateRegistry;
use bevy_ecs::{entity::Entity, event::EntityEvent};

use bevy_gauge::{
    AttributeComponent, AttributeResolvable,
    prelude::{AttributesMut, WriteBack},
};
use bevy_prng::WyRand;
use bevy_rand::global::GlobalRng;

use rand::RngExt;

use crate::{
    abilities::abilities_templates::AbilityKind,
    deck::{
        card_blueprints::CardBlueprint,
        deck_and_cards::{Card, Deck, InDeck, SoulLife, StatelessCard},
    },
    game_flow::turns::CurrentDeckReference,
};

#[bevy_trait_query::queryable]
pub trait PoolSupplier {
    fn get_pools(&self) -> Vec<(CardPool, CardPoolStatus)>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPool {
    Heat,
    Cold,
    Ice,
    Melee,
    Ranged,
    Flying,
    Clawed,
    Punch,
    Heavy,
    Light,
    Swift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPoolStatus {
    Required,
    Accepted,
    Forbidden,
}

#[repr(u32)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Clone, Copy, Debug)]
pub enum RarityTier {
    Common = 1,
    Uncommon = 2,
    Rare = 3,
    Legendary = 4,
    Secret = 5,
}

impl RarityTier {
    fn as_f32(&self) -> f32 {
        self.clone() as u32 as f32
    }

    fn from_f32(&self, val: f32) -> Self {
        match val {
            1.0 => RarityTier::Common,
            2.0 => RarityTier::Uncommon,
            3.0 => RarityTier::Rare,
            4.0 => RarityTier::Legendary,
            5.0 => RarityTier::Secret,
            _ => RarityTier::Common,
        }
    }
}

#[derive(Component, AttributeComponent)]
pub struct CardRarity {
    #[write("Rarity")]
    #[read("Rarity")]
    tier: f32,
}

impl RarityTier {
    pub fn rarities_vec() -> Vec<RarityTier> {
        vec![
            RarityTier::Common,
            RarityTier::Uncommon,
            RarityTier::Rare,
            RarityTier::Legendary,
            RarityTier::Secret,
        ]
    }
}

#[derive(Debug)]
pub enum RarityCond {
    EqOrHigher(RarityTier),
    EqOrBelow(RarityTier),
    InRange(Range<RarityTier>),
    EqTo(RarityTier),
}

pub enum RarityPicker {
    Random(RarityCond),
    Static(RarityTier),
}

impl RarityPicker {
    pub(crate) fn pick(self: &Self, rng: &mut WyRand) -> CardRarity {
        let rarities = RarityTier::rarities_vec();
        let found = match self {
            RarityPicker::Random(rarity_cond) => match rarity_cond {
                RarityCond::EqOrHigher(rarity_tier) => {
                    let valid_rarites: Vec<&RarityTier> =
                        rarities.iter().filter(|r| *r >= rarity_tier).collect();

                    **valid_rarites
                        .get(rng.random_range(0..valid_rarites.len()))
                        .unwrap()
                }
                RarityCond::EqOrBelow(rarity_tier) => {
                    let valid_rarites: Vec<&RarityTier> =
                        rarities.iter().filter(|r| *r <= rarity_tier).collect();

                    **valid_rarites
                        .get(rng.random_range(0..valid_rarites.len()))
                        .unwrap()
                }
                RarityCond::InRange(range) => {
                    let valid_rarites: Vec<&RarityTier> =
                        rarities.iter().filter(|r| range.contains(r)).collect();

                    **valid_rarites
                        .get(rng.random_range(0..valid_rarites.len()))
                        .unwrap()
                }
                RarityCond::EqTo(rarity_tier) => *rarity_tier,
            },
            RarityPicker::Static(rarity_tier) => *rarity_tier,
        };
        CardRarity {
            tier: found.as_f32(),
        }
    }
}

impl RarityPicker {}

pub trait CardBuilder {
    fn build(
        self: &Self,
        rng: &mut WyRand,
        cmd: &mut Commands,
        rarity: RarityPicker,
        templates: &Res<TemplateRegistry>,
        blueprints: &Vec<&CardBlueprint>,
    ) -> impl Bundle;
}

pub struct StaticCardBuilder {
    ability_kind: AbilityKind,
    // ability_modifiers: Vec<AbilityModifierKind>,
}

impl StaticCardBuilder {
    pub fn new(ability_kind: AbilityKind) -> Self {
        Self { ability_kind }
    }
}

pub struct RandomPoolCardBuilder {
    pub pools: Vec<(CardPool, CardPoolStatus)>,
    pub rarity: RarityCond,
}

impl RandomPoolCardBuilder {
    pub fn new(accepted_pools: Vec<(CardPool, CardPoolStatus)>, rarity: RarityCond) -> Self {
        Self {
            pools: accepted_pools,
            rarity,
        }
    }
}

impl CardBuilder for RandomPoolCardBuilder {
    fn build(
        self: &Self,
        rng: &mut WyRand,
        cmd: &mut Commands,
        rarity: RarityPicker,
        templates: &Res<TemplateRegistry>,
        blueprints: &Vec<&CardBlueprint>,
    ) -> impl Bundle {
        let matching_blueprints: Vec<&CardBlueprint> = blueprints
            .iter()
            .filter(|b| b.does_match(&self.pools))
            .map(|b| *b)
            .collect();

        let selected_blueprint = matching_blueprints
            .get(rng.random_range(0..matching_blueprints.len()))
            .expect(
                format!(
                    "could no select a blueprint in matching, matching list : {:?}",
                    matching_blueprints,
                )
                .as_str(),
            );

        selected_blueprint.generate(cmd, templates, rng, rarity)
    }
}

#[derive(EntityEvent)]
pub struct DefaultDeckGenRequested {
    pub entity: Entity,
}

pub fn gen_and_spawn_default_deck(
    e: On<DefaultDeckGenRequested>,
    q: Query<&dyn PoolSupplier>,
    templates: Res<TemplateRegistry>,
    rng: Single<&mut WyRand, With<GlobalRng>>,
    blueprint_q: Query<&CardBlueprint>,
    mut attributes: AttributesMut,
    mut cmd: Commands,
) {
    println!("gen def deck for e : {:?}", e.entity);
    let blueprints: Vec<&CardBlueprint> = blueprint_q.iter().map(|b| b).collect();

    let creature_entity = e.entity;

    let pools: Vec<(CardPool, CardPoolStatus)> = q
        .get(creature_entity)
        .expect("Entity should have the gen component")
        .iter()
        .flat_map(|ps| ps.get_pools())
        .collect();

    let mut rng = rng.into_inner();

    let default_deck_size = 20;
    let deck_entity = cmd.spawn(Deck).id();

    let card_builder = RandomPoolCardBuilder::new(pools, RarityCond::EqOrBelow(RarityTier::Common));

    for _ in 0..default_deck_size {
        let card_bundle = card_builder.build(
            &mut rng,
            &mut cmd,
            RarityPicker::Random(RarityCond::EqOrBelow(RarityTier::Rare)),
            &templates,
            &blueprints,
        );

        let card_entity = cmd
            .spawn((card_bundle, StatelessCard::new(), InDeck(deck_entity)))
            .id();
        attributes.register_source(card_entity, "Invoker", creature_entity);
    }

    cmd.entity(deck_entity).insert((SoulLife {
        current: default_deck_size as f32,
        max: default_deck_size as f32,
    },));

    attributes.register_source(deck_entity, "Invoker", creature_entity);

    cmd.entity(creature_entity)
        .insert(CurrentDeckReference(deck_entity));
}
