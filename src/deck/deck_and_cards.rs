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

use crate::{
    abilities::abilities_templates::AbilityHandler,
    creatures::definitions::CreatureKind,
    deck::card_builders::{PoolSupplier, gen_and_spawn_default_deck},
    game_flow::turns::EntityTurnEnd,
    ui::{CardTextureCamera, CardUiTargetMesh},
};

pub struct DeckAndCardsPlugin;

impl Plugin for DeckAndCardsPlugin {
    fn build(&self, app: &mut App) {
        use bevy_trait_query::RegisterExt;

        app.register_component_as::<dyn PoolSupplier, CreatureKind>()
            .add_observer(set_hand_cards)
            .add_observer(handle_entity_turn_end)
            .add_observer(gen_and_spawn_default_deck);
    }
}

// ===============
// Observer systems
// ===============
//

pub fn set_hand_cards(
    e: On<DrawHand>,
    mut cmd: Commands,
    decks_q: Query<(&HandDrawData, &CardPile), With<Deck>>,
    drawable_cards_q: Query<Entity, (With<Card>, With<CardState<InDrawPile>>)>,
) {
    println!("drawing hand cards");
    let (hand_draw_data, card_pile) = decks_q.get(e.entity).expect("Target deck not found");

    println!("deck cards count = {:?}", card_pile.len());

    let hand_size = hand_draw_data.cards_per_turn as usize;

    let new_hand_cards: Vec<Entity> = card_pile
        .iter()
        .filter(|card_entity| drawable_cards_q.contains(*card_entity))
        // This ensure the Vec never grows larger than the hand size.
        .take(hand_size)
        .collect();

    for (idx, card_entity) in new_hand_cards.iter().enumerate() {
        let mut entity_cmds = cmd.entity(*card_entity);
        entity_cmds.remove::<CardState<InDrawPile>>();
        entity_cmds.insert(HandCard::new());
        println!("drawn card entity : {:?}", card_entity);
        cmd.trigger(CardDrawn {
            entity: *card_entity,
            card_hand_index: idx as u16,
        });
    }
}

pub fn handle_entity_turn_end(
    _: On<EntityTurnEnd>,
    mut cmd: Commands,
    hand_cards: Query<Entity, (With<Card>, With<HandCard>)>,
    ui_cards: Query<Entity, With<CardUiTargetMesh>>,
    ui_card_textures: Query<Entity, With<CardTextureCamera>>,
) {
    // TODO: Also remove the Image and Material asset entries for the UI cards and UI card source
    cmd.trigger(HandDiscarded);
    for card_entity in &hand_cards {
        let mut entity_cmds = cmd.entity(card_entity);
        entity_cmds.remove::<HandCard>();
        entity_cmds.insert(DiscardPileCard::new());
        cmd.trigger(CardDiscarded {
            entity: card_entity,
        });
    }

    for ui_card_entity in &ui_cards {
        cmd.entity(ui_card_entity).despawn();
    }

    for tex in &ui_card_textures {
        cmd.entity(tex).despawn();
    }
}

// Structs etc

#[derive(Component, Clone, Debug)]
#[require(CardPile, HandDrawData, Attributes)]
pub struct Deck;

pub struct DeckBuilder;

// impl DeckBuilder {
//     pub fn spawn_default_deck(cmd: &mut Commands, cards_count: u32) {
//         let mut cards: Vec<Entity> =
//             (0..cards_count).map(|_| cmd.spawn(Card::new(ability_name)).id());

//         let deck_entity = cmd
//             .spawn((
//                 Deck,
//                 CardPile::default(),
//                 HandDrawData::default(),
//             ))
//             .id();
//     }
// }

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

#[derive(Component)]
#[relationship_target(relationship = InDeck, linked_spawn)]
pub struct CardPile {
    #[relationship_target]
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

#[derive(Component, Clone)]
pub struct Card {
    pub ability_handler: AbilityHandler,
}

impl Card {
    pub fn new(ability_handler: AbilityHandler) -> Self {
        Self { ability_handler }
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
