use std::range::Range;

use bevy::ecs::{
    bundle::Bundle,
    component::Component,
    lifecycle::Add,
    observer::On,
    query::With,
    system::{Commands, Query, Res, Single},
};
use bevy_ecs::{entity::Entity, event::EntityEvent};
use bevy_gauge::{attributes, prelude::Attributes};
use bevy_prng::WyRand;
use bevy_rand::global::GlobalRng;
use bevy_trait_query::queryable;
use rand::RngExt;

use crate::{
    abilities::abilities_templates::{AbilityKind, AbilityModifierKind, AbilityTemplateRegistry},
    creatures::definitions::CreatureKind,
    deck::deck_and_cards::{
        Card, CardState, Deck, InDeck, SoulLife, StatelessCard, UnassignedDeckState,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPoolStatus {
    Required,
    Accepted,
    Forbidden,
}

#[repr(u32)]
#[derive(PartialOrd, Ord, PartialEq, Eq, Component, Clone, Copy)]
pub enum RarityTier {
    Common = 0,
    Uncommon = 1,
    Rare = 2,
    Legendary = 3,
    Secret = 4,
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
    pub(crate) fn pick(self: &Self, rng: &mut WyRand) -> RarityTier {
        let rarities = RarityTier::rarities_vec();
        match self {
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
        }
    }
}

impl RarityPicker {}

pub trait CardBuilder {
    fn build(
        self: &Self,
        registry: &Res<AbilityTemplateRegistry>,
        rng: &mut WyRand,
        cmd: &mut Commands,
        rarity: RarityPicker,
    ) -> (Card, impl Bundle);
}

pub struct StaticCardBuilder {
    ability_kind: AbilityKind,
    // ability_modifiers: Vec<AbilityModifierKind>,
}

// impl CardBuilder for StaticCardBuilder {
//     fn build(
//         self: &Self,
//         registry: &Res<AbilityTemplateRegistry>,
//         rng: &mut WyRand,
//         cmd: &mut Commands,
//         rarity: RarityPicker,
//     ) -> (Card, impl Bundle) {
//         let ability_entity = registry.build_ability(cmd, self.ability_kind, None, vec![]);
//         (
//             Card::new(ability_entity),
//             (rarity.pick(rng), TemplateCard::new()),
//         )
//     }
// }

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
        registry: &Res<AbilityTemplateRegistry>,
        rng: &mut WyRand,
        cmd: &mut Commands,
        rarity: RarityPicker,
    ) -> (Card, impl Bundle) {
        let ability_entity = registry.build_ability(cmd, &self.pools, rng, vec![]);
        (Card::new(ability_entity), (rarity.pick(rng)))
    }
}

#[derive(EntityEvent)]
pub struct DefaultDeckGenRequested {
    pub entity: Entity,
}

pub fn gen_and_spawn_default_deck(
    e: On<DefaultDeckGenRequested>,
    q: Query<&dyn PoolSupplier>,
    registry: Res<AbilityTemplateRegistry>,
    rng: Single<&mut WyRand, With<GlobalRng>>,
    mut cmd: Commands,
) {
    println!("gen def deck for e : {:?}", e.entity);

    let pools: Vec<(CardPool, CardPoolStatus)> = q
        .get(e.entity)
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
            &registry,
            &mut rng,
            &mut cmd,
            RarityPicker::Random(RarityCond::EqOrBelow(RarityTier::Rare)),
        );

        cmd.spawn((card_bundle, StatelessCard::new(), InDeck(deck_entity)));
    }

    cmd.entity(deck_entity).insert((
        SoulLife {
            current: default_deck_size as f32,
            max: default_deck_size as f32,
        },
        Attributes::new(),
    ));

    cmd.entity(e.entity)
        .insert(CurrentDeckReference(deck_entity));
}
