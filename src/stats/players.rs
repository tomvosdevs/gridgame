use bevy_ecs::prelude::*;
use bevy_gauge::prelude::*;

// === BASE STATS

#[derive(Component, Default, AttributeComponent)]
pub struct Speed {
    #[read("Speed")]
    #[write("Speed")]
    pub current: i32,
}

impl Speed {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct Strength {
    #[read("Strength")]
    #[write("Strength")]
    pub current: i32,
}

impl Strength {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct MeleeRange {
    #[read("MeleeRange")]
    #[write("MeleeRange")]
    pub current: i32,
}

impl MeleeRange {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}

#[derive(Component, Default, AttributeComponent)]
pub struct RangedRange {
    #[read("RangedRange")]
    #[write("RangedRange")]
    pub current: i32,
}

impl RangedRange {
    pub fn new(val: i32) -> Self {
        Self { current: val }
    }
}
