use std::collections::HashMap;

use bevy::{
    app::{App, Plugin, Startup, Update},
    ecs::{
        component::{Component, ComponentId},
        entity::Entity,
        query::AnyOf,
        resource::Resource,
        system::{Commands, EntityCommands, Query, Res},
        world::World,
    },
};

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_elements_and_reactions)
            .add_systems(Startup, spawn_test_effets)
            .add_systems(Update, resolve_effects);
    }
}

pub trait StatusEffect {}

#[derive(Component, Debug, Clone)]
pub struct Burning(pub i32);
impl StatusEffect for Burning {}

#[derive(Component, Debug, Clone, Default)]
pub struct Heat;
impl StatusEffect for Heat {}

#[derive(Component, Debug, Clone, Default)]
pub struct Steam;
impl StatusEffect for Steam {}

#[derive(Component, Debug, Clone, Default)]
pub struct Humid;
impl StatusEffect for Humid {}

#[derive(Component, Debug, Clone, Default)]
pub struct Iced;
impl StatusEffect for Iced {}

#[derive(Component, Debug, Clone, Default)]
pub struct Cold;
impl StatusEffect for Cold {}

#[derive(Clone, Debug)]
enum EffectComponent {
    Humid,
    Heat,
    Cold,
    Iced,
    Burning,
    Steam,
}

impl EffectComponent {
    fn insert(&self, entity: &mut EntityCommands) {
        match self {
            EffectComponent::Humid => {
                entity.insert(Humid);
            }
            EffectComponent::Heat => {
                entity.insert(Heat);
            }
            EffectComponent::Cold => {
                entity.insert(Cold);
            }
            EffectComponent::Iced => {
                entity.insert(Iced);
            }
            EffectComponent::Burning => {
                entity.insert(Burning(1));
            }
            EffectComponent::Steam => {
                entity.insert(Steam);
            }
        }
    }
}

#[derive(Component, Debug)]
pub struct Reaction {
    inputs: Vec<ComponentId>,
    removes: Vec<ComponentId>,
    spawns: Vec<EffectComponent>,
}

impl Reaction {
    fn new_empty() -> Self {
        Self {
            inputs: vec![],
            removes: vec![],
            spawns: vec![],
        }
    }

    fn with_inputs(mut self, inputs: Vec<ComponentId>) -> Self {
        self.inputs = inputs;
        self
    }

    fn with_removes(mut self, removes: Vec<ComponentId>) -> Self {
        self.removes = removes;
        self
    }

    fn with_added_components(mut self, spawns: Vec<EffectComponent>) -> Self {
        self.spawns = spawns;
        self
    }
}

#[derive(Resource)]
pub struct EffectsEquivalence(HashMap<ComponentId, Vec<ComponentId>>);

impl EffectsEquivalence {
    pub fn get_combined_effects(self, key: ComponentId) -> Vec<ComponentId> {
        let mut results = vec![key];
        if let Some(equivalences) = self.0.get(&key) {
            results.append(&mut equivalences.clone());
        }
        results
    }
}

fn spawn_elements_and_reactions(world: &mut World) {
    let mut effects_eq_map = HashMap::new();

    let humid = world.register_component::<Humid>();
    let heat = world.register_component::<Heat>();
    let cold = world.register_component::<Cold>();
    let iced = world.register_component::<Iced>();
    let burning = world.register_component::<Burning>();
    let steam = world.register_component::<Steam>();

    effects_eq_map.insert(burning, vec![heat]);
    effects_eq_map.insert(steam, vec![heat, humid]);
    effects_eq_map.insert(iced, vec![cold]);

    world.spawn(
        Reaction::new_empty()
            .with_inputs(vec![humid, heat])
            .with_removes(vec![humid])
            .with_added_components(vec![EffectComponent::Steam]),
    );

    world.spawn(
        Reaction::new_empty()
            .with_inputs(vec![cold, humid])
            .with_removes(vec![humid])
            .with_added_components(vec![EffectComponent::Iced]),
    );

    world.spawn(
        Reaction::new_empty()
            .with_inputs(vec![heat, iced])
            .with_removes(vec![heat, iced])
            .with_added_components(vec![EffectComponent::Humid]),
    );

    world.insert_resource(EffectsEquivalence(effects_eq_map));
}

#[derive(Component)]
pub struct TempFilterC;

fn spawn_test_effets(mut cmd: Commands) {
    println!("spawning test eff");
    cmd.spawn((Humid, Heat, TempFilterC));
}

fn resolve_effects(
    mut cmd: Commands,
    world: &World,
    reactions_q: Query<&Reaction>,
    _effects_equivalence: Res<EffectsEquivalence>,
    effects_q: Query<(
        Entity,
        AnyOf<(&Humid, &Heat, &Cold, &Iced, &Burning, &Steam)>,
    )>,
) {
    let reactions: Vec<&Reaction> = reactions_q.iter().collect();
    for (target_entity, opt_effects) in &effects_q {
        let mut ids: Vec<ComponentId> = Vec::new();

        if opt_effects.0.is_some() {
            ids.push(world.component_id::<Humid>().unwrap());
        }
        if opt_effects.1.is_some() {
            ids.push(world.component_id::<Heat>().unwrap());
        }
        if opt_effects.2.is_some() {
            ids.push(world.component_id::<Cold>().unwrap());
        }
        if opt_effects.3.is_some() {
            ids.push(world.component_id::<Iced>().unwrap());
        }
        if opt_effects.4.is_some() {
            ids.push(world.component_id::<Burning>().unwrap());
        }
        if opt_effects.5.is_some() {
            ids.push(world.component_id::<Steam>().unwrap());
        }

        for reaction in &reactions {
            let mut r = reaction.inputs.clone();
            let mut e = ids.clone();
            r.sort();
            e.sort();
            if r == e {
                println!("found mergeable reaction : {:?}", reaction);
                let mut entity_cmd = cmd.entity(target_entity);
                for c_id in &reaction.removes {
                    entity_cmd.remove_by_id(*c_id);
                }
                for eff_c in &reaction.spawns {
                    eff_c.insert(&mut entity_cmd);
                }
            }
        }
    }
}
