use std::thread::{self, JoinHandle};

use cluster::register_storage_engine_node;
use common::{
    config::storage_engine::StorageEngineConfig, log::info_meta, runtime::create_runtime,
};
use protocol::storage_engine::storage::storage_engine_service_server::StorageEngineServiceServer;
use services::StorageService;
use tokio::{signal, sync::broadcast};
use tonic::transport::Server;

mod index;
mod raft_group;
mod record;
mod segment;
mod services;
mod shard;
mod storage;
mod v1;
mod v2;
mod cluster;

pub struct StorageEngine {
    config: StorageEngineConfig,
}

impl StorageEngine {
    pub fn new(config: StorageEngineConfig) -> Self {
        return StorageEngine { config };
    }

    pub async fn start(
        &self,
        stop_send: broadcast::Sender<bool>,
    ) -> Vec<Result<JoinHandle<()>, std::io::Error>> {
        let mut thread_result = Vec::new();
        
        // Register Node
        register_storage_engine_node(self.config.clone()).await;
        
        // start GRPC && HTTP Server
        let tcp_thread_join = self.start_server(stop_send.subscribe());
        thread_result.push(tcp_thread_join);

        // Threads that run the daemon thread
        let daemon_thread_join = self.start_daemon_thread(stop_send);
        thread_result.push(daemon_thread_join);
        return thread_result;
    }

    // start GRPC && HTTP Server
    fn start_server(
        &self,
        mut stop_recv: broadcast::Receiver<bool>,
    ) -> Result<JoinHandle<()>, std::io::Error> {
        let config = self.config.clone();
        let tcp_thread = thread::Builder::new().name("storage-engine-server-thread".to_owned());
        tcp_thread.spawn(move || {
            let runtime =
                create_runtime("storage-engine-server-runtime", config.runtime_work_threads);

            runtime.spawn(async move {
                let ip = format!("{}:{}", config.addr, config.port).parse().unwrap();

                info_meta(&format!(
                    "RobustMQ StorageEngine Grpc Server start success. bind addr:{}",
                    ip
                ));

                let service_handler = StorageService::new();

                Server::builder()
                    .add_service(StorageEngineServiceServer::new(service_handler))
                    .serve(ip)
                    .await
                    .unwrap();
            });

            runtime.block_on(async {
                if stop_recv.recv().await.unwrap() {
                    info_meta("TCP and GRPC Server services stop.");
                }
            });
        })
    }

    // Start Daemon Thread
    fn start_daemon_thread(&self, stop_send: broadcast::Sender<bool>) -> Result<JoinHandle<()>, std::io::Error> {
        let daemon_thread = thread::Builder::new().name("daemon-thread".to_owned());
        let config = self.config.clone();
        daemon_thread.spawn(move || {
            
            let daemon_runtime = create_runtime("daemon-runtime", config.runtime_work_threads);
            daemon_runtime.block_on(async move {
                loop {
                    signal::ctrl_c().await.expect("failed to listen for event");
                    match stop_send.send(true) {
                        Ok(_) => {
                            info_meta("When ctrl + c is received, the service starts to stop");
                            break;
                        }
                        Err(_) => {}
                    }
                }
            });
        })
    }
}