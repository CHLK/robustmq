// Copyright 2023 RobustMQ Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use axum_extra::headers::UserAgent;
use axum_extra::TypedHeader;
use axum_server::tls_rustls::RustlsConfig;
use bytes::{BufMut, BytesMut};
use common_base::config::broker_mqtt::broker_mqtt_conf;
use futures_util::stream::StreamExt;
use grpc_clients::pool::ClientPool;
use log::{debug, error, info};
use protocol::mqtt::codec::{MqttCodec, MqttPacketWrapper};
use protocol::mqtt::common::{MqttPacket, MqttProtocol};
use storage_adapter::storage::StorageAdapter;
use tokio::select;
use tokio::sync::broadcast::{self};

use crate::handler::cache::CacheManager;
use crate::handler::command::Command;
use crate::security::AuthDriver;
use crate::server::connection::NetworkConnection;
use crate::server::connection_manager::ConnectionManager;
use crate::subscribe::subscribe_manager::SubscribeManager;

pub const ROUTE_ROOT: &str = "/mqtt";

#[derive(Clone)]
pub struct WebSocketServerState<S> {
    sucscribe_manager: Arc<SubscribeManager>,
    cache_manager: Arc<CacheManager>,
    message_storage_adapter: Arc<S>,
    client_pool: Arc<ClientPool>,
    stop_sx: broadcast::Sender<bool>,
    connection_manager: Arc<ConnectionManager>,
    auth_driver: Arc<AuthDriver>,
}

impl<S> WebSocketServerState<S>
where
    S: StorageAdapter + Sync + Send + 'static + Clone,
{
    pub fn new(
        sucscribe_manager: Arc<SubscribeManager>,
        cache_manager: Arc<CacheManager>,
        connection_manager: Arc<ConnectionManager>,
        message_storage_adapter: Arc<S>,
        client_pool: Arc<ClientPool>,
        auth_driver: Arc<AuthDriver>,
        stop_sx: broadcast::Sender<bool>,
    ) -> Self {
        Self {
            sucscribe_manager,
            cache_manager,
            connection_manager,
            message_storage_adapter,
            client_pool,
            auth_driver,
            stop_sx,
        }
    }
}

pub async fn websocket_server<S>(state: WebSocketServerState<S>)
where
    S: StorageAdapter + Sync + Send + 'static + Clone,
{
    let config = broker_mqtt_conf();
    let ip: SocketAddr = format!("0.0.0.0:{}", config.network.websocket_port)
        .parse()
        .unwrap();
    let app = routes_v1(state);
    info!(
        "Broker WebSocket Server start success. port:{}",
        config.network.websocket_port
    );
    match axum_server::bind(ip)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
    {
        Ok(()) => {}
        Err(e) => panic!("{}", e.to_string()),
    }
}

pub async fn websockets_server<S>(state: WebSocketServerState<S>)
where
    S: StorageAdapter + Sync + Send + 'static + Clone,
{
    let config = broker_mqtt_conf();
    let ip: SocketAddr = format!("0.0.0.0:{}", config.network.websockets_port)
        .parse()
        .unwrap();
    let app = routes_v1(state);

    let tls_config = match RustlsConfig::from_pem_file(
        PathBuf::from(config.network.tls_cert.clone()),
        PathBuf::from(config.network.tls_key.clone()),
    )
    .await
    {
        Ok(cf) => cf,
        Err(e) => {
            panic!("{}", e.to_string());
        }
    };

    info!(
        "Broker WebSocket TLS Server start success. port:{}",
        config.network.websockets_port
    );
    match axum_server::bind_rustls(ip, tls_config)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
    {
        Ok(()) => {}
        Err(e) => panic!("{}", e.to_string()),
    }
}

fn routes_v1<S>(state: WebSocketServerState<S>) -> Router
where
    S: StorageAdapter + Sync + Send + 'static + Clone,
{
    let mqtt_ws = Router::new().route(ROUTE_ROOT, get(ws_handler));

    let app = Router::new().merge(mqtt_ws);
    app.with_state(state)
}

async fn ws_handler<S>(
    ws: WebSocketUpgrade,
    State(state): State<WebSocketServerState<S>>,
    user_agent: Option<TypedHeader<UserAgent>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response
where
    S: StorageAdapter + Sync + Send + 'static + Clone,
{
    let user_agent = if let Some(TypedHeader(user_agent)) = user_agent {
        user_agent.to_string()
    } else {
        String::from("Unknown Source")
    };
    info!("`{user_agent}` at {addr} connected.");
    let command = Command::new(
        state.cache_manager.clone(),
        state.message_storage_adapter.clone(),
        state.sucscribe_manager.clone(),
        state.client_pool.clone(),
        state.connection_manager.clone(),
        state.auth_driver.clone(),
    );
    let codec = MqttCodec::new(None);
    ws.protocols(["mqtt", "mqttv3.1"])
        .on_upgrade(move |socket| {
            handle_socket(
                socket,
                addr,
                command,
                codec,
                state.connection_manager.clone(),
                state.stop_sx.clone(),
            )
        })
}

async fn handle_socket<S>(
    socket: WebSocket,
    addr: SocketAddr,
    mut command: Command<S>,
    mut codec: MqttCodec,
    connection_manager: Arc<ConnectionManager>,
    stop_sx: broadcast::Sender<bool>,
) where
    S: StorageAdapter + Sync + Send + 'static + Clone,
{
    let (sender, mut receiver) = socket.split();
    let mut tcp_connection = NetworkConnection::new(
        crate::server::connection::NetworkConnectionType::WebSocket,
        addr,
        None,
    );

    connection_manager.add_websocket_write(tcp_connection.connection_id, sender);
    connection_manager.add_connection(tcp_connection.clone());
    let mut protocol_version = MqttProtocol::Mqtt5;
    let mut stop_rx = stop_sx.subscribe();

    loop {
        select! {
            val = stop_rx.recv() =>{
                if let Ok(flag) = val {
                    if flag {
                        break;
                    }
                }
            },
            val = receiver.next()=>{
                if let Some(msg) = val{
                    match msg {
                        Ok(Message::Binary(data)) => {
                            let mut buf = BytesMut::with_capacity(data.len());
                            buf.put(data.as_slice());
                            match codec.decode_data(&mut buf) {
                                Ok(Some(packet)) => {
                                    info!("recv websocket packet:{packet:?}");
                                    if let Some(resp_pkg) = command
                                        .apply(
                                            connection_manager.clone(),
                                            tcp_connection.clone(),
                                            addr,
                                            packet.clone(),
                                        )
                                        .await
                                    {
                                        if let MqttPacket::Connect(_,_,_,_,_,_) = packet {
                                            if let Some(pv) = connection_manager.get_connect_protocol(tcp_connection.connection_id){
                                                protocol_version = pv.clone();
                                                tcp_connection.set_protocol(pv);
                                            }
                                        }

                                        let mut response_buff = BytesMut::new();
                                        let packet_wrapper = MqttPacketWrapper {
                                            protocol_version: protocol_version.clone().into(),
                                            packet: resp_pkg,
                                        };

                                        match codec.encode_data(packet_wrapper, &mut response_buff){
                                            Ok(()) => {},
                                            Err(e) => {
                                                error!("Websocket encode back packet failed with error message: {e:?}");
                                            }
                                        }
                                        match connection_manager.write_websocket_frame(tcp_connection.connection_id, Message::Binary(response_buff.to_vec())).await{
                                            Ok(()) => {},
                                            Err(e) => {
                                                error!("websocket returns failure to write the packet to the client with error message {e:?}");
                                                connection_manager.close_connect(tcp_connection.connection_id).await;
                                                break;
                                            }
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    error!("Websocket failed to parse MQTT protocol packet with error message :{e:?}");
                                }
                            }
                        }
                        Ok(Message::Text(data)) => {
                            debug!(
                                "websocket server receives a TEXT message with the following content: {data}"
                            );
                        }
                        Ok(Message::Ping(data)) => {
                            debug!(
                                "websocket server receives a Ping message with the following content: {data:?}"
                            );
                        }
                        Ok(Message::Pong(data)) => {
                            debug!(
                                "websocket server receives a Pong message with the following content: {data:?}"
                            );
                        }
                        Ok(Message::Close(data)) => {
                            if let Some(cf) = data {
                                info!(
                                    ">>> {} sent close with code {} and reason `{}`",
                                    addr, cf.code, cf.reason
                                );
                            } else {
                                info!(
                                    ">>> {addr} somehow sent close message without CloseFrame"
                                );
                            }
                            break;
                        }
                        Err(e) => {
                            debug!("websocket server parsing request packet error, error message :{e:?}");
                        },
                    }
                } else {
                    debug!("Web socket server receives an empty request packet");
                }
            }
        }
    }
}
