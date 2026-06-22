//! Raw Input keyboard monitor — uses RIDEV_INPUTSINK.
//!
//! Reads keyboard events directly from the HID device stack via
//! RegisterRawInputDevices. Works in exclusive-fullscreen DirectInput
//! games (Witcher 3, Cyberpunk, etc.) — same method as OBS/Discord
//! push-to-talk. Does NOT require RegisterClassExW; uses the egui
//! window's HWND indirectly via a message-only thread approach that
//! avoids creating a new window class entirely.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::hotkeys::{HotkeyAction, HotkeyBindings};

/// Map Windows Virtual-Key code → HotkeyBindings string key.
fn vk_to_code_str(vk: u16, flags: u16) -> Option<&'static str> {
    let extended = (flags & 0x0100) != 0;
    Some(match vk as u32 {
        0x41=>"KeyA", 0x42=>"KeyB", 0x43=>"KeyC", 0x44=>"KeyD",
        0x45=>"KeyE", 0x46=>"KeyF", 0x47=>"KeyG", 0x48=>"KeyH",
        0x49=>"KeyI", 0x4A=>"KeyJ", 0x4B=>"KeyK", 0x4C=>"KeyL",
        0x4D=>"KeyM", 0x4E=>"KeyN", 0x4F=>"KeyO", 0x50=>"KeyP",
        0x51=>"KeyQ", 0x52=>"KeyR", 0x53=>"KeyS", 0x54=>"KeyT",
        0x55=>"KeyU", 0x56=>"KeyV", 0x57=>"KeyW", 0x58=>"KeyX",
        0x59=>"KeyY", 0x5A=>"KeyZ",
        0x30=>"Digit0",0x31=>"Digit1",0x32=>"Digit2",0x33=>"Digit3",
        0x34=>"Digit4",0x35=>"Digit5",0x36=>"Digit6",0x37=>"Digit7",
        0x38=>"Digit8",0x39=>"Digit9",
        0x70=>"F1", 0x71=>"F2", 0x72=>"F3", 0x73=>"F4",
        0x74=>"F5", 0x75=>"F6", 0x76=>"F7", 0x77=>"F8",
        0x78=>"F9", 0x79=>"F10",0x7A=>"F11",0x7B=>"F12",
        0x7C=>"F13",0x7D=>"F14",0x7E=>"F15",0x7F=>"F16",
        0x80=>"F17",0x81=>"F18",0x82=>"F19",0x83=>"F20",
        0x84=>"F21",0x85=>"F22",0x86=>"F23",0x87=>"F24",
        // Numpad (non-extended = numpad, extended = cursor cluster)
        0x60 if !extended=>"Numpad0", 0x61 if !extended=>"Numpad1",
        0x62 if !extended=>"Numpad2", 0x63 if !extended=>"Numpad3",
        0x64 if !extended=>"Numpad4", 0x65 if !extended=>"Numpad5",
        0x66 if !extended=>"Numpad6", 0x67 if !extended=>"Numpad7",
        0x68 if !extended=>"Numpad8", 0x69 if !extended=>"Numpad9",
        0x6A=>"NumpadMultiply", 0x6B=>"NumpadAdd",
        0x6D=>"NumpadSubtract", 0x6E=>"NumpadDecimal",
        0x6F if extended=>"NumpadDivide",
        0x0D if extended=>"NumpadEnter",
        0x6C=>"NumpadComma",
        // Navigation
        0x26=>"ArrowUp",   0x28=>"ArrowDown",
        0x25=>"ArrowLeft", 0x27=>"ArrowRight",
        0x24=>"Home",   0x23=>"End",
        0x21=>"PageUp", 0x22=>"PageDown",
        0x2D=>"Insert", 0x2E=>"Delete",
        // Common
        0x20=>"Space",  0x09=>"Tab",
        0x0D=>"Enter",  0x1B=>"Escape",
        0x08=>"Backspace",
        0xBD=>"Minus",  0xBB=>"Equal",
        0xDB=>"BracketLeft", 0xDD=>"BracketRight",
        0xDC=>"Backslash",   0xBA=>"Semicolon",
        0xBC=>"Comma",  0xBE=>"Period", 0xBF=>"Slash",
        0xC0=>"Backquote",
        _ => return None,
    })
}

#[cfg(target_os = "windows")]
pub fn start_worker(
    bindings: Arc<Mutex<HotkeyBindings>>,
    stop: Arc<AtomicBool>,
    dispatch: Arc<dyn Fn(HotkeyAction) + Send + Sync + 'static>,
) -> bool {
    let ready = Arc::new(AtomicBool::new(false));
    let ready2 = ready.clone();
    let ok = std::thread::Builder::new()
        .name("osiris-raw-input".into())
        .spawn(move || worker_body(bindings, stop, dispatch, ready2))
        .is_ok();
    if ok {
        // Give the thread 80 ms to call RegisterRawInputDevices.
        for _ in 0..16 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            if ready.load(Ordering::Relaxed) { break; }
        }
        ready.load(Ordering::Relaxed)
    } else { false }
}

#[cfg(target_os = "windows")]
fn worker_body(
    bindings: Arc<Mutex<HotkeyBindings>>,
    stop: Arc<AtomicBool>,
    dispatch: Arc<dyn Fn(HotkeyAction) + Send + Sync + 'static>,
    ready: Arc<AtomicBool>,
) {
    use windows::Win32::{
        Foundation::HWND,
        UI::{
            Input::{
                GetRawInputData, RegisterRawInputDevices,
                HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER,
                RID_INPUT, RIDEV_INPUTSINK, RIM_TYPEKEYBOARD,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DispatchMessageW, PeekMessageW,
                HWND_MESSAGE, MSG, WM_INPUT,
                WS_OVERLAPPEDWINDOW, PM_REMOVE,
            },
        },
    };
    use windows::core::PCWSTR;

    // We need a real HWND to pass to RIDEV_INPUTSINK (NULL is not
    // accepted). Use CreateWindowExW with a pre-registered system
    // class ("Static") — no RegisterClassExW needed.
    let hwnd = unsafe {
        let class: Vec<u16> = "Static\0".encode_utf16().collect();
        let title: Vec<u16> = "OsirisRawInput\0".encode_utf16().collect();
        CreateWindowExW(
            Default::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            0, 0, 0, 0,
            HWND_MESSAGE, None, None, None,
        )
    };
    if hwnd.0 == 0 {
        log::warn!("RawInput: CreateWindowExW failed");
        return;
    }

    let rid = windows::Win32::UI::Input::RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    let reg_ok = unsafe {
        RegisterRawInputDevices(
            std::slice::from_ref(&rid),
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        )
    };
    if reg_ok.0 == 0 {
        log::warn!("RawInput: RegisterRawInputDevices failed");
        return;
    }
    log::info!("RawInput hotkey worker ready (RIDEV_INPUTSINK)");
    ready.store(true, Ordering::Relaxed);

    let mut last_vk: u16 = 0;
    let mut msg = MSG::default();
    loop {
        if stop.load(Ordering::Relaxed) { break; }
        unsafe {
            while PeekMessageW(&mut msg, HWND(0), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_INPUT {
                    let hri = HRAWINPUT(msg.lParam.0);
                    let mut sz: u32 = 0;
                    GetRawInputData(
                        hri, RID_INPUT, None, &mut sz,
                        std::mem::size_of::<RAWINPUTHEADER>() as u32,
                    );
                    if sz > 0 && sz < 512 {
                        let mut buf = vec![0u8; sz as usize];
                        let got = GetRawInputData(
                            hri, RID_INPUT,
                            Some(buf.as_mut_ptr() as *mut _), &mut sz,
                            std::mem::size_of::<RAWINPUTHEADER>() as u32,
                        );
                        if got == sz {
                            let ri = &*(buf.as_ptr() as *const RAWINPUT);
                            if ri.header.dwType == RIM_TYPEKEYBOARD.0 {
                                let kbd = ri.data.keyboard;
                                let vk    = kbd.VKey;
                                let flags = kbd.Flags;
                                let is_down = (flags & 0x01) == 0;
                                if is_down && vk != last_vk {
                                    last_vk = vk;
                                    if let Some(key) = vk_to_code_str(vk, flags) {
                                        if let Ok(b) = bindings.lock() {
                                            for &action in crate::hotkeys::HotkeyAction::ALL {
                                                if b.label_for(action) == key {
                                                    dispatch(action);
                                                }
                                            }
                                        }
                                    }
                                } else if !is_down && vk == last_vk {
                                    last_vk = 0;
                                }
                            }
                        }
                    }
                }
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

#[cfg(not(target_os = "windows"))]
pub fn start_worker(
    _: Arc<Mutex<HotkeyBindings>>,
    _: Arc<AtomicBool>,
    _: Arc<dyn Fn(HotkeyAction) + Send + Sync + 'static>,
) -> bool { false }
