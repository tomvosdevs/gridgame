use std::{any::Any, marker::PhantomData};

use bevy::{
    app::{Plugin, Startup, Update},
    ecs::{
        bundle::Bundle,
        component::{Component, ComponentId},
        entity::Entity,
        event::EntityEvent,
        name::Name,
        observer::On,
        query::{QueryData, QueryFilter, QueryState, ReadOnlyQueryData, With},
        schedule::IntoScheduleConfigs,
        system::{Commands, EntityCommand, EntityCommands, Query},
        world::{DeferredWorld, World},
    },
    math::bool,
    transform::components::Transform,
};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;

use crate::{BLOCK_SIZE, GridCell, Health, HealthState, NODE_SIZE};

pub struct ActionPlugin;

impl Plugin for ActionPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(Startup, (setup_reactions, setup_actions_observers).chain());
    }
}

pub struct Confusion;
#[derive(Component, Debug, Default)]
pub struct Range(i32);

pub trait AsAny: Any {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Component + DamageKind> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub trait DamageKind: Any + Component {
    // fn apply_damage(self) -> impl Bundle;
}

#[derive(Component, Debug, Default)]
pub struct Physical;

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Ranged(i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Melee(pub i32);

#[derive(Component, Debug, Default)]
#[require(Physical)]
pub struct Piercing(i32);

#[derive(Component, Debug, Default)]
pub struct Fire(i32);

#[derive(Component, Debug, Default)]
pub struct Electric(i32);

#[derive(Component, Debug, Default)]
pub struct Water(i32);

impl DamageKind for Physical {}
impl DamageKind for Ranged {}
impl DamageKind for Melee {}
impl DamageKind for Piercing {}
impl DamageKind for Fire {}
impl DamageKind for Electric {}
impl DamageKind for Water {}

#[derive(Component)]
pub struct Damage(pub i32);

#[derive(Component, Debug)]
pub struct MovementPoints(pub i32);

#[derive(Component, Debug, Default)]
pub struct ActionPoints(pub i32);

pub trait ActionBuilderState {}

pub struct ActionCreationStart;
impl ActionBuilderState for ActionCreationStart {}
pub struct ActionCreated;
impl ActionBuilderState for ActionCreated {}
pub struct ActionNameDefined;
impl ActionBuilderState for ActionNameDefined {}
pub struct ActionRangeDefined;
impl ActionBuilderState for ActionRangeDefined {}
pub struct ActionPointsDefined;
impl ActionBuilderState for ActionPointsDefined {}
pub struct ActionDamageDefined;
impl ActionBuilderState for ActionDamageDefined {}

pub struct ActionBuilder<'a, S: ActionBuilderState = ActionCreated> {
    _state: PhantomData<S>,
    entity: Entity,
    entity_commands: EntityCommands<'a>,
}

impl<'a, S: ActionBuilderState> ActionBuilder<'a, S> {
    pub fn get_entity(self: Self) -> Entity {
        self.entity
    }

    pub fn with_effects(mut self, effects: Vec<ActionEffect>) -> Self {
        self.entity_commands.insert(ActionEffects(effects));
        self
    }
}

impl<'a> ActionBuilder<'a, ActionCreationStart> {
    pub fn empty(commands: &'a mut Commands) -> ActionBuilder<'a, ActionCreated> {
        let entity_commands = commands.spawn(Action);
        ActionBuilder {
            _state: PhantomData,
            entity: entity_commands.id(),
            entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionCreated> {
    pub fn with_name(mut self: Self, name: &'static str) -> ActionBuilder<'a, ActionNameDefined> {
        self.entity_commands.insert(Name::new(name));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionNameDefined> {
    pub fn with_range(mut self: Self, range: i32) -> ActionBuilder<'a, ActionRangeDefined> {
        self.entity_commands.insert(Range(range));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionRangeDefined> {
    pub fn with_mp_cost(mut self: Self, movement_points: i32) -> Self {
        self.entity_commands.insert(MovementPoints(movement_points));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }

    pub fn with_ap_cost(
        mut self: Self,
        action_points: i32,
    ) -> ActionBuilder<'a, ActionPointsDefined> {
        self.entity_commands.insert(ActionPoints(action_points));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }
}

impl<'a> ActionBuilder<'a, ActionPointsDefined> {
    pub fn with_melee_damage(
        mut self: Self,
        damage: i32,
    ) -> ActionBuilder<'a, ActionDamageDefined> {
        self.entity_commands.insert(Melee(damage));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }

    pub fn with_ranged_damage(
        mut self: Self,
        damage: i32,
    ) -> ActionBuilder<'a, ActionDamageDefined> {
        self.entity_commands.insert(Ranged(damage));
        ActionBuilder {
            _state: PhantomData,
            entity: self.entity,
            entity_commands: self.entity_commands,
        }
    }

    pub fn with_damage_of_kind<T: DamageKind + Component>(mut self, damage_kind: T) -> Self {
        self.entity_commands.insert(damage_kind);
        self
    }
}

#[derive(Component)]
pub struct UsedAction;

#[derive(Component)]
pub struct MainTarget(pub Entity);

#[derive(Component, Debug)]
#[require(ActionPoints, Range, ActionEffects)]
pub struct Action;

impl<'a> Action {
    pub fn spawn_empty(commands: &'a mut Commands) -> ActionBuilder<'a, ActionCreated> {
        ActionBuilder::empty(commands)
    }
}

impl Action {
    pub fn cast(
        entity_cmds: &mut EntityCommands,
        // , &Action, &Range, &ActionPoints, Option<&MovementPoints>, Option<&Physical>, Option<&Ranged>, Option<&Melee>, Option<&Piercing>, Option<&Fire>, Option<&Electric>, Option<&Water>)
    ) {
        entity_cmds.trigger(|e| ActionCast { entity: e });
    }
}

#[derive(EntityEvent)]
pub struct ActionCast {
    pub entity: Entity,
}

#[derive(EntityEvent)]
pub struct ActionEffectReceived {
    pub entity: Entity,
    key: ActionEffect,
}

pub struct Predicate {
    validate_fn: Box<dyn FnMut(&World, Entity) -> bool + Send + Sync + 'static>,
}

impl Predicate {
    pub fn validate(&mut self, world: &World, entity: Entity) -> bool {
        (self.validate_fn)(world, entity)
    }

    pub fn new(predicate: impl Fn(&World, Entity) -> bool + Send + Sync + 'static) -> Self {
        Self {
            validate_fn: Box::new(predicate),
        }
    }
}

pub struct ReactionResult {
    apply_fn: Box<dyn FnMut(&mut World, Entity) + Send + Sync + 'static>,
}

impl ReactionResult {
    pub fn apply(&mut self, world: &mut World, entity: Entity) {
        (self.apply_fn)(world, entity)
    }

    /// For simple structural changes via Commands
    pub fn new(effect: impl Fn(&mut Commands, Entity) + Send + Sync + 'static) -> Self {
        Self {
            apply_fn: Box::new(move |world: &mut World, entity: Entity| {
                effect(&mut world.commands(), entity);
            }),
        }
    }

    /// For advanced cases needing full DeferredWorld access
    pub fn new_deferred(effect: impl Fn(&mut World, Entity) + Send + Sync + 'static) -> Self {
        Self {
            apply_fn: Box::new(move |world: &mut World, entity: Entity| {
                effect(world, entity);
            }),
        }
    }
}

#[derive(Component)]
pub struct Reaction {
    pub trigger_on: Vec<ActionEffect>,
    pub predicates: Vec<Predicate>,
    pub results: Vec<ReactionResult>,
}

impl Reaction {
    pub fn new(
        trigger_on: Vec<ActionEffect>,
        predicates: Vec<Predicate>,
        results: Vec<ReactionResult>,
    ) -> Self {
        Self {
            trigger_on,
            predicates,
            results,
        }
    }

    pub fn validate_all(&mut self, world: &mut World, entity: Entity) -> bool {
        self.predicates
            .iter_mut()
            .all(|p| p.validate(world, entity))
    }

    pub fn try_apply_reaction(
        &mut self,
        entity: Entity,
        world: &mut World,
        effect_key: ActionEffect,
    ) -> Result<(), &'static str> {
        if !self.trigger_on.contains(&effect_key) {
            return Err("trigger on doesn't conntain effect key");
        }
        if !self.validate_all(world, entity) {
            return Err("predicates were not all validated");
        }
        for result in &mut self.results {
            println!("applying reaction result");
            result.apply(world, entity);
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, PartialOrd, Ord, Eq, Clone, Copy)]
pub enum ActionEffect {
    Split,
    Infuse,
}

#[derive(Component)]
pub struct ActionEffects(pub Vec<ActionEffect>);

impl Default for ActionEffects {
    fn default() -> Self {
        Self(Vec::new())
    }
}

#[derive(Component)]
pub struct ActionTargets(pub Vec<Entity>);

pub fn setup_reactions(mut cmd: Commands) {
    cmd.spawn(Reaction::new(
        vec![ActionEffect::Split],
        vec![Predicate::new(|w: &World, e: Entity| {
            let has_gridcell = w.get::<GridCell>(e).is_some();
            let has_gridnode = w.get::<GridNode>(e).is_some();
            let has_tf = w.get::<Transform>(e).is_some();
            has_gridcell && has_gridnode && has_tf
        })],
        vec![ReactionResult::new_deferred(|w: &mut World, e: Entity| {
            // 1. Collect all data first (immutable borrows, then drop them)
            let curr_node = w.get::<GridNode>(e).unwrap().0.clone();
            let curr_tf = w.get::<Transform>(e).unwrap().clone();

            let (new_pos, new_index, new_tf) = {
                let mut grid_q = w.query::<&CartesianGrid<Cartesian3D>>();
                let grid = grid_q.iter(w).next().unwrap();
                let mut curr_pos = grid.pos_from_index(curr_node.clone());
                // TODO - ERR - Currently this crashes if used on a Y = grid height cell
                let mut new_pos = grid
                    .get_next_pos_in_direction(
                        &curr_pos,
                        bevy_ghx_grid::ghx_grid::direction::Direction::YForward,
                    )
                    .unwrap();
                let new_index = grid.index_from_pos(&new_pos);
                let mut new_tf = curr_tf.clone();
                new_tf.translation.y += NODE_SIZE.y;
                //let new_position =
                (new_pos, new_index, new_tf)
            }; // grid_q and grid dropped here

            // 2. Now issue commands (mutable borrow, no live immutable borrows)
            let clone_ent = w.commands().entity(e).clone_and_spawn().id();
            w.commands()
                .entity(clone_ent)
                .insert((new_pos, GridNode(new_index), new_tf));
        })],
    ));
}

pub fn setup_actions_observers(world: &mut World) {
    world.add_observer(
        |e: On<ActionCast>, mut cmd: Commands, q: Query<(&ActionEffects, &MainTarget)>| {
            println!("Action : {:?} got cast !", e.entity);
            let Ok((effects, main_target)) = q.get(e.entity) else {
                return;
            };

            for effect in effects.0.iter() {
                cmd.trigger(ActionEffectReceived {
                    entity: main_target.0,
                    key: *effect,
                });
            }
        },
    );

    world.add_observer(
        |e: On<ActionEffectReceived>,
         mut world_obs: DeferredWorld,
         q: Query<Entity, With<Reaction>>| {
            let entity = e.entity;
            let effect_key = e.key;

            let reaction_entities: Vec<Entity> = q.iter().collect();
            println!("action effect received");
            for reaction_entity in reaction_entities {
                println!("checking reaction");
                let (entities, mut commands) = world_obs.entities_and_commands();

                // Read predicates via entities (immutable)
                let reaction = entities.get(reaction_entity).unwrap();
                let reaction_applies_to_curr_effect = reaction
                    .get::<Reaction>()
                    .map_or(false, |r| r.trigger_on.contains(&effect_key));

                println!(
                    "reaction should apply : {:?}",
                    reaction_applies_to_curr_effect
                );

                if reaction_applies_to_curr_effect {
                    // Apply results via commands
                    commands.queue(move |w: &mut World| {
                        let mut reaction =
                            w.entity_mut(reaction_entity).take::<Reaction>().unwrap();
                        println!(
                            "in command queue : {:?} - e : {:?}",
                            reaction.results.len(),
                            entity
                        );
                        // run predicates + results here with full world access
                        match reaction.try_apply_reaction(entity, w, effect_key) {
                            Ok(_) => println!("REACTION STATUS - SUCESS !"),
                            Err(msg) => println!("REACTION STATUS - FAILED : {:?}", msg),
                        }
                        w.entity_mut(reaction_entity).insert(reaction);
                    });
                }
            }
        },
    );
}
