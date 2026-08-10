use bevy::math::UVec3;
use bevy_ecs::system::Single;

pub trait IntoVec<T> {
    fn into_vec(self) -> Vec<T>;
}

impl<T: Sized> IntoVec<T> for T {
    fn into_vec(self) -> Vec<T> {
        vec![self]
    }
}

impl<T: Sized> IntoVec<T> for Vec<T> {
    fn into_vec(self) -> Vec<T> {
        self
    }
}
