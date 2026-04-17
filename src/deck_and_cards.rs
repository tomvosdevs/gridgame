use std::marker::PhantomData;

use bevy::{
    app::{App, Plugin, Startup},
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::{Entity, MapEntities},
        event::EntityEvent,
        name::Name,
        observer::On,
        query::{QueryFilter, With},
        relationship::RelationshipTarget,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query},
    },
};
use bevy_gauge::{
    AttributeComponent, attributes,
    prelude::{Attributes, AttributesAppExt, AttributesMut, WriteBack},
    register_write_back,
};
use serde::{Deserialize, Serialize};

use crate::{
    abilities::PROJECTILE_ABILITY,
    states::EntityTurnEnd,
    ui::{CardUiTextContent, TextSection},
};

pub struct DeckAndCardsPlugin;

impl Plugin for DeckAndCardsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(set_hand_cards)
            .add_observer(discard_hand)
            .add_systems(
                Startup,
                (
                    spawn_test_template_cards,
                    // print_template_cards,
                    spawn_test_deck,
                )
                    .chain(),
            );
    }
}

pub fn spawn_test_template_cards(mut cmd: Commands) {
    let cards_names = vec![
        "Une carte",
        "Une autre",
        "toto",
        "tata",
        "tutu",
        "jean",
        "jeannot",
        "poissoj",
        "souris",
    ];
    for name in cards_names {
        cmd.spawn(Card::new_template(PROJECTILE_ABILITY, name));
    }
}

pub fn print_template_cards(q: Query<&Name, With<CardState<InCardTemplateRegistry>>>) {
    for card_name in &q {
        println!("found template card : {:?}", card_name);
    }
}

pub fn spawn_test_deck(
    mut cmd: Commands,
    template_cards_q: Query<Entity, With<CardState<InCardTemplateRegistry>>>,
) {
    let test_deck_ent = cmd
        .spawn((
            Deck,
            attributes! {"SoulLife.Current" => 0.0, "SoulLife.Max" => 0.0},
            Attributes::new(),
            SoulLife::default(),
        ))
        .id();

    for template_card_ent in &template_cards_q {
        println!("adding card to deck");
        add_card_to_deck(&mut cmd, template_card_ent, test_deck_ent);
    }
}

// ===============
// Observer systems
// ===============

pub fn set_hand_cards(
    e: On<DrawHand>,
    mut cmd: Commands,
    decks_q: Query<(&HandDrawData, &CardPile), With<Deck>>,
    mut attributes: AttributesMut<(With<Deck>, With<CardPile>, With<HandDrawData>)>,
    drawable_cards_q: Query<Entity, (With<Card>, With<CardState<InDrawPile>>)>,
) {
    println!("drawing hand cards");
    let (hand_draw_data, card_pile) = decks_q.get(e.entity).expect("Target deck not found");
    let hand_size = hand_draw_data.cards_per_turn as usize;

    let new_hand_cards: Vec<Entity> = card_pile
        .iter()
        .filter(|card_entity| drawable_cards_q.contains(*card_entity))
        // This ensure the Vec never grows larger than the hand size.
        .take(hand_size)
        .collect();

    let drawn_cards_count = new_hand_cards.iter().count() as f32;
    let current_soullife = attributes.value(e.entity, "SoulLife.Current");
    let new_soullife = (current_soullife - drawn_cards_count).max(0.0);
    attributes.set(e.entity, "SoulLife.Current", new_soullife);

    for (idx, card_entity) in new_hand_cards.iter().enumerate() {
        let mut entity_cmds = cmd.entity(*card_entity);
        entity_cmds.remove::<CardState<InDrawPile>>();
        entity_cmds.insert(HandCard::new());
        cmd.trigger(CardDrawn {
            entity: *card_entity,
            card_hand_index: idx as u16,
        });
    }
}

pub fn discard_hand(
    _: On<EntityTurnEnd>,
    mut cmd: Commands,
    hand_cards: Query<Entity, (With<Card>, With<HandCard>)>,
) {
    for card_entity in &hand_cards {
        let mut entity_cmds = cmd.entity(card_entity);
        entity_cmds.remove::<HandCard>();
        entity_cmds.insert(DiscardPileCard::new());
        cmd.trigger(CardDiscarded {
            entity: card_entity,
        });
    }
}

// Structs etc

#[derive(Component, Clone, Debug)]
#[require(CardPile, HandDrawData, SoulLife)]
pub struct Deck;

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

impl WriteBack for CardPile {
    fn should_write_back(&self, attrs: &Attributes) -> bool {
        let current = attrs.value("SoulLife.Max");
        self.cards.len() as f32 != current
    }

    fn write_back<F: QueryFilter>(
        &self,
        entity: Entity,
        attributes: &mut AttributesMut<'_, '_, F>,
    ) {
        attributes.set(entity, "SoulLife.Max", self.cards.len() as f32);
    }
}

register_write_back!(CardPile);

#[derive(Component, AttributeComponent)]
pub struct SoulLife {
    #[read("SoulLife.Current")]
    #[init_from("SoulLife.Max")]
    pub current: f32,
    #[read("SoulLife.Max")]
    pub max: f32,
}

impl Default for SoulLife {
    fn default() -> Self {
        Self {
            current: 0.,
            max: 0.,
        }
    }
}

impl Default for CardPile {
    fn default() -> Self {
        Self { cards: vec![] }
    }
}

#[derive(Component)]
#[relationship(relationship_target = CardPile)]
pub struct InDeck(Entity);

#[derive(Component, Clone, Default)]
pub struct Card {
    pub ability: &'static str,
}

pub trait CardStateMarker {}

pub struct InCardTemplateRegistry;
impl CardStateMarker for InCardTemplateRegistry {}
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
#[require(Card)]
pub struct CardState<S: CardStateMarker> {
    pub _state: PhantomData<S>,
}

pub type TemplateCard = CardState<InCardTemplateRegistry>;
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

impl Card {
    pub fn new(ability_name: &'static str) -> Self {
        Self {
            ability: ability_name,
        }
    }

    pub fn new_template(ability_name: &'static str, name: &'static str) -> impl Bundle {
        (
            Card::new(ability_name),
            CardState::<InCardTemplateRegistry>::new(),
            Name::new(name),
        )
    }
}

pub fn add_card_to_deck(cmd: &mut Commands, card_template: Entity, deck_entity: Entity) {
    let new_card_instance = cmd
        .spawn((InDeck(deck_entity), CardState::<UnassignedDeckState>::new()))
        .id();

    // clone template component into the instance
    let _instance = cmd
        .entity(card_template)
        .clone_with_opt_out(new_card_instance, |builder| {
            builder.deny::<CardState<InCardTemplateRegistry>>();
        })
        .id();
}
