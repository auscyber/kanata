use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

/// A single operation bound to a key on one layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    /// Coarse category of the operation, e.g. `"keycode"`, `"holdtap"`,
    /// `"layer-while-held"`, `"custom"`, `"noop"`.
    pub kind: String,
    /// Human-readable description of what the key does, e.g. `"a"`,
    /// `"cmd notify-send hi"`, or `"tap=esc hold=layer-while-held:nav"`.
    pub desc: String,
    /// For hold-tap actions, the description of the tap action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
    /// For hold-tap actions, the description of the hold action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<String>,
}

impl KeyBinding {
    /// Build a binding with just a kind and description (no tap/hold).
    pub fn simple(kind: impl Into<String>, desc: impl Into<String>) -> Self {
        KeyBinding {
            kind: kind.into(),
            desc: desc.into(),
            tap: None,
            hold: None,
        }
    }
}

/// A key-centric view of a keymap: `keys[key_name][layer_name]` is the operation bound to
/// that physical key on that layer. A key/layer pair is present only if the key is actually
/// bound there (transparent fall-through positions are omitted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keymap {
    /// Layer names in definition order, so consumers can lay out columns consistently.
    pub layers: Vec<String>,
    /// `key -> (layer -> binding)`, keyed by human-readable physical key name (e.g. `"a"`).
    pub keys: BTreeMap<String, BTreeMap<String, KeyBinding>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServerMessage {
    LayerChange { new: String },
    LayerNames { names: Vec<String> },
    CurrentLayerInfo { name: String, cfg_text: String },
    ConfigFileReload { new: String },
    CurrentLayerName { name: String },
    MessagePush { message: serde_json::Value },
    /// Key-centric keymap of the running configuration: for every physical key, the
    /// operation bound to it on each layer. Sent in response to `RequestKeymap`.
    /// Matches the JSON produced by kanata's `--export-keymap` CLI flag.
    Keymap { keymap: Keymap },
    Error { msg: String },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status")]
pub enum ServerResponse {
    Ok,
    Error { msg: String },
}

impl ServerResponse {
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut msg = serde_json::to_vec(self).expect("ServerResponse should serialize");
        msg.push(b'\n');
        msg
    }
}

impl ServerMessage {
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut msg = serde_json::to_vec(self).expect("ServerMessage should serialize");
        msg.push(b'\n');
        msg
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    ChangeLayer {
        new: String,
    },
    RequestLayerNames {},
    RequestCurrentLayerInfo {},
    RequestCurrentLayerName {},
    /// Request the full key-centric keymap of the running configuration. Answered with
    /// a `ServerMessage::Keymap`.
    RequestKeymap {},
    ActOnFakeKey {
        name: String,
        action: FakeKeyActionMessage,
    },
    SetMouse {
        x: u16,
        y: u16,
    },
    Reload {},
    ReloadNext {},
    ReloadPrev {},
    ReloadNum {
        index: usize,
    },
    ReloadFile {
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum FakeKeyActionMessage {
    Press,
    Release,
    Tap,
    Toggle,
}

impl FromStr for ClientMessage {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_response_json_format() {
        // Test that our API contract matches expected JSON structure
        assert_eq!(
            serde_json::to_string(&ServerResponse::Ok).unwrap(),
            r#"{"status":"Ok"}"#
        );
        assert_eq!(
            serde_json::to_string(&ServerResponse::Error {
                msg: "test".to_string()
            })
            .unwrap(),
            r#"{"status":"Error","msg":"test"}"#
        );
    }

    #[test]
    fn test_request_keymap_roundtrips() {
        let msg = ClientMessage::RequestKeymap {};
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"RequestKeymap":{}}"#);
        assert!(matches!(
            ClientMessage::from_str(&json).unwrap(),
            ClientMessage::RequestKeymap {}
        ));
    }

    #[test]
    fn test_keymap_server_message_roundtrips() {
        let mut keys = BTreeMap::new();
        let mut a_layers = BTreeMap::new();
        a_layers.insert("base".to_string(), KeyBinding::simple("keycode", "a"));
        keys.insert("a".to_string(), a_layers);
        let payload = Keymap {
            layers: vec!["base".to_string()],
            keys,
        };
        let msg = ServerMessage::Keymap {
            keymap: payload.clone(),
        };
        let bytes = msg.as_bytes();
        assert!(bytes.ends_with(b"\n"));
        let parsed: ServerMessage = serde_json::from_slice(&bytes).unwrap();
        match parsed {
            ServerMessage::Keymap { keymap } => assert_eq!(keymap, payload),
            other => panic!("expected Keymap, got {other:?}"),
        }
    }

    #[test]
    fn test_as_bytes_includes_newline() {
        // Test our specific logic that adds newline termination
        let response = ServerResponse::Ok;
        let bytes = response.as_bytes();
        assert!(bytes.ends_with(b"\n"), "Response should end with newline");

        let error_response = ServerResponse::Error {
            msg: "test".to_string(),
        };
        let error_bytes = error_response.as_bytes();
        assert!(
            error_bytes.ends_with(b"\n"),
            "Error response should end with newline"
        );
    }
}
