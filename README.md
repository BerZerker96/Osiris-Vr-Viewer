<div align="center">

<img src="assets/logo.ico" width="120" alt="Osiris VR Viewer logo" />

# Osiris VR Viewer

### A full-resolution OpenXR stereoscopic 3D viewer with VHT-grade screen geometry, head-tracking output, and a real-time tuning GUI.

[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Graphics](https://img.shields.io/badge/wgpu-Vulkan-AC162C?logo=vulkan&logoColor=white)](https://wgpu.rs/)
[![XR Runtime](https://img.shields.io/badge/OpenXR-1.0-1F6FEB)](https://www.khronos.org/openxr/)
[![Status](https://img.shields.io/badge/version-0.6.0--dev-success)](#)

</div>

---

## 📖 About

**Osiris VR Viewer** is a Rust fork of [VRScreenCap](https://github.com/artumino/VRScreenCap) by [@artumino](https://github.com/artumino), heavily extended into a full-featured stereoscopic 3D playback platform for OpenXR runtimes (SteamVR, Oculus, Virtual Desktop, Varjo, Pimax, etc.).

It captures stereoscopic 3D content from `geo-11` / `Katanga` mods, desktop duplication, or any side-by-side / top-and-bottom source, and projects it into VR on a curved screen, sphere, box room, or fishbowl-style mesh, with per-eye supersampling, image post-processing, simulated 6DoF parallax, and head-tracking output to drive games and 6DoF mods over the network.

Built on:
- 🦀 **Rust** — zero-overhead, memory-safe, no GC stutter
- 🎮 **OpenXR 1.0** — works with every major VR runtime
- ⚡ **wgpu / Vulkan** — modern multiview rendering, single command buffer for both eyes
- 🪟 **Windows** primary target; **D3D11** & **D3D12** zero-copy texture interop
- 💜 **egui** for the control panel — instant-mode, dark themed, GPU-accelerated

---

## ✅ Requirements

Osiris is a self-contained pair of executables — there is no installer. Drop the two `.exe` files into the same folder and run. For it to work on any PC, the target machine needs:

- **Windows 10 or 11 (64-bit).**
- **An installed and active OpenXR runtime** — e.g. SteamVR or the Pimax OpenXR runtime. The viewer has the OpenXR loader built in, but it still needs a runtime to talk to your headset. Set your desired runtime as the active/default OpenXR runtime before launching. Any existing PCVR setup already has this.
- **Up-to-date GPU drivers with Vulkan support** — rendering runs on Vulkan, which ships with current NVIDIA / AMD / Intel drivers. A modern GPU is recommended (developed and tested on an RTX 4080 + Pimax).
- **Microsoft Visual C++ Redistributable (x64), 2015–2022.** This ships with essentially every modern game and is already present on any gaming PC, but on a fresh Windows install you may need it — grab the latest "vc_redist.x64.exe" from Microsoft if the app fails to start.
- **A stereoscopic 3D source** — a `geo-11` / `Katanga` mod (or the SuperVrExport / GeoVrExport ReShade addon) injected into your game, or any SBS / TAB content via desktop duplication.

> Put the two exes in the same folder, keep a `presets/` folder beside them for saved configurations, and install the geo-11 / addon DLL into your game as that mod instructs.

---

## 🚀 Highlights

- 🎬 **9 stereo modes** including a true Checkerboard 3D demux for SuperDepth3D output
- 🌐 **3 screen shapes** — curved sphere, box theatre, fisheye dome — each with mesh-level extrusion control
- 🖱️ **Mouse emulation** — drive mouse-look games with head movement (3 compatibility modes)
- 📡 **6DoF UDP streaming** — OpenTrack wire format, drives community 6DoF mods (RE Requiem, etc.)
- 🎢 **Simulated 6DoF** — parallax for flat 3D content, no game-side support required
- ⌨️ **Global hotkeys** — every action bindable, works while minimized
- 💾 **Live presets** — save/load full configurations, hot-reloaded by the viewer
- 🎯 **Real-time tuning GUI** — every slider takes effect within a single VR frame

---

## 📦 What's in the box

Two binaries, one workspace, shared wire format:

| Binary | Purpose |
|---|---|
| 🥽 **`osiris-vr-viewer.exe`** | The runtime. Tray-icon only — no desktop window. Starts an OpenXR session and renders captured stereo content onto the configured screen geometry in VR. |
| 🎛️ **`osiris-gui.exe`** | Optional control panel. Dark theme with blue / purple / yellow section accents. Real-time sliders for everything. Writes presets next to the viewer; the viewer hot-reloads. |

---

## ✨ Features

### 📥 Input Sources (auto-detected, in priority order)

1. **Katanga / geo-11** — `Local\KatangaMappedFile` shared texture (D3D11 KMT and D3D12 `DX12VRStream`). Zero-copy import into Vulkan via `VK_KHR_external_memory_win32`.
2. **Desktop Duplication** — DXGI duplication on Windows for capturing non-Katanga sources.
3. **`captrs`** — system-memory desktop capture for Linux/Unix builds.
4. **Blank** — grey fallback when nothing else attaches.

The runtime probes every 10 seconds; when a `geo-11` game starts, the viewer hot-swaps from desktop duplication to the Katanga loader without restart.

### 🎬 Stereo Modes

1. **Mono** — single source, no stereo separation
2. **Half-SBS** — half-width side-by-side
3. **Full-SBS** — full-width side-by-side
4. **Half-TAB** — half-height top-and-bottom
5. **Full-TAB** — full-height top-and-bottom
6. **Line-Interlaced** — alternates source eye per scanline (passive 3D TVs, fallback for runtimes that degrade to mono)
7. **Checkerboard 3D** — full-frame source with parity-exact demux + 4-cardinal-neighbor average for SuperDepth3D's checkerboard output

### 📐 Screen Shapes

1. 🌐 **Sphere** — curved screen with independent X/Y curvature. Default. Wraps gracefully into the periphery via the expansion stretch system.
2. 📦 **Box** — six-face theatre room with adjustable corner radius. Wraps around the viewer for letterbox-style cinema content.
3. 🐟 **Fisheye** — full dome projection.

Each shape supports independent X/Y size, X/Y curvature, head-lock following, and the full edge-stretch / extrusion system.

### 🖼️ Image Pipeline

1. **Brightness** — offset in [-1, +1]
2. **Contrast** — multiplier around mid-grey
3. **Saturation** — 0 = greyscale, 1 = neutral, 2 = punchy
4. **Sharpness** — unsharp-mask amount (0 = off)
5. **Texture sharpener** — micro-detail USM filter, separate from the global sharpness pass
6. **Filter mode** — toggle bilinear vs trilinear vs nearest at the sampler level
7. **Filter blend** — slider mix between bilinear and unfiltered (sharper but blockier)
8. **Per-eye flip / swap** — fix mirrored sources or swapped eyes from VR mods
9. **Supersampling** — render the OpenXR swapchain at up to 3× native resolution and downsample in the compositor

### ↔️ Edge Stretch & Mesh Extrusion (VHT-Grade)

Two complementary systems for filling the periphery beyond the source rectangle:

1. **Expansion Stretch** (UV-walk, 0–3 reach + 0–3 seamlessness)
   - Periphery fragments sample stretched source content, walking from the rim toward the source centre
   - Both axes walk simultaneously — no dominant-axis collapse
   - Slider 1 = how far the stretch reaches; Slider 2 = how deep into the source it consumes

2. **Mesh Forward-Extrusion** (vertex deformation, 0–3 strength + −1..+1 direction)
   - Periphery vertices physically curl forward toward the viewer in 3D space — true VHT-style fishbowl
   - Direction slider lets the user push the periphery TOWARD or AWAY from the viewer
   - Independent of the UV walk so mesh shape and source coverage are tuned separately

Plus:
3. **Edge Stretch / Extend** — classic mirror-pixel and sample-extension fillers for the frame's outer ring
4. **Edge Expand** — gradual variant that tapers smoothly into the periphery

### 🎢 Simulated 6DoF

1. **Movement amount** — head translation drives screen-space parallax (0–20)
2. **Zoom amount** — head forward/back drives screen zoom independently (0–20)
3. **Motion smoothness** — exponential damping (0–0.99)
4. **Auto-anchor on toggle** — re-anchors when enabled so the screen doesn't jump

Disabled when head-lock is on (the screen is already following the head).

### 🖱️ Mouse Emulation

Convert head movement into OS mouse cursor motion to drive 3DoF camera control in mouse-look games (Forza, Skyrim, Witcher 3, etc.).

**Three compatibility modes:**

1. **Relative (`SendInput`)** — Uses `MOUSEEVENTF_MOVE` with `MOUSEEVENTF_MOVE_NOCOALESCE`. Reaches Win32-message games and most raw-input games (modern FPS, racing sims).
2. **Absolute (`SetCursorPos`)** — Tracks a virtual cursor and sets it absolutely each frame. Reaches games that poll `GetCursorPos` (Witcher 3 with Hardware Cursor OFF, older RPGs, point-and-click titles).
3. **Both** *(default)* — Sends through both paths simultaneously. Maximum compatibility.

Sub-pixel accumulator preserves slow head movements that would otherwise truncate to 0 px. Sensitivity and mouse-speed sliders independently tune response.

### 📡 6DoF UDP over Network

Stream the head pose to any UDP listener as 48-byte packets in **OpenTrack wire format** (6 little-endian f64: x, y, z in cm, yaw, pitch, roll in degrees). Drives community 6DoF mods like the RE Requiem head-tracking mod, REFramework integrations, or any OpenTrack-aware receiver.

1. ⚙️ Configurable target IP and port (default `127.0.0.1:4242`)
2. 🔄 **Per-axis flips** — X, Y, Z, Yaw, Pitch, Roll — invert any axis to match game conventions
3. 📊 **Rotational gains** — independent multipliers for Yaw, Pitch, Roll (0–3)
4. 📊 **Position gains** — independent multipliers for X, Y, Z (0–3)
5. 🎯 Reference pose captured on enable; all packets carry deltas
6. ⚡ Non-blocking socket — never stalls the render loop

### ⌨️ Global Hotkeys

Every action is bindable from the GUI's yellow Hotkeys section. Captured with click-to-bind, persisted with presets, and active even when the GUI is minimized via a background polling thread:

1. Cycle 3D mode
2. Cycle screen shape
3. Recenter
4. Toggle head-lock
5. Toggle simulated 6DoF
6. Toggle mouse emulation
7. Toggle 6DoF UDP stream
8. Screenshot
9. Move forward / backward (Z)
10. Move left / right (X)
11. Move up / down (Y)
12. Swap eyes
13. Restart session

### 🔒 Head-Lock

Anchors the screen to the user's head pose every frame — full follow on all 3 axes with averaged per-eye orientation (cancels HMD toe-in bias). Compatible with all screen shapes and stereo modes.

### 🎯 Recenter & Roll

1. **Recenter** — Re-anchors the screen to the current head pose. Available as a button, hotkey, or tray menu item.
2. **Roll offset** — corrects head-tilt bias around the forward axis. Works in head-lock and free modes.

### 📷 Screenshot

Captures the current left-eye VR view to a PNG file in the presets folder. Bindable to a hotkey for in-VR captures.

### 💾 Presets

1. Save / load full configurations as JSON next to the viewer
2. `presets/default.json` is hot-reloaded by the running viewer when overwritten by the GUI
3. Forward-compatible: old preset files load cleanly into newer builds via `#[serde(default)]` defaults on every field

### 🎨 GUI Polish

1. 🎬 ⚙ 💾 📐 🖼 ↔ 🎢 🖱 📡 ⌨ — every section has a fitting symbol
2. Custom banner image in the title bar
3. Three-section accent system: blue (rendering), purple (head tracking), yellow (hotkeys)
4. Compact layout — every slider/widget kept full size; only padding tightened
5. Dark theme with white-text-on-color section headers
6. Native window icon embedded via `winres`

---

## 🧱 Architecture

```
┌─────────────────────┐     SHM (POD struct, ~1 KB)     ┌──────────────────────┐
│   osiris-gui.exe    │ ◄──────────────────────────────► │ osiris-vr-viewer.exe │
│  (egui control UI)  │      LiveParams v24              │   (OpenXR runtime)   │
└─────────────────────┘                                  └──────────────────────┘
         │                                                          │
         ▼                                                          ▼
   presets/*.json                                            ┌──────────────┐
   hotkey bindings                                           │   Loaders:   │
   live config                                               │   Katanga →  │
                                                             │   DesktopDup │
                                                             │   → captrs → │
                                                             │   Blank      │
                                                             └──────┬───────┘
                                                                    │
                                                                    ▼
                                                             ┌──────────────┐
                                                             │ wgpu / Vulkan│
                                                             │  multiview   │
                                                             │  renderer    │
                                                             └──────┬───────┘
                                                                    │
                                                                    ▼
                                                             ┌──────────────┐
                                                             │   OpenXR     │
                                                             │   Compositor │
                                                             └──────────────┘
```

- **`osiris-shared`** crate: wire format for SHM (`LiveParams` struct, version-checked)
- **`osiris-gui`**: egui app with live preview of all settings
- **`osiris-vr-viewer`**: tray-only runtime, hot-reloads `LiveParams` from SHM each frame
- All inputs and settings flow GUI → SHM → viewer with version handshake and atomic updates

---

## ⚙️ Build

Requires **Rust 1.74+** and the **Vulkan SDK** on Windows.

```sh
git clone https://github.com/<your-org>/osiris-vr-viewer
cd osiris-vr-viewer
cargo build --release
```

Built binaries land in `target/release/`. The GUI's app icon is embedded via `winres` from `gui/app-icon.ico`; the viewer's icon comes from `assets/icon.ico`.

---

## 🎮 Usage

### Quick Start

1. Launch `osiris-vr-viewer.exe` — appears as a tray icon
2. Launch `osiris-gui.exe` — opens the control panel
3. Pick a stereo mode and screen shape
4. Don your headset — the viewer auto-detects available capture sources and renders

### Tray Menu

Right-click the tray icon for: Recenter, Screenshot, Toggle Head-Lock, Cycle Stereo Mode, Cycle Screen Shape, Quit.

### Mouse Emulation Setup

1. Toggle **Enable mouse emulation** in the GUI's purple Mouse Emulation section
2. Pick a compatibility mode:
   - **Both** for first-time use (works in most games)
   - **Relative** if the game has rapid camera over-rotation in Both mode
   - **Absolute** if the game ignores Relative entirely (Witcher 3 hardware-cursor-off)
3. Tune Sensitivity and Mouse speed to taste

### 6DoF UDP Setup (RE Requiem mod example)

1. In the game's mod config, set OpenTrack listener to `127.0.0.1:4242`
2. In Osiris GUI's purple **6DOF MODS** section, enable the UDP stream
3. Verify IP is `127.0.0.1` and port is `4242`
4. If any axis goes the wrong way, toggle its **Axis flip** checkbox
5. Tune **Rotational gains** and **Position gains** to match game scale

---

## 📋 Tested

- ✅ SteamVR / OpenXR runtime
- ✅ Oculus / Meta Quest Link
- ✅ Virtual Desktop (OpenXR)
- ✅ Varjo runtime
- ✅ Pimax PiTool / OpenXR
- ✅ Mouse emulation: Skyrim VR (mod), Forza Horizon, Witcher 3 (HW cursor off — Absolute mode)
- ✅ UDP 6DoF: RE Requiem mod, REFramework

---

## 🙏 Credits

- **[VRScreenCap](https://github.com/artumino/VRScreenCap)** by [@artumino](https://github.com/artumino) — the original architecture this fork is built on
- **[geo-11](https://www.helixmod.com/)** — stereoscopic 3D mod platform
- **[OpenTrack](https://github.com/opentrack/opentrack)** — wire format reference for the 6DoF UDP output
- **[SuperDepth3D](https://reshade.me/forum/shader-presentation/3935-superdepth3d)** — checkerboard 3D output reference

---

<div align="center">

### Built for VR enthusiasts who want full control of their stereoscopic 3D playback

🦀 Made with Rust • 🥽 Powered by OpenXR • ⚡ Rendered with wgpu

</div>
