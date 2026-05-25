#[derive(Component)]
#[relationship_target(relationship = StatusEffectOf)]
pub struct StatusEffects(Vec<Entity>);

#[derive(Component)]
#[relationship(relationship_target = StatusEffects)]
pub struct StatusEffectOf(pub Entity);

#[derive(Component)]
pub struct StatusTimer(Timer);

#[derive(Component)]
pub struct Poison;

fn poison(duration: f32) -> impl Bundle {
    (
        Poison,
        StatusTimer(Timer::new(
            Duration::from_secs_f32(duration),
            TimerMode::Once,
        )),
        observe(poison_effect), // Note: requires `bevy_ui_widgets` feature
    )
}

fn poison_effect(
    tick: On<Tick>,
    effect: Query<&StatusEffectOf>,
    mut health: Query<&mut Health>,
) -> Result {
    let StatusEffectOf(character) = effect.get(tick.status)?;
    let mut health = health.get_mut(character)?;

    health.0 -= 10;

    Ok(())
}

#[derive(Component)]
pub struct Healing;

fn healing(duration: f32) -> impl Bundle {
    (
        Healing,
        StatusTimer(Timer::new(
            Duration::from_secs_f32(duration),
            TimerMode::Once,
        )),
        observe(heal), // Note: requires `bevy_ui_widgets` feature
    )
}

fn heal(effect: Query<&StatusEffectOf>, mut health: Query<&mut Health>) -> Result {
    let StatusEffectOf(character) = effect.get(tick.status)?;
    let mut health = health.get_mut(character)?;

    health.0 += 10;

    Ok(())
}

#[derive(EntityEvent)]
pub struct Tick {
    #[event_target]
    status: Entity,
}

fn tick_status_timers(
    mut timers: Query<&mut StatusTimers>,
    mut commands: Commands,
    time: Res<Time>,
) {
    for (entity, StatusTimer(timer)) in &mut timers {
        timer.tick(time.delta_seconds());

        if timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

fn tick_status(statuses: Query<Entity, With<StatusEffectOf>>, mut commands: Commands) {
    for status in statuses {
        commands.trigger(Tick { status });
    }
}
//

fn tick_status(
    statuses: Query<(Entity, &mut TickEffectTimer), With<StatusEffectOf>>,
    mut commands: Commands,
    time: Res<Time>,
) {
    for (status, timer) in statuses {
        timer.tick(time.delta_seconds());
        if timer.is_finished() {
            commands.trigger(Tick { status });
        }
    }
}

//
//
// One shot system id's as entities / component
//
//
//
#[derive(Component)]
pub struct TickOnHit;

#[derive(EntityEvent)]
pub struct TriggerEffect<C: EntityEvent + Clone> {
    entity: Entity,
    cause: C,
}

fn explode_on_hit_effect() -> impl Bundle {
    (
        TickOnHit,
        observer(|hit: On<TriggerEffect<Hit>>, mut commands: Commands| {
            commands.entity(hit.entity).trigger(Explode);
        }),
    )
}

fn player() -> impl Bundle {
    (Player, related!(StatusEffects[explode_on_hit_effect()]))
}

fn trigger_status_on_hit(
    hit: On<Hit>,
    mut commands: Commands,
    status_effects: Query<&StatusEffects>,
    effects: Query<Entity, With<TriggerOnHit>>,
) -> Result {
    let status_effects = status_effects.get(hit.entity)?;

    for effect in effects.iter_mant(&status_effects) {
        commands.trigger(TriggerEffect {
            entity: effect,
            cause: hit.event().clone(),
        });
    }

    Ok(())
}

//
//
//
//
#[derive(Component)]
#[relationship_target(relationship = StatusEffectOf)]
pub struct StatusEffects(Vec<Entity>);

#[derive(Component)]
#[relationship(relationship_target = StatusEffects)]
pub struct StatusEffectOf(pub Entity);

#[derive(EntityEvent)]
#[entity_event(propagate = &'static StatusEffects, auto_propogate)]
pub struct Hit {
    entity: Entity,
}

fn explode_on_hit_effect() -> impl Bundle {
    (observer(|hit: On<Hit>, mut commands: Commands| {
        commands.entity(hit.entity).trigger(Explode);
    }))
}

fn player() -> impl Bundle {
    (Player, related!(StatusEffects[explode_on_hit_effect()]))
}
