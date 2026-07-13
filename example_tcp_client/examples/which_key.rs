//! A "which-key" overlay for your whole computer, à la which-key.nvim.
//!
//! It connects to a running kanata instance's TCP server, fetches the key-centric
//! keymap once, then listens for layer changes. Whenever the active layer changes —
//! e.g. because you are *holding* a `layer-while-held` key — it prints to stdout the
//! keys available on that layer and what they do. Keys that start a sequence (leader
//! keys) are highlighted separately.
//!
//! Run kanata with a TCP port, e.g. `kanata --cfg your.kbd --port 8081`, then:
//!
//! ```text
//! cargo run -p kanata_example_tcp_client --example which_key -- --port 8081
//! ```
//!
//! Then hold a layer key and watch the available keys for that layer appear.

use clap::Parser;
use kanata_tcp_protocol::{ClientMessage, KeyBinding, Keymap, ServerMessage, ServerResponse};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::exit;
use std::time::Duration;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Port that kanata's TCP server is listening on.
    #[clap(short, long)]
    port: u16,
}

fn main() {
    let args = Args::parse();

    let stream = match TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], args.port)),
        Duration::from_secs(5),
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not connect to kanata on port {}: {e}", args.port);
            exit(1);
        }
    };
    eprintln!("connected to kanata; requesting keymap...");

    // Ask for the full keymap up front. We will render it as the active layer changes.
    let mut writer = stream.try_clone().expect("clone stream");
    writer
        .write_all(&encode(&ClientMessage::RequestKeymap {}))
        .expect("request keymap");

    let mut keymap: Option<Keymap> = None;
    // The active layer, which may be known before the keymap has arrived.
    let mut active_layer: Option<String> = None;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("connection closed: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        // Command acknowledgements share the stream; ignore them here.
        if serde_json::from_str::<ServerResponse>(&line).is_ok() {
            continue;
        }
        let msg: ServerMessage = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("could not parse server message: {e}");
                continue;
            }
        };
        match msg {
            ServerMessage::Keymap { keymap: km } => {
                keymap = Some(km);
                if let (Some(km), Some(layer)) = (keymap.as_ref(), active_layer.as_ref()) {
                    render(km, layer);
                }
            }
            // Fires when the active layer changes, including when a layer key is held.
            ServerMessage::LayerChange { new } => {
                active_layer = Some(new.clone());
                match keymap.as_ref() {
                    Some(km) => render(km, &new),
                    None => eprintln!("(layer \"{new}\" active; waiting for keymap...)"),
                }
            }
            _ => {}
        }
    }
}

/// Serialize a client message as a newline-terminated JSON line.
fn encode(msg: &ClientMessage) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(msg).expect("serialize client message");
    bytes.push(b'\n');
    bytes
}

/// Print the which-key overlay for `layer`: every key bound on it and what it does.
fn render(keymap: &Keymap, layer: &str) {
    let mut rows: Vec<(&String, &KeyBinding)> = keymap
        .keys
        .iter()
        .filter_map(|(key, per_layer)| per_layer.get(layer).map(|binding| (key, binding)))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));

    println!("\n┌─ which-key ─ layer \"{layer}\" ─ {} keys", rows.len());
    if rows.is_empty() {
        println!("└─ (no keys bound on this layer)");
        return;
    }
    for (key, binding) in &rows {
        println!("│  {key:<14} {}", binding.desc);
    }

    // Highlight leader / sequence-start keys, the closest analog to a "prefix".
    let leaders: Vec<&str> = rows
        .iter()
        .filter(|(_, b)| b.kind == "sequence")
        .map(|(k, _)| k.as_str())
        .collect();
    if !leaders.is_empty() {
        println!("├─ leader keys: {}", leaders.join(", "));
    }
    println!("└─");
}
