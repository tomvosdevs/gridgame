use bevy::{
    app::{App, Plugin},
    ecs::{
        component::Component,
        entity::Entity,
        event::{EntityEvent, Event},
        lifecycle::Add,
        observer::On,
        query::With,
        system::{Commands, Query, Single},
    },
};
use bevy_prng::WyRand;
use bevy_rand::global::GlobalRng;

use crate::{
    creatures::definitions::{Creature, CreatureKind},
    deck::card_builders::DefaultDeckGenRequested,
};

pub struct CreatureGenerationPlugin;

impl Plugin for CreatureGenerationPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(handle_creature_gen);
    }
}

fn handle_creature_gen(
    e: On<CreatureGenerationRequested>,
    mut cmd: Commands,
    rng: Single<&mut WyRand, With<GlobalRng>>,
) {
    let entity = cmd
        .entity(e.entity)
        .insert((
            Creature::new(e.kind),
            e.kind,
            e.kind.get_stats_spread(&mut rng.into_inner()),
        ))
        .id();

    cmd.trigger(DefaultDeckGenRequested { entity });
}

#[derive(EntityEvent)]
pub struct CreatureGenerationRequested {
    pub entity: Entity,
    pub kind: CreatureKind,
    // TODO : Later add data about the terrain etc to generate the creature in a way that fits it
}

impl CreatureGenerationRequested {
    pub fn new(entity: Entity, kind: CreatureKind) -> Self {
        Self { entity, kind }
    }
}
