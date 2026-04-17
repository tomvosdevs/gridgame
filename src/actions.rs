use std::{any::Any, marker::PhantomData, time::Duration};

use bevy::{
    app::{Plugin, Startup},
    asset::Assets,
    camera::{Camera, primitives::Aabb},
    color::Srgba,
    ecs::{
        component::Component,
        entity::Entity,
        event::EntityEvent,
        message::MessageWriter,
        name::Name,
        observer::On,
        query::{With, Without},
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, EntityCommand, EntityCommands, Query, Res, ResMut, Single},
        world::{DeferredWorld, World},
    },
    math::{Quat, Vec2, Vec3Swizzles, VectorSpace, bool},
    mesh::Mesh3d,
    pbr::{ExtendedMaterial, MeshMaterial3d, StandardMaterial},
    picking::{
        Pickable,
        events::{Move, Out, Over, Pointer},
    },
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::{prelude::InvokedBy, spawn::TemplateRegistry};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_tween::{
    combinator::TransformTargetState,
    prelude::{AnimationBuilderExt, EaseKind, TransformTargetStateExt},
    tween::{AssetTween, IntoTarget, TargetAsset},
};

use crate::{
    ActiveCamera, GridCell, InterpolateSkew, NODE_SIZE, SkewMaterial,
    deck_and_cards::{Card, TemplateCard},
    grid_abilities_backend::{GridInvokerTarget, GridStartInvoke, GridTarget},
    states::{CurrentPlayingEntity, PlayingEntity},
    tiles_templates::Targetable,
    ui::{CardUiTargetMesh, DraggedCard},
};

pub struct ActionPlugin;

impl Plugin for ActionPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(HighlightedTarget {
            target_entity: None,
            highlighter_entity: None,
        })
        .add_systems(Startup, (setup_reactions, setup_actions_observers).chain())
        .add_observer(handle_card_release)
        .add_observer(handle_targetable_mouseover_check)
        .add_observer(handle_targetable_mousemove_check)
        .add_observer(handle_targetable_mouseout_check);
    }
}

#[derive(Resource)]
pub struct HighlightedTarget {
    pub target_entity: Option<Entity>,
    pub highlighter_entity: Option<Entity>,
}

impl HighlightedTarget {
    pub fn highlight_at_position(
        self: &mut Self,
        cmd: &mut Commands,
        target_tf: &GlobalTransform,
        target_mesh: &Mesh3d,
        target_entity: Entity,
        mut materials: ResMut<Assets<StandardMaterial>>,
    ) {
        self.target_entity = Some(target_entity);
        let mat = StandardMaterial::from_color(Srgba::new(1.0, 1.0, 1.0, 0.6));
        let mat_handle = materials.add(mat);
        let highlighter_ent = cmd
            .spawn((
                target_mesh.clone(),
                MeshMaterial3d(mat_handle),
                Pickable::IGNORE,
                Transform::from_translation(target_tf.translation())
                    .with_scale(target_tf.scale() * 1.2),
            ))
            .id();

        self.highlighter_entity = Some(highlighter_ent);
    }

    pub fn reset(self: &mut Self) {
        self.highlighter_entity = None;
        self.target_entity = None;
    }

    pub fn reset_and_remove_highlighter(self: &mut Self, cmd: &mut Commands) {
        if let Some(ent) = self.highlighter_entity {
            cmd.entity(ent).despawn();
        }
        self.highlighter_entity = None;
        self.target_entity = None;
    }
}

#[derive(Component, Clone)]
pub struct CardAnimatedBy {
    pub skew_animator_entity: Entity,
    pub rotation_animator_entity: Entity,
    pub skew_target: TargetAsset<ExtendedMaterial<StandardMaterial, SkewMaterial>>,
    pub duration: Duration,
    pub ease: EaseKind,
    pub start_rotation: Quat,
    pub needs_rotation_setup: bool,
}

impl CardAnimatedBy {
    pub fn new(
        skew_animator_entity: Entity,
        rotation_animator_entity: Entity,
        skew_target: TargetAsset<ExtendedMaterial<StandardMaterial, SkewMaterial>>,
        duration: Duration,
        ease: EaseKind,
        start_rotation: Quat,
    ) -> Self {
        Self {
            skew_animator_entity,
            rotation_animator_entity,
            skew_target,
            duration,
            ease,
            start_rotation,
            needs_rotation_setup: true,
        }
    }
}

pub fn handle_targetable_mouseover_check(
    e: On<Pointer<Over>>,
    mut cmd: Commands,
    targetables_q: Query<(Entity, &GlobalTransform, &Mesh3d), With<Targetable>>,
    dragged_card: Res<DraggedCard>,
    card_animated_by_q: Query<&CardAnimatedBy, With<CardUiTargetMesh>>,
    skew_material_q: Query<
        &MeshMaterial3d<ExtendedMaterial<StandardMaterial, SkewMaterial>>,
        With<CardUiTargetMesh>,
    >,
    cards_q: Query<&Transform, With<CardUiTargetMesh>>,
    mut highlighted_target: ResMut<HighlightedTarget>,
    materials: ResMut<Assets<StandardMaterial>>,
    skew_materials: Res<Assets<ExtendedMaterial<StandardMaterial, SkewMaterial>>>,
) {
    // ==> Handle highlight
    let Ok((target_ent, target_global_tf, target_mesh)) = targetables_q.get(e.entity) else {
        return;
    };
    let Some(card_entity) = dragged_card.entity else {
        return;
    };

    highlighted_target.highlight_at_position(
        &mut cmd,
        target_global_tf,
        target_mesh,
        target_ent,
        materials,
    );

    let Ok(card_material) = skew_material_q.get(card_entity) else {
        return;
    };

    let Ok(card_tf) = cards_q.get(card_entity) else {
        return;
    };

    // If the animator is already setup, we can just modify the existing animation
    let (skew_animator_entity, rotation_animator_entity, animated_by_spawned, needs_rotation_setup) =
        match card_animated_by_q.get(card_entity) {
            Ok(animated_by) => (
                animated_by.skew_animator_entity,
                animated_by.rotation_animator_entity,
                true,
                animated_by.needs_rotation_setup,
            ),
            Err(_) => (cmd.spawn_empty().id(), cmd.spawn_empty().id(), false, true),
        };

    // ==> Handle tween setup (only called first time, or should only lol)

    let asset_target = TargetAsset::Asset(card_material.0.clone());
    let skew_tween = {
        let current_skew = skew_materials
            .get(card_material.id())
            .expect("could not find skew material")
            .clone()
            .extension
            .skew_amount;

        let tween_start = current_skew;

        let tween_end = 0.2;

        let tween_interpolator = InterpolateSkew {
            start: tween_start,
            end: tween_end,
        };

        let mut tween = AssetTween::new(tween_interpolator);
        tween.target = asset_target.clone();
        tween
    };

    let duration = Duration::from_millis(200);
    let ease = EaseKind::ExponentialOut;
    let skew_animation_handler_entity = cmd
        .entity(skew_animator_entity)
        .animation()
        .insert_tween_here(duration, ease, skew_tween)
        .id();

    if !needs_rotation_setup {
        return;
    }

    let base_rotation = card_tf.rotation.clone();

    let rotation_tween = {
        let target = card_entity.into_target();
        let mut tween_start = target.transform_state(*card_tf);

        let new_rotation = base_rotation * Quat::from_rotation_x(-20.0_f32.to_radians());
        tween_start.rotation_to(new_rotation)
    };

    let rotation_animation_handler_entity = cmd
        .entity(rotation_animator_entity)
        .animation()
        .insert_tween_here(duration, ease, rotation_tween)
        .id();

    if animated_by_spawned {
        return;
    }

    cmd.entity(card_entity).insert(CardAnimatedBy::new(
        skew_animation_handler_entity,
        rotation_animation_handler_entity,
        asset_target,
        duration,
        ease,
        base_rotation.clone(),
    ));
}

pub fn handle_targetable_mousemove_check(
    e: On<Pointer<Move>>,
    mut cmd: Commands,
    targetables_q: Query<&GlobalTransform, With<Targetable>>,
    dragged_card: Res<DraggedCard>,
    card_animated_by_q: Query<&CardAnimatedBy, With<CardUiTargetMesh>>,
    cards_q: Query<(&GlobalTransform, &Transform, &Aabb), With<CardUiTargetMesh>>,
    main_cam_q: Query<(&GlobalTransform, &Camera), With<ActiveCamera>>,
) {
    // ==> Handle highlight
    let Ok(target_global_tf) = targetables_q.get(e.entity) else {
        return;
    };
    let Some(card_entity) = dragged_card.entity else {
        return;
    };

    let Ok((card_global_tf, card_tf, card_aabb)) = cards_q.get(card_entity) else {
        return;
    };

    let (cam_global_tf, cam) = main_cam_q.single().expect("found more than one active cam");

    // If the animator is already setup, we can just modify the existing animation
    let Ok(animated_by) = card_animated_by_q.get(card_entity) else {
        return;
    };

    let target_center = target_global_tf.translation();
    let card_top_center =
        card_global_tf.translation() + card_aabb.half_extents.to_vec3().with_xz(Vec2::ZERO);

    let target_center_vp = cam
        .world_to_viewport(cam_global_tf, target_center)
        .expect("could not find targetable mesh's center in cam vp");
    let card_top_center_vp = cam
        .world_to_viewport(cam_global_tf, card_top_center)
        .expect("could not find card mesh's center in cam vp");

    // offset from card to the target center on X axis in range -1.0..1.0
    let x_offset_scalar = (target_center_vp - card_top_center_vp).normalize().x;
    println!(
        "offset scalar :{:?} - val : {:?}",
        x_offset_scalar,
        (x_offset_scalar * 40.0)
    );

    let rotation_tween = {
        let target = card_entity.into_target();
        let mut tween_start = target.transform_state(*card_tf);

        let max_rot_degs = 55.0_f32.to_radians();
        let new_rotation = animated_by.start_rotation
            * Quat::from_rotation_x(-20.0_f32.to_radians())
            * Quat::from_rotation_y(max_rot_degs * x_offset_scalar);
        tween_start.rotation_to(new_rotation)
    };

    cmd.entity(animated_by.rotation_animator_entity)
        .animation()
        .insert_tween_here(animated_by.duration, animated_by.ease, rotation_tween);
}

#[derive(EntityEvent)]
pub struct CardReleased {
    pub entity: Entity,
    pub selected_target: Option<Entity>,
}

fn animate_card_to_initial(
    cmd: &mut Commands,
    animated_by: &CardAnimatedBy,
    rotation_from: TransformTargetState,
    skew_from: f32,
) {
    let tween_end = 0.0;

    let tween_interpolator = InterpolateSkew {
        start: skew_from,
        end: tween_end,
    };

    let mut skew_tween = AssetTween::new(tween_interpolator);
    skew_tween.target = animated_by.skew_target.clone();

    let rotation_tween = {
        let mut tween_start = rotation_from;
        tween_start.rotation_to(animated_by.start_rotation)
    };

    cmd.entity(animated_by.skew_animator_entity)
        .animation()
        .insert_tween_here(animated_by.duration, animated_by.ease, skew_tween);

    cmd.entity(animated_by.rotation_animator_entity)
        .animation()
        .insert_tween_here(animated_by.duration, animated_by.ease, rotation_tween);
}

pub fn handle_card_release(
    e: On<CardReleased>,
    mut cmd: Commands,
    ui_cards_q: Query<(&Transform, &CardAnimatedBy, &CardUiTargetMesh)>,
    cards_q: Query<&Card, Without<TemplateCard>>,
    skew_materials: Res<Assets<ExtendedMaterial<StandardMaterial, SkewMaterial>>>,
    skew_material_q: Query<
        &MeshMaterial3d<ExtendedMaterial<StandardMaterial, SkewMaterial>>,
        With<CardUiTargetMesh>,
    >,
    abilities: Res<TemplateRegistry>,
    currently_playing: Res<CurrentPlayingEntity>,
    mut writer: MessageWriter<GridStartInvoke>,
    cells_q: Query<&GridNode, With<GridCell>>,
    playing_q: Query<&CartesianPosition, With<PlayingEntity>>,
    grid: Single<&mut CartesianGrid<Cartesian3D>>,
) {
    let ui_card_entity = e.entity;

    let Ok((card_tf, animated_by, ui_card_data)) = ui_cards_q.get(ui_card_entity) else {
        return;
    };

    let Ok(card) = cards_q.get(ui_card_data.source_card) else {
        return;
    };

    if let Some(target_entity) = e.selected_target {
        println!("in handle card release : {:?}", card.ability);
        let ability = abilities
            .get(card.ability)
            .expect("ability name should be valid and inside registry");
        let ability_instance_entity = ability(&mut cmd, None);
        cmd.entity(ability_instance_entity)
            .insert(InvokedBy(currently_playing.0));

        let position = cells_q.get(target_entity).map_or_else(
            |_| {
                *playing_q
                    .get(target_entity)
                    .expect("Target should be a GridCell or PlayingEntity")
            },
            |node| grid.pos_from_index(node.0),
        );
        println!("spawning ability to grid pos : {:?}", position);
        let target = GridTarget::entity(target_entity, position);
        cmd.entity(currently_playing.0)
            .insert(GridInvokerTarget::entity(target_entity, position));

        writer.write(GridStartInvoke::new(ability_instance_entity, target));

        cmd.entity(animated_by.rotation_animator_entity)
            .despawn_children();
        cmd.entity(animated_by.rotation_animator_entity).despawn();
        cmd.entity(ui_card_entity).despawn_children();
        cmd.entity(ui_card_entity).despawn();

        return;
    }

    let Ok(card_material) = skew_material_q.get(ui_card_entity) else {
        return;
    };

    let current_skew = skew_materials
        .get(card_material.id())
        .expect("could not find skew material")
        .clone()
        .extension
        .skew_amount;

    let target = ui_card_entity.into_target();
    let current_rotation = target.transform_state(*card_tf);

    let mut animated_by = animated_by.clone();
    animated_by.duration = Duration::from_millis(400);
    animated_by.ease = EaseKind::CircularOut;
    animate_card_to_initial(&mut cmd, &animated_by, current_rotation, current_skew);
}

pub fn handle_targetable_mouseout_check(
    e: On<Pointer<Out>>,
    mut cmd: Commands,
    dragged_card: Res<DraggedCard>,
    cards_q: Query<&Transform, With<CardUiTargetMesh>>,
    card_animated_by_q: Query<&CardAnimatedBy, With<CardUiTargetMesh>>,
    mut highlighted_target: ResMut<HighlightedTarget>,
    skew_materials: Res<Assets<ExtendedMaterial<StandardMaterial, SkewMaterial>>>,
    skew_material_q: Query<
        &MeshMaterial3d<ExtendedMaterial<StandardMaterial, SkewMaterial>>,
        With<CardUiTargetMesh>,
    >,
) {
    let Some(highlighter_ent) = highlighted_target.highlighter_entity else {
        return;
    };

    let Some(target_ent) = highlighted_target.target_entity else {
        return;
    };

    if e.entity != target_ent {
        return;
    }

    highlighted_target.reset();
    {
        cmd.entity(highlighter_ent).despawn();
    }

    let Some(card_entity) = dragged_card.entity else {
        return;
    };

    let Ok(card_tf) = cards_q.get(card_entity) else {
        return;
    };

    let Ok(card_material) = skew_material_q.get(card_entity) else {
        return;
    };

    // Already being tweened on rotation, we ignore it
    let Ok(animated_by) = card_animated_by_q.get(card_entity) else {
        return;
    };

    let current_skew = skew_materials
        .get(card_material.id())
        .expect("could not find skew material")
        .clone()
        .extension
        .skew_amount;

    let tween_end = 0.0;

    let tween_interpolator = InterpolateSkew {
        start: current_skew,
        end: tween_end,
    };

    let mut skew_tween = AssetTween::new(tween_interpolator);
    skew_tween.target = animated_by.skew_target.clone();

    let rotation_tween = {
        let target = card_entity.into_target();
        let mut tween_start = target.transform_state(*card_tf);

        tween_start.rotation_to(animated_by.start_rotation)
    };

    cmd.entity(animated_by.skew_animator_entity)
        .animation()
        .insert_tween_here(animated_by.duration, animated_by.ease, skew_tween);

    cmd.entity(animated_by.rotation_animator_entity)
        .animation()
        .insert_tween_here(animated_by.duration, animated_by.ease, rotation_tween);
}

pub struct Confusion;
#[derive(Component, Debug, Default)]
pub struct Range(pub i32);

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
                let curr_pos = grid.pos_from_index(curr_node.clone());
                // TODO - ERR - Currently this crashes if used on a Y = grid height cell
                let new_pos = grid
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
