use bevy_ecs::{
    component::Component,
    message::MessageReader,
    system::{Commands, Query},
};
use bevy_gauge::{instant, prelude::AttributesMut};

use crate::{game_flow::turns::CurrentDeckReference, grid_abilities_backend::GridGoOff};

#[derive(Component)]
pub struct DamageEffect(pub f32);

pub fn damage_effect_system(
    mut reader: MessageReader<GridGoOff>,
    q_effect: Query<&DamageEffect>,
    mut attributes_list: AttributesMut,
    curr_deck_refs_q: Query<&CurrentDeckReference>,
) {
    for go_off in reader.read() {
        let effect_entity = go_off.entity;
        let Ok(damage) = q_effect.get(effect_entity) else {
            continue;
        };
        let Some(target_entity) = go_off.target.entity else {
            continue;
        };

        match curr_deck_refs_q.get(target_entity) {
            Ok(target_deck_ref) => {
                attributes_list.add_modifier(target_deck_ref.0, "SoulLife.current", damage.0)
            }
            Err(_) => {}
        }
    }
}
