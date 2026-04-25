use crate::node::Node;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse<T> {
    pub jsonrpc: String,
    pub result: Option<T>,
    pub error: Option<String>,
    pub id: u64,
}

#[derive(Debug, Clone)]
pub struct RpcSnapshot {
    pub active: bool,
    pub bind_addr: String,
    pub advertised_url: String,
    pub allow_remote: bool,
    pub log_lines: Vec<String>,
}

#[derive(Debug)]
pub struct RpcServer {
    running: Arc<AtomicBool>,
    bind_addr: Arc<Mutex<String>>,
    advertised_url: Arc<Mutex<String>>,
    logs: Arc<Mutex<VecDeque<String>>>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl RpcServer {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            bind_addr: Arc::new(Mutex::new("127.0.0.1:18443".to_string())),
            advertised_url: Arc::new(Mutex::new(String::new())),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(128))),
            handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(
        &self,
        node: Node,
        bind_addr: String,
        advertised_url: Option<String>,
        mining_address: String,
    ) -> Result<(), String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("RPC server is already running".to_string());
        }

        let advertised_url =
            normalize_advertised_rpc_url(&bind_addr, advertised_url.unwrap_or_default().trim());
        *self.bind_addr.lock().unwrap() = bind_addr.clone();
        *self.advertised_url.lock().unwrap() = advertised_url.clone();
        push_log_line(&self.logs, format!("RPC server listening on {bind_addr}"));
        if !advertised_url.is_empty() {
            push_log_line(&self.logs, format!("RPC published at {advertised_url}"));
        }

        let running = self.running.clone();
        let logs = self.logs.clone();
        let bind_store = self.bind_addr.clone();
        let advertised_store = self.advertised_url.clone();

        let handle = thread::spawn(move || {
            let listener = match TcpListener::bind(&bind_addr) {
                Ok(listener) => listener,
                Err(err) => {
                    push_log_line(&logs, format!("RPC bind failed: {err}"));
                    running.store(false, Ordering::SeqCst);
                    return;
                }
            };
            if listener.set_nonblocking(true).is_err() {
                push_log_line(&logs, "Failed to set RPC listener nonblocking".to_string());
            }

            while running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        push_log_line(&logs, format!("RPC client connected: {addr}"));
                        let node = node.clone();
                        let logs = logs.clone();
                        let mining_address = mining_address.clone();
                        thread::spawn(move || {
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut request_line = String::new();
                            if reader.read_line(&mut request_line).is_ok() {
                                if let Ok(request) =
                                    serde_json::from_str::<JsonRpcRequest>(&request_line)
                                {
                                    let response =
                                        handle_rpc_request(&node, request, &mining_address);
                                    let mut stream = stream;
                                    let payload = serde_json::to_string(&response).unwrap_or_else(|_| {
                                        "{\"jsonrpc\":\"2.0\",\"result\":null,\"error\":\"serialization failure\",\"id\":0}".to_string()
                                    });
                                    let _ = writeln!(stream, "{payload}");
                                } else {
                                    push_log_line(
                                        &logs,
                                        "Received invalid RPC request".to_string(),
                                    );
                                }
                            }
                        });
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(err) => {
                        push_log_line(&logs, format!("RPC accept failed: {err}"));
                        thread::sleep(Duration::from_millis(250));
                    }
                }
            }

            push_log_line(&logs, "RPC server stopped".to_string());
            *bind_store.lock().unwrap() = bind_addr;
            *advertised_store.lock().unwrap() = advertised_url;
        });

        *self.handle.lock().unwrap() = Some(handle);
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.handle.lock().unwrap().take() {
            let _ = handle.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> RpcSnapshot {
        let bind_addr = self.bind_addr.lock().unwrap().clone();
        RpcSnapshot {
            active: self.is_running(),
            allow_remote: !is_loopback_endpoint(&bind_addr),
            bind_addr,
            advertised_url: self.advertised_url.lock().unwrap().clone(),
            log_lines: self.logs.lock().unwrap().iter().cloned().collect(),
        }
    }
}

fn normalize_advertised_rpc_url(bind_addr: &str, advertised_url: &str) -> String {
    let trimmed = advertised_url.trim();
    if !trimmed.is_empty() {
        if trimmed.contains("://") {
            return trimmed.to_string();
        }
        return format!("http://{trimmed}");
    }

    auto_rpc_url(bind_addr).unwrap_or_default()
}

fn auto_rpc_url(bind_addr: &str) -> Option<String> {
    let addr = bind_addr.parse::<SocketAddr>().ok()?;
    let host = if addr.ip().is_loopback() {
        IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    } else if addr.ip().is_unspecified() {
        discover_local_ip().unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
    } else {
        addr.ip()
    };
    Some(format!("http://{host}:{}", addr.port()))
}

fn discover_local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn handle_rpc_request(
    node: &Node,
    request: JsonRpcRequest,
    mining_address: &str,
) -> JsonRpcResponse<serde_json::Value> {
    let response = match request.method.as_str() {
        "getinfo" | "getmininginfo" => {
            let status = node.get_status();
            Ok(json!({
                "best_height": status.best_height,
                "mempool_size": status.mempool_size,
                "connected_peers": status.connected_peers,
                "mining_active": status.mining_active,
                "hash_rate": status.hash_rate,
                "rpc_bind_addr": status.rpc_bind_addr,
                "rpc_advertised_url": status.rpc_advertised_url,
                "rpc_allow_remote": status.rpc_allow_remote,
                "standard_fee_rate": status.standard_fee_rate,
                "instant_fee_rate": status.instant_fee_rate,
            }))
        }
        "getblocktemplate" => match node.create_block_template(mining_address.as_bytes()) {
            Ok(template) => Ok(json!({
                "height": template.header.height,
                "previous_block_hash": hex::encode(template.header.previous_block_hash),
                "bits": template.header.bits,
                "reward": template.block.transactions.first().map(|tx| tx.total_output_value()).unwrap_or(0),
                "transaction_count": template.block.transactions.len().saturating_sub(1),
            })),
            Err(err) => Err(err),
        },
        "submitblock" => {
            let block_value = request
                .params
                .get("block")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match serde_json::from_value(block_value) {
                Ok(block) => node.submit_block(block).map(|_| json!({"accepted": true})),
                Err(err) => Err(format!("Invalid block payload: {err}")),
            }
        }
        "sendtransaction" => {
            let tx_value = request
                .params
                .get("transaction")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match serde_json::from_value(tx_value) {
                Ok(transaction) => node
                    .submit_transaction(transaction)
                    .map(|_| json!({"accepted": true})),
                Err(err) => Err(format!("Invalid transaction payload: {err}")),
            }
        }
        _ => Err(format!("Unknown RPC method '{}'", request.method)),
    };

    match response {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id: request.id,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id: request.id,
        },
    }
}

fn is_loopback_endpoint(bind_addr: &str) -> bool {
    let lower = bind_addr.trim().to_ascii_lowercase();
    lower.starts_with("127.0.0.1:")
        || lower.starts_with("localhost:")
        || lower.starts_with("[::1]:")
}

fn push_log_line(logs: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let mut logs = logs.lock().unwrap();
    while logs.len() >= 100 {
        logs.pop_front();
    }
    logs.push_back(line);
}
