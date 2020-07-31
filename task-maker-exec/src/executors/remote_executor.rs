use std::sync::mpsc::channel;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use task_maker_cache::Cache;
use task_maker_store::FileStore;

use crate::executor::{Executor, ExecutorInMessage};
use crate::scheduler::ClientInfo;
use crate::{derive_key_from_password, ChannelSender, ChannelServer, WorkerConn};
use std::net::SocketAddr;

/// Version of task-maker
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// An executor that accepts remote connections from clients and workers.
pub struct RemoteExecutor {
    file_store: Arc<FileStore>,
}

/// Message sent only by remote clients and workers for connecting to the server.
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteEntityMessage {
    /// Tell the remote executor the name of the client or of the worker.
    Welcome {
        /// The name of the client or of the worker.
        name: String,
        /// The required version of task-maker.
        version: String,
    },
}

/// Message sent only by the server in response of a `RemoteEntityMessage`.
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteEntityMessageResponse {
    /// The server accepted the connection of the client, the communication can continue.
    Accepted,
    /// The server rejected the connection of the client, the channel will be closed.
    Rejected(String),
}

impl RemoteExecutor {
    /// Make a new `RemoteExecutor`.
    pub fn new(file_store: Arc<FileStore>) -> Self {
        RemoteExecutor { file_store }
    }

    /// Start the executor binding the TCP sockets and waiting for clients and workers connections.
    pub fn start<S: Into<String>, S2: Into<String>>(
        self,
        bind_client_addr: S,
        bind_worker_addr: S2,
        client_password: Option<String>,
        worker_password: Option<String>,
        cache: Cache,
    ) {
        let file_store = self.file_store;
        let bind_client_addr = bind_client_addr.into();
        let bind_worker_addr = bind_worker_addr.into();

        let (executor_tx, executor_rx) = channel();
        let executor = Executor::new(file_store, cache, executor_rx, true);

        let client_executor_tx = executor_tx.clone();
        let client_listener_thread = std::thread::Builder::new()
            .name("Client listener".to_string())
            .spawn(move || {
                let server = match client_password {
                    Some(password) => {
                        let key = derive_key_from_password(password);
                        ChannelServer::bind_with_enc(&bind_client_addr, key)
                            .expect("Failed to bind client address")
                    }
                    None => ChannelServer::bind(&bind_client_addr)
                        .expect("Failed to bind client address"),
                };
                info!(
                    "Accepting client connections at tcp://{}",
                    server.local_addr().unwrap()
                );
                for (sender, receiver, addr) in server {
                    info!("Client connected from {}", addr);
                    let uuid = Uuid::new_v4();
                    let name = if let Ok(RemoteEntityMessage::Welcome { name, version }) =
                        receiver.recv()
                    {
                        if !validate_welcome(addr, &name, version, &sender, "Client") {
                            continue;
                        }
                        name
                    } else {
                        warn!(
                            "Client at {} has not sent the correct welcome message!",
                            addr
                        );
                        continue;
                    };
                    let client = ClientInfo { uuid, name };
                    client_executor_tx
                        .send(ExecutorInMessage::ClientConnected {
                            client,
                            sender: sender.change_type(),
                            receiver: receiver.change_type(),
                        })
                        .expect("Executor is gone");
                }
            })
            .expect("Cannot spawn client listener thread");
        let worker_listener_thread = std::thread::Builder::new()
            .name("Worker listener".to_string())
            .spawn(move || {
                let server = match worker_password {
                    Some(password) => {
                        let key = derive_key_from_password(password);
                        ChannelServer::bind_with_enc(&bind_worker_addr, key)
                            .expect("Failed to bind worker address")
                    }
                    None => ChannelServer::bind(&bind_worker_addr)
                        .expect("Failed to bind worker address"),
                };
                info!(
                    "Accepting worker connections at tcp://{}",
                    server.local_addr().unwrap()
                );
                for (sender, receiver, addr) in server {
                    info!("Worker connected from {}", addr);
                    let uuid = Uuid::new_v4();
                    let name = if let Ok(RemoteEntityMessage::Welcome { name, version }) =
                        receiver.recv()
                    {
                        if !validate_welcome(addr, &name, version, &sender, "Worker") {
                            continue;
                        }
                        name
                    } else {
                        warn!(
                            "Worker at {} has not sent the correct welcome message!",
                            addr
                        );
                        continue;
                    };
                    let worker = WorkerConn {
                        uuid,
                        name,
                        sender: sender.change_type(),
                        receiver: receiver.change_type(),
                    };
                    executor_tx
                        .send(ExecutorInMessage::WorkerConnected { worker })
                        .expect("Executor is dead");
                }
            })
            .expect("Cannot spawn worker listener thread");

        executor.run().expect("Executor failed");

        client_listener_thread
            .join()
            .expect("Client listener failed");
        worker_listener_thread
            .join()
            .expect("Worker listener failed");
    }
}

fn validate_welcome(
    addr: SocketAddr,
    name: &str,
    version: String,
    sender: &ChannelSender<RemoteEntityMessageResponse>,
    client: &str,
) -> bool {
    if version != VERSION {
        warn!(
            "{} '{}' from {} connected with version {}, server has {}",
            client, name, addr, version, VERSION
        );
        let _ = sender.send(RemoteEntityMessageResponse::Rejected(format!(
            "Wrong task-maker version, you have {}, server has {}",
            version, VERSION
        )));
        false
    } else {
        let _ = sender.send(RemoteEntityMessageResponse::Accepted);
        true
    }
}
