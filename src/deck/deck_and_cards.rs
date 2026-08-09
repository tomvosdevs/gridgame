use std::marker::PhantomData;

use bevy::{
    app::{App, Plugin},
    ecs::{
        component::Component,
        entity::Entity,
        event::{EntityEvent, Event},
        observer::On,
        query::With,
        relationship::RelationshipTarget,
        system::{Commands, Query},
    },
};
use bevy_gauge::{AttributeComponent, prelude::Attributes};
use bevy_replicon::prelude::Replicated;
use serde::{Deserialize, Serialize};

use crate::{creatures::definitions::CreatureKind, deck::card_builders::PoolSupplier};

pub struct DeckAndCardsPlugin;

impl Plugin for DeckAndCardsPlugin {
    fn build(&self, app: &mut App) {
        use bevy_trait_query::RegisterExt;

        app.register_component_as::<dyn PoolSupplier, CreatureKind>();
    }
}

// ===============
// Observer systems
// ===============
//

// Structs etc

#[derive(Component, Clone, Debug)]
#[require(CardPile, HandDrawData, Attributes)]
pub struct Deck;

#[derive(EntityEvent)]
pub struct DeckGenerationRequested {
    entity: Entity,
}

#[derive(Component)]
pub struct ActiveDeck;

#[derive(Component)]
pub struct HandDrawData {
    pub cards_per_turn: u16,
}

impl HandDrawData {
    pub fn from_cards_per_turn(amount: u16) -> Self {
        Self {
            cards_per_turn: amount,
        }
    }
}

impl Default for HandDrawData {
    fn default() -> Self {
        Self { cards_per_turn: 5 }
    }
}

#[derive(EntityEvent)]
pub struct CardDrawn {
    pub entity: Entity,
    pub card_hand_index: u16,
}

#[derive(Event)]
pub struct HandDiscarded;

#[derive(EntityEvent)]
pub struct CardDiscarded {
    pub entity: Entity,
}

#[derive(EntityEvent)]
pub struct DrawHand {
    pub entity: Entity,
}

impl DrawHand {
    pub fn from_deck_entity(deck_entity: Entity) -> Self {
        Self {
            entity: deck_entity,
        }
    }
}

#[derive(Component, Clone, Debug, Serialize, Deserialize)]
#[relationship_target(relationship = InDeck, linked_spawn)]
pub struct CardPile {
    #[relationship_target]
    #[entities]
    cards: Vec<Entity>,
}

#[derive(Component, AttributeComponent, Clone, Copy)]
pub struct SoulLife {
    #[read("MaxSoulLife")]
    #[write("MaxSoulLife")]
    pub max: f32,
    #[read("SoulLife.current")]
    #[write]
    #[init_from("MaxSoulLife")]
    pub current: f32,
}

impl Default for CardPile {
    fn default() -> Self {
        Self { cards: vec![] }
    }
}

#[derive(Component)]
#[relationship(relationship_target = CardPile)]
pub struct InDeck(pub Entity);

#[derive(Component, Clone, Serialize, Deserialize)]
#[require(Replicated)]
pub struct Card {}

impl Card {
    pub fn new() -> Self {
        Self {}
    }
}

pub trait CardStateMarker {}

pub struct InDrawPile;
impl CardStateMarker for InDrawPile {}
pub struct InDiscardPile;
impl CardStateMarker for InDiscardPile {}
pub struct InHand;
impl CardStateMarker for InHand {}
// For cards outside of combat
pub struct UnassignedDeckState;
impl CardStateMarker for UnassignedDeckState {}

#[derive(Component, Clone)]
pub struct CardState<S: CardStateMarker> {
    pub _state: PhantomData<S>,
}

pub type DrawPileCard = CardState<InDrawPile>;
pub type DiscardPileCard = CardState<InDiscardPile>;
pub type HandCard = CardState<InHand>;
pub type StatelessCard = CardState<UnassignedDeckState>;

impl<S: CardStateMarker> CardState<S> {
    pub fn new() -> Self {
        Self {
            _state: PhantomData,
        }
    }
}
