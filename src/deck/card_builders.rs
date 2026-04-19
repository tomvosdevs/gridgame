use std::range::Range;

use bevy::ecs::{
    bundle::Bundle,
    system::{Commands, Res},
};

use crate::{
    abilities::abilities_templates::{AbilityKind, AbilityModifierKind, AbilityTemplateRegistry},
    deck::deck_and_cards::{Card, CardState, InCardTemplateRegistry},
};

pub enum Pool {
    Heat,
    Cold,
    Ice,
}

#[repr(u32)]
#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub enum RarityTier {
    Common = 0,
    Uncommon = 1,
    Rare = 2,
    Legendary = 3,
    Secret = 4,
}

pub enum RarityCond {
    EqOrHigher(RarityTier),
    EqOrBelow(RarityTier),
    InRange(Range<RarityTier>),
    EqTo(RarityTier),
}

pub trait CardBuilder {
    fn build(
        self: &Self,
        registry: &Res<AbilityTemplateRegistry>,
        cmd: &mut Commands,
    ) -> (Card, impl Bundle);
}

pub struct StaticCardBuilder {
    ability_kind: AbilityKind,
    // ability_modifiers: Vec<AbilityModifierKind>,
}

impl CardBuilder for StaticCardBuilder {
    fn build(
        self: &Self,
        registry: &Res<AbilityTemplateRegistry>,
        cmd: &mut Commands,
    ) -> (Card, impl Bundle) {
        let ability_entity = registry.build_ability(cmd, self.ability_kind, vec![]);
        (
            Card::new(ability_entity),
            (CardState::<InCardTemplateRegistry>::new()),
        )
    }
}

impl StaticCardBuilder {
    pub fn new(ability_kind: AbilityKind) -> Self {
        Self { ability_kind }
    }
}

pub struct RandomPoolCardBuilder {
    accepted_pools: Vec<Pool>,
    rarity: RarityCond,
}

// impl CardBuilder for RandomPoolCardBuilder {
//     fn build() -> (Card, impl Bundle) {
//         (Card::new(""), ())
//     }
// }
