//! Key-centric keymap querying.
//!
//! This module turns a parsed/live kanata layout into a serializable, *key-centric*
//! map: for every physical key it lists what that key does on each layer where it is
//! bound. The shape is designed to feed an external visual-keymap generator, both
//! statically (from a config via the CLI) and live (over the TCP server).
//!
//! The top-level [`Keymap`] serializes to JSON like:
//!
//! ```json
//! {
//!   "layers": ["base", "nav"],
//!   "keys": {
//!     "caps": { "base": {"kind":"holdtap",
//!                         "desc":"Tap for ESC, hold for Switch to layer \"nav\" while held",
//!                         "tap":"ESC","hold":"Switch to layer \"nav\" while held"} },
//!     "a":    { "base": {"kind":"keycode","desc":"A"} }
//!   }
//! }
//! ```

use std::collections::BTreeMap;

use kanata_keyberon::action::{
    Action, ChordsGroup, ForkConfig, HoldTapAction, OneShot, Switch, TapDance,
};
use kanata_keyberon::key_code::KeyCode;
use kanata_parser::cfg::{KanataLayout, KeyDocs, LayerInfo, SimpleSExpr};
use kanata_parser::custom_action::{
    CustomAction, FakeKeyAction, MWheelDirection, MoveDirection, UnmodMods,
};
use kanata_parser::keys::OsCode;

// The serializable data model (`Keymap`, `KeyBinding`) lives in the shared `tcp_protocol`
// crate so it can be sent over the TCP server as a strongly-typed message and reused here
// for the CLI export. This module only builds it from a layout.
pub use kanata_tcp_protocol::{KeyBinding, Keymap};

/// The physical-key row within a layer (row 1 holds virtual/fake keys, which are not
/// included here).
const PHYSICAL_ROW: usize = 0;

/// Build a [`Keymap`] from a layout and its per-layer metadata.
///
/// Works for both the statically parsed config (`cfg.layout`, `cfg.layer_info`) and a live
/// running instance (`kanata.layout`, `kanata.layer_info`), since both expose the same types.
///
/// `key_docs` holds authored documentation for individual key positions (from `defalias` and
/// `deftemplate` docstrings). When a position has a doc, it overwrites the auto-generated
/// description, so a which-key overlay shows the doc the user wrote rather than a derived label.
pub fn build_keymap(
    layout: &KanataLayout,
    layer_info: &[LayerInfo],
    key_docs: &KeyDocs,
) -> Keymap {
    let borrowed = layout.b();
    let layers = borrowed.layers;

    let layer_names: Vec<String> = layer_info.iter().map(|li| li.name.clone()).collect();
    let name_of = |idx: usize| -> String {
        layer_names
            .get(idx)
            .cloned()
            .unwrap_or_else(|| format!("layer{idx}"))
    };

    let mut keys: BTreeMap<String, BTreeMap<String, KeyBinding>> = BTreeMap::new();

    // Number of physical-key columns; identical across layers.
    let num_cols = layers
        .first()
        .map(|layer| layer[PHYSICAL_ROW].len())
        .unwrap_or(0);

    for col in 0..num_cols {
        let osc = match OsCode::try_from(col) {
            Ok(osc) => osc,
            Err(_) => continue,
        };

        let mut per_layer: BTreeMap<String, KeyBinding> = BTreeMap::new();
        // Only include this key at all if it does something real (non-transparent,
        // non-noop) on at least one layer.
        let mut has_real_binding = false;

        for (layer_idx, layer) in layers.iter().enumerate() {
            let action = &layer[PHYSICAL_ROW][col];
            // Transparent means "fall through to the layer below" — not a binding here.
            if matches!(action, Action::Trans) {
                continue;
            }
            if !matches!(action, Action::NoOp) {
                has_real_binding = true;
            }
            let mut binding = describe_action(action, name_of);
            // Authored docs (defalias / deftemplate docstrings) overwrite the derived description.
            if let Some(doc) = key_docs.get(&(layer_idx, col)) {
                binding.desc = doc.clone();
            }
            per_layer.insert(name_of(layer_idx), binding);
        }

        if has_real_binding {
            keys.insert(osc.to_string(), per_layer);
        }
    }

    Keymap {
        layers: layer_names,
        keys,
    }
}

/// Serialize a [`Keymap`] to pretty JSON.
pub fn keymap_to_json(keymap: &Keymap) -> String {
    serde_json::to_string_pretty(keymap).expect("Keymap serializes")
}

/// Describe a single action as a [`KeyBinding`]. `name_of` resolves a layer index to its name.
fn describe_action<'a, F>(action: &Action<'a, &'a &'a [&'a CustomAction]>, name_of: F) -> KeyBinding
where
    F: Fn(usize) -> String + Copy,
{
    match action {
        Action::NoOp => KeyBinding::simple("noop", "Do nothing"),
        Action::Trans => {
            KeyBinding::simple("trans", "Transparent, falling through to the layer below")
        }
        Action::Repeat => KeyBinding::simple("repeat", "Repeat the last key"),
        Action::KeyCode(kc) => {
            let name = OsCode::from(kc).to_string();
            KeyBinding::simple("keycode", name)
        }
        Action::MultipleKeyCodes(kcs) => {
            let desc = kcs
                .iter()
                .map(|kc| OsCode::from(kc).to_string())
                .collect::<Vec<_>>()
                .join("+");
            KeyBinding::simple("chord", desc)
        }
        Action::MultipleActions(actions) => {
            let desc = actions
                .iter()
                .map(|ac| describe_action(ac, name_of).desc)
                .collect::<Vec<_>>()
                .join(" + ");
            KeyBinding::simple("multi", desc)
        }
        Action::Layer(n) => {
            let name = name_of(*n);
            KeyBinding::simple("layer-while-held", format!("Switch to layer \"{name}\" while held"))
        }
        Action::DefaultLayer(n) => {
            let name = name_of(*n);
            KeyBinding::simple("layer-switch", format!("Switch the base layer to \"{name}\""))
        }
        Action::HoldTap(HoldTapAction { tap, hold, .. }) => {
            let tap_desc = describe_action(tap, name_of).desc;
            let hold_desc = describe_action(hold, name_of).desc;
            KeyBinding {
                kind: "holdtap".to_string(),
                desc: format!("Tap for {tap_desc}, hold for {hold_desc}"),
                tap: Some(tap_desc),
                hold: Some(hold_desc),
            }
        }
        Action::OneShot(OneShot { action: ac, .. }) => {
            let inner = describe_action(ac, name_of).desc;
            KeyBinding::simple("oneshot", format!("One-shot: {inner}"))
        }
        Action::TapDance(TapDance { actions, .. }) => {
            let desc = actions
                .iter()
                .map(|ac| describe_action(ac, name_of).desc)
                .collect::<Vec<_>>()
                .join(", then ");
            KeyBinding::simple("tapdance", format!("Tap repeatedly for: {desc}"))
        }
        Action::Fork(ForkConfig { left, right, .. }) => {
            let left = describe_action(left, name_of).desc;
            let right = describe_action(right, name_of).desc;
            KeyBinding::simple("fork", format!("{left}, or {right} when a chosen key is held"))
        }
        Action::Chords(ChordsGroup { .. }) => {
            KeyBinding::simple("chords", "Press with other keys to trigger a chord")
        }
        Action::Switch(Switch { cases }) => KeyBinding::simple(
            "switch",
            format!("Choose an action based on {} conditions", cases.len()),
        ),
        Action::Sequence { .. } => KeyBinding::simple("sequence", "Play a key sequence"),
        Action::RepeatableSequence { .. } => {
            KeyBinding::simple("sequence", "Play a key sequence, repeating while held")
        }
        Action::CancelSequences => KeyBinding::simple("sequence", "Cancel any sequence in progress"),
        Action::ReleaseState(_) => KeyBinding::simple("release", "Release a held key or layer"),
        Action::OneShotIgnoreEventsTicks(_) => {
            KeyBinding::simple("oneshot", "Pause processing for a one-shot key")
        }
        Action::Src => KeyBinding::simple("src", "Use the key from defsrc"),
        Action::Custom(cacs) => {
            let desc = cacs
                .iter()
                .map(|ca| describe_custom(ca))
                .collect::<Vec<_>>()
                .join(" + ");
            KeyBinding::simple("custom", desc)
        }
    }
}

/// Describe a single [`CustomAction`] as a natural-language sentence. Each arm reads as prose
/// explaining what the built-in action does, with the binding's *instantiated* values (timeouts,
/// keys, directions, commands, …) woven in, so a which-key overlay shows what this specific
/// binding does rather than a bare action name.
fn describe_custom(ca: &CustomAction) -> String {
    match ca {
        CustomAction::Cmd(args) => format!("Run the command: {}", args.join(" ")),
        CustomAction::CmdLog(_, _, args) => {
            format!("Run and log the command: {}", args.join(" "))
        }
        CustomAction::CmdOutputKeys(args) => {
            format!("Run a command and type its output: {}", args.join(" "))
        }
        CustomAction::PushMessage(msg) => format!(
            "Send a message to connected clients: {}",
            msg.iter().map(simple_sexpr_desc).collect::<Vec<_>>().join(" ")
        ),
        CustomAction::Unicode(c) => format!("Type the Unicode character {c}"),
        CustomAction::Mouse(b) => format!("Hold mouse button {b}"),
        CustomAction::MouseTap(b) => format!("Click mouse button {b}"),
        CustomAction::FakeKey { action, .. } => {
            format!("On press, {} a virtual key", fake_key_action_desc(*action))
        }
        CustomAction::FakeKeyOnRelease { action, .. } => {
            format!("On release, {} a virtual key", fake_key_action_desc(*action))
        }
        CustomAction::FakeKeyOnIdle(f) => format!(
            "After {}ms idle, {} a virtual key",
            f.idle_duration,
            fake_key_action_desc(f.action)
        ),
        CustomAction::FakeKeyOnPhysicalIdle(f) => format!(
            "After {}ms of physical idle, {} a virtual key",
            f.idle_duration,
            fake_key_action_desc(f.action)
        ),
        CustomAction::FakeKeyHoldForDuration(f) => {
            format!("Hold a virtual key for {}ms", f.hold_duration)
        }
        CustomAction::Delay(ms) => format!("Wait {ms}ms"),
        CustomAction::DelayOnRelease(ms) => format!("Wait {ms}ms on release"),
        CustomAction::MWheel {
            direction,
            interval,
            distance,
        } => format!(
            "Scroll the mouse wheel {} ({distance} every {interval}ms)",
            mwheel_dir(*direction)
        ),
        CustomAction::MWheelNotch { direction } => {
            format!("Scroll the mouse wheel {} by one notch", mwheel_dir(*direction))
        }
        CustomAction::MoveMouse {
            direction,
            interval,
            distance,
        } => format!(
            "Move the mouse {} ({distance} every {interval}ms)",
            move_dir(*direction)
        ),
        CustomAction::MoveMouseAccel {
            direction,
            interval,
            accel_time,
            min_distance,
            max_distance,
        } => format!(
            "Move the mouse {}, accelerating from {min_distance} to {max_distance} over {accel_time}ms (every {interval}ms)",
            move_dir(*direction)
        ),
        CustomAction::MoveMouseSpeed { speed } => format!("Set the mouse-move speed to {speed}%"),
        CustomAction::SetMouse { x, y } => format!("Jump the mouse cursor to ({x}, {y})"),
        CustomAction::SequenceCancel => "Cancel the sequence in progress".to_string(),
        CustomAction::SequenceLeader(..) => "Begin a key sequence".to_string(),
        CustomAction::SequenceNoerase(_) => {
            "Begin a key sequence without erasing typed keys".to_string()
        }
        CustomAction::Repeat => "Repeat the last key".to_string(),
        CustomAction::CancelMacroOnRelease => "Cancel the running macro on release".to_string(),
        CustomAction::CancelMacroOnNextPress(timeout) => {
            format!("Cancel the running macro on the next key press within {timeout}ms")
        }
        CustomAction::DynamicMacroRecord(id) => format!("Start recording dynamic macro {id}"),
        CustomAction::DynamicMacroRecordStop(truncate) => {
            format!("Stop recording the dynamic macro, dropping the last {truncate} keys")
        }
        CustomAction::DynamicMacroPlay(id) => format!("Play dynamic macro {id}"),
        CustomAction::SendArbitraryCode(code) => format!("Send the raw key code {code}"),
        CustomAction::CapsWord(cfg) => {
            format!("Enable Caps Word (times out after {}ms)", cfg.timeout)
        }
        CustomAction::Unmodded { keys, mods } => {
            let k = keys_desc(keys);
            if *mods == UnmodMods::all() {
                format!("Send {k} with all active modifiers suppressed")
            } else {
                format!("Send {k} with {} suppressed", unmod_mods_desc(*mods))
            }
        }
        CustomAction::Unshifted { keys } => format!("Send {} with Shift suppressed", keys_desc(keys)),
        CustomAction::ReverseReleaseOrder => {
            "Release the held keys in reverse order".to_string()
        }
        CustomAction::LiveReload => "Live-reload the configuration".to_string(),
        CustomAction::LiveReloadNext => "Live-reload the next configuration".to_string(),
        CustomAction::LiveReloadPrev => "Live-reload the previous configuration".to_string(),
        CustomAction::LiveReloadNum(n) => format!("Live-reload configuration #{n}"),
        CustomAction::LiveReloadFile(f) => format!("Live-reload the configuration file {f}"),
        CustomAction::ClipboardSet(s) => format!("Set the clipboard to \"{s}\""),
        CustomAction::ClipboardCmdSet(args) => {
            format!("Set the clipboard from the command: {}", args.join(" "))
        }
        CustomAction::ClipboardSave(id) => format!("Save the clipboard into slot {id}"),
        CustomAction::ClipboardRestore(id) => format!("Restore the clipboard from slot {id}"),
        CustomAction::ClipboardSaveSet(id, s) => {
            format!("Save the clipboard into slot {id}, then set it to \"{s}\"")
        }
        CustomAction::ClipboardSaveCmdSet(id, args) => format!(
            "Save the clipboard into slot {id}, then set it from the command: {}",
            args.join(" ")
        ),
        CustomAction::ClipboardSaveSwap(a, b) => {
            format!("Swap the clipboard with saved slots {a} and {b}")
        }
    }
}

/// Render a [`SimpleSExpr`] back to config-like text (used for `push-msg` payloads).
fn simple_sexpr_desc(e: &SimpleSExpr) -> String {
    match e {
        SimpleSExpr::Atom(a) => a.clone(),
        SimpleSExpr::List(l) => {
            format!("({})", l.iter().map(simple_sexpr_desc).collect::<Vec<_>>().join(" "))
        }
    }
}

/// Join key codes into a `+`-separated list of their OS key names, e.g. `LEFTCTRL+A`.
fn keys_desc(keys: &[KeyCode]) -> String {
    keys.iter()
        .map(|kc| OsCode::from(kc).to_string())
        .collect::<Vec<_>>()
        .join("+")
}

/// The set of modifiers an `unmod` suppresses, as a `+`-separated lowercase list, e.g. `lsft+lctl`.
fn unmod_mods_desc(mods: UnmodMods) -> String {
    mods.iter_names()
        .map(|(name, _)| name.to_lowercase())
        .collect::<Vec<_>>()
        .join("+")
}

fn fake_key_action_desc(action: FakeKeyAction) -> &'static str {
    match action {
        FakeKeyAction::Press => "press",
        FakeKeyAction::Release => "release",
        FakeKeyAction::Tap => "tap",
        FakeKeyAction::Toggle => "toggle",
    }
}

fn mwheel_dir(d: MWheelDirection) -> &'static str {
    match d {
        MWheelDirection::Up => "up",
        MWheelDirection::Down => "down",
        MWheelDirection::Left => "left",
        MWheelDirection::Right => "right",
    }
}

fn move_dir(d: MoveDirection) -> &'static str {
    match d {
        MoveDirection::Up => "up",
        MoveDirection::Down => "down",
        MoveDirection::Left => "left",
        MoveDirection::Right => "right",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanata_parser::cfg;
    use rustc_hash::FxHashMap;

    fn keymap_of(src: &str) -> Keymap {
        let cfg = cfg::new_from_str(src, FxHashMap::default()).expect("config parses");
        build_keymap(&cfg.layout, &cfg.layer_info, &cfg.key_docs)
    }

    #[test]
    fn keycodes_and_holdtap_and_layers_are_described() {
        let src = concat!(
            "(defsrc a b caps)\n",
            "(defalias hrm (tap-hold 200 200 esc lctl)\n",
            "          tolyr (layer-while-held other))\n",
            "(deflayer base a @tolyr @hrm)\n",
            "(deflayer other b _ _)\n",
        );
        let km = keymap_of(src);

        assert_eq!(km.layers, vec!["base".to_string(), "other".to_string()]);

        // Plain keycode; physical `a` is remapped to `b` on the other layer.
        let a = &km.keys["A"];
        assert_eq!(a["base"].kind, "keycode");
        assert_eq!(a["base"].desc, "A");
        assert_eq!(a["other"].desc, "B");

        // Physical `b` holds a layer-while-held action in base, and is transparent
        // (thus omitted) on the other layer.
        let b = &km.keys["B"];
        assert_eq!(b["base"].kind, "layer-while-held");
        assert_eq!(b["base"].desc, "Switch to layer \"other\" while held");
        assert!(!b.contains_key("other"));

        // hold-tap on caps in base exposes tap/hold.
        let caps = &km.keys["CAPSLOCK"];
        assert_eq!(caps["base"].kind, "holdtap");
        assert_eq!(caps["base"].tap.as_deref(), Some("ESC"));
        assert_eq!(caps["base"].hold.as_deref(), Some("LEFTCTRL"));
        // caps is transparent on the other layer, so it must be absent there.
        assert!(!caps.contains_key("other"));
    }

    #[test]
    #[cfg(feature = "cmd")]
    fn command_custom_action_is_labeled() {
        let src = concat!(
            "(defcfg danger-enable-cmd yes)\n",
            "(defsrc a)\n",
            "(deflayer base (cmd echo hi))\n",
        );
        let km = keymap_of(src);
        let a = &km.keys["A"]["base"];
        assert_eq!(a.kind, "custom");
        assert!(a.desc.starts_with("Run the command"), "got desc: {}", a.desc);
        assert!(a.desc.contains("echo"), "got desc: {}", a.desc);
    }

    #[test]
    fn builtin_list_actions_render_their_instantiated_values() {
        // Each of these previously fell back to a raw `{:?}` debug dump; they should now
        // render a compact label reflecting the concrete arguments the user wrote.
        let src = concat!(
            "(defsrc a b c d e)\n",
            "(deflayer base\n",
            "  (mwheel-down 50 120)\n",
            "  (movemouse-accel-up 1 1000 1 5)\n",
            "  (unmod lsft a)\n",
            "  (unshift ralt 8)\n",
            "  (caps-word 2000))\n",
        );
        let km = keymap_of(src);

        // Each description reads as prose with the binding's concrete values woven in.
        assert_eq!(
            km.keys["A"]["base"].desc,
            "Scroll the mouse wheel down (120 every 50ms)"
        );
        assert_eq!(
            km.keys["B"]["base"].desc,
            "Move the mouse up, accelerating from 1 to 5 over 1000ms (every 1ms)"
        );
        // `(unmod lsft a)` has no parenthesized modifier list, so all modifiers are suppressed.
        assert_eq!(
            km.keys["C"]["base"].desc,
            "Send LEFTSHIFT+A with all active modifiers suppressed"
        );
        assert_eq!(
            km.keys["D"]["base"].desc,
            "Send RIGHTALT+8 with Shift suppressed"
        );
        assert_eq!(
            km.keys["E"]["base"].desc,
            "Enable Caps Word (times out after 2000ms)"
        );
    }

    #[test]
    #[cfg(feature = "cmd")]
    fn alias_docstring_overwrites_description() {
        // A docstring after a defalias action documents that alias, and becomes the keymap
        // description (the which-key label) for any key bound to it, overriding the derived one.
        let src = concat!(
            "(defcfg danger-enable-cmd yes)\n",
            "(defsrc a b)\n",
            "(defalias\n",
            "  term (cmd echo hi) #'Launch the terminal'\n",
            "  plain (cmd echo bye))\n",
            "(deflayer base @term @plain)\n",
        );
        let km = keymap_of(src);

        // Documented alias: the docstring wins over \"Run the command: echo hi\".
        assert_eq!(km.keys["A"]["base"].desc, "Launch the terminal");
        // Undocumented alias keeps the auto-generated description.
        assert!(
            km.keys["B"]["base"].desc.starts_with("Run the command"),
            "got desc: {}",
            km.keys["B"]["base"].desc
        );
    }

    #[test]
    fn template_docstring_flows_through_alias_and_switch() {
        // A documented template's docstring should surface on keys whose action was produced by
        // that template, even when the template is used indirectly inside a switch behind an
        // alias. Composition is algebraic: the three identical template docs collapse to one.
        let src = concat!(
            "(deftemplate accent (key mod)\n",
            "  #'Type an accented letter'\n",
            "  ((key-history $key 1)) (macro bspc $mod $key) break)\n",
            "(defalias quote\n",
            "  (switch\n",
            "    (t! accent a b)\n",
            "    (t! accent e b)\n",
            "    () use-defsrc fallthrough))\n",
            "(defsrc a)\n",
            "(deflayer base @quote)\n",
        );
        let km = keymap_of(src);
        assert_eq!(km.keys["A"]["base"].desc, "Type an accented letter");
    }
}
