use std::marker::PhantomData;

use bevy::app::{App, Plugin};
use bevy_ecs::{
    bundle::Bundle,
    component::Component,
    entity::Entity,
    event::EntityEvent,
    lifecycle::Add,
    observer::On,
    query::With,
    system::{Commands, Query},
};

use crate::{
    deck::deck_and_cards::SoulLife,
    game_flow::turns::{CurrentDeckReference, PlayingEntity},
    movement::StepOnTile,
};

pub struct EventReactionsPlugin;

impl Plugin for EventReactionsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(apply_reaction_damage)
            .add_observer(on_spiky_add)
            .add_observer(
                |e: On<StepOnTile>,
                 mut cmd: Commands,
                 reactions: Query<&Reactions>,
                 matcher_q: Query<&ReactOn<StepOnTile>>| {
                    println!("check for reaction on tile step");
                    let source = e.entity;
                    let target = e.moving_entity;

                    let Ok(reaction_entities) = reactions.get(source) else {
                        return;
                    };

                    println!("yup I can see some reactions");

                    let applicable: Vec<&Entity> = reaction_entities
                        .0
                        .iter()
                        .filter(|e| matcher_q.contains(**e))
                        .collect();

                    println!("I think we can apply these : {:?}", applicable.len());

                    for reaction in applicable {
                        cmd.entity(*reaction).clone_with_opt_out(target, |_| {});
                        cmd.trigger(ReactionTriggered { entity: target });
                    }
                },
            );
    }
}

#[derive(Component)]
#[relationship_target(relationship = ReactionFor)]
pub struct Reactions(Vec<Entity>);

#[derive(Component)]
#[relationship(relationship_target = Reactions)]
pub struct ReactionFor(Entity);

impl ReactionFor {
    pub fn get_bundle<T: EntityEvent>(
        parent: Entity,
        reaction_components: impl Bundle,
    ) -> impl Bundle {
        (Self(parent), ReactOn::<T>::new(), reaction_components)
    }
}

#[derive(Component)]
struct ReactOn<T>
where
    T: EntityEvent,
{
    _data: PhantomData<T>,
}

impl<T: EntityEvent> ReactOn<T> {
    pub fn new() -> Self {
        Self { _data: PhantomData }
    }
}

//                                       //
// ==== Reaction effects components ==== //
//                                       //

#[derive(Component)]
pub struct SpikyCell;

fn on_spiky_add(e: On<Add, SpikyCell>, mut cmd: Commands) {
    println!("spiky cell added");
    cmd.spawn(ReactionFor::get_bundle::<StepOnTile>(
        e.entity,
        ApplyDamage(2),
    ));
}

#[derive(Component, Clone)]
pub struct ApplyDamage(pub u32);

#[derive(EntityEvent)]
struct ReactionTriggered {
    entity: Entity,
}

fn apply_reaction_damage(
    e: On<ReactionTriggered>,
    mut q: Query<(&ApplyDamage, &CurrentDeckReference), With<PlayingEntity>>,
    mut soullife_q: Query<&mut SoulLife>,
    mut cmd: Commands,
) {
    println!("on reaction trig");
    let e = e.entity;
    cmd.entity(e).log_components();
    let Ok((dmg, deck_ref)) = q.get_mut(e) else {
        return;
    };

    println!("found a deck ref");

    let mut soullife = soullife_q
        .get_mut(deck_ref.0)
        .expect("deck ref should always have soullife");

    soullife.current -= dmg.0 as f32;

    println!("applied damage to sl");

    cmd.entity(e).remove::<ApplyDamage>();
}
