<div align="center">

<img width="1584" height="672" alt="Osiris VR Viewer banner" src="https://github.com/user-attachments/assets/4c28d98c-b4b8-4b49-94a4-623201f72af4" />

# Osiris VR Viewer

**A full-resolution OpenXR viewer for gaming in stereoscopic 3D.**
Projects 3D-mod output onto a curved screen, sphere, box theatre, or fisheye dome — with a full image pipeline, depth/parallax controls, and head-tracking output.

[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Graphics](https://img.shields.io/badge/wgpu-Vulkan-AC162C?logo=vulkan&logoColor=white)](https://wgpu.rs/)
[![XR Runtime](https://img.shields.io/badge/OpenXR-1.0-1F6FEB)](https://www.khronos.org/openxr/)
[![Status](https://img.shields.io/badge/version-0.6.0--dev-success)](#)

📖 **New here? Read the two PDF guides in the release** — an app **glossary** and a **3D-mod setup guide**.

</div>

---

## What it is

Osiris takes a stereoscopic 3D image — from a **geo-11 / Katanga** mod, a **SuperDepth3D / Geo3D** export, or any **SBS / TAB** source — and displays it in your headset on a curved screen. It adds sharpening/clarity/filtering, depth and parallax controls, and can drive games with your head (mouse, gamepad, or 6DoF output).

It's a Rust fork of [VRScreenCap](https://github.com/artumino/VRScreenCap) by [@artumino](https://github.com/artumino), and runs on any OpenXR runtime (SteamVR, Oculus/Quest, Varjo, Pimax…).

**Two programs, no installer:**

| Program | What it does |
|---|---|
| 🥽 `osiris-vr-viewer.exe` | The viewer. Runs in the system tray and renders 3D content in VR. |
| 🎛️ `osiris-gui.exe` | The control panel. Every setting is a live slider — changes apply within one VR frame. |

> **They're linked.** Launching the GUI auto-starts the viewer; **closing the GUI closes the viewer too.** Just close the GUI when you're done.

---

## 🚀 Quick Start

1. Put both `.exe` files in one folder. (Keep a `presets/` folder beside them.)
2. **Run both as Administrator** — right-click each → Properties → Compatibility → *Run as administrator*. *(Required for hotkeys and mouse/gamepad emulation to reach games.)*
3. Set your **OpenXR runtime** active (your headset's native runtime, or SteamVR).
4. Launch **`osiris-vr-viewer.exe`** — it lands in the tray and **auto-opens the control panel**.
5. Pick a **stereo mode** + **screen shape**, put on your headset. Osiris auto-detects the source and renders.
6. Start your **geo-11 / addon game** — it hot-swaps to the full-res image automatically.

**Tray menu:** Recenter · Screenshot · Toggle Head-Lock · Cycle Stereo Mode · Cycle Screen Shape · Quit.

---

## ✅ Requirements

- **VR headset on an OpenXR runtime.** **Wired PCVR is recommended** (DisplayPort/HDMI or USB Link). Wireless (Virtual Desktop) works but can add encoding/tracking issues depending on your Wi-Fi.
- **Windows 10/11 (64-bit)** + **up-to-date GPU drivers** (Vulkan). Tested on RTX 4080 + Pimax.
- **An active OpenXR runtime** (SteamVR, Pimax OpenXR, Oculus…). Any PCVR setup has this.
- **A 3D source** — a geo-11/Katanga mod, the [Super-VRExport addon](https://github.com/BerZerker96/Super-VRExport-Addon), or SBS/TAB via desktop capture.
- If the app won't start on a fresh Windows install, get the [MS Visual C++ Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe).

---

## 🎮 3D Mods & Companion Tools

Install a mod into your game per its own docs, then let Osiris pick up the image. → [3D-Mod Setup Guide (PDF)](https://github.com/user-attachments/files/29437218/3D-Mod-Setup-Guide-1.pdf)

| Mod / Tool | What it is |
|---|---|
| **[geo-11](https://helixmod.blogspot.com/2013/10/game-list-automatically-updated.html)** | Main DX11 stereo driver. Per-game fixes at the link. Outputs full-SBS via Katanga. |
| **[Geo3D](https://github.com/Flugan/Geo3D-Installer)** | Automated geometry-3D installer for a large game library. |
| **[SuperDepth3D](https://github.com/BlueSkyDefender/Depth3D)** | ReShade depth-based 3D — works on almost any game. |
| **[wiz3D](https://github.com/effcol/wiz3D)** | Geometry-3D injector. |

**Recommended add-ons:**
- 🧭 **[6DOF Head-Tracking Mods Hub](https://github.com/BerZerker96/6DOF-Head-Tracking-Mods-Hub)** — the central hub collecting 6DoF head-tracking mods for many games. **Start here.**
- 🎯 **[6DOF Mods by itsloopyo](https://github.com/itsloopyo/itsloopyo)** — best full-6DoF camera-tracking mods, updated regularly ([Discord](https://discord.gg/rYG4Nphxf)).
- 🆕 **[Super-VRExport addon](https://github.com/BerZerker96/Super-VRExport-Addon)** — the preferred way to get **full-res SBS** out of SuperDepth3D / Geo3D.
- 🕹️ **[DS4Windows — Osiris VR](https://github.com/BerZerker96/DS4WINDOWS---OSIRS-VR)** — alternative 3DoF head-tracking feed.

---

<div align="center">

<!-- ⬇️  GUI SCREENSHOT — replace the src below with your control-panel image  ⬇️ -->
<img width="900" alt="Osiris VR Viewer — control panel GUI" src="docs/gui-screenshot.png" />

*The Osiris control panel*

</div>

---

## ✨ Features

### 🖼️ Picture & Screen
- 👓 **7 stereo modes** — Mono, Half/Full-SBS, Half/Full-TAB, Line-Interlaced, and true **Checkerboard 3D**.
- 📐 **3 screen shapes** — **Sphere** (curved), **Box** (theatre room), **Fisheye** (dome) — each with X/Y size, curvature, head-lock, and a concave back-wall.
- 🎨 **Image pipeline** — brightness/contrast/saturation, sharpen + texture sharpen, **CAS**, **Dehaze/Clarity**, and **Katanga Filters** (one-toggle "stronger image" for live Katanga sources).
- 🔍 **Resampling** — Bilinear, Bicubic, Lanczos (blend sliders). *⚠️ Bicubic/Lanczos are GPU-heavy.*
- ⬆️ **Supersampling** — render above native for clarity. *⚠️ Very heavy — max ~1.20 recommended.*
- ↔️ **Edge stretch & Hybrid Immersion** — fill the periphery beyond the source: expansion stretch, **mesh forward-extrusion** (wrap-around fishbowl), mirror/extend fillers, and a **rear-360 wrap**.

### 🎯 Depth, Parallax & Motion
- 🧊 **Depth & stereo** — Separation, Convergence, **Dynamic Depth** (lean to expand the scene).
- 🪆 **Depth Layers** — a 10-layer "diorama" that adds soft depth to flat 3D, with band layouts (rings / rows / columns), cascading follow-through delay, motion-reactive warp, and a lean-in dolly-zoom.
- 🪟 **Simulated 6DoF** — head movement creates parallax on flat 3D, in two modes: **Follow** (screen moves with you) or **Off-axis "window"** (look-through-a-window feel).
- 🔒 **Head-Lock & Stable Lock** — pin the screen to your head; modes include **Default**, **Delayed Lock**, **DeJitter** (soft-lock spring), and **Stable Lock** (subtle fish-tank parallax).
- 🧭 **Directional 6DoF** — turns head rotation into a small "peek around the edges" position shift.
- ⚙️ **Auto Adjust** — moves the screen when head-lock turns on and reverts when off (X/Y/Z/height/roll).
- 🌀 **Motion features** *(experimental — enable one at a time)* — optical-flow extrapolation, temporal blend, frame pacing, pose prediction.

### 🎛️ Overlay, Hotkeys & Presets
- 🖥️ **Katanga ImGui** — the **full control panel floating inside VR**, mouse-driven, hotkey-toggled mid-game. Works in any window mode (renders as its own VR layer). Tune stereo, image, 6DoF, and input controls without leaving the headset.
- ⌨️ **Global hotkeys** — every action rebindable (click-to-bind), saved with presets, works while minimized. Also bindable to **VR controllers**, which sync back to the GUI.
- 📍 **Recenter · Roll · Screenshot** — via button, hotkey, or tray.
- 💾 **Live presets** — JSON configs the running viewer hot-reloads instantly; old presets keep working in newer builds.
- 🌈 **11 GUI themes** + custom banner/logo/section/background images.

---

## 🕹️ Controlling Games With Your Head

| Output | What it does | Needs |
|---|---|---|
| 🖱️ **Mouse emulation** | Head → mouse-look (Skyrim, Forza, Witcher 3…). Modes: Relative / Absolute / Both / **Interception**. | Interception mode: [Interception driver](https://github.com/oblitum/Interception/releases) |
| 🕹️ **Joystick emulation** | Head → virtual Xbox right-stick. Modes: Relative-Delta (FPS) / Joy-Look (flight/driving). | [ViGEmBus driver](https://github.com/nefarius/ViGEmBus/releases) |
| 📡 **6DoF UDP output** | Head pose in **OpenTrack format** to drive community 6DoF mods (default `127.0.0.1:4242`). Get mods from the **[6DOF Hub](https://github.com/BerZerker96/6DOF-Head-Tracking-Mods-Hub)** and **[itsloopyo](https://github.com/itsloopyo/itsloopyo)**. | — |
| 🎯 **TrackIR output** | Drives **native TrackIR games** (DCS, Elite, ETS2, Assetto, ARMA…) directly. | Bridge DLL (below) |

> ⚠️ **Anti-cheat:** Interception and ViGEm are kernel-level inputs — some anti-cheats flag them. Turn emulation **off before launching** a protected game and on once it's running, or use a user-mode mouse mode.

All outputs share per-axis flips and gains, capture a reference pose on enable, and never stall rendering.

### 🎯 TrackIR setup (one-time)

Osiris writes your head pose into the standard **FreeTrack shared memory**; a bridge **`NPClient64.dll`** carries it into the game.

```
Osiris viewer → FT_SharedMem → NPClient64.dll → game
```

1. Take the **`NPClient64.dll`** included in the release.
2. Put it where the game looks:
   - **Registry games** (DCS, Elite, ETS2, Assetto, Falcon BMS, ARMA): keep the DLL in a folder (e.g. `C:\Osiris\trackir\`) and set `HKEY_CURRENT_USER\Software\NaturalPoint\NATURALPOINT\NPClient Location\Path` to that folder.
   - **Game-folder games** (Everspace 2 etc.): back up the game's existing `NPClient64.dll` → `.bak` and drop this one in its place.
3. Flip **TrackIR Game** in Osiris, enable head-tracking in the game's options, and look around. Flip any reversed axis with the per-axis controls.

> Every TrackIR tool (real TrackIR, OpenTrack, vorpX) works this way. Full details in `osiris-npclient/README.md`.

### 🔌 VR Data to UDP
A second independent output streaming head + per-controller data to **[FreePIE](https://github.com/Ofisare/FreePIE)** / **[VRCompanion](https://github.com/Ofisare/VRCompanion)** (own IP/port, flips, gains).

---

## 🎧 Headset Compatibility *(fixes Quest/Pico/streaming jitter)*

A toggle + headset picker (in the header, left of Debug) that fixes **jitter and lag on non-Pimax headsets**. **Default OFF** — Pimax and SteamVR are unaffected.

**How to use:** pick your headset from the dropdown (Meta Quest · Quest/Pico via Virtual Desktop · Pico Connect · HTC Vive streaming · Varjo · WMR · Other streaming). Picking one turns it on. Fixes apply **live — no restart**.

**What it does:** skips a wrong depth submission (stops head-movement swimming), removes a desktop-vsync jitter, lets streaming headsets pace themselves, and adds a little prediction to hide encode lag. **Leave OFF for Pimax / SteamVR** — they're already correct.

---

## 🕹️ Runtimes & ⚡ Performance

**OpenXR runtimes** — Osiris runs on your headset's **native runtime** (Pimax OpenXR, Oculus…) or **SteamVR** (SteamVR → Settings → OpenXR → *Set as OpenXR Runtime*). Performance is near-identical; pick whichever is more stable.

> ⚠️ **Katanga full-res + SteamVR performs poorly** — for Katanga gaming, prefer your headset's **native OpenXR runtime**. SteamVR is fine for desktop/SBS content.

**Performance tips** — a few features are powerful but costly:
- **Lower supersampling first** if you drop frames (≤ ~1.20), then filter quality (Lanczos → Bicubic → Bilinear), then the experimental motion features.
- **Costs stack** — high SS + Lanczos + CAS + flow together will tax any GPU.

**🟣 Pimax flicker / ATW drag** — try, in order: (1) switch to the **SteamVR OpenXR runtime**, (2) lock to a clean **90/120 Hz half-rate**, (3) add a few ms of **pose prediction** (~8–10 ms for Crystal).

---

## 🩺 Troubleshooting

- **Reporting a bug?** Toggle **Debug** to write `osiris-diagnostics.log`; include it with `osiris.log`.
- **Won't start / "device not available":** headset off/asleep or no active OpenXR runtime — power on, start the runtime, relaunch. On fresh Windows, install the VC++ Redistributable.
- **Black screen:** confirm a source is detected (start the geo-11 game or enable desktop capture) and **match the stereo mode to the source**.
- **Flat / doubled / eyes swapped:** pick the correct stereo mode; use the **Swap eyes** hotkey.
- **Katanga ImGui panel missing:** enable it (Katanga ImGui section) and bind a hotkey; move your mouse to move its cursor.
- **Mouse/joystick/hotkeys do nothing in a game:** **run both `.exe` files as Administrator.** For mouse, try Both → Relative → Absolute, or the Interception driver. For joystick, install ViGEmBus.
- **Dropped frames:** lower supersampling, drop filter quality, disable experimental motion features one at a time.
- **Presets not applying:** keep a `presets/` folder beside the exes; `presets/default.json` hot-reloads on save.
- **Wireless artifacts:** switch to a **wired** connection.

---

## 🙏 Credits

- **[VRScreenCap](https://github.com/artumino/VRScreenCap)** by [@artumino](https://github.com/artumino) — original architecture
- **[Katanga](https://github.com/bo3b/katanga)** by [@bo3b](https://github.com/bo3b) — shared-texture VR streaming layer
- **[geo-11 / HelixMod](https://helixmod.blogspot.com/2013/10/game-list-automatically-updated.html)** — stereoscopic 3D platform & per-game fixes
- **[Geo3D-Installer](https://github.com/Flugan/Geo3D-Installer)** by [@Flugan](https://github.com/Flugan)
- **[SuperDepth3D / Depth3D](https://github.com/BlueSkyDefender/Depth3D)** by [@BlueSkyDefender](https://github.com/BlueSkyDefender)
- **[wiz3D](https://github.com/effcol/wiz3D)** by [@effcol](https://github.com/effcol)
- **[OpenTrack](https://github.com/opentrack/opentrack)** — wire-format reference for the 6DoF UDP + TrackIR output

---

<div align="center">

<img width="3420" height="4048" alt="Osiris screenshot" src="https://github.com/user-attachments/assets/0f3375a7-cc8b-4822-a780-ebfeaf618b65" width="45%" />
<img width="3420" height="4048" alt="Osiris screenshot" src="https://github.com/user-attachments/assets/db22d488-5a47-4434-a025-2113cf873762" width="45%" />

</div>
