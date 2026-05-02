use std::time::Duration;

use bevy::{
    app::Plugin,
    asset::Assets,
    camera::{Camera, primitives::Aabb},
    color::Srgba,
    ecs::{
        component::Component,
        entity::Entity,
        event::EntityEvent,
        message::MessageWriter,
        observer::On,
        query::{With, Without},
        resource::Resource,
        system::{Commands, Query, Res, ResMut, Single},
    },
    math::{Quat, Vec2, Vec3Swizzles, bool},
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
    ActiveCamera, GridCell, InterpolateSkew, SkewMaterial,
    deck::deck_and_cards::Card,
    game_flow::turns::{CurrentPlayingEntity, PlayingEntity},
    grid_abilities_backend::{GridInvokerTarget, GridStartInvoke, GridTarget},
    tiles_templates::Targetable,
    ui::{CardUiTargetMesh, DraggedCard},
};

pub struct DiegeticCardTweenPlugin;

impl Plugin for DiegeticCardTweenPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.insert_resource(HighlightedTarget {
            target_entity: None,
            highlighter_entity: None,
        })
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
    cards_q: Query<&Card>,
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
    println!("received release");
    let ui_card_entity = e.entity;

    let (card_tf, animated_by, ui_card_data) = ui_cards_q
        .get(ui_card_entity)
        .expect("should have found ui entity comps");

    let card = cards_q
        .get(ui_card_data.source_card)
        .expect("CardUI datat's source_card should be a valid Card");

    if let Some(target_entity) = e.selected_target {
        cmd.entity(card.ability_entity)
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

        writer.write(GridStartInvoke::new(card.ability_entity, target));

        cmd.entity(animated_by.rotation_animator_entity).despawn();
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
