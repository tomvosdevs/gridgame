use std::{f32::consts::TAU, i32, ops::Deref};

use bevy::{
    app::{Plugin, Update},
    ecs::{
        entity::Entity,
        query::{With, Without},
        system::{Local, Query, Single, SystemParam},
    },
    math::{Dir3, I16Vec3, ShapeSample, U16Vec3, UVec3, Vec2, Vec3, primitives::Sphere},
    prelude::IntoScheduleConfigs,
    transform::components::{GlobalTransform, Transform},
};
use bevy_diesel::{pipeline::propagate_system, prelude::SpatialBackend, target::Target};
use bevy_gauge::{AttributeResolvable, prelude::AttributesAppExt};
use bevy_ghx_grid::ghx_grid::cartesian::{
    coordinates::{Cartesian3D, CartesianPosition},
    grid::CartesianGrid,
};
use bevy_ghx_proc_gen::GridNode;
use bevy_prng::WyRand;
use bevy_rand::{
    prelude::GlobalRng,
    traits::{ForkableAsRng, ForkableRng},
};
use rand::{Rng, RngExt, SeedableRng};

use crate::{
    GridCell,
    states::{FromGrid, PlayingEntity, ToWorldPos},
};

pub struct Grid3dDieselPlugin;

impl Plugin for Grid3dDieselPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins(Grid3DBackend::plugin_core());

        use bevy_diesel::bevy_gauge::prelude::AttributesAppExt;
        app.register_attribute_derived::<bevy_diesel::spawn::SpawnConfig<Grid3DBackend>>();
        app.register_attribute_derived::<bevy_diesel::target::TargetMutator<Grid3DBackend>>();

        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::effect::go_off_on_entry::<Grid3DBackend>,
                propagate_system::<Grid3DBackend>,
            )
                .chain()
                .in_set(bevy_diesel::DieselSet::Propagation),
        );

        // Leaf effect systems: read GoOff
        app.add_systems(
            bevy_diesel::bevy_gearbox::GearboxSchedule,
            (
                bevy_diesel::spawn::spawn_system::<Grid3DBackend>,
                bevy_diesel::print::print_effect::<<Grid3DBackend as SpatialBackend>::Pos>,
            )
                .in_set(bevy_diesel::DieselSet::Effects),
        );

        // Sustained modifier apply — monomorphized here because the generic
        // fn needs B::Context which can only resolve with a concrete backend.
        app.add_systems(
            Update,
            bevy_diesel::gauge::modifiers::sustained_modifier_apply::<Grid3DBackend>
                .in_set(bevy_diesel::gauge::SustainedModifierSet),
        );

        // #TODO: Will need to do something similar
        // // Collision types + system (unfiltered - entities with Collides marker)
        // app.register_transition::<collision::CollidedEntity>();
        // app.register_side_effect::<collision::CollidedEntity, bevy_diesel::effect::GoOffOrigin<Vec3>>();
        // app.register_transition::<collision::CollidedPosition>();
        // app.register_side_effect::<collision::CollidedPosition, bevy_diesel::effect::GoOffOrigin<Vec3>>();
        // collision::plugin(app);
    }
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum NumberType {
    All,
    Fixed(usize),
    Random { min: usize, max: usize },
}

impl Default for NumberType {
    fn default() -> Self {
        Self::All
    }
}

impl NumberType {
    /// Resolve to a concrete count. Panics on `All`. Use only for gatherers
    /// where a count is always required.
    pub fn resolve_count(&self, mut rng: WyRand) -> usize {
        match self {
            NumberType::All => panic!("NumberType::All has no concrete count"),
            NumberType::Fixed(n) => *n,
            NumberType::Random { min, max } => {
                if min >= max {
                    return *min;
                }
                let range = max - min + 1;
                let r = (rng.next_u64() as usize) % range;
                min + r
            }
        }
    }

    /// Resolve to a concrete count, or `None` for unlimited.
    fn resolve_limit(&self, rng: WyRand) -> Option<usize> {
        match self {
            NumberType::All => None,
            _ => Some(self.resolve_count(rng)),
        }
    }
}

#[derive(Debug, Clone, AttributeResolvable, PartialEq, Eq)]
pub enum EntityGatheringFilter {
    All,
    Playing,
    Cells,
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum GridCheckShape {
    Circle(f32),
    Sphere(f32),
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum Grid3DGatherer {
    // Position generators - produce N random points around origin
    Sphere {
        radius: f32,
        count: NumberType,
    },
    Circle {
        radius: f32,
        count: NumberType,
    },
    // Box {
    //     #[skip]
    //     half_extents: CartesianPosition,
    //     count: NumberType,
    // },
    // Line {
    //     #[skip]
    //     direction: Dir3,
    //     length: f32,
    //     count: NumberType,
    // },
    /// All entities in an shape.
    EntitiesInShape {
        shape: GridCheckShape,
        gathering_filter: EntityGatheringFilter,
        sort_by_nearest: bool,
    },
}

#[derive(Clone, Debug, AttributeResolvable)]
pub struct Grid3DFilter {
    /// Max target count. `NumberType::All` passes everything through.
    pub count: NumberType,
    /// Require line-of-sight (TODO).
    #[skip]
    pub line_of_sight: bool,
}

impl Default for Grid3DFilter {
    fn default() -> Self {
        Self {
            count: NumberType::All,
            line_of_sight: false,
        }
    }
}

#[derive(SystemParam)]
pub struct Grid3DContext<'w, 's> {
    pub grid: Single<'w, 's, &'static CartesianGrid<Cartesian3D>>,
    pub grid_tf: Single<'w, 's, &'static Transform, With<CartesianGrid<Cartesian3D>>>,
    pub transforms: Query<'w, 's, &'static Transform, Without<CartesianGrid<Cartesian3D>>>,
    pub grid_cells:
        Query<'w, 's, (Entity, &'static GridNode), (With<GridCell>, Without<PlayingEntity>)>,
    pub playing: Query<'w, 's, (Entity, &'static CartesianPosition), With<PlayingEntity>>,
    global_transforms: Query<'w, 's, &'static GlobalTransform>,
    rng: Single<'w, 's, &'static mut WyRand, With<GlobalRng>>,
}

fn rand_u32(mut rng: Single<&mut WyRand, With<GlobalRng>>) -> u32 {
    rng.next_u32() / u32::MAX
}

fn rand_u32_range(rng: Single<&mut WyRand, With<GlobalRng>>, min: u32, max: u32) -> u32 {
    min + rand_u32(rng) * (max - min)
}

pub enum SmoothingShape {
    Circle(u32),
    // Diamond,
    // RoundedSquare,
    // Square,
    // Superellipse(i32), // custom n exponent
    Annulus { inner: f32, outer: f32 },
    Gaussian { std_dev_ratio: f32, radius: f32 }, // ratio of r, e.g. 0.33 = r/3
}

impl SmoothingShape {
    fn exponent(&self) -> Option<i32> {
        match self {
            SmoothingShape::Circle(_) => Some(2),
            // SmoothingShape::Diamond => Some(1),
            // SmoothingShape::RoundedSquare => Some(4),
            // SmoothingShape::Square => Some(i32::MAX),
            // SmoothingShape::Superellipse(n) => Some(*n),
            _ => None,
        }
    }
}

pub fn random_in_shape(mut rng: WyRand, shape: &SmoothingShape) -> (i32, i32) {
    match shape {
        SmoothingShape::Circle(r) => {
            let angle = rng.random_range(0.0..TAU);
            let radius = (*r as f32) * rng.random::<f32>().sqrt();
            ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
        }

        SmoothingShape::Annulus { inner, outer } => {
            let angle = rng.random_range(0.0..TAU);
            let t = rng.random::<f32>();
            let radius: f32 = (inner * inner + t * (outer * outer - inner * inner)).sqrt();
            ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
        }

        SmoothingShape::Gaussian {
            std_dev_ratio,
            radius,
        } => {
            let std_dev = radius * std_dev_ratio;
            let u1: f32 = rng.random();
            let u2: f32 = rng.random();
            let mag = std_dev * (-2.0 * u1.ln()).sqrt();
            (
                (mag * (TAU * u2).cos()) as i32,
                (mag * (TAU * u2).sin()) as i32,
            )
        }
    }
}

// pub fn random_in_shape_specific(
//     mut rng: Single<&mut WyRand, With<GlobalRng>>,
//     shape: &SmoothingShape,
//     rad: f32,
// ) -> (i32, i32) {
//     match shape {
//         SmoothingShape::Circle(r) => {
//             let angle = rng.random_range(0.0..TAU);
//             let radius = r * rng.random::<f32>().sqrt();
//             ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
//         }

//         SmoothingShape::Annulus { inner, outer } => {
//             let angle = rng.random_range(0.0..TAU);
//             let t = rng.random::<f32>();
//             let radius: f32 = (inner * inner + t * (outer * outer - inner * inner)).sqrt();
//             ((radius * angle.cos()) as i32, (radius * angle.sin()) as i32)
//         }

//         SmoothingShape::Gaussian {
//             std_dev_ratio,
//             radius,
//         } => {
//             let std_dev = radius * std_dev_ratio;
//             let u1: f32 = rng.random();
//             let u2: f32 = rng.random();
//             let mag = std_dev * (-2.0 * u1.ln()).sqrt();
//             (
//                 (mag * (TAU * u2).cos()) as i32,
//                 (mag * (TAU * u2).sin()) as i32,
//             )
//         }

//         shape => {
//             let n = shape.exponent().unwrap();
//             let rf = rad;
//             if n == f32::INFINITY {
//                 let r = rad as i32;
//                 return (rng.random_range(-r..=r), rng.random_range(-r..=r));
//             }
//             let r_i = rad as i32;
//             loop {
//                 let x = rng.random_range(-r_i..=r_i);
//                 let y = rng.random_range(-r_i..=r_i);
//                 let xf = x as f32 / rf;
//                 let yf = y as f32 / rf;
//                 if xf.abs().powf(n) + yf.abs().powf(n) <= 1.0 {
//                     return (x, y);
//                 }
//             }
//         }
//     }
// }

#[derive(Clone, Debug, AttributeResolvable)]
pub struct GridDirectionOffset {
    #[skip]
    pub dir: Dir3,
    pub min_dist: i32,
    pub max_dist: i32,
}

#[derive(Clone, Debug, AttributeResolvable)]
pub struct GridFixedOffset {
    x: u32,
    y: u32,
    z: u32,
}

impl GridFixedOffset {
    pub fn new(x: u32, y: u32, z: u32) -> Self {
        Self { x, y, z }
    }

    pub fn from_cartesian_pos(pos: CartesianPosition) -> Self {
        Self {
            x: pos.x,
            y: pos.y,
            z: pos.z,
        }
    }
}

#[derive(Clone, Debug, AttributeResolvable)]
pub enum GridPosOffset {
    None,
    Fixed(GridFixedOffset),
    RandomInDir(GridDirectionOffset),
    RandomInCircle(u32),
    RandomInSphere(u32),
}

impl GridPosOffset {
    pub fn apply_to(
        self: &Self,
        target: CartesianPosition,
        mut rng: WyRand,
        grid: &CartesianGrid<Cartesian3D>,
    ) -> CartesianPosition {
        match self {
            GridPosOffset::None => target,
            GridPosOffset::Fixed(grid_fixed_offset) => CartesianPosition {
                x: target.x + grid_fixed_offset.x,
                y: target.y + grid_fixed_offset.y,
                z: target.z + grid_fixed_offset.z,
            }
            .grid_clamped(grid),
            GridPosOffset::RandomInDir(grid_direction_offset) => {
                let random_dist = rng
                    .random_range(grid_direction_offset.min_dist..grid_direction_offset.max_dist);

                let random_offset = random_dist as f32 * grid_direction_offset.dir;
                CartesianPosition {
                    x: (target.x as f32 + random_offset.x.trunc()).max(0.) as u32,
                    y: (target.y as f32 + random_offset.y.trunc()).max(0.) as u32,
                    z: (target.z as f32 + random_offset.z.trunc()).max(0.) as u32,
                }
            }
            GridPosOffset::RandomInCircle(radius) => {
                let (x, z) = random_in_shape(rng, &SmoothingShape::Circle(*radius));
                CartesianPosition {
                    x: (target.x as i32 + x).max(0) as u32,
                    y: target.y,
                    z: (target.z as i32 + z).max(0) as u32,
                }
                .grid_clamped(grid)
            }
            GridPosOffset::RandomInSphere(radius) => {
                let sphere = Sphere::new(*radius as f32);
                let rand_offset = sphere.sample_interior(&mut rng);
                CartesianPosition::new(
                    (target.x as f32 + rand_offset.x).max(0.) as u32,
                    (target.y as f32 + rand_offset.y).max(0.) as u32,
                    (target.z as f32 + rand_offset.z).max(0.) as u32,
                )
                .grid_clamped(grid)
            }
        }
    }
}

pub trait InGridBoundaries {
    fn grid_clamped(self: Self, grid: &CartesianGrid<Cartesian3D>) -> Self;
}

impl InGridBoundaries for CartesianPosition {
    fn grid_clamped(mut self: Self, grid: &CartesianGrid<Cartesian3D>) -> Self {
        self.x = self.x.clamp(0, grid.size_x());
        self.y = self.y.clamp(0, grid.size_y());
        self.z = self.z.clamp(0, grid.size_z());
        self
    }
}

impl Default for GridPosOffset {
    fn default() -> Self {
        GridPosOffset::None
    }
}

pub fn get_entity_targets_in_shape(
    origin: CartesianPosition,
    potential_targets: &mut Vec<(Entity, CartesianPosition)>,
    shape: &GridCheckShape,
    sort_by_nearest: bool,
) -> Vec<Target<CartesianPosition>> {
    let radius = match shape {
        GridCheckShape::Circle(r) => *r,
        GridCheckShape::Sphere(r) => *r,
    };

    let mut unsorted: Vec<(f32, Target<CartesianPosition>)> = potential_targets
        .iter()
        .filter_map(|(e, node_pos)| {
            match shape {
                GridCheckShape::Circle(_) => {
                    if node_pos.y != origin.y {
                        return None;
                    }
                }
                _ => {}
            }

            let distance = Vec3::new(node_pos.x as f32, node_pos.y as f32, node_pos.z as f32)
                .distance(Vec3::new(origin.x as f32, origin.y as f32, origin.z as f32));
            if distance <= radius {
                Some((distance, Target::entity(*e, *node_pos)))
            } else {
                None
            }
        })
        .collect();

    if !sort_by_nearest {
        return unsorted.iter().map(|(_, t)| *t).collect();
    }

    unsorted.sort_by(|(dist_a, _), (dist_b, _)| dist_a.total_cmp(&dist_b));
    unsorted.iter().map(|(_, t)| *t).collect()
}

pub struct Grid3DBackend;

impl SpatialBackend for Grid3DBackend {
    type Pos = CartesianPosition;

    type Offset = GridPosOffset;

    type Gatherer = Grid3DGatherer;

    type Filter = Grid3DFilter;

    type Context<'w, 's> = Grid3DContext<'w, 's>;

    fn apply_offset(
        ctx: &mut Self::Context<'_, '_>,
        pos: Self::Pos,
        offset: &Self::Offset,
    ) -> Self::Pos {
        offset.apply_to(pos, ctx.rng.fork(), &ctx.grid.deref())
    }

    fn distance(a: &Self::Pos, b: &Self::Pos) -> f32 {
        a.manhattan_distance(b) as f32
    }

    fn position_of(ctx: &Self::Context<'_, '_>, entity: Entity) -> Option<Self::Pos> {
        ctx.grid_cells
            .get(entity)
            .ok()
            .map(|c| ctx.grid.pos_from_index(c.1.0))
    }

    fn gather(
        ctx: &mut Self::Context<'_, '_>,
        origin: Self::Pos,
        gatherer: &Self::Gatherer,
        exclude: Entity,
    ) -> Vec<bevy_diesel::prelude::Target<Self::Pos>> {
        match gatherer {
            Grid3DGatherer::Sphere { radius, count } => {
                let rng = ctx.rng.fork();
                let n = count.resolve_count(rng.clone());
                (0..n)
                    .map(|_| {
                        let (rand_x, rand_z) =
                            random_in_shape(rng.clone(), &SmoothingShape::Circle(*radius as u32));
                        let pos = CartesianPosition::new(
                            (origin.x as i32 + rand_x) as u32,
                            origin.y,
                            (origin.z as i32 + rand_z) as u32,
                        );
                        Target::position(pos)
                    })
                    .collect()
            }
            Grid3DGatherer::Circle { radius, count } => {
                let rng = ctx.rng.fork();
                let n = count.resolve_count(rng.clone());
                (0..n)
                    .map(|_| {
                        let (rand_x, rand_z) =
                            random_in_shape(rng.clone(), &SmoothingShape::Circle(*radius as u32));
                        let pos = CartesianPosition::new(
                            (origin.x as i32 + rand_x) as u32,
                            origin.y,
                            (origin.z as i32 + rand_z) as u32,
                        );
                        Target::position(pos)
                    })
                    .collect()
            }
            // Grid3DGatherer::Box {
            //     half_extents,
            //     count,
            // } => todo!(),
            // Grid3DGatherer::Line {
            //     direction,
            //     length,
            //     count,
            // } => todo!(),
            Grid3DGatherer::EntitiesInShape {
                shape,
                gathering_filter,
                sort_by_nearest,
            } => {
                let grid = ctx.grid.deref();
                let mut found_entities: Vec<Target<Self::Pos>> = vec![];

                if *gathering_filter == EntityGatheringFilter::Cells
                    || *gathering_filter == EntityGatheringFilter::All
                {
                    let mut potential_targets: Vec<(Entity, CartesianPosition)> = ctx
                        .grid_cells
                        .iter()
                        .filter_map(|(e, grid_node)| {
                            if e == exclude {
                                return None;
                            }

                            let node_index = grid_node.0;
                            let node_pos = grid.pos_from_index(node_index);
                            Some((e, node_pos))
                        })
                        .collect();

                    let mut matching_targets = get_entity_targets_in_shape(
                        origin,
                        &mut potential_targets,
                        shape,
                        *sort_by_nearest,
                    );

                    found_entities.append(&mut matching_targets);
                }

                if *gathering_filter == EntityGatheringFilter::Playing
                    || *gathering_filter == EntityGatheringFilter::All
                {
                    let mut potential_targets: Vec<(Entity, CartesianPosition)> = ctx
                        .playing
                        .iter()
                        .filter_map(|(e, cartesian_pos)| {
                            if e == exclude {
                                return None;
                            }
                            Some((e, *cartesian_pos))
                        })
                        .collect();

                    let mut matching_targets = get_entity_targets_in_shape(
                        origin,
                        &mut potential_targets,
                        shape,
                        *sort_by_nearest,
                    );

                    found_entities.append(&mut matching_targets);
                }

                found_entities
            }
        }
    }

    fn apply_filter(
        ctx: &mut Self::Context<'_, '_>,
        targets: Vec<bevy_diesel::prelude::Target<Self::Pos>>,
        filter: &Self::Filter,
        invoker: bevy::ecs::entity::Entity,
        origin: Self::Pos,
    ) -> Vec<bevy_diesel::prelude::Target<Self::Pos>> {
        let resolved_count = match filter.count {
            NumberType::All => usize::MAX,
            NumberType::Fixed(n) => n,
            NumberType::Random { min, max } => ctx.rng.random_range(min..max),
        };

        targets.into_iter().take(resolved_count).collect()
    }

    fn insert_position(
        commands: &mut bevy::ecs::system::EntityCommands,
        ctx: &Self::Context<'_, '_>,
        pos: Self::Pos,
        parent: Option<bevy::ecs::entity::Entity>,
    ) {
        let world_pos = pos.as_world_pos(ctx.grid_tf.translation);

        let transform = if let Some(parent_entity) = parent {
            if let Ok(parent_gt) = ctx.global_transforms.get(parent_entity) {
                let local_pos = parent_gt.affine().inverse().transform_point3(world_pos);
                Transform::from_translation(local_pos)
            } else {
                Transform::from_translation(world_pos)
            }
        } else {
            Transform::from_translation(world_pos)
        };
        commands.insert(transform);
    }

    fn plugin() -> impl Plugin {
        Grid3dDieselPlugin
    }
}
