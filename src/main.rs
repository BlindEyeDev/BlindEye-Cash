mod block;
mod blockchain;
mod mempool;
mod mining;
mod network;
mod node;
mod paths;
mod p2p;
mod pow;
mod privacy;
mod protocol;
mod rpc;
mod rpc_registry;
mod transaction;
mod wallet;

use clap::{Parser, Subcommand};
use eframe::egui;
use blockchain::{WalletTransactionDirection, WalletTransactionRecord};
use mining::MiningSettings;
use node::{default_node_state_path, Node};
use p2p::P2PManager;
use protocol::{format_bec_amount, parse_bec_amount};
use qrcode::{render::unicode, QrCode};
use rpc_registry::PublicRpcEndpoint;
use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use wallet::{
    default_wallet_backup_path, default_wallet_state_path, TransactionPreview, Wallet,
};

const COLOR_NEAR_BLACK: egui::Color32 = egui::Color32::from_rgb(7, 10, 19);
const COLOR_CHARCOAL: egui::Color32 = egui::Color32::from_rgb(16, 23, 37);
const COLOR_PANEL: egui::Color32 = egui::Color32::from_rgb(22, 33, 54);
const COLOR_CARD: egui::Color32 = egui::Color32::from_rgb(18, 29, 47);
const COLOR_CARD_SOFT: egui::Color32 = egui::Color32::from_rgb(24, 40, 64);
const COLOR_STROKE: egui::Color32 = egui::Color32::from_rgb(54, 102, 188);
const COLOR_BLUE: egui::Color32 = egui::Color32::from_rgb(31, 94, 179);
const COLOR_CYAN: egui::Color32 = egui::Color32::from_rgb(92, 207, 224);
const COLOR_SILVER: egui::Color32 = egui::Color32::from_rgb(224, 228, 236);
const COLOR_MUTED: egui::Color32 = egui::Color32::from_rgb(144, 166, 196);
const DEFAULT_PUBLIC_RPC_REGISTRY_URL: &str = "https://comboss.co.uk/rpc-registry.php";

#[derive(Parser)]
#[command(name = "blindeye")]
#[command(about = "BlindEye (BEC) - wallet, node, miner", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Launch the GUI wallet instead of CLI
    #[arg(long)]
    gui: bool,
}

#[derive(Subcommand)]
enum Commands {
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },
    Node {
        #[command(subcommand)]
        action: NodeAction,
    },
    Mining {
        #[command(subcommand)]
        action: MiningAction,
    },
}

#[derive(Subcommand)]
enum WalletAction {
    New,
    Info,
    Send {
        #[arg(value_name = "ADDRESS")]
        to: String,
        #[arg(value_name = "AMOUNT")]
        amount: String,
        #[arg(value_name = "FEE", default_value = "0")]
        fee: String,
    },
    Backup {
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    Restore {
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    ImportSeed {
        #[arg(value_name = "SEED_PHRASE")]
        seed_phrase: String,
    },
}

#[derive(Subcommand)]
enum NodeAction {
    Start,
    Status,
    Peers,
    P2p {
        #[arg(long, value_name = "ADDR", default_value = "0.0.0.0:30303")]
        listen: String,
        #[arg(long, value_name = "ADDR")]
        bootstrap: Option<String>,
    },
}

#[derive(Subcommand)]
enum MiningAction {
    Start {
        #[arg(value_name = "WORKERS", default_value_t = 1)]
        workers: usize,
    },
    Stop,
    Status,
    Rpc {
        #[arg(value_name = "ADDR", default_value = "127.0.0.1:18443")]
        addr: String,
        #[arg(long, value_name = "PUBLIC_URL")]
        advertise: Option<String>,
    },
}

fn prompt_password(prompt_text: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Write;
    print!("{}", prompt_text);
    std::io::stdout().flush()?;
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    Ok(password.trim().to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.gui {
        launch_gui()?;
    } else if let Some(command) = cli.command {
        handle_cli(command)?;
    } else {
        launch_gui()?;
    }

    Ok(())
}

fn handle_cli(command: Commands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Commands::Wallet { action } => handle_wallet_cli(action),
        Commands::Node { action } => handle_node_cli(action),
        Commands::Mining { action } => handle_mining_cli(action),
    }
}

fn handle_wallet_cli(action: WalletAction) -> Result<(), Box<dyn std::error::Error>> {
    let wallet_state_path = default_wallet_state_path();

    match action {
        WalletAction::New => {
            let wallet = Wallet::new();
            let password = prompt_password("Enter password to protect wallet: ")?;
            wallet.save_to_file(&wallet_state_path, &password)?;
            println!(
                "New wallet created and saved to {}",
                wallet_state_path.display()
            );
            println!("Address: {}", wallet.address);
            println!(
                "Seed Phrase (shown on creation, store offline): {}",
                wallet.seed_phrase()
            );
            Ok(())
        }
        WalletAction::Info => {
            let password = prompt_password("Enter wallet password: ")?;
            let node = load_or_create_node()?;
            let mut wallet = Wallet::load_or_create_state(&wallet_state_path, &password)?;
            let blockchain = node.blockchain.lock().unwrap();
            wallet.sync_balance(&blockchain);
            println!("Wallet Address: {}", wallet.address);
            println!("Balance: {} BEC", wallet.balance_bec());
            println!("Transactions: {}", wallet.transactions.len());
            println!("Wallet State: {}", wallet_state_path.display());
            println!("Blockchain State: {}", node.storage_path.as_ref().display());
            Ok(())
        }
        WalletAction::Send { to, amount, fee } => {
            let password = prompt_password("Enter wallet password: ")?;
            let node = load_or_create_node()?;
            let wallet = Wallet::load_or_create_state(&wallet_state_path, &password)?;
            let amount_units = parse_bec_amount(&amount)?;
            let fee_units = parse_bec_amount(&fee)?;
            let transaction = {
                let blockchain = node.blockchain.lock().unwrap();
                wallet.build_transaction(&blockchain, &to, amount_units, fee_units)?
            };
            let txid = transaction.txid();
            node.submit_transaction(transaction)?;
            println!("Submitted {} BEC to {} with {} BEC fee", amount, to, fee);
            println!("Transaction ID: {}", hex::encode(txid));
            Ok(())
        }
        WalletAction::Backup { path } => {
            let password = prompt_password("Enter wallet password: ")?;
            let backup_path = path.map(PathBuf::from).unwrap_or_else(default_wallet_backup_path);
            let wallet = Wallet::load_or_create_state(&wallet_state_path, &password)?;
            wallet.save_to_file(&backup_path, &password)?;
            println!("Saved wallet backup to {}", backup_path.display());
            println!("Address: {}", wallet.address);
            Ok(())
        }
        WalletAction::Restore { path } => {
            let backup_path = path.map(PathBuf::from).unwrap_or_else(default_wallet_backup_path);
            let password = prompt_password("Enter password for backup file: ")?;
            let wallet = Wallet::load_from_file(&backup_path, &password)?;
            let new_password = prompt_password("Enter new password to protect wallet: ")?;
            wallet.save_to_file(&wallet_state_path, &new_password)?;
            println!("Restored wallet from {}", backup_path.display());
            println!("Address: {}", wallet.address);
            Ok(())
        }
        WalletAction::ImportSeed { seed_phrase } => {
            let wallet = Wallet::from_mnemonic(seed_phrase)?;
            let password = prompt_password("Enter password to protect wallet: ")?;
            wallet.save_to_file(&wallet_state_path, &password)?;
            println!("Imported wallet seed to {}", wallet_state_path.display());
            println!("Address: {}", wallet.address);
            Ok(())
        }
    }
}

fn handle_node_cli(action: NodeAction) -> Result<(), Box<dyn std::error::Error>> {
    let node = load_or_create_node()?;

    match action {
        NodeAction::Start => {
            println!("Starting BlindEye node...");
            println!(
                "Genesis hash: {}",
                hex::encode(node.blockchain.lock().unwrap().genesis_hash)
            );
            let status = node.get_status();
            println!("Current height: {}", status.best_height);
            println!("Peers connected: {}", status.connected_peers);
            println!("Consensus threshold: {}", status.consensus_threshold);
            println!("RPC active: {}", status.rpc_active);
            Ok(())
        }
        NodeAction::Status => {
            let status = node.get_status();
            println!("Node Status:");
            println!("  Best Height: {}", status.best_height);
            println!("  Connected Peers: {}", status.connected_peers);
            println!("  Mempool Size: {}", status.mempool_size);
            println!("  Consensus Threshold: {}", status.consensus_threshold);
            println!("  Mining Active: {}", status.mining_active);
            println!("  Hash Rate: {:.0} H/s", status.hash_rate);
            println!("  Total Hashes: {}", status.total_hashes);
            println!("  RPC Active: {}", status.rpc_active);
            println!("  RPC Bind: {}", status.rpc_bind_addr);
            println!("  RPC Advertise: {}", status.rpc_advertised_url);
            println!("  RPC Remote Enabled: {}", status.rpc_allow_remote);
            println!(
                "  Standard Fee Rate: {} units/byte",
                status.standard_fee_rate
            );
            println!("  Instant Fee Rate: {} units/byte", status.instant_fee_rate);
            Ok(())
        }
        NodeAction::Peers => {
            let peer_manager = node.peer_manager.lock().unwrap();
            let peers = peer_manager.get_connected_peers();
            if peers.is_empty() {
                println!("No peers connected");
            } else {
                println!("Connected Peers:");
                for peer in peers {
                    println!(
                        "  {} @ {} (height: {})",
                        peer.id, peer.address, peer.best_height
                    );
                }
            }
            Ok(())
        }
        NodeAction::P2p {
            listen,
            bootstrap,
        } => {
            run_p2p_node(node, &listen, bootstrap.as_deref())
        }
    }
}

fn handle_mining_cli(action: MiningAction) -> Result<(), Box<dyn std::error::Error>> {
    let password = prompt_password("Enter wallet password: ")?;
    let node = load_or_create_node()?;
    let wallet = Wallet::load_or_create_state(default_wallet_state_path(), &password)?;

    match action {
        MiningAction::Start { workers } => {
            node.start_continuous_mining(
                &wallet.address_bytes(),
                MiningSettings {
                    worker_count: workers.max(1),
                    mine_empty_blocks: true,
                },
            )?;
            println!(
                "Mining to {} with {} worker(s). Press Ctrl+C to stop.",
                wallet.address,
                workers.max(1)
            );
            loop {
                let snapshot = node.mining_snapshot();
                println!(
                    "height={} hash_rate={:.0} H/s total_hashes={}",
                    node.get_best_height(),
                    snapshot.hash_rate,
                    snapshot.total_hashes
                );
                thread::sleep(Duration::from_secs(1));
            }
        }
        MiningAction::Stop => {
            node.stop_continuous_mining();
            println!("Mining stop command received");
            Ok(())
        }
        MiningAction::Status => {
            let status = node.get_status();
            println!("Mining Status:");
            println!("  Active: {}", status.mining_active);
            println!("  Best Height: {}", status.best_height);
            println!("  Mempool Size: {}", status.mempool_size);
            println!("  Hash Rate: {:.0} H/s", status.hash_rate);
            Ok(())
        }
        MiningAction::Rpc { addr, advertise } => {
            node.rpc_server.start(
                node.clone(),
                addr.clone(),
                advertise.clone(),
                wallet.address.clone(),
            )?;
            let rpc = node.rpc_server.snapshot();
            println!("RPC server started on {}", rpc.bind_addr);
            if !rpc.advertised_url.is_empty() {
                println!("Published RPC URL: {}", rpc.advertised_url);
            }
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
    }
}

fn run_p2p_node(
    node: Node,
    listen_addr: &str,
    bootstrap_addr: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let listen_addr: std::net::SocketAddr = listen_addr.parse()?;
        let node = Arc::new(node);
        let manager = Arc::new(P2PManager::new(listen_addr, node.clone(), 16));

        println!("[P2P] Starting P2P node on {}", listen_addr);
        println!("[P2P] Genesis hash: {}", hex::encode(
            node.blockchain.lock().unwrap().genesis_hash
        ));

        let sync_manager = manager.clone();
        tokio::spawn(async move {
            let mut sync_interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                sync_interval.tick().await;
                if let Err(e) = sync_manager.synchronize_blocks().await {
                    eprintln!("[P2P] Background sync error: {}", e);
                }
            }
        });

        if let Some(bootstrap) = bootstrap_addr {
            let bootstrap_addr: std::net::SocketAddr = bootstrap.parse()?;
            println!("[P2P] Connecting to bootstrap peer: {}", bootstrap_addr);
            if let Err(e) = manager.clone().connect_peer(bootstrap_addr).await {
                eprintln!("[P2P] Bootstrap connection error: {}", e);
            }
        }

        manager.start().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

fn launch_gui() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = if let Some(icon) = load_app_icon() {
        egui::ViewportBuilder::default()
            .with_inner_size([520.0, 320.0])
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_icon(Arc::new(icon))
    } else {
        egui::ViewportBuilder::default()
            .with_inner_size([520.0, 320.0])
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let _ = eframe::run_native(
        "BlindEye Wallet + Miner",
        options,
        Box::new(|_cc| Ok(Box::<WalletApp>::default())),
    );
    Ok(())
}

struct WalletApp {
    node: Node,
    wallet: Wallet,
    active_tab: Tab,
    logo_texture: Option<egui::TextureHandle>,
    spendable_balance: u64,
    send_to: String,
    send_amount: String,
    send_fee: String,
    send_priority: SendPriority,
    seed_phrase_input: String,
    show_seed_phrase: bool,
    show_private_key: bool,
    wallet_state_path: String,
    wallet_backup_path: String,
    rpc_bind_addr: String,
    rpc_advertise_url: String,
    rpc_registry_url: String,
    auto_publish_rpc: bool,
    discovered_public_rpcs: Vec<PublicRpcEndpoint>,
    mining_worker_count: String,
    mine_empty_blocks: bool,
    message: String,
    pending_transfers: Vec<PendingTransfer>,
    wallet_history: Vec<WalletTransactionRecord>,
    wallet_password: String,
    p2p_manager: Option<Arc<P2PManager>>,
    peer_count: usize,
    last_observed_height: u64,
    last_observed_mempool_size: usize,
    last_notified_block_hash: Option<[u8; 32]>,
    last_auto_sync_attempt: Instant,
    sync_in_progress: Arc<AtomicBool>,
    sync_result_message: Arc<Mutex<Option<String>>>,
    app_started_at: Instant,
    splash_window_restored: bool,
    show_password_dialog: bool,
    password_input: String,
    password_confirm: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Balance,
    Send,
    Receive,
    Mining,
    Network,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SendPriority {
    Standard,
    Instant,
}

struct PendingTransfer {
    txid: [u8; 32],
    to: String,
    amount: u64,
    fee: u64,
}

impl Default for WalletApp {
    fn default() -> Self {
        let wallet_state_path = default_wallet_state_path();
        let wallet_backup_path = default_wallet_backup_path();
        let wallet_state_exists = wallet_state_path.exists();
        let node = load_or_create_node().unwrap_or_else(|_| Node::in_memory(None));
        let initial_status = node.get_status();
        let wallet_password = String::new();
        let wallet = if wallet_state_exists {
            Wallet::load_or_create_state(&wallet_state_path, &wallet_password)
                .unwrap_or_else(|_| Wallet::new())
        } else {
            Wallet::new()
        };

        let node_arc = Arc::new(node.clone());
        let listen_addr: std::net::SocketAddr = "0.0.0.0:30303"
            .parse()
            .unwrap_or("0.0.0.0:30303".parse().unwrap());
        let p2p_manager = Arc::new(P2PManager::new(listen_addr, node_arc, 16));
        let p2p_manager_clone = p2p_manager.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| {
                eprintln!("[P2P] Failed to create tokio runtime");
                return tokio::runtime::Runtime::new().unwrap();
            });
            rt.block_on(async {
                if let Err(e) = p2p_manager_clone.start().await {
                    eprintln!("[P2P] Startup error: {}", e);
                }
            });
        });

        let mut app = Self {
            node,
            wallet,
            active_tab: Tab::Balance,
            logo_texture: None,
            spendable_balance: 0,
            send_to: String::new(),
            send_amount: String::new(),
            send_fee: "0".to_string(),
            send_priority: SendPriority::Standard,
            seed_phrase_input: String::new(),
            show_seed_phrase: false,
            show_private_key: false,
            wallet_state_path: wallet_state_path.display().to_string(),
            wallet_backup_path: wallet_backup_path.display().to_string(),
            rpc_bind_addr: "127.0.0.1:18443".to_string(),
            rpc_advertise_url: String::new(),
            rpc_registry_url: std::env::var("BLINDEYE_RPC_REGISTRY_URL")
                .unwrap_or_else(|_| DEFAULT_PUBLIC_RPC_REGISTRY_URL.to_string()),
            auto_publish_rpc: true,
            discovered_public_rpcs: Vec::new(),
            mining_worker_count: MiningSettings::default().worker_count.to_string(),
            mine_empty_blocks: true,
            message: String::new(),
            pending_transfers: Vec::new(),
            wallet_history: Vec::new(),
            wallet_password,
            p2p_manager: Some(p2p_manager),
            peer_count: 0,
            last_observed_height: initial_status.best_height,
            last_observed_mempool_size: initial_status.mempool_size,
            last_notified_block_hash: None,
            last_auto_sync_attempt: Instant::now(),
            sync_in_progress: Arc::new(AtomicBool::new(false)),
            sync_result_message: Arc::new(Mutex::new(None)),
            app_started_at: Instant::now(),
            splash_window_restored: false,
            show_password_dialog: !wallet_state_exists,
            password_input: String::new(),
            password_confirm: String::new(),
        };
        
        // Only save if wallet exists with password, otherwise wait for password dialog
        if wallet_state_exists {
            let _ = app.wallet.save_to_file(&app.wallet_state_path, &app.wallet_password);
            app.refresh_wallet_state();
            app.message = format!(
                "Wallet loaded. Address {} is ready. Mining is off by default.",
                app.wallet.address
            );
        } else {
            app.message = "Please set a password to protect your new wallet.".to_string();
        }
        app
    }
}

impl Drop for WalletApp {
    fn drop(&mut self) {
        self.node.stop_continuous_mining();
        self.node.rpc_server.stop();
        let _ = self.wallet.save_to_file(&self.wallet_state_path, &self.wallet_password);
        let _ = self.node.save_state();
    }
}

impl eframe::App for WalletApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx);
        self.ensure_logo_texture(ctx);

        if self.show_startup_splash(ctx) {
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }

        if !self.splash_window_restored {
            self.restore_main_window(ctx);
        }

        self.poll_background_state();

        // Show password dialog if wallet is new
        if self.show_password_dialog {
            let mut is_open = true;
            egui::Window::new("Set Wallet Password")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .open(&mut is_open)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label("Create a password to protect your wallet");
                        ui.add_space(8.0);
                        
                        ui.label("Password:");
                        ui.text_edit_singleline(&mut self.password_input);
                        ui.add_space(4.0);
                        
                        ui.label("Confirm Password:");
                        ui.text_edit_singleline(&mut self.password_confirm);
                        ui.add_space(12.0);
                        
                        let passwords_match = !self.password_input.is_empty() && self.password_input == self.password_confirm;
                        
                        if ui.add_enabled(passwords_match, egui::Button::new("Create Wallet")).clicked() {
                            self.wallet_password = self.password_input.clone();
                            match self.wallet.save_to_file(&self.wallet_state_path, &self.wallet_password) {
                                Ok(()) => {
                                    self.show_password_dialog = false;
                                    self.password_input.clear();
                                    self.password_confirm.clear();
                                    self.refresh_wallet_state();
                                    self.message = format!(
                                        "Wallet created and encrypted with your password. Address: {}. Your seed phrase is on the Receive tab.",
                                        self.wallet.address
                                    );
                                }
                                Err(err) => {
                                    self.message = format!("Error saving wallet: {}", err);
                                }
                            }
                        }
                        
                        if !passwords_match && !self.password_input.is_empty() && !self.password_confirm.is_empty() {
                            ui.small("Passwords do not match");
                        }
                    });
                });
            if !is_open {
                self.show_password_dialog = false;
            }
        }

        egui::TopBottomPanel::top("app_header")
            .frame(
                egui::Frame::none()
                    .fill(COLOR_NEAR_BLACK)
                    .inner_margin(egui::Margin::symmetric(18.0, 10.0)),
            )
            .exact_height(84.0)
            .show(ctx, |ui| {
                self.show_header(ui);
            });

        egui::SidePanel::left("app_sidebar")
            .frame(
                egui::Frame::none()
                    .fill(COLOR_CHARCOAL)
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0)),
            )
            .resizable(false)
            .exact_width(240.0)
            .show(ctx, |ui| {
                self.show_sidebar(ui);
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(COLOR_PANEL)
                    .inner_margin(egui::Margin::symmetric(18.0, 16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_source("main_content_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if !self.message.is_empty() {
                            section_card(ui, "Status", |ui| {
                                ui.label(&self.message);
                            });
                            ui.add_space(8.0);
                        }

                        match self.active_tab {
                            Tab::Balance => self.show_balance_tab(ui),
                            Tab::Send => self.show_send_tab(ui),
                            Tab::Receive => self.show_receive_tab(ui),
                            Tab::Mining => self.show_mining_tab(ui),
                            Tab::Network => self.show_network_tab(ui),
                        }
                    });
            });

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

impl WalletApp {
    fn ensure_logo_texture(&mut self, ctx: &egui::Context) {
        if self.logo_texture.is_none() {
            self.logo_texture = load_logo_texture(ctx);
        }
    }

    fn show_startup_splash(&mut self, ctx: &egui::Context) -> bool {
        let splash_duration = 1.15_f32;
        let elapsed = self.app_started_at.elapsed().as_secs_f32();
        if elapsed >= splash_duration {
            return false;
        }

        let progress = (elapsed / splash_duration).clamp(0.0, 1.0);
        let zoom_progress = ease_out_cubic(progress);
        let fade_progress = ((progress - 0.8) / 0.2).clamp(0.0, 1.0);
        let alpha = (1.0 - ease_in_cubic(fade_progress)).clamp(0.0, 1.0);
        let rotation = TAU * (0.2 + (3.2 * zoom_progress));
        let icon_size = egui::lerp(120.0..=360.0, zoom_progress);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let painter = ui.painter();
                let overlay_alpha = (alpha * 255.0).round() as u8;
                let title_alpha = (alpha * 230.0).round() as u8;

                if let Some(texture) = &self.logo_texture {
                    paint_rotating_texture(
                        painter,
                        texture,
                        rect.center() + egui::vec2(0.0, -18.0),
                        icon_size,
                        rotation,
                        egui::Color32::from_rgba_unmultiplied(
                            255,
                            255,
                            255,
                            overlay_alpha,
                        ),
                    );
                }

                painter.text(
                    rect.center() + egui::vec2(0.0, rect.height() * 0.28),
                    egui::Align2::CENTER_CENTER,
                    "BLINDEYE",
                    egui::FontId::proportional(36.0),
                    egui::Color32::from_rgba_unmultiplied(
                        COLOR_SILVER.r(),
                        COLOR_SILVER.g(),
                        COLOR_SILVER.b(),
                        title_alpha,
                    ),
                );
            });

        true
    }

    fn restore_main_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1180.0, 860.0)));
        self.splash_window_restored = true;
    }

    fn broadcast_transaction_to_peers(&self, transaction: transaction::Transaction) {
        let Some(p2p_manager) = &self.p2p_manager else {
            return;
        };

        let manager = p2p_manager.clone();
        thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else {
                eprintln!("[P2P] Unable to create runtime for transaction broadcast");
                return;
            };
            runtime.block_on(async move {
                manager.broadcast_transaction(&transaction).await;
            });
        });
    }

    fn broadcast_block_to_peers(&self, block: block::Block) {
        let Some(p2p_manager) = &self.p2p_manager else {
            return;
        };

        let manager = p2p_manager.clone();
        thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Runtime::new() else {
                eprintln!("[P2P] Unable to create runtime for block broadcast");
                return;
            };
            runtime.block_on(async move {
                manager.broadcast_block(&block).await;
            });
        });
    }

    fn clear_sensitive_wallet_views(&mut self) -> bool {
        let had_sensitive_view = self.show_seed_phrase || self.show_private_key;
        self.show_seed_phrase = false;
        self.show_private_key = false;
        had_sensitive_view
    }

    fn set_active_tab(&mut self, tab: Tab) {
        if self.active_tab == tab {
            return;
        }

        if self.active_tab == Tab::Receive
            && tab != Tab::Receive
            && self.clear_sensitive_wallet_views()
        {
            self.message = "Sensitive wallet data hidden after leaving the Receive tab".to_string();
        }

        self.active_tab = tab;
    }

    fn connected_peer_count(&self, status: &node::NodeStatus) -> usize {
        self.peer_count.max(status.connected_peers)
    }

    fn refresh_public_rpc_registry(&mut self) {
        match rpc_registry::fetch_public_rpcs(&self.rpc_registry_url) {
            Ok(endpoints) => {
                let count = endpoints.len();
                self.discovered_public_rpcs = endpoints;
                self.message = if count == 0 {
                    "Registry checked successfully, but no open public RPC endpoints were reported."
                        .to_string()
                } else {
                    format!("Loaded {} public RPC endpoint(s) from the registry.", count)
                };
            }
            Err(err) => {
                self.message = format!("Public RPC registry fetch failed: {}", err);
            }
        }
    }

    fn publish_current_rpc_to_registry(&mut self) -> Result<String, String> {
        let rpc = self.node.rpc_server.snapshot();
        if !rpc.active {
            return Err("Start the RPC server before publishing it.".to_string());
        }
        if !rpc.allow_remote {
            return Err(
                "This RPC is localhost-only right now. Switch the bind to 0.0.0.0 before publishing."
                    .to_string(),
            );
        }
        if rpc.advertised_url.trim().is_empty() {
            return Err(
                "The RPC server does not have a published endpoint yet. Set or auto-generate an advertised URL first."
                    .to_string(),
            );
        }

        let response = rpc_registry::publish_public_rpc(
            &self.rpc_registry_url,
            &rpc.advertised_url,
            &self.wallet.address,
        )?;
        if let Ok(endpoints) = rpc_registry::fetch_public_rpcs(&self.rpc_registry_url) {
            self.discovered_public_rpcs = endpoints;
        }
        Ok(response)
    }

    fn maybe_publish_current_rpc_to_registry(&mut self, event: &str) -> Option<String> {
        if !self.auto_publish_rpc || self.rpc_registry_url.trim().is_empty() {
            return None;
        }

        match self.publish_current_rpc_to_registry() {
            Ok(result) => Some(format!("Registry updated after {}. {}", event, result)),
            Err(err) => Some(format!("Registry publish skipped after {}: {}", event, err)),
        }
    }

    fn queue_block_sync(&mut self, manual: bool) {
        let Some(p2p_manager) = &self.p2p_manager else {
            if manual {
                self.message = "P2P networking is not initialized".to_string();
            }
            return;
        };
        if self.connected_peer_count(&self.node.get_status()) == 0 {
            if manual {
                self.message = "No peers are connected for block sync".to_string();
            }
            return;
        }

        if self.sync_in_progress.swap(true, Ordering::SeqCst) {
            if manual {
                self.message = "Block sync is already running".to_string();
            }
            return;
        }

        self.last_auto_sync_attempt = Instant::now();
        if manual {
            self.message = "Block sync started".to_string();
        }

        let manager = p2p_manager.clone();
        let node = self.node.clone();
        let sync_in_progress = self.sync_in_progress.clone();
        let sync_result_message = self.sync_result_message.clone();
        thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(err) => {
                    *sync_result_message.lock().unwrap() =
                        Some(format!("Unable to start block sync: {}", err));
                    sync_in_progress.store(false, Ordering::SeqCst);
                    return;
                }
            };

            let result = runtime.block_on(async { manager.synchronize_blocks().await });
            let next_message = match result {
                Ok(imported_blocks) if manual => Some(format!(
                    "Block sync finished. Imported {} block(s). Height is now {}.",
                    imported_blocks,
                    node.get_best_height()
                )),
                Ok(imported_blocks) if imported_blocks > 0 => Some(format!(
                    "Auto-sync imported {} block(s). Height is now {}.",
                    imported_blocks,
                    node.get_best_height()
                )),
                Ok(_) => None,
                Err(err) => Some(format!("Block sync failed: {}", err)),
            };

            if let Some(message) = next_message {
                *sync_result_message.lock().unwrap() = Some(message);
            }
            sync_in_progress.store(false, Ordering::SeqCst);
        });
    }

    fn poll_background_state(&mut self) {
        if let Some(p2p_manager) = &self.p2p_manager {
            self.peer_count = p2p_manager.peer_count_now();
        } else {
            self.peer_count = 0;
        }

        if let Some(message) = self.sync_result_message.lock().unwrap().take() {
            self.message = message;
        }

        let status = self.node.get_status();
        let previous_height = self.last_observed_height;
        let previous_mempool_size = self.last_observed_mempool_size;
        let height_changed = status.best_height != previous_height;
        let mempool_changed = status.mempool_size != previous_mempool_size;

        if height_changed || mempool_changed {
            self.refresh_wallet_state();
            self.last_observed_height = status.best_height;
            self.last_observed_mempool_size = status.mempool_size;
        }

        let mining = self.node.mining_snapshot();
        if mining.last_block_hash != self.last_notified_block_hash {
            self.last_notified_block_hash = mining.last_block_hash;
            if let Some(block_hash) = mining.last_block_hash {
                if let Some(block) = self.node.get_block(&block_hash) {
                    self.broadcast_block_to_peers(block.clone());
                    self.refresh_wallet_state();
                    let reward = block
                        .transactions
                        .first()
                        .map(|transaction| transaction.total_output_value())
                        .unwrap_or(0);
                    self.message = format!(
                        "Accepted block {} at height {}. Reward {} BEC. Difficulty bits 0x{:08x}. Balance {} BEC.",
                        shorten_middle(&hex::encode(block_hash), 10),
                        block.header.height,
                        format_bec_amount(reward),
                        block.header.bits,
                        self.wallet.balance_bec()
                    );
                    if let Some(registry_note) =
                        self.maybe_publish_current_rpc_to_registry("accepted block")
                    {
                        self.message = format!("{} {}", self.message, registry_note);
                    }
                }
            }
        } else if height_changed && status.best_height > previous_height {
            self.message = format!(
                "Chain advanced to height {}. Balance {} BEC.",
                status.best_height,
                self.wallet.balance_bec()
            );
        }

        if self.p2p_manager.is_some()
            && self.connected_peer_count(&status) > 0
            && !self.sync_in_progress.load(Ordering::SeqCst)
            && self.last_auto_sync_attempt.elapsed() >= Duration::from_secs(5)
        {
            self.queue_block_sync(false);
        }
    }

    fn show_header(&mut self, ui: &mut egui::Ui) {
        let status = self.node.get_status();
        let connected_peers = self.connected_peer_count(&status);
        let mining = self.node.mining_snapshot();

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if let Some(texture) = &self.logo_texture {
                ui.image((texture.id(), egui::vec2(54.0, 54.0)));
            }

            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("BlindEye Wallet + Miner")
                        .size(28.0)
                        .strong()
                        .color(COLOR_SILVER),
                );
                ui.label(
                    egui::RichText::new(
                        "Persistent BEC wallet, peer-sync-ready node state, and continuous mining controls",
                    )
                    .color(COLOR_MUTED),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mining_label = if mining.active {
                    "Mining On"
                } else {
                    "Mining Off"
                };
                ui.label(
                    egui::RichText::new(mining_label)
                        .strong()
                        .background_color(if mining.active {
                            COLOR_CYAN
                        } else {
                            COLOR_BLUE
                        })
                        .color(COLOR_NEAR_BLACK),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Height {}", status.best_height))
                        .strong()
                        .background_color(COLOR_BLUE)
                        .color(COLOR_SILVER),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("Peers {}", connected_peers))
                        .strong()
                        .background_color(COLOR_CARD_SOFT)
                        .color(COLOR_SILVER),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} BEC spendable",
                        format_bec_amount(self.spendable_balance)
                    ))
                    .strong()
                    .color(COLOR_SILVER),
                );
            });
        });
        ui.add_space(10.0);
    }

    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);

        if let Some(texture) = &self.logo_texture {
            ui.vertical_centered(|ui| {
                ui.image((texture.id(), egui::vec2(72.0, 72.0)));
            });
            ui.add_space(10.0);
        }

        ui.vertical(|ui| {
            if nav_button(ui, self.active_tab == Tab::Balance, "Overview") {
                self.set_active_tab(Tab::Balance);
            }
            if nav_button(ui, self.active_tab == Tab::Send, "Send") {
                self.set_active_tab(Tab::Send);
            }
            if nav_button(ui, self.active_tab == Tab::Receive, "Receive") {
                self.set_active_tab(Tab::Receive);
            }
            if nav_button(ui, self.active_tab == Tab::Mining, "Mining") {
                self.set_active_tab(Tab::Mining);
            }
            if nav_button(ui, self.active_tab == Tab::Network, "Network") {
                self.set_active_tab(Tab::Network);
            }
        });

        ui.add_space(16.0);

        section_card(ui, "Persistent State", |ui| {
            ui.label("Wallet file:");
            ui.monospace(&self.wallet_state_path);
            ui.add_space(4.0);
            ui.label("Blockchain file:");
            ui.monospace(self.node.storage_path.as_ref().display().to_string());
            ui.add_space(6.0);
            ui.small(
                "Wallet and chain state are stored in your user data directory so this node can relaunch cleanly and participate in wider peer sync.",
            );
        });

        ui.add_space(12.0);

        section_card(ui, "Quick Stats", |ui| {
            let status = self.node.get_status();
            let connected_peers = self.connected_peer_count(&status);
            ui.label(format!(
                "Address: {}",
                shorten_middle(&self.wallet.address, 10)
            ));
            ui.label(format!("Height: {}", status.best_height));
            ui.label(format!("Mempool: {}", status.mempool_size));
            ui.label(format!("Peers: {}", connected_peers));
            ui.label(format!("Hash Rate: {:.0} H/s", status.hash_rate));
        });
    }

    fn reserved_outpoints(&self) -> std::collections::HashSet<transaction::OutPoint> {
        self.node.mempool.lock().unwrap().reserved_outpoints()
    }

    fn refresh_wallet_state(&mut self) {
        let reserved_outpoints = self.reserved_outpoints();
        {
            let blockchain = self.node.blockchain.lock().unwrap();
            self.wallet.sync_balance(&blockchain);
            self.spendable_balance = self
                .wallet
                .spendable_balance(&blockchain, &reserved_outpoints);
            self.wallet_history =
                blockchain.wallet_transaction_history(&self.wallet.address_bytes());
        }

        let mempool = self.node.mempool.lock().unwrap();
        self.pending_transfers
            .retain(|pending| mempool.contains_transaction(&pending.txid));
        drop(mempool);

        let status = self.node.get_status();
        self.last_observed_height = status.best_height;
        self.last_observed_mempool_size = status.mempool_size;
    }

    fn current_send_preview(&self) -> Result<Option<TransactionPreview>, String> {
        if self.send_to.trim().is_empty() || self.send_amount.trim().is_empty() {
            return Ok(None);
        }

        let amount = parse_bec_amount(&self.send_amount)?;
        let fee = parse_bec_amount(&self.send_fee)?;

        let blockchain = self.node.blockchain.lock().unwrap();
        let reserved_outpoints = self.reserved_outpoints();
        self.wallet
            .build_transaction_preview(
                &blockchain,
                &reserved_outpoints,
                self.send_to.trim(),
                amount,
                fee,
            )
            .map(Some)
    }

    fn suggested_fee_for_priority(&self) -> Result<Option<u64>, String> {
        if self.send_to.trim().is_empty() || self.send_amount.trim().is_empty() {
            return Ok(None);
        }

        let amount = parse_bec_amount(&self.send_amount)?;
        let blockchain = self.node.blockchain.lock().unwrap();
        let reserved_outpoints = self.reserved_outpoints();
        let status = self.node.get_status();
        let fee_rate = match self.send_priority {
            SendPriority::Standard => status.standard_fee_rate,
            SendPriority::Instant => status.instant_fee_rate,
        };

        self.wallet
            .estimate_fee_for_rate(
                &blockchain,
                &reserved_outpoints,
                self.send_to.trim(),
                amount,
                fee_rate,
            )
            .map(Some)
    }

    fn show_balance_tab(&mut self, ui: &mut egui::Ui) {
        let status = self.node.get_status();
        let connected_peers = self.connected_peer_count(&status);
        ui.columns(2, |columns| {
            section_card(&mut columns[0], "Wallet Overview", |ui| {
                ui.label(format!("Address: {}", self.wallet.address));
                ui.label(format!("On-chain Balance: {} BEC", self.wallet.balance_bec()));
                ui.label(format!(
                    "Spendable Balance: {} BEC",
                    format_bec_amount(self.spendable_balance)
                ));
                ui.label(format!(
                    "Pending Transactions: {}",
                    self.pending_transfers.len()
                ));
                ui.small("Seed phrase and private key are hidden by default. Open Receive if you need to reveal them deliberately.");

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Copy Address").clicked() {
                        ui.ctx().copy_text(self.wallet.address.clone());
                        self.message = "Address copied to clipboard".to_string();
                    }

                    if ui.button("Refresh").clicked() {
                        self.refresh_wallet_state();
                        let _ = self.node.save_state();
                        self.message = format!(
                            "State refreshed. Height {} with {} tx(s) in mempool.",
                            status.best_height, status.mempool_size
                        );
                    }
                });

                if ui.button("Generate New Wallet").clicked() {
                    self.node.stop_continuous_mining();
                    self.wallet = Wallet::new();
                    self.clear_sensitive_wallet_views();
                    if let Err(err) = self.wallet.save_to_file(&self.wallet_state_path, &self.wallet_password) {
                        self.message = err;
                    } else {
                        self.pending_transfers.clear();
                        self.refresh_wallet_state();
                        self.message = format!(
                            "Generated a new wallet. New address: {}. Reveal the seed phrase in Receive to back it up offline.",
                            self.wallet.address
                        );
                    }
                }
            });

            section_card(&mut columns[1], "Node Overview", |ui| {
                ui.label(format!("Node Height: {}", status.best_height));
                ui.label(format!("Connected Peers: {}", connected_peers));
                ui.label(format!("Mempool Size: {}", status.mempool_size));
                ui.label(format!(
                    "Consensus Threshold: {}",
                    status.consensus_threshold
                ));
                ui.label(format!("Mining Active: {}", status.mining_active));
                ui.label(format!("Hash Rate: {:.0} H/s", status.hash_rate));
                ui.label(format!("Total Hashes: {}", status.total_hashes));
                ui.label(format!("RPC Active: {}", status.rpc_active));
                ui.label(format!("RPC Bind: {}", status.rpc_bind_addr));
                if !status.rpc_advertised_url.is_empty() {
                    ui.label(format!("RPC Public URL: {}", status.rpc_advertised_url));
                }
                ui.label(format!("Remote RPC Enabled: {}", status.rpc_allow_remote));
                ui.label(format!(
                    "Standard Fee Rate: {} units/byte",
                    status.standard_fee_rate
                ));
                ui.label(format!(
                    "Instant Fee Rate: {} units/byte",
                    status.instant_fee_rate
                ));
                ui.add_space(6.0);
                ui.label("Blockchain storage:");
                ui.monospace(self.node.storage_path.as_ref().display().to_string());
            });
        });

        if !self.pending_transfers.is_empty() {
            ui.add_space(12.0);
            section_card(ui, "Recent Pending Sends", |ui| {
                for transfer in self.pending_transfers.iter().rev().take(5) {
                    ui.label(format!(
                        "{} BEC to {} (fee {} BEC, tx {})",
                        format_bec_amount(transfer.amount),
                        transfer.to,
                        format_bec_amount(transfer.fee),
                        hex::encode(transfer.txid)
                    ));
                }
            });
        }

        ui.add_space(12.0);
        section_card(ui, "Transaction History", |ui| {
            if self.wallet_history.is_empty() {
                ui.small("No confirmed wallet transactions yet.");
            } else {
                egui::ScrollArea::vertical()
                    .id_source("wallet_history_scroll")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for record in self.wallet_history.iter().take(20) {
                            let direction = match record.direction {
                                WalletTransactionDirection::Incoming => "Incoming",
                                WalletTransactionDirection::Outgoing => "Outgoing",
                                WalletTransactionDirection::MiningReward => "Mining Reward",
                                WalletTransactionDirection::SelfTransfer => "Self Transfer",
                            };
                            let counterparty = record
                                .counterparty
                                .as_deref()
                                .map(|value| shorten_middle(value, 10))
                                .unwrap_or_else(|| "n/a".to_string());

                            ui.label(format!(
                                "{} {} BEC at height {}",
                                direction,
                                format_bec_amount(record.amount),
                                record.height
                            ));
                            ui.small(format!(
                                "Counterparty: {} | Fee: {} BEC | Time: {}",
                                counterparty,
                                format_bec_amount(record.fee),
                                format_unix_timestamp(record.timestamp)
                            ));
                            ui.small(format!(
                                "TX {} | inputs {} | outputs {}",
                                shorten_middle(&hex::encode(record.txid), 10),
                                record.transaction.inputs.len(),
                                record.transaction.outputs.len()
                            ));
                            ui.add_space(8.0);
                        }
                    });
            }
        });
    }

    fn show_send_tab(&mut self, ui: &mut egui::Ui) {
        let preview_result = self.current_send_preview();
        let suggested_fee = self.suggested_fee_for_priority();
        let status = self.node.get_status();
        ui.columns(2, |columns| {
            section_card(&mut columns[0], "Send Transaction", |ui| {
                ui.label("Recipient Address");
                ui.text_edit_singleline(&mut self.send_to);

                ui.label("Amount (BEC)");
                ui.text_edit_singleline(&mut self.send_amount);

                ui.label("Confirmation Mode");
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.send_priority,
                        SendPriority::Standard,
                        "Standard (~60s)",
                    );
                    ui.selectable_value(
                        &mut self.send_priority,
                        SendPriority::Instant,
                        "Instant-Send",
                    );
                });

                ui.label("Fee (BEC)");
                ui.text_edit_singleline(&mut self.send_fee);

                match &suggested_fee {
                    Ok(Some(fee)) => {
                        ui.label(format!(
                            "Suggested {} fee: {} BEC",
                            match self.send_priority {
                                SendPriority::Standard => "standard",
                                SendPriority::Instant => "instant",
                            },
                            format_bec_amount(*fee)
                        ));
                        if ui.button("Use Suggested Fee").clicked() {
                            self.send_fee = format_bec_amount(*fee);
                        }
                    }
                    Ok(None) => {
                        ui.small("Enter a destination and amount to get a suggested fee.");
                    }
                    Err(err) => {
                        ui.small(format!("Fee suggestion unavailable: {}", err));
                    }
                }

                ui.small(
                    "Balances and send amounts are shown as decimal BEC, not raw atomic units.",
                );
                ui.small(
                    "Standard sends usually become eligible for mining after about 60 seconds. Instant-Send pays only a slightly higher fee rate and gets next-block priority.",
                );
                ui.small(
                    "Transparent sends are the only wallet send mode right now. Optional anonymous sends are not integrated yet.",
                );
                ui.small(format!(
                    "Current network fee rates: standard {} units/byte, instant {} units/byte.",
                    status.standard_fee_rate, status.instant_fee_rate
                ));
            });

            section_card(
                &mut columns[1],
                "Transaction Preview",
                |ui| match &preview_result {
                    Ok(Some(preview)) => {
                        let fee_rate = preview
                            .fee
                            .div_ceil(preview.transaction.estimated_size().max(1));
                        let confirmation_lane = if fee_rate >= status.instant_fee_rate {
                            "Instant-Send priority (next eligible block)"
                        } else if fee_rate >= status.standard_fee_rate {
                            "Standard queue (~60s eligibility)"
                        } else {
                            "Below current network minimum"
                        };
                        ui.label(format!("Recipient: {}", preview.recipient_address));
                        ui.label(format!(
                            "Send Amount: {} BEC",
                            format_bec_amount(preview.amount)
                        ));
                        ui.label(format!("Fee: {} BEC", format_bec_amount(preview.fee)));
                        ui.label(format!(
                            "Total Debit: {} BEC",
                            format_bec_amount(preview.total_debit())
                        ));
                        ui.label(format!(
                            "Selected Inputs: {} BEC",
                            format_bec_amount(preview.selected_input_value)
                        ));
                        ui.label(format!(
                            "Change Back To Wallet: {} BEC",
                            format_bec_amount(preview.change)
                        ));
                        ui.label(format!("Inputs Used: {}", preview.input_count()));
                        ui.label(format!(
                            "Preview TXID: {}",
                            hex::encode(preview.transaction.txid())
                        ));
                        ui.label(format!("Priority Lane: {}", confirmation_lane));
                        ui.label(format!("Effective Fee Rate: {} units/byte", fee_rate));
                    }
                    Ok(None) => {
                        ui.small("Enter a destination and amount to build a preview.");
                    }
                    Err(err) => {
                        ui.label(format!("Preview unavailable: {}", err));
                    }
                },
            );
        });

        ui.add_space(12.0);

        if ui.button("Send").clicked() {
            if self.send_to.trim().is_empty() || self.send_amount.trim().is_empty() {
                self.message = "Please fill in all fields".to_string();
                return;
            }

            let preview = match preview_result {
                Ok(Some(preview)) => preview,
                Ok(None) => {
                    self.message = "Please fill in all fields".to_string();
                    return;
                }
                Err(err) => {
                    self.message = err;
                    return;
                }
            };

            let txid = preview.transaction.txid();
            match self.node.submit_transaction(preview.transaction.clone()) {
                Ok(()) => {
                    self.broadcast_transaction_to_peers(preview.transaction.clone());
                    self.wallet.add_transaction(preview.transaction.clone());
                    self.pending_transfers.push(PendingTransfer {
                        txid,
                        to: preview.recipient_address,
                        amount: preview.amount,
                        fee: preview.fee,
                    });
                    self.send_to.clear();
                    self.send_amount.clear();
                    self.send_fee = "0".to_string();
                    self.refresh_wallet_state();
                    self.message = format!(
                        "Submitted transaction {}. Spendable balance is now {} BEC",
                        hex::encode(txid),
                        format_bec_amount(self.spendable_balance)
                    );
                }
                Err(err) => {
                    self.message = err;
                }
            }
        }
    }

    fn show_receive_tab(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            section_card(&mut columns[0], "Receive Funds", |ui| {
                ui.label(format!("Your Address: {}", self.wallet.address));

                if let Ok(code) = QrCode::new(self.wallet.address.as_bytes()) {
                    let rendered = code.render::<unicode::Dense1x2>().build();
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        ui.monospace(rendered);
                    });
                }

                if ui.button("Copy Address").clicked() {
                    ui.ctx().copy_text(self.wallet.address.clone());
                    self.message = "Address copied to clipboard".to_string();
                }
            });

            section_card(&mut columns[1], "Recovery Controls", |ui| {
                ui.small(
                    "Sensitive wallet material stays hidden until you explicitly reveal it on this screen.",
                );
                ui.add_space(6.0);

                if self.show_seed_phrase {
                    ui.colored_label(
                        COLOR_CYAN,
                        "Seed phrase revealed. Keep it offline and never share it.",
                    );
                    ui.monospace(self.wallet.seed_phrase());
                    ui.horizontal(|ui| {
                        if ui.button("Copy Seed Phrase").clicked() {
                            ui.ctx().copy_text(self.wallet.seed_phrase().to_string());
                            self.message = "Seed phrase copied to clipboard".to_string();
                        }
                        if ui.button("Hide Seed Phrase").clicked() {
                            self.show_seed_phrase = false;
                            self.message = "Seed phrase hidden".to_string();
                        }
                    });
                } else if ui.button("Reveal Seed Phrase").clicked() {
                    self.show_seed_phrase = true;
                    self.message = "Seed phrase revealed on this screen".to_string();
                }

                ui.add_space(8.0);

                if self.show_private_key {
                    ui.colored_label(
                        COLOR_CYAN,
                        "Private key revealed. Treat it like cash.",
                    );
                    ui.monospace(self.wallet.private_key_hex());
                    ui.horizontal(|ui| {
                        if ui.button("Copy Private Key").clicked() {
                            ui.ctx().copy_text(self.wallet.private_key_hex());
                            self.message = "Private key copied to clipboard".to_string();
                        }
                        if ui.button("Hide Private Key").clicked() {
                            self.show_private_key = false;
                            self.message = "Private key hidden".to_string();
                        }
                    });
                } else if ui.button("Reveal Private Key").clicked() {
                    self.show_private_key = true;
                    self.message = "Private key revealed on this screen".to_string();
                }
            });
        });

        ui.add_space(12.0);

        section_card(ui, "Wallet Storage", |ui| {
            ui.label("Wallet State Path");
            ui.text_edit_singleline(&mut self.wallet_state_path);
            ui.label("Backup Path");
            ui.text_edit_singleline(&mut self.wallet_backup_path);

            ui.horizontal(|ui| {
                if ui.button("Save Wallet").clicked() {
                    match self.wallet.save_to_file(&self.wallet_state_path, &self.wallet_password) {
                        Ok(()) => {
                            self.message =
                                format!("Saved wallet state to {}", self.wallet_state_path);
                        }
                        Err(err) => self.message = err,
                    }
                }
                if ui.button("Backup Wallet").clicked() {
                    match self.wallet.save_to_file(&self.wallet_backup_path, &self.wallet_password) {
                        Ok(()) => {
                            self.message =
                                format!("Saved wallet backup to {}", self.wallet_backup_path);
                        }
                        Err(err) => self.message = err,
                    }
                }
                if ui.button("Restore Backup").clicked() {
                    match Wallet::load_from_file(&self.wallet_backup_path, &self.wallet_password) {
                        Ok(wallet) => {
                            self.wallet = wallet;
                            self.clear_sensitive_wallet_views();
                            if let Err(err) = self.wallet.save_to_file(&self.wallet_state_path, &self.wallet_password) {
                                self.message = err;
                            } else {
                                self.pending_transfers.clear();
                                self.refresh_wallet_state();
                                self.message =
                                    format!("Restored wallet from {}", self.wallet_backup_path);
                            }
                        }
                        Err(err) => self.message = err,
                    }
                }
            });
        });

        ui.add_space(12.0);

        section_card(ui, "Import Seed Phrase", |ui| {
            ui.text_edit_multiline(&mut self.seed_phrase_input);

            if ui.button("Import Seed Phrase").clicked() {
                match Wallet::from_mnemonic(self.seed_phrase_input.clone()) {
                    Ok(wallet) => {
                        self.node.stop_continuous_mining();
                        self.wallet = wallet;
                        self.clear_sensitive_wallet_views();
                        if let Err(err) = self.wallet.save_to_file(&self.wallet_state_path, &self.wallet_password) {
                            self.message = err;
                        } else {
                            self.pending_transfers.clear();
                            self.refresh_wallet_state();
                            self.message =
                                "Imported seed phrase and saved the wallet state".to_string();
                        }
                    }
                    Err(err) => self.message = err,
                }
            }
        });
    }

    fn show_mining_tab(&mut self, ui: &mut egui::Ui) {
        let mining = self.node.mining_snapshot();
        let rpc = self.node.rpc_server.snapshot();

        ui.columns(2, |columns| {
            section_card(&mut columns[0], "Mining Control", |ui| {
                ui.label(format!("Mining Address: {}", self.wallet.address));
                ui.label(format!("Wallet Balance: {} BEC", self.wallet.balance_bec()));
                ui.label(format!("Chain Height: {}", self.node.get_status().best_height));
                ui.label(format!("Mining Active: {}", mining.active));
                ui.label(format!("Workers In Use: {}", mining.worker_count));
                ui.label(format!("Hash Rate: {:.0} H/s", mining.hash_rate));
                ui.label(format!("Total Hashes: {}", mining.total_hashes));
                if let Some(last_block_hash) = mining.last_block_hash {
                    ui.label(format!("Last Block: {}", hex::encode(last_block_hash)));
                }

                ui.add_space(8.0);
                ui.label("Mining Settings");
                ui.horizontal(|ui| {
                    ui.label("Worker Count");
                    ui.text_edit_singleline(&mut self.mining_worker_count);
                });
                ui.checkbox(&mut self.mine_empty_blocks, "Mine empty blocks");
                ui.small("Default worker count keeps one CPU core free so the GUI stays responsive while mining.");

                ui.horizontal(|ui| {
                    if ui.button("Start Mining").clicked() {
                        match self.mining_worker_count.trim().parse::<usize>() {
                            Ok(worker_count) => {
                                let requested_worker_count = worker_count.max(1);
                                let safe_worker_count = thread::available_parallelism()
                                    .map(|parallelism| {
                                        requested_worker_count
                                            .min(parallelism.get().saturating_sub(1).max(1))
                                    })
                                    .unwrap_or(requested_worker_count);
                                match self.node.start_continuous_mining(
                                &self.wallet.address_bytes(),
                                MiningSettings {
                                    worker_count: safe_worker_count,
                                    mine_empty_blocks: self.mine_empty_blocks,
                                },
                                ) {
                                    Ok(()) => {
                                        let p2p_status = if self.p2p_manager.is_some() {
                                            " (P2P networking active)"
                                        } else {
                                            ""
                                        };
                                        self.message = if safe_worker_count != requested_worker_count {
                                            format!(
                                                "Continuous mining started with {} worker(s){} to keep the GUI responsive",
                                                safe_worker_count,
                                                p2p_status
                                            )
                                        } else {
                                            format!(
                                                "Continuous mining started with {} worker(s){}",
                                                safe_worker_count,
                                                p2p_status
                                            )
                                        };
                                        if let Some(registry_note) = self
                                            .maybe_publish_current_rpc_to_registry("mining start")
                                        {
                                            self.message =
                                                format!("{} {}", self.message, registry_note);
                                        }
                                    }
                                    Err(err) => self.message = err,
                                }
                            }
                            Err(_) => {
                                self.message = "Worker count must be a whole number".to_string();
                            }
                        }
                    }

                    if ui.button("Stop Mining").clicked() {
                        self.node.stop_continuous_mining();
                        self.refresh_wallet_state();
                        self.message = "Mining stopped".to_string();
                    }
                });
            });

            section_card(&mut columns[1], "Local RPC / Pool Wiring", |ui| {
                ui.horizontal(|ui| {
                    ui.label("RPC Bind");
                    ui.text_edit_singleline(&mut self.rpc_bind_addr);
                });
                ui.horizontal(|ui| {
                    if ui.button("Localhost").clicked() {
                        self.rpc_bind_addr = "127.0.0.1:18443".to_string();
                    }
                    if ui.button("Accept Remote").clicked() {
                        self.rpc_bind_addr = "0.0.0.0:18443".to_string();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Advertise URL");
                    ui.text_edit_singleline(&mut self.rpc_advertise_url);
                });
                if self.rpc_advertise_url.trim().is_empty() {
                    ui.small("Leave Advertise URL blank to auto-publish a usable endpoint from the bind address.");
                }
                ui.horizontal(|ui| {
                    if ui.button("Start RPC").clicked() {
                        match self.node.rpc_server.start(
                            self.node.clone(),
                            self.rpc_bind_addr.clone(),
                            Some(self.rpc_advertise_url.clone()),
                            self.wallet.address.clone(),
                        ) {
                            Ok(()) => {
                                self.message =
                                    format!("RPC server started on {}", self.rpc_bind_addr);
                                if self.auto_publish_rpc
                                    && !self.rpc_registry_url.trim().is_empty()
                                {
                                    match self.publish_current_rpc_to_registry() {
                                        Ok(result) => {
                                            self.message = format!(
                                                "RPC server started on {}. {}",
                                                self.rpc_bind_addr, result
                                            );
                                        }
                                        Err(err) => {
                                            self.message = format!(
                                                "RPC server started on {}. Registry publish skipped: {}",
                                                self.rpc_bind_addr, err
                                            );
                                        }
                                    }
                                }
                            }
                            Err(err) => self.message = err,
                        }
                    }
                    if ui.button("Stop RPC").clicked() {
                        self.node.rpc_server.stop();
                        self.message = "RPC server stopped".to_string();
                    }
                });
                ui.label(format!("RPC Active: {}", rpc.active));
                ui.label(format!("RPC Bind: {}", rpc.bind_addr));
                if !rpc.advertised_url.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Published RPC URL: {}", rpc.advertised_url));
                        if ui.button("Copy URL").clicked() {
                            ui.ctx().copy_text(rpc.advertised_url.clone());
                            self.message = "RPC URL copied to clipboard".to_string();
                        }
                    });
                }
                ui.label(format!("Remote RPC Enabled: {}", rpc.allow_remote));
                ui.small(
                    "Bitcoin-style default is local RPC first. Keep it on localhost unless you intentionally want pool workers or external operator tooling to connect.",
                );
                ui.small("Methods: getinfo, getmininginfo, getblocktemplate, submitblock, sendtransaction");
                if !rpc.advertised_url.is_empty() {
                    ui.monospace(format!(
                        "{{\"jsonrpc\":\"2.0\",\"method\":\"getinfo\",\"params\":{{}},\"id\":1}}  ->  {}",
                        rpc.advertised_url
                    ));
                }
            });
        });

        ui.add_space(12.0);

        section_card(ui, "Public RPC Registry", |ui| {
            ui.horizontal(|ui| {
                ui.label("Registry URL");
                ui.text_edit_singleline(&mut self.rpc_registry_url);
            });
            ui.checkbox(
                &mut self.auto_publish_rpc,
                "Auto-publish my RPC when remote RPC starts",
            );
            ui.small(
                "Point this at your hosted registry PHP file. The app can fetch open public RPC endpoints from it and publish this node there once remote RPC is enabled.",
            );

            ui.horizontal(|ui| {
                if ui.button("Refresh Public RPCs").clicked() {
                    self.refresh_public_rpc_registry();
                }
                if ui.button("Publish My RPC").clicked() {
                    match self.publish_current_rpc_to_registry() {
                        Ok(result) => self.message = result,
                        Err(err) => self.message = err,
                    }
                }
            });

            if self.rpc_registry_url.trim().is_empty() {
                ui.small("Set a registry URL before checking for public RPC endpoints.");
            } else if self.discovered_public_rpcs.is_empty() {
                ui.small(
                    "No public RPCs are cached yet. Refresh the registry URL above before deciding there are none.",
                );
            } else {
                egui::ScrollArea::vertical()
                    .id_source("public_rpc_registry_scroll")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for endpoint in self.discovered_public_rpcs.iter().take(20) {
                            ui.label(format!(
                                "{} | height {} | peers {}",
                                endpoint.rpc_url,
                                endpoint.best_height,
                                endpoint.connected_peers
                            ));
                            ui.small(format!(
                                "Owner: {} | Remote: {} | Source: {} | Last seen: {}",
                                if endpoint.owner_address.trim().is_empty() {
                                    "n/a"
                                } else {
                                    endpoint.owner_address.as_str()
                                },
                                endpoint.remote_enabled,
                                if endpoint.source.trim().is_empty() {
                                    "unknown"
                                } else {
                                    endpoint.source.as_str()
                                },
                                endpoint.last_seen
                            ));
                            ui.add_space(6.0);
                        }
                    });
                if let Some(example) = self.discovered_public_rpcs.first() {
                    ui.small(format!(
                        "HTTP bridge example: POST {{\"rpc_url\":\"{}\",\"method\":\"getinfo\",\"params\":{{}},\"id\":1}} to {}?action=proxy",
                        example.rpc_url,
                        self.rpc_registry_url.trim()
                    ));
                }
            }
        });

        ui.add_space(12.0);

        section_card(ui, "Mining Log", |ui| {
            egui::ScrollArea::vertical()
                .id_source("mining_log_scroll")
                .max_height(220.0)
                .show(ui, |ui| {
                    for line in mining.log_lines.iter().rev().take(100) {
                        ui.monospace(line);
                    }
                });
        });

        ui.add_space(12.0);

        section_card(ui, "RPC Log", |ui| {
            egui::ScrollArea::vertical()
                .id_source("rpc_log_scroll")
                .max_height(180.0)
                .show(ui, |ui| {
                    for line in rpc.log_lines.iter().rev().take(100) {
                        ui.monospace(line);
                    }
                });
        });
    }

    fn show_network_tab(&mut self, ui: &mut egui::Ui) {
        let status = self.node.get_status();
        let connected_peers = self.connected_peer_count(&status);
        let peer_snapshot = self
            .p2p_manager
            .as_ref()
            .map(|manager| manager.peers_now())
            .unwrap_or_default();
        let best_known_peer_height = peer_snapshot
            .iter()
            .map(|peer| peer.best_height)
            .max()
            .unwrap_or(0);
        let blocks_to_sync = best_known_peer_height.saturating_sub(status.best_height);
        let sync_in_progress = self.sync_in_progress.load(Ordering::SeqCst);

        section_card(ui, "P2P Network Status", |ui| {
            if self.p2p_manager.is_some() {
                ui.label("P2P Status: Listening on 0.0.0.0:30303 (public peer-ready)");
                ui.label(format!("Connected Peers: {}", connected_peers));
                ui.label(format!("Best Known Peer Height: {}", best_known_peer_height));
                ui.label(format!("Local Height: {}", status.best_height));
                ui.small("GUI mode can accept remote peers. Bootstrap one or more seed peers to join a wider public network.");
            } else {
                ui.label("P2P network not initialized");
            }
        });

        ui.add_space(12.0);

        section_card(ui, "Block Synchronization", |ui| {
            ui.label(format!(
                "Synchronization Status: {}",
                if sync_in_progress {
                    "Running"
                } else {
                    "Auto-sync enabled"
                }
            ));
            ui.label(format!("Blocks to sync: {}", blocks_to_sync));
            ui.label("Automatic sync runs every 5 seconds while peers are connected.");
            if ui.button("Sync Blocks from Peers").clicked() {
                self.queue_block_sync(true);
            }
            if ui.button("Reset Blockchain to Genesis").clicked() {
                match self.node.reset_to_genesis() {
                    Ok(()) => {
                        self.pending_transfers.clear();
                        self.last_notified_block_hash = None;
                        self.refresh_wallet_state();
                        self.message = "Blockchain reset to the genesis block".to_string();
                    }
                    Err(err) => self.message = err,
                }
            }
        });

        ui.add_space(12.0);

        section_card(ui, "Network Configuration", |ui| {
            ui.label("Listen Address: 0.0.0.0:30303 (GUI default)");
            ui.label("Max Peers: 32");
            ui.label("Peer Timeout: 5 minutes");
            ui.small("Production example: cargo run --release -- node p2p --listen 0.0.0.0:30303 --bootstrap <PEER_ADDR>");
        });

        ui.add_space(12.0);

        section_card(ui, "Connected Peer List", |ui| {
            if peer_snapshot.is_empty() {
                ui.small("No live peers connected yet.");
            } else {
                egui::ScrollArea::vertical()
                    .id_source("peer_list_scroll")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for peer in peer_snapshot {
                            let last_seen = peer
                                .last_seen
                                .elapsed()
                                .unwrap_or_default()
                                .as_secs();
                            ui.monospace(format!(
                                "{} | height {} | v{} | last seen {}s ago | conn {}",
                                peer.address,
                                peer.best_height,
                                peer.version,
                                last_seen,
                                peer.connection_addr
                            ));
                        }
                    });
            }
        });
    }
}

fn load_app_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../assets/blindeye_icon.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn load_logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = include_bytes!("../assets/blindeye_icon.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let filtered =
        image::imageops::resize(&image, 320, 320, image::imageops::FilterType::CatmullRom);
    let (width, height) = filtered.dimensions();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        filtered.as_raw(),
    );
    Some(ctx.load_texture("blindeye-logo", color_image, egui::TextureOptions::LINEAR))
}

fn paint_rotating_texture(
    painter: &egui::Painter,
    texture: &egui::TextureHandle,
    center: egui::Pos2,
    size: f32,
    angle: f32,
    tint: egui::Color32,
) {
    let half = size * 0.5;
    let rotation = egui::emath::Rot2::from_angle(angle);
    let offsets = [
        egui::vec2(-half, -half),
        egui::vec2(half, -half),
        egui::vec2(half, half),
        egui::vec2(-half, half),
    ];
    let uvs = [
        egui::pos2(0.0, 0.0),
        egui::pos2(1.0, 0.0),
        egui::pos2(1.0, 1.0),
        egui::pos2(0.0, 1.0),
    ];

    let mut mesh = egui::Mesh::with_texture(texture.id());
    for (offset, uv) in offsets.into_iter().zip(uvs.into_iter()) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center + rotation * offset,
            uv,
            color: tint,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t.powi(3)
}

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.button_padding = egui::vec2(16.0, 10.0);
    style.spacing.indent = 14.0;
    style.visuals.window_rounding = egui::Rounding::same(18.0);
    style.visuals.menu_rounding = egui::Rounding::same(16.0);
    style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(16.0);
    style.visuals.widgets.inactive.rounding = egui::Rounding::same(14.0);
    style.visuals.widgets.hovered.rounding = egui::Rounding::same(14.0);
    style.visuals.widgets.active.rounding = egui::Rounding::same(14.0);
    ctx.set_style(style);

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(COLOR_SILVER);
    visuals.panel_fill = COLOR_PANEL;
    visuals.window_fill = COLOR_PANEL;
    visuals.extreme_bg_color = COLOR_CHARCOAL;
    visuals.faint_bg_color = COLOR_CARD;
    visuals.code_bg_color = COLOR_NEAR_BLACK;
    visuals.selection.bg_fill = COLOR_BLUE;
    visuals.selection.stroke = egui::Stroke::new(1.0, COLOR_CYAN);
    visuals.window_shadow.blur = 18.0;
    visuals.window_shadow.offset = egui::vec2(0.0, 10.0);
    visuals.widgets.noninteractive.bg_fill = COLOR_CHARCOAL;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, COLOR_STROKE);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, COLOR_MUTED);
    visuals.widgets.inactive.bg_fill = COLOR_CARD_SOFT;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, COLOR_STROKE);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, COLOR_SILVER);
    visuals.widgets.hovered.bg_fill = COLOR_BLUE;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, COLOR_CYAN);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, COLOR_SILVER);
    visuals.widgets.active.bg_fill = COLOR_CYAN;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, COLOR_CYAN);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, COLOR_NEAR_BLACK);
    ctx.set_visuals(visuals);
}

fn section_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .fill(COLOR_CARD)
        .stroke(egui::Stroke::new(1.0, COLOR_STROKE))
        .rounding(egui::Rounding::same(20.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 10.0),
            blur: 24.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(70),
        })
        .inner_margin(egui::Margin::same(14.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(18.0)
                    .strong()
                    .color(COLOR_CYAN),
            );
            ui.add_space(8.0);
            add_contents(ui);
        });
}

fn nav_button(ui: &mut egui::Ui, is_active: bool, label: &str) -> bool {
    let button = egui::Button::new(egui::RichText::new(label).strong().color(if is_active {
        COLOR_NEAR_BLACK
    } else {
        COLOR_SILVER
    }))
    .min_size(egui::vec2(ui.available_width(), 42.0))
    .fill(if is_active {
        COLOR_CYAN
    } else {
        COLOR_CARD_SOFT
    })
    .stroke(egui::Stroke::new(
        1.0,
        if is_active { COLOR_CYAN } else { COLOR_STROKE },
    ));

    ui.add(button).clicked()
}

fn shorten_middle(value: &str, edge: usize) -> String {
    if value.len() <= edge * 2 + 3 {
        return value.to_string();
    }

    format!(
        "{}...{}",
        &value[..edge],
        &value[value.len().saturating_sub(edge)..]
    )
}

fn format_unix_timestamp(timestamp: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = now.saturating_sub(timestamp);
    format!("{} ({}s ago)", timestamp, age)
}

fn load_or_create_node() -> Result<Node, String> {
    Node::load_or_create(default_node_state_path(), None)
}
