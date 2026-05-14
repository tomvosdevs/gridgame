use bevy::ecs::component::Component;
use bevy_gauge::prelude::Attributes;
use bevy_prng::WyRand;
use rand::RngExt;

use crate::{
    deck::card_builders::{CardPool, CardPoolStatus, PoolSupplier},
    stats::players::{MeleeRange, RangedRange, Speed, Strength},
};

#[derive(Clone, Copy, PartialEq, Eq, Component)]
#[require(Attributes)]
pub enum CreatureKind {
    TestMelee,
    TestRanged,
}

impl PoolSupplier for CreatureKind {
    fn get_pools(&self) -> Vec<(CardPool, CardPoolStatus)> {
        match self {
            CreatureKind::TestMelee => vec![(CardPool::Melee, CardPoolStatus::Required)],
            CreatureKind::TestRanged => vec![(CardPool::Ranged, CardPoolStatus::Required)],
        }
    }
}

pub enum CardFilter {
    RequiredPool(CardPool),
    BannedPool(CardPool),
}

impl CreatureKind {
    pub fn get_card_filters(self: &Self) -> Vec<CardFilter> {
        match self {
            CreatureKind::TestMelee => vec![CardFilter::RequiredPool(CardPool::Melee)],
            CreatureKind::TestRanged => vec![CardFilter::RequiredPool(CardPool::Ranged)],
        }
    }

    pub fn get_stats_spread(
        self: &Self,
        rng: &mut WyRand,
    ) -> (Speed, Strength, MeleeRange, RangedRange) {
        match self {
            CreatureKind::TestMelee => (
                Speed::new(rng.random_range(1..2)),
                Strength::new(rng.random_range(3..5)),
                MeleeRange::new(1),
                RangedRange::new(2),
            ),
            CreatureKind::TestRanged => (
                Speed::new(rng.random_range(3..5)),
                Strength::new(rng.random_range(2..3)),
                MeleeRange::new(1),
                RangedRange::new(4),
            ),
        }
    }
}

#[derive(Component)]
pub struct Creature {
    pub kind: CreatureKind,
}

impl Creature {
    pub fn new(kind: CreatureKind) -> Self {
        Self { kind }
    }
}
