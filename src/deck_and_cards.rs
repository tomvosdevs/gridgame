use std::marker::PhantomData;

use bevy::{
    app::{App, Plugin, Startup, Update},
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        name::Name,
        query::{With, Without},
        related,
        schedule::IntoScheduleConfigs,
        system::{Commands, EntityCommands, Query},
        world::World,
    },
};

pub struct DeckAndCardsPlugin;

impl Plugin for DeckAndCardsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            (
                spawn_test_template_cards,
                print_template_cards,
                spawn_test_deck,
            )
                .chain(),
        )
        .add_systems(Update, log_cards_in_deck);
    }
}

#[derive(Component, Clone)]
#[require(DeckCards)]
pub struct Deck;

#[derive(Component)]
#[relationship_target(relationship = InDeck, linked_spawn)]
pub struct DeckCards(Vec<Entity>);

impl Default for DeckCards {
    fn default() -> Self {
        Self(vec![])
    }
}

#[derive(Component)]
#[relationship(relationship_target = DeckCards)]
pub struct InDeck(Entity);

#[derive(Component, Clone)]
pub struct Card;

pub trait CardStateMarker {}

pub struct TemplateCard;
impl CardStateMarker for TemplateCard {}
pub struct InDrawPile;
impl CardStateMarker for InDrawPile {}
pub struct InDiscardPile;
impl CardStateMarker for InDiscardPile {}
pub struct InHand;
impl CardStateMarker for InHand {}
pub struct OutOfCombat;
impl CardStateMarker for OutOfCombat {}

#[derive(Component, Clone)]
pub struct CardState<S: CardStateMarker> {
    pub _state: PhantomData<S>,
}

impl<S: CardStateMarker> CardState<S> {
    pub fn new() -> Self {
        Self {
            _state: PhantomData,
        }
    }
}

impl Card {
    pub fn new_template(name: &'static str) -> impl Bundle {
        (Card, CardState::<TemplateCard>::new(), Name::new(name))
    }
}

pub fn add_card_to_deck(cmd: &mut Commands, card_template: Entity, deck_entity: Entity) {
    // let new_card_instance = cmd
    //     .spawn((InDeck(deck_entity), CardState::<OutOfCombat>::new()))
    //     .id();

    // clone template component into the instance
    let instance = cmd.entity(card_template).clone_and_spawn().id();

    println!("log of instance components : ");
    cmd.entity(instance).log_components();
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
        cmd.spawn(Card::new_template(name));
    }
}

pub fn print_template_cards(q: Query<&Name, With<CardState<TemplateCard>>>) {
    for card_name in &q {
        println!("found template card : {:?}", card_name);
    }
}

pub fn spawn_test_deck(
    mut cmd: Commands,
    template_cards_q: Query<Entity, With<CardState<TemplateCard>>>,
) {
    let test_deck_ent = cmd.spawn(Deck).id();

    for template_card_ent in &template_cards_q {
        println!("adding card to deck");
        add_card_to_deck(&mut cmd, template_card_ent, test_deck_ent);
    }
}

pub fn log_cards_in_deck(
    instance_cards_q: Query<&Name, (With<Card>, Without<CardState<TemplateCard>>)>,
) {
    println!(
        "about to log instances of cards : {:?}",
        instance_cards_q.iter().len()
    );
    for card_name in &instance_cards_q {
        println!("found card in deck with name : {:?}", card_name);
    }
}
