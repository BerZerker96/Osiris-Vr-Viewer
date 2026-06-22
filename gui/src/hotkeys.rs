//! Global keyboard hotkeys for Osiris GUI.
//!
//! Uses the `global-hotkey` crate to register OS-level hotkeys that fire
//! even when the GUI window is minimized and the user is in-game. Each
//! `HotkeyAction` maps to one optional `Code` (the user picks the key in
//! the GUI's Hotkeys section).
//!
//! On Windows this uses RegisterHotKey under the hood, which is exclusive
//! per-key globally — if another app has bound the same key it'll fail
//! silently (the hotkey just won't fire).

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// All actions the user can bind to a hotkey. Variants are kept in a
/// stable order for the GUI's "Hotkeys" section list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum HotkeyAction {
    Cycle3DMode = 0,
    CycleScreenShape = 1,
    Recenter = 2,
    ToggleHeadlock = 3,
    ToggleSim6dof = 4,
    ZoomIn = 5,
    ZoomOut = 6,
    Screenshot = 7,
    OffsetZForward = 8,
    OffsetZBackward = 9,
    SwapEyes = 10,
    OffsetXLeft = 11,
    OffsetXRight = 12,
    OffsetYUp = 13,
    OffsetYDown = 14,
    Restart = 15,
    ToggleMouseEmu = 16,
    ToggleUdp6dof = 17,
    /// Roll the screen counter-clockwise by 2°.
    RollOffsetLeft = 18,
    /// Roll the screen clockwise by 2°.
    RollOffsetRight = 19,
    /// Cycle through saved presets in alphabetical order.
    /// With 1 preset: stays on it. With 2: toggles. With 3+: full cycle.
    CyclePreset = 20,
    /// Toggle Joystick Emulation on/off.
    ToggleJoyEmu = 21,
    /// Toggle Katanga desktop overlay on/off.
    ToggleKatangaOverlay = 22,
    /// Toggle Katanga Filters (the stronger image-adjustment set) on/off.
    ToggleKatangaFilters = 23,
    /// Force the view to the desktop immediately (manual fallback). A TOGGLE:
    /// first press drops Katanga and HOLDS the desktop (Katanga is skipped by
    /// loader selection until released); second press releases the hold and
    /// reprobes Katanga immediately. Isolated from the automatic fallback
    /// machinery — it only sets/clears its own dedicated hold flag.
    ForceDesktop = 24,
}

impl HotkeyAction {
    /// Wire-format byte (= discriminant + 1, so 0 = none/unbound).
    pub fn to_wire(self) -> u8 {
        (self as u8) + 1
    }
    /// Decode a wire byte back into a `HotkeyAction`. 0 = none.
    pub fn from_wire(b: u8) -> Option<Self> {
        if b == 0 {
            return None;
        }
        Some(match b - 1 {
            0 => Self::Cycle3DMode,
            1 => Self::CycleScreenShape,
            2 => Self::Recenter,
            3 => Self::ToggleHeadlock,
            4 => Self::ToggleSim6dof,
            5 => Self::ZoomIn,
            6 => Self::ZoomOut,
            7 => Self::Screenshot,
            8 => Self::OffsetZForward,
            9 => Self::OffsetZBackward,
            10 => Self::SwapEyes,
            11 => Self::OffsetXLeft,
            12 => Self::OffsetXRight,
            13 => Self::OffsetYUp,
            14 => Self::OffsetYDown,
            15 => Self::Restart,
            16 => Self::ToggleMouseEmu,
            17 => Self::ToggleUdp6dof,
            18 => Self::RollOffsetLeft,
            19 => Self::RollOffsetRight,
            20 => Self::CyclePreset,
            21 => Self::ToggleJoyEmu,
            22 => Self::ToggleKatangaOverlay,
            23 => Self::ToggleKatangaFilters,
            24 => Self::ForceDesktop,
            _ => return None,
        })
    }
}

/// VR controller buttons / triggers / thumbstick directions that can
/// be bound to a `HotkeyAction`. Discriminant byte values are stable
/// — used in the SHM wire format that flows GUI → viewer (binding
/// map) and viewer → GUI (last pressed during capture).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ControllerButton {
    RightA = 1,
    RightB = 2,
    LeftX = 3,
    LeftY = 4,
    RightTrigger = 5,
    LeftTrigger = 6,
    RightGrip = 7,
    LeftGrip = 8,
    RightThumbUp = 9,
    RightThumbDown = 10,
    RightThumbLeft = 11,
    RightThumbRight = 12,
    LeftThumbUp = 13,
    LeftThumbDown = 14,
    LeftThumbLeft = 15,
    LeftThumbRight = 16,
}

impl ControllerButton {
    pub fn to_wire(self) -> u8 {
        self as u8
    }
    pub fn from_wire(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::RightA,
            2 => Self::RightB,
            3 => Self::LeftX,
            4 => Self::LeftY,
            5 => Self::RightTrigger,
            6 => Self::LeftTrigger,
            7 => Self::RightGrip,
            8 => Self::LeftGrip,
            9 => Self::RightThumbUp,
            10 => Self::RightThumbDown,
            11 => Self::RightThumbLeft,
            12 => Self::RightThumbRight,
            13 => Self::LeftThumbUp,
            14 => Self::LeftThumbDown,
            15 => Self::LeftThumbLeft,
            16 => Self::LeftThumbRight,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::RightA => "Right A",
            Self::RightB => "Right B",
            Self::LeftX => "Left X",
            Self::LeftY => "Left Y",
            Self::RightTrigger => "Right trigger",
            Self::LeftTrigger => "Left trigger",
            Self::RightGrip => "Right grip",
            Self::LeftGrip => "Left grip",
            Self::RightThumbUp => "Right thumb up",
            Self::RightThumbDown => "Right thumb down",
            Self::RightThumbLeft => "Right thumb left",
            Self::RightThumbRight => "Right thumb right",
            Self::LeftThumbUp => "Left thumb up",
            Self::LeftThumbDown => "Left thumb down",
            Self::LeftThumbLeft => "Left thumb left",
            Self::LeftThumbRight => "Left thumb right",
        }
    }
}

impl HotkeyAction {
    /// Stable list of all actions in display order. The GUI iterates
    /// this to lay out the Hotkeys section.
    ///
    /// Note: ZoomIn / ZoomOut variants exist in the enum (so older
    /// preset files that bound them load cleanly) but are NOT exposed
    /// in the UI list anymore — zooming is fast enough through the
    /// slider, and the hotkey rows were taking up screen space.
    pub const ALL: &'static [HotkeyAction] = &[
        HotkeyAction::Cycle3DMode,
        HotkeyAction::CycleScreenShape,
        HotkeyAction::Recenter,
        HotkeyAction::ToggleHeadlock,
        HotkeyAction::ToggleSim6dof,
        HotkeyAction::ToggleMouseEmu,
        HotkeyAction::ToggleJoyEmu,
        HotkeyAction::ToggleKatangaOverlay,
        HotkeyAction::ToggleKatangaFilters,
        HotkeyAction::ForceDesktop,
        HotkeyAction::ToggleUdp6dof,
        HotkeyAction::Screenshot,
        HotkeyAction::OffsetZForward,
        HotkeyAction::OffsetZBackward,
        HotkeyAction::SwapEyes,
        HotkeyAction::OffsetXLeft,
        HotkeyAction::OffsetXRight,
        HotkeyAction::OffsetYUp,
        HotkeyAction::OffsetYDown,
        HotkeyAction::RollOffsetLeft,
        HotkeyAction::RollOffsetRight,
        HotkeyAction::Restart,
        HotkeyAction::CyclePreset,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            HotkeyAction::Cycle3DMode => "Cycle 3D mode",
            HotkeyAction::CycleScreenShape => "Cycle shape",
            HotkeyAction::Recenter => "Recenter",
            HotkeyAction::ToggleHeadlock => "Head-lock",
            HotkeyAction::ToggleSim6dof => "Sim 6DoF",
            HotkeyAction::ZoomIn => "Zoom in",
            HotkeyAction::ZoomOut => "Zoom out",
            HotkeyAction::Screenshot => "Screenshot",
            HotkeyAction::OffsetZForward => "Move fwd (Z+)",
            HotkeyAction::OffsetZBackward => "Move back (Z-)",
            HotkeyAction::SwapEyes => "Swap eyes",
            HotkeyAction::OffsetXLeft => "Move left (X-)",
            HotkeyAction::OffsetXRight => "Move right (X+)",
            HotkeyAction::OffsetYUp => "Move up (Y+)",
            HotkeyAction::OffsetYDown => "Move down (Y-)",
            HotkeyAction::RollOffsetLeft => "Roll left",
            HotkeyAction::RollOffsetRight => "Roll right",
            HotkeyAction::Restart => "Restart",
            HotkeyAction::ToggleMouseEmu => "Mouse emu",
            HotkeyAction::ToggleJoyEmu => "Joystick emu",
            HotkeyAction::ToggleKatangaOverlay => "Katanga ImGui",
            HotkeyAction::ToggleKatangaFilters => "Katanga filters",
            HotkeyAction::ForceDesktop => "Force desktop",
            HotkeyAction::ToggleUdp6dof => "6DoF UDP",
            HotkeyAction::CyclePreset => "Cycle presets",
        }
    }
}

/// Persistent map of HotkeyAction → key code. Empty by default (user
/// binds keys explicitly in the GUI). Code is stored as a string
/// representation so it round-trips cleanly through JSON.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HotkeyBindings {
    /// Map action → key code string (e.g. "F1", "KeyA", "Space").
    /// Missing entry = unbound.
    pub bindings: HashMap<HotkeyAction, String>,
}

impl HotkeyBindings {
    pub fn get(&self, action: HotkeyAction) -> Option<Code> {
        self.bindings
            .get(&action)
            .and_then(|s| code_from_str(s))
    }

    pub fn set(&mut self, action: HotkeyAction, code: Code) {
        self.bindings.insert(action, format!("{:?}", code));
    }

    pub fn clear(&mut self, action: HotkeyAction) {
        self.bindings.remove(&action);
    }

    pub fn label_for(&self, action: HotkeyAction) -> String {
        self.bindings
            .get(&action)
            .cloned()
            .unwrap_or_else(|| String::new())
    }
}

/// Parse a `Code` from its `Debug` string form. The `global-hotkey`
/// crate doesn't expose `FromStr` so we match each variant manually.
/// All keyboard keys the crate supports are listed below — this is
/// the user's "all keyboard keys can be used" bar.
pub fn code_from_str(s: &str) -> Option<Code> {
    Some(match s {
        // Letters
        "KeyA" => Code::KeyA, "KeyB" => Code::KeyB, "KeyC" => Code::KeyC,
        "KeyD" => Code::KeyD, "KeyE" => Code::KeyE, "KeyF" => Code::KeyF,
        "KeyG" => Code::KeyG, "KeyH" => Code::KeyH, "KeyI" => Code::KeyI,
        "KeyJ" => Code::KeyJ, "KeyK" => Code::KeyK, "KeyL" => Code::KeyL,
        "KeyM" => Code::KeyM, "KeyN" => Code::KeyN, "KeyO" => Code::KeyO,
        "KeyP" => Code::KeyP, "KeyQ" => Code::KeyQ, "KeyR" => Code::KeyR,
        "KeyS" => Code::KeyS, "KeyT" => Code::KeyT, "KeyU" => Code::KeyU,
        "KeyV" => Code::KeyV, "KeyW" => Code::KeyW, "KeyX" => Code::KeyX,
        "KeyY" => Code::KeyY, "KeyZ" => Code::KeyZ,
        // Top-row digits
        "Digit0" => Code::Digit0, "Digit1" => Code::Digit1,
        "Digit2" => Code::Digit2, "Digit3" => Code::Digit3,
        "Digit4" => Code::Digit4, "Digit5" => Code::Digit5,
        "Digit6" => Code::Digit6, "Digit7" => Code::Digit7,
        "Digit8" => Code::Digit8, "Digit9" => Code::Digit9,
        // Function keys
        "F1" => Code::F1, "F2" => Code::F2, "F3" => Code::F3,
        "F4" => Code::F4, "F5" => Code::F5, "F6" => Code::F6,
        "F7" => Code::F7, "F8" => Code::F8, "F9" => Code::F9,
        "F10" => Code::F10, "F11" => Code::F11, "F12" => Code::F12,
        "F13" => Code::F13, "F14" => Code::F14, "F15" => Code::F15,
        "F16" => Code::F16, "F17" => Code::F17, "F18" => Code::F18,
        "F19" => Code::F19, "F20" => Code::F20, "F21" => Code::F21,
        "F22" => Code::F22, "F23" => Code::F23, "F24" => Code::F24,
        // Navigation
        "ArrowUp" => Code::ArrowUp, "ArrowDown" => Code::ArrowDown,
        "ArrowLeft" => Code::ArrowLeft, "ArrowRight" => Code::ArrowRight,
        "Home" => Code::Home, "End" => Code::End,
        "PageUp" => Code::PageUp, "PageDown" => Code::PageDown,
        "Insert" => Code::Insert, "Delete" => Code::Delete,
        // Whitespace / control
        "Space" => Code::Space, "Tab" => Code::Tab,
        "Enter" => Code::Enter, "Escape" => Code::Escape,
        "Backspace" => Code::Backspace,
        // Punctuation
        "Backquote" => Code::Backquote, "Minus" => Code::Minus,
        "Equal" => Code::Equal, "BracketLeft" => Code::BracketLeft,
        "BracketRight" => Code::BracketRight, "Backslash" => Code::Backslash,
        "Semicolon" => Code::Semicolon, "Quote" => Code::Quote,
        "Comma" => Code::Comma, "Period" => Code::Period, "Slash" => Code::Slash,
        // Numpad
        "Numpad0" => Code::Numpad0, "Numpad1" => Code::Numpad1,
        "Numpad2" => Code::Numpad2, "Numpad3" => Code::Numpad3,
        "Numpad4" => Code::Numpad4, "Numpad5" => Code::Numpad5,
        "Numpad6" => Code::Numpad6, "Numpad7" => Code::Numpad7,
        "Numpad8" => Code::Numpad8, "Numpad9" => Code::Numpad9,
        "NumpadAdd" => Code::NumpadAdd, "NumpadSubtract" => Code::NumpadSubtract,
        "NumpadMultiply" => Code::NumpadMultiply, "NumpadDivide" => Code::NumpadDivide,
        "NumpadDecimal" => Code::NumpadDecimal, "NumpadEnter" => Code::NumpadEnter,
        "NumpadEqual" => Code::NumpadEqual,
        // Misc
        "PrintScreen" => Code::PrintScreen, "ScrollLock" => Code::ScrollLock,
        "Pause" => Code::Pause, "ContextMenu" => Code::ContextMenu,
        _ => return None,
    })
}

/// Convert an egui Key to a global-hotkey Code. Used when the user
/// presses a key inside the "click to bind" capture area in the GUI.
pub fn egui_key_to_code(k: egui::Key) -> Option<Code> {
    use egui::Key;
    Some(match k {
        Key::A => Code::KeyA, Key::B => Code::KeyB, Key::C => Code::KeyC,
        Key::D => Code::KeyD, Key::E => Code::KeyE, Key::F => Code::KeyF,
        Key::G => Code::KeyG, Key::H => Code::KeyH, Key::I => Code::KeyI,
        Key::J => Code::KeyJ, Key::K => Code::KeyK, Key::L => Code::KeyL,
        Key::M => Code::KeyM, Key::N => Code::KeyN, Key::O => Code::KeyO,
        Key::P => Code::KeyP, Key::Q => Code::KeyQ, Key::R => Code::KeyR,
        Key::S => Code::KeyS, Key::T => Code::KeyT, Key::U => Code::KeyU,
        Key::V => Code::KeyV, Key::W => Code::KeyW, Key::X => Code::KeyX,
        Key::Y => Code::KeyY, Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0, Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2, Key::Num3 => Code::Digit3,
        Key::Num4 => Code::Digit4, Key::Num5 => Code::Digit5,
        Key::Num6 => Code::Digit6, Key::Num7 => Code::Digit7,
        Key::Num8 => Code::Digit8, Key::Num9 => Code::Digit9,
        Key::F1 => Code::F1, Key::F2 => Code::F2, Key::F3 => Code::F3,
        Key::F4 => Code::F4, Key::F5 => Code::F5, Key::F6 => Code::F6,
        Key::F7 => Code::F7, Key::F8 => Code::F8, Key::F9 => Code::F9,
        Key::F10 => Code::F10, Key::F11 => Code::F11, Key::F12 => Code::F12,
        Key::F13 => Code::F13, Key::F14 => Code::F14, Key::F15 => Code::F15,
        Key::F16 => Code::F16, Key::F17 => Code::F17, Key::F18 => Code::F18,
        Key::F19 => Code::F19, Key::F20 => Code::F20,
        Key::ArrowUp => Code::ArrowUp, Key::ArrowDown => Code::ArrowDown,
        Key::ArrowLeft => Code::ArrowLeft, Key::ArrowRight => Code::ArrowRight,
        Key::Home => Code::Home, Key::End => Code::End,
        Key::PageUp => Code::PageUp, Key::PageDown => Code::PageDown,
        Key::Insert => Code::Insert, Key::Delete => Code::Delete,
        Key::Space => Code::Space, Key::Tab => Code::Tab,
        Key::Enter => Code::Enter, Key::Escape => Code::Escape,
        Key::Backspace => Code::Backspace,
        Key::Backtick => Code::Backquote, Key::Minus => Code::Minus,
        Key::Equals => Code::Equal,
        Key::OpenBracket => Code::BracketLeft,
        Key::CloseBracket => Code::BracketRight,
        Key::Backslash => Code::Backslash,
        Key::Semicolon => Code::Semicolon,
        Key::Comma => Code::Comma, Key::Period => Code::Period,
        Key::Slash => Code::Slash,
        _ => return None,
    })
}

/// Manager that tracks current registrations and re-registers when
/// bindings change. Each binding gets a unique HotKey id that we map
/// back to the originating action when an event fires.
///
/// `id_to_action` is wrapped in `Arc<Mutex<>>` so the background hotkey
/// worker thread (spawned in main.rs) can read it independently of the
/// main GUI thread. This is what makes hotkeys work when the window is
/// minimized — the worker drains events and applies actions itself,
/// rather than waiting for egui's update() to run.
pub struct HotkeyManager {
    inner: Option<GlobalHotKeyManager>,
    /// HotKey id → action. Shared with the background worker thread.
    pub id_to_action: std::sync::Arc<std::sync::Mutex<HashMap<u32, HotkeyAction>>>,
    /// Action → registered HotKey, so we can unregister on change.
    registered: HashMap<HotkeyAction, HotKey>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        let inner = GlobalHotKeyManager::new().ok();
        if inner.is_none() {
            log::warn!("Failed to create global hotkey manager — hotkeys disabled");
        }
        Self {
            inner,
            id_to_action: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
            registered: HashMap::new(),
        }
    }

    /// Drop all current registrations and re-register from `bindings`.
    /// Called whenever the user changes a binding in the GUI.
    pub fn sync(&mut self, bindings: &HotkeyBindings) {
        let Some(mgr) = &self.inner else { return };
        // Unregister all previous.
        for (_, hk) in self.registered.drain() {
            let _ = mgr.unregister(hk);
        }
        let mut map = match self.id_to_action.lock() {
            Ok(m) => m,
            Err(_) => return,
        };
        map.clear();
        // Register each bound action.
        for &action in HotkeyAction::ALL {
            if let Some(code) = bindings.get(action) {
                let hk = HotKey::new(Some(Modifiers::empty()), code);
                let id = hk.id();
                match mgr.register(hk) {
                    Ok(()) => {
                        map.insert(id, action);
                        self.registered.insert(action, hk);
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to register hotkey for {:?}: {} — \
                             another app may have it bound",
                            action, e
                        );
                    }
                }
            }
        }
    }

    /// Drain pending hotkey events, returning the actions that fired.
    /// Call once per frame from the GUI's update loop.
    ///
    /// NOTE: This is now mostly a fast-path for when the GUI is in
    /// focus. The background worker thread (spawned in main.rs) is
    /// the one that handles events when the GUI is minimized. Both
    /// drain from the same global crossbeam channel, so whichever
    /// thread polls first wins.
    pub fn poll(&self) -> Vec<HotkeyAction> {
        let mut out = Vec::new();
        let receiver = GlobalHotKeyEvent::receiver();
        while let Ok(evt) = receiver.try_recv() {
            if evt.state == global_hotkey::HotKeyState::Pressed {
                if let Ok(map) = self.id_to_action.lock() {
                    if let Some(&action) = map.get(&evt.id) {
                        out.push(action);
                    }
                }
            }
        }
        out
    }
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}
