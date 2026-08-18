use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    ops::Deref,
    time::{Duration, SystemTime},
};

use bevy::{
    app::{App, FixedUpdate, Plugin, PluginGroup, Startup, Update},
    input::{ButtonInput, keyboard::KeyCode},
    math::{Rot2, curve::EaseFunction},
    prelude::{Deref, DerefMut},
    state::{
        app::AppExtStates,
        commands::CommandsStatesExt,
        condition::in_state,
        state::{OnEnter, OnExit, State, States},
    },
    ui::{Node, UiTransform},
    utils::default,
};
use bevy_ecs::{
    change_detection::DetectChanges,
    component::{Component, Mutable},
    entity::{Entity, MapEntities},
    error::BevyError,
    event::{EntityEvent, Event},
    hierarchy::ChildOf,
    lifecycle::{Add, Insert, Remove},
    message::MessageWriter,
    observer::On,
    query::{Changed, Or, With},
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Commands, Local, ParallelCommands, Query, Res, ResMut},
    world::Ref,
};

use bevy_flair::style::components::Styled;
use bevy_renet::{
    RenetClient, RenetServer,
    netcode::{
        ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
        ServerConfig,
    },
    renet::{Bytes, ConnectionConfig},
};
use bevy_replicon::{
    RepliconPlugins,
    client::UserdataReceived,
    postcard_utils,
    prelude::{
        AppMarkerExt, AppRuleExt, Channel, ClientEventAppExt, ClientId, ClientState,
        ClientTriggerExt, ConnectedClient, DisconnectRequest, FromClient, ProtocolHash,
        ProtocolMismatch, Replicated, ReplicationStorage, RepliconChannels, RepliconTick, RuleFns,
        SendTargets, ServerEventAppExt, ServerState, ServerTriggerExt, Signature,
        SyncRelatedAppExt, ToClients,
    },
    server::{AuthorizedClient, ReplicationUserdata, ServerPlugin, server_tick::ServerTick},
    shared::{
        AuthMethod, RepliconSharedPlugin,
        replication::{
            deferred_entity::DeferredEntity,
            receive_markers::MarkerConfig,
            registry::{
                ctx::{RemoveCtx, SerializeCtx, WriteCtx},
                rule_fns::{default_deserialize, default_serialize},
            },
        },
    },
};
use bevy_replicon_renet::{RenetChannelsExt, RepliconRenetPlugins};
use bevy_tweening::{AnimTarget, Tween, TweenAnim};
use moonshine_view::Viewable;
use serde::{Deserialize, Serialize};

use crate::{
    AnimMode, BattleData, BattleTick, BeingDrawn, BoardUtilsCommandsExt, CardCast, CardInPile,
    CardWidgetFor, CastTicksRequirement, DeckKind, DelayCompleted, Delayer, DrawPile, EnemyCard,
    EnemyData, HandPile, InDiscard, InDrawPile, InHand, Magnetic, MainSceneUiRoot, PlayerCard,
    PlayerData, PosInDeck, TurnAnimator, UiCardMarker, WorldPos, WorldPosLens,
    abilities::effects::{StatusEffectOf, StatusEffects},
    deck::deck_and_cards::{Card, CardPile},
    game_flow::turns::{
        BattleGlobalState, BattleTriggered, CardTicked, CheckClientBattleReady, ConfirmBattleReady,
        EnemyBoardMarker, PlayerBoardMarker, confirm_server_battle_ready,
        handle_client_confirm_battle_start,
    },
    update_subtick,
};

pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RepliconPlugins
                .build()
                .set(RepliconSharedPlugin {
                    auth_method: AuthMethod::Custom,
                })
                .set(ServerPlugin {
                    track_mutate_messages: true,
                    ..Default::default()
                }),
            RepliconRenetPlugins,
        ))
        .init_resource::<Cli>()
        .insert_resource(RepliconTickToBattleTick(HashMap::new()))
        .init_state::<GameState>()
        // .replicate::<CastTicksRequirement>()
        .replicate::<Card>()
        // .replicate::<CardPile>()
        // .replicate::<CardInPile>()
        .replicate::<Magnetic>()
        .replicate::<UiCardMarker>()
        // .replicate::<InHand>()
        // .replicate::<BeingDrawn>()
        // .replicate::<InDrawPile>()
        // .replicate::<InDiscard>()
        // .replicate::<PosInDeck>()
        .register_marker_with::<SaveHistory>(MarkerConfig {
            need_history: true,
            ..Default::default()
        })
        // .replicate::<SaveHistory>()
        .set_marker_fns::<SaveHistory, PosInDeck>(write_history, remove_history::<PosInDeck>)
        // .set_marker_fns::<SaveHistory, CastTicksRequirement>(
        //     write_history,
        //     remove_history::<CastTicksRequirement>,
        // )
        .replicate::<PlayerBoardMarker>()
        .replicate::<EnemyBoardMarker>()
        .replicate_once::<Node>()
        .replicate_once::<MainSceneUiRoot<PlayerData>>()
        .replicate_once::<MainSceneUiRoot<EnemyData>>()
        // .sync_related_entities::<CardInPile>()
        // .replicate::<StatusEffects>()
        // .replicate::<StatusEffectOf>()
        // .sync_related_entities::<StatusEffectOf>()
        // .replicate::<DrawPile>()
        // .replicate::<HandPile>()
        .replicate::<PlayerCard>()
        .replicate::<EnemyCard>()
        .sync_related_entities::<ChildOf>()
        .add_client_event::<ShareClientProtocol>(Channel::Ordered)
        .add_client_event::<ConfirmBattleReady>(Channel::Ordered)
        .add_server_event::<CheckClientBattleReady>(Channel::Ordered)
        .add_server_event::<ProtocolMismatch>(Channel::Unreliable)
        .make_event_independent::<ProtocolMismatch>()
        .add_server_event::<BattleTickingJustStarted>(Channel::Ordered)
        .add_observer(handle_sent_client_protocol)
        .add_systems(OnEnter(ClientState::Connected), client_start)
        .add_client_event::<IncrementReq>(Channel::Ordered)
        .add_systems(OnEnter(GameState::Disconnected), stop_networking)
        .add_systems(OnExit(ClientState::Connected), disconnect_by_server)
        .add_systems(
            OnEnter(GameState::InGame),
            game_started.run_if(in_state(ClientState::Connected)),
        )
        .add_observer(disconnect_by_client)
        .add_observer(init_client)
        .add_observer(apply_increment_req)
        .add_observer(handle_client_confirm_battle_start)
        .add_observer(confirm_server_battle_ready)
        .add_observer(user_data_received)
        .add_systems(Startup, setup_networking)
        .add_systems(
            OnEnter(ServerState::Running),
            |mut replication_storage: ResMut<ReplicationStorage>, mut cmd: Commands| {
                replication_storage
                    .global
                    .insert::<BattleData>(BattleData::NoBattle);

                let battle_data = BattleData::NoBattle;
                let mut message: Vec<u8> = Vec::new();
                postcard_utils::to_extend_mut(&battle_data, &mut message)
                    .expect("Could not serialize battle data");
                cmd.insert_resource(ReplicationUserdata(message));
            },
        )
        .add_systems(OnEnter(ClientState::Connected), |mut cmd: Commands| {
            cmd.insert_resource(ReplicationStorage::default());
        })
        .add_systems(
            Update,
            check_battle_tick_changed
                .after(update_subtick)
                .run_if(in_state(ServerState::Running)),
        )
        .add_systems(
            FixedUpdate,
            (
                check_inputs.run_if(in_state(ClientState::Connected)),
                log_shared_vals,
            ),
        )
        .add_observer(handle_card_changes)
        .add_mapped_server_event::<CardChangedPos>(Channel::Ordered)
        .add_mapped_server_event::<CardTicked>(Channel::Ordered)
        .add_mapped_server_event::<CardCast>(Channel::Ordered)
        .add_observer(
            |e: On<CardTicked>, mut cmd: Commands, s: Res<State<ClientState>>| {
                match s.get() {
                    ClientState::Connected => {}
                    _ => {
                        return;
                    }
                }

                let card = e.card.clone();
                let ticks_since_cast = e.ticks_since_cast;
                let cast_at = e.cast_at;

                cmd.spawn((
                    AnimInstructionsOf(e.card),
                    AnimTargetTick { turn: e.tick },
                    TurnAnimator { turn: e.tick.turn },
                ))
                .observe(
                    move |trig_e: On<TriggerAnim>,
                          q: Query<&Viewable<Card>>,
                          q_pos: Query<&WorldPos>,
                          mut obs_cmd: Commands| {
                        let view = q.get(card.clone()).unwrap().view().entity();

                        obs_cmd.entity(view).insert(CastData {
                            ticks_since_cast,
                            cast_at,
                        });

                        let curr_world_pos =
                            q_pos.get(view).expect("should find world pos on this");
                        let start_tf = curr_world_pos.transform;
                        let degs_offset = 22.0;
                        let duration = Duration::from_millis(140);

                        let tf_tween = Tween::new(
                            EaseFunction::SmoothStepIn,
                            duration / 2,
                            WorldPosLens {
                                pos: AnimMode::NoAnim,
                                tf: AnimMode::FromTo {
                                    start: UiTransform::from_rotation(start_tf.rotation),
                                    end: UiTransform::from_rotation(Rot2::degrees(degs_offset)),
                                },
                                translation: AnimMode::NoAnim,
                            },
                        )
                        .then(Tween::new(
                            EaseFunction::CubicOut,
                            duration / 2,
                            WorldPosLens {
                                pos: AnimMode::NoAnim,
                                tf: AnimMode::FromTo {
                                    start: UiTransform::from_rotation(Rot2::degrees(degs_offset)),
                                    end: UiTransform::from_rotation(Rot2::degrees(0.0)),
                                },
                                translation: AnimMode::NoAnim,
                            },
                        ));

                        let anim = TweenAnim::new(tf_tween);
                        let anim_target = AnimTarget::component::<WorldPos>(view);

                        obs_cmd.entity(trig_e.0).insert((anim, anim_target));
                    },
                );
            },
        )
        .add_observer(
            |e: On<CardChangedPos>, mut q: Query<&mut History<PosInDeck>>, mut cmd: Commands| {
                println!(
                    "received card pos change: {:?} for tick : {:?} - AND log comps after :",
                    e.new_pos, e.turn
                );

                match q.get_mut(e.card) {
                    Ok(mut history) => match history.changes.get_mut(&e.turn) {
                        Some(tick_change_list) => {
                            tick_change_list.push(e.new_pos);
                        }
                        None => {
                            history.changes.insert(e.turn, vec![e.new_pos]);
                        }
                    },
                    Err(_) => {
                        cmd.entity(e.card)
                            .insert(History::<PosInDeck>::from_initial_change(
                                e.new_pos, &e.turn,
                            ));
                    }
                }

                cmd.entity(e.card).log_components();
            },
        );
    }
}

#[derive(Component, Clone)]
pub struct CastData {
    pub ticks_since_cast: u32,
    pub cast_at: u32,
}

#[derive(EntityEvent, Clone)]
pub struct TriggerAnim(pub Entity);

#[derive(Component, Debug, Clone)]
#[relationship_target(relationship = AnimInstructionsOf, linked_spawn)]
pub struct AnimInstructions(Vec<Entity>);

#[derive(Component, Debug, Clone)]
#[relationship(relationship_target = AnimInstructions)]
pub struct AnimInstructionsOf(Entity);

#[derive(Component, Debug, Clone)]
pub struct AnimTargetTick {
    pub turn: BattleTick,
}

// Battle events
//

pub fn handle_card_changes(
    e: On<Insert, PosInDeck>,
    q: Query<(&PosInDeck, &CardInPile), With<Card>>,
    battle_data: Res<BattleData>,
    mut cmd: Commands,
) {
    let Some(battle_tick) = (match battle_data.into_inner() {
        BattleData::NoBattle => None,
        BattleData::InCombat { battle_tick } => Some(battle_tick),
    }) else {
        return;
    };

    let card = e.entity;
    let (curr_pos, card_in_pile) = q
        .get(card)
        .expect("PosInDeck was inserted so it should match this query, also should have a Card");

    let pile = card_in_pile.0;

    cmd.server_trigger(ToClients {
        targets: SendTargets::CLIENTS_ONLY,
        message: CardChangedPos {
            card,
            new_pos: *curr_pos,
            turn: *battle_tick,
        },
    });
}

#[derive(EntityEvent, MapEntities, Serialize, Deserialize)]
pub struct CardChangedPos {
    #[event_target]
    #[entities]
    pub card: Entity,
    pub new_pos: PosInDeck,
    pub turn: BattleTick,
}

#[derive(Component, Default, Serialize, Deserialize)]
#[require(Replicated)]
pub struct SaveHistory;

#[derive(Event, Serialize, Deserialize, Clone)]
pub struct BattleTickingJustStarted;

#[derive(Event, Clone)]
pub struct BattleTickIncremented(pub BattleTick);

#[derive(Component, Clone)]
pub struct LastReplicationBattleData(BattleData);

#[derive(Resource)]
pub struct RepliconTickToBattleTick(pub HashMap<u32, BattleTick>);

fn user_data_received(
    mut e: On<UserdataReceived>,
    s: Res<State<ClientState>>,
    mut tick_map: ResMut<RepliconTickToBattleTick>,
) {
    match s.get() {
        ClientState::Connected => {}
        _ => {
            return;
        }
    }

    let data: BattleData = postcard_utils::from_buf(&mut e.bytes).expect("could not deser data");
    match data {
        BattleData::NoBattle => {
            return;
        }
        BattleData::InCombat { battle_tick } => {
            tick_map.0.insert(e.message_tick.get(), battle_tick);
        }
    }
}

#[derive(Resource)]
pub struct ClientBattleTickingInitialized;

fn check_battle_tick_changed(
    battle_data: Res<BattleData>,
    mut replicon_storage: ResMut<ReplicationStorage>,
    maybe_client_ticking_init: Option<Res<ClientBattleTickingInitialized>>,
    server_tick: Res<ServerTick>,
    mut prev_battle_data: Local<Option<BattleData>>,
    mut cmd: Commands,
) {
    if !battle_data.is_changed() {
        return;
    }

    // println!("BATTLE DATA CHANGED TO : {:?}", battle_data.clone());

    let mut message: Vec<u8> = Vec::new();
    let data = battle_data.clone();
    postcard_utils::to_extend_mut(&data, &mut message).expect("could not serialize data");
    cmd.insert_resource(ReplicationUserdata(message));

    let battle_data = battle_data.into_inner();
    replicon_storage
        .global
        .insert::<BattleData>(battle_data.clone());

    'block: {
        match battle_data {
            BattleData::NoBattle => {
                break 'block;
            }
            BattleData::InCombat { battle_tick } => {
                if maybe_client_ticking_init.is_some() {
                    break 'block;
                }

                cmd.insert_resource(ClientBattleTickingInitialized);
            }
        }
    }

    *prev_battle_data = Some(battle_data.clone());
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum TickedAt {
    Ordered(BattleTick),
    Unordered(u32),
}

#[derive(Component)]
pub struct History<C: Component + Clone> {
    pub changes: BTreeMap<BattleTick, Vec<C>>,
}

impl<C: Component + Clone> History<C> {
    pub fn from_initial_change(initial: C, tick: &BattleTick) -> Self {
        let mut changes = BTreeMap::new();
        changes.insert(tick.clone(), vec![initial]);
        Self {
            changes: BTreeMap::new(),
        }
    }
}

pub trait RecordsLastChangeTick {
    fn get_last_change_tick(&self) -> BattleTick;
}

fn write_history<C: Component + Eq + Clone + RecordsLastChangeTick>(
    ctx: &mut WriteCtx,
    rule_fns: &RuleFns<C>,
    entity: &mut DeferredEntity,
    message: &mut Bytes,
) -> Result<(), BevyError> {
    let component: C = rule_fns.deserialize(ctx, message)?;
    let battle_tick = component.get_last_change_tick();

    if let Some(mut history) = entity.get_mut::<History<C>>() {
        match history.changes.get_mut(&battle_tick) {
            Some(change_list) => {
                change_list.push(component);
            }
            None => {
                history.changes.insert(battle_tick, vec![component]);
            }
        }
    } else {
        entity.insert(History::<C>::from_initial_change(component, &battle_tick));
    };

    Ok(())
}

fn remove_history<C: Component + Clone>(_ctx: &mut RemoveCtx, entity: &mut DeferredEntity) {
    entity.remove::<History<C>>().remove::<C>();
}

fn handle_sent_client_protocol(
    e: On<FromClient<ShareClientProtocol>>,
    mut cmd: Commands,
    mut disconnects: MessageWriter<DisconnectRequest>,
    protocol: Res<ProtocolHash>,
) {
    println!("received sent client protocol");
    let client = e
        .client_id
        .entity()
        .expect("protocol hash sent only from clients");

    if e.protocol != *protocol {
        cmd.server_trigger(ToClients {
            targets: SendTargets::Single(e.client_id),
            message: ProtocolMismatch,
        });
        disconnects.write(DisconnectRequest { client });
    } else {
        cmd.entity(client).insert(AuthorizedClient);
    }
}

#[derive(Event, Serialize, Deserialize, Clone, Copy)]
struct IncrementReq {
    amount: i32,
}

fn check_inputs(keys: Res<ButtonInput<KeyCode>>, mut cmd: Commands) {
    for key in keys.get_just_pressed() {
        match key {
            KeyCode::ArrowUp => {
                println!("sending incre req");
                cmd.client_trigger(IncrementReq { amount: 2 });
            }
            _ => {}
        }
    }
}

fn log_shared_vals(vals_q: Query<&SharedVal>) {
    for val in vals_q.iter().filter(|v| v.0 != 0) {
        println!("val is : {:?}", val);
    }
}

fn apply_increment_req(e: On<FromClient<IncrementReq>>, mut vals_q: Query<&mut SharedVal>) {
    println!("received increment req");
    for mut val in &mut vals_q {
        val.0 += e.amount;
    }
}

const DEFAULT_PORT: u16 = 5000;

#[derive(PartialEq, Resource)]
enum Cli {
    /// Run locally without any networking.
    Local,
    /// Create a server.
    Server {
        port: u16,
    },
    /// Connect to a host.
    Client {
        ip: IpAddr,
        port: u16,
    },
    AutoConnect {
        ip: IpAddr,
        port: u16,
    },
}

impl Default for Cli {
    fn default() -> Self {
        Self::AutoConnect {
            ip: Ipv4Addr::LOCALHOST.into(),
            port: DEFAULT_PORT,
        }
    }
}

#[derive(Component)]
#[require(Replicated)]
struct LocalPlayer;

#[derive(Component, Hash, Serialize, Deserialize)]
#[require(Replicated, Signature::of::<ClientPlayer>())]
struct ClientPlayer;

fn server_already_running(port: u16) -> bool {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).is_err()
}

fn host_or_join(channels: &Res<RepliconChannels>, ip: IpAddr, port: u16, cmd: &mut Commands) {
    const PROTOCOL_ID: u64 = 0;

    if !server_already_running(port) {
        println!("starting server");
        let server_channels_config = channels.server_configs();
        let client_channels_config = channels.client_configs();

        let server = RenetServer::new(ConnectionConfig {
            server_channels_config,
            client_channels_config,
            ..Default::default()
        });

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).unwrap();
        let server_config = ServerConfig {
            current_time,
            max_clients: 10,
            protocol_id: PROTOCOL_ID,
            authentication: ServerAuthentication::Unsecure,
            public_addresses: Default::default(),
        };
        let transport = NetcodeServerTransport::new(server_config, socket).unwrap();

        // Keep in mind in case
        // cmd.remove_resource::<RenetServer>();
        cmd.insert_resource(server);
        cmd.insert_resource(transport);

        cmd.spawn((LocalPlayer, SharedVal(0), Replicated));
    } else {
        println!("starting client");
        let server_channels_config = channels.server_configs();
        let client_channels_config = channels.client_configs();

        let client = RenetClient::new(ConnectionConfig {
            server_channels_config,
            client_channels_config,
            ..Default::default()
        });

        let current_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let client_id = current_time.as_millis() as u64;
        let server_addr = SocketAddr::new(ip, port);
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let authentication = ClientAuthentication::Unsecure {
            client_id,
            protocol_id: PROTOCOL_ID,
            server_addr,
            user_data: None,
        };
        let transport = NetcodeClientTransport::new(current_time, authentication, socket).unwrap();

        cmd.insert_resource(client);
        cmd.insert_resource(transport);

        cmd.spawn((LocalPlayer, ClientPlayer, Replicated));
    }
}

fn setup_networking(cli: Res<Cli>, channels: Res<RepliconChannels>, mut cmd: Commands) {
    match *cli {
        Cli::Local => {}
        Cli::Server { port } => {}
        Cli::Client { ip, port } => {}
        Cli::AutoConnect { ip, port } => {
            host_or_join(&channels, ip, port, &mut cmd);
        }
    }
}

fn disconnect_by_client(
    _on: On<Remove, ConnectedClient>,
    game_state: Res<State<GameState>>,
    mut commands: Commands,
) {
    println!("client closed the connection");
    if *game_state == GameState::InGame {
        commands.set_state(GameState::Disconnected);
    }
}

fn disconnect_by_server(mut commands: Commands) {
    println!("server closed the connection");
    commands.set_state(GameState::Disconnected);
}

/// Closes all sockets.
fn stop_networking(mut cmd: Commands) {
    println!("stop networks");
    cmd.remove_resource::<RenetServer>();
    cmd.remove_resource::<RenetClient>();
}

#[derive(Component, Clone, Debug, Serialize, Deserialize, Copy)]
struct SharedVal(i32);

impl Default for SharedVal {
    fn default() -> Self {
        Self(0)
    }
}

#[derive(States, Clone, Copy, Debug, Eq, Hash, PartialEq, Default)]
enum GameState {
    #[default]
    Loading,
    InGame,
    Disconnected,
}

#[derive(Event, Serialize, Deserialize, Clone, Copy)]
struct ShareClientProtocol {
    protocol: ProtocolHash,
}

/// Starts the game after connection.
///
/// Used only for a client.
fn client_start(mut cmd: Commands) {
    println!("client started, sending protocol");
    cmd.set_state(GameState::InGame);
}

fn game_started(mut cmd: Commands, protocol: Res<ProtocolHash>) {
    cmd.client_trigger(ShareClientProtocol {
        protocol: *protocol,
    });
}

/// Associates client with a symbol and starts the game.
///
/// Used only for server.
fn init_client(add: On<Add, AuthorizedClient>, mut cmd: Commands) {
    println!("client authorized");
    // Utilize client entity as a player for convenient lookups by `client`.
    cmd.entity(add.entity)
        .insert((ClientPlayer, Signature::of::<ClientPlayer>(), Replicated));

    cmd.trigger(BattleTriggered);
}
