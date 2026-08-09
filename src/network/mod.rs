use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::{
    app::{App, FixedUpdate, Plugin, PluginGroup, Startup, Update},
    input::{ButtonInput, keyboard::KeyCode},
    prelude::{Deref, DerefMut},
    state::{
        app::AppExtStates,
        commands::CommandsStatesExt,
        condition::in_state,
        state::{OnEnter, OnExit, State, States},
    },
    ui::Node,
    utils::default,
};
use bevy_ecs::{
    change_detection::DetectChanges,
    component::{Component, Mutable},
    entity::Entity,
    error::BevyError,
    event::{EntityEvent, Event},
    hierarchy::ChildOf,
    lifecycle::{Add, Remove},
    message::MessageWriter,
    observer::On,
    query::{Changed, Or, With},
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Commands, ParallelCommands, Query, Res, ResMut},
    world::Ref,
};

use bevy_flair::style::components::NodeStyleSheet;
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
    prelude::{
        AppMarkerExt, AppRuleExt, Channel, ClientEventAppExt, ClientId, ClientState,
        ClientTriggerExt, ConnectedClient, DisconnectRequest, FromClient, ProtocolHash,
        ProtocolMismatch, Replicated, RepliconChannels, RuleFns, SendTargets, ServerEventAppExt,
        ServerState, ServerTriggerExt, Signature, SyncRelatedAppExt, ToClients,
    },
    server::AuthorizedClient,
    shared::{
        AuthMethod, RepliconSharedPlugin,
        replication::{
            deferred_entity::DeferredEntity,
            receive_markers::MarkerConfig,
            registry::ctx::{RemoveCtx, SerializeCtx, WriteCtx},
        },
    },
};
use bevy_replicon_renet::{RenetChannelsExt, RepliconRenetPlugins};
use serde::{Deserialize, Serialize};

use crate::{
    BattleTick, BeingDrawn, BoardUtilsCommandsExt, CardCast, CardInPile, CardWidgetFor,
    DelayCompleted, Delayer, DrawPile, EnemyCard, EnemyData, HandPile, InDiscard, InDrawPile,
    InHand, Magnetic, MainSceneUiRoot, PlayerCard, PlayerData, PosInDeck, TickAmount, UiCardMarker,
    abilities::effects::{StatusEffectOf, StatusEffects},
    deck::deck_and_cards::{Card, CardPile},
    game_flow::turns::{
        BattleState, BattleTriggered, CheckClientBattleReady, ConfirmBattleReady, EnemyBoardMarker,
        PlayerBoardMarker, confirm_server_battle_ready, handle_client_confirm_battle_start,
    },
};

pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            RepliconPlugins.set(RepliconSharedPlugin {
                auth_method: AuthMethod::Custom,
            }),
            RepliconRenetPlugins,
        ))
        .init_resource::<Cli>()
        .init_state::<GameState>()
        .replicate::<TickAmount>()
        .replicate::<BattleTick>()
        .replicate::<SharedVal>()
        .replicate::<Card>()
        .replicate::<CardPile>()
        .replicate::<CardInPile>()
        .replicate::<Magnetic>()
        .replicate::<UiCardMarker>()
        .replicate::<InHand>()
        .replicate::<BeingDrawn>()
        .replicate::<InDrawPile>()
        .replicate::<InDiscard>()
        .replicate::<PosInDeck>()
        .register_marker_with::<SaveHistory>(MarkerConfig {
            need_history: true,
            ..Default::default()
        })
        .set_marker_fns::<SaveHistory, PosInDeck>(write_history, remove_history::<PosInDeck>)
        .set_marker_fns::<SaveHistory, TickAmount>(write_history, remove_history::<TickAmount>)
        .replicate::<PlayerBoardMarker>()
        .replicate::<EnemyBoardMarker>()
        .replicate_once::<Node>()
        .replicate_once::<MainSceneUiRoot<PlayerData>>()
        .replicate_once::<MainSceneUiRoot<EnemyData>>()
        .sync_related_entities::<CardInPile>()
        .replicate::<StatusEffects>()
        .replicate::<StatusEffectOf>()
        .sync_related_entities::<StatusEffectOf>()
        .replicate::<DrawPile>()
        .replicate::<HandPile>()
        .replicate::<PlayerCard>()
        .replicate::<EnemyCard>()
        .sync_related_entities::<ChildOf>()
        .add_client_event::<ShareClientProtocol>(Channel::Ordered)
        .add_client_event::<ConfirmBattleReady>(Channel::Ordered)
        .add_server_event::<CheckClientBattleReady>(Channel::Ordered)
        .add_server_event::<ProtocolMismatch>(Channel::Unreliable)
        .make_event_independent::<ProtocolMismatch>()
        .add_server_event::<BattleTickStarted>(Channel::Ordered)
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
        .add_observer(handle_battle_tick_incremented)
        .add_systems(Startup, setup_networking)
        .add_systems(
            FixedUpdate,
            check_battle_tick_changed.run_if(in_state(ServerState::Running)),
        )
        .add_systems(
            FixedUpdate,
            (
                check_inputs.run_if(in_state(ClientState::Connected)),
                log_shared_vals,
            ),
        );
    }
}

#[derive(Component, Default)]
pub struct SaveHistory;

#[derive(Event, Serialize, Deserialize, Clone)]
pub struct BattleTickStarted(pub BattleTick);

#[derive(Event, Clone)]
pub struct BattleTickIncremented(pub BattleTick);

fn handle_battle_tick_incremented(
    e: On<BattleTickIncremented>,
    mut q: Query<(Entity, &mut TickAmount)>,
    par_cmd: ParallelCommands,
) {
    q.par_iter_mut()
        .for_each(|(card, mut t)| match t.tick(&e.0) {
            crate::CastState::Pending => {}
            crate::CastState::Triggered => {
                par_cmd.command_scope(|mut cmd| {
                    cmd.trigger(CardCast { card });
                });
            }
        });
}

fn check_battle_tick_changed(
    q_battle_tick: Query<(&BattleTick, Ref<BattleTick>)>,
    mut cmd: Commands,
) {
    if q_battle_tick.count() == 0 {
        return;
    }

    let (tick, tick_ref) = q_battle_tick
        .single()
        .expect("only should have one BattleTick at max");

    if !tick_ref.is_changed() {
        return;
    }

    let cloned_tick = tick.clone();
    cmd.trigger(BattleTickIncremented(tick.clone()));

    if !tick_ref.is_added() {
        return;
    }

    cmd.spawn(Delayer::from_secs(0.1)).observe(
        move |_: On<DelayCompleted>, mut obs_cmd: Commands| {
            obs_cmd.server_trigger(ToClients {
                targets: SendTargets::CLIENTS_ONLY,
                message: BattleTickStarted(cloned_tick.clone()),
            });
        },
    );
}

pub trait ProvidesLastChangeTick {
    fn get_last_change_tick(&self) -> BattleTick;
}

#[derive(Component, Deref, DerefMut)]
pub struct History<C>(pub BTreeMap<BattleTick, Vec<C>>)
where
    C: Component + ProvidesLastChangeTick;

fn write_history<C: Component + Eq + ProvidesLastChangeTick>(
    ctx: &mut WriteCtx,
    rule_fns: &RuleFns<C>,
    entity: &mut DeferredEntity,
    message: &mut Bytes,
) -> Result<(), BevyError> {
    let component: C = rule_fns.deserialize(ctx, message)?;
    let battle_tick = component.get_last_change_tick();

    if let Some(mut history) = entity.get_mut::<History<C>>() {
        match history.0.get_mut(&battle_tick) {
            Some(changes) => {
                if changes.iter().any(|v| *v == component) {
                    return Ok(());
                }
                changes.push(component);
            }
            None => {
                history.0.insert(battle_tick, vec![component]);
            }
        }
    } else {
        entity.insert(History::<C>([(battle_tick, vec![component])].into()));
    }

    Ok(())
}

fn remove_history<C: Component + ProvidesLastChangeTick>(
    _ctx: &mut RemoveCtx,
    entity: &mut DeferredEntity,
) {
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
