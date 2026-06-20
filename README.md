<div align="center">

<img width="1584" height="672" alt="Gemini_Generated_Image_w1uoysw1uoysw1uo" src="https://github.com/user-attachments/assets/4c28d98c-b4b8-4b49-94a4-623201f72af4" />

# Osiris VR Viewer

### A full-resolution OpenXR 3D viewer made for Gaming with Stereoscopic 3D mods — curved screen geometry, head-tracking output, and a real-time tuning GUI.

[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Graphics](https://img.shields.io/badge/wgpu-Vulkan-AC162C?logo=vulkan&logoColor=white)](https://wgpu.rs/)
[![XR Runtime](https://img.shields.io/badge/OpenXR-1.0-1F6FEB)](https://www.khronos.org/openxr/)
[![Status](https://img.shields.io/badge/version-0.6.0--dev-success)](#)

</div>

---

## 📖 What it is

**Osiris VR Viewer** takes a stereoscopic 3D image — from a `geo-11` / `Katanga` mod, the SuperDepth3D / Geo3D ReShade export, or any side-by-side / top-and-bottom source — and projects it into your headset on a curved screen, a sphere, a box theatre, or a fisheye dome. On top of that it adds a full image pipeline (sharpening, clarity, filtering), depth and parallax controls, and the ability to drive games with your head (mouse, gamepad, or 6DoF tracking output).

It's a Rust fork of [VRScreenCap](https://github.com/artumino/VRScreenCap) by [@artumino](https://github.com/artumino), heavily extended, and runs on any wired OpenXR runtime (SteamVR, Oculus / Quest Link, Varjo, Pimax, etc.).

**Two programs, no installer:**

| Program | What it does |
|---|---|
| 🥽 **`osiris-vr-viewer.exe`** | The viewer. Lives in the system tray (no window) and renders your 3D content in VR. |
| 🎛️ **`osiris-gui.exe`** | The control panel. Every setting is a live slider — changes apply within one VR frame. Saves presets the viewer reads instantly. |

> 🔗 **The two are linked.** Launching the GUI auto-starts the viewer, and **closing the GUI control panel also closes the viewer** — the GUI sends a clean shutdown, then makes sure the viewer process exits. So just close the GUI when you're done; there's no separate viewer to quit.

---

<div align="center">

<img width="4764" height="2904" alt="Gemini_Generated_Image_4fhcem4fhcem4fhc" src="https://github.com/user-attachments/assets/61c862e1-735e-448a-9b6c-786dc78b6410" />


*A few of the games in stereoscopic 3D through Osiris*

</div>

---

## 🎮 Supported 3D Mods & Companion Tools

Install the mod into your game as its own docs say, then let Osiris pick up the 3D image.

| Mod / Tool | Link | What it is |
|---|---|---|
| **geo-11** | [Game list (HelixMod)](https://helixmod.blogspot.com/2013/10/game-list-automatically-updated.html) | The main DX11 stereo driver. Per-game fixes are at the link. Outputs full-SBS via Katanga. |
| **Geo3D** | [Geo3D-Installer](https://github.com/Flugan/Geo3D-Installer) | Automated geo-3d installer / geometry 3D for a large game library. |
| **SuperDepth3D** | [Depth3D (BlueSkyDefender)](https://github.com/BlueSkyDefender/Depth3D) | ReShade depth-based 3D — works on almost any game. |
| **wiz3D** | [wiz3D (effcol)](https://github.com/effcol/wiz3D) | geometry 3D injector. |

> **🆕 🎯🎯🎯 ⚠️⚠️⚠️ Best Camera Tracking Mods FULL 6DOF — [6DOF MODS by itsloopyo](https://github.com/itsloopyo/itsloopyo)**
> Highly recommended 6dof mods for various games , new mod releases regularly,  join the project discord to try WIP mods [Discord](https://discord.gg/Vb9JEgArV).
 
> **🆕 Best full-res capture — [Super-VRExport / Geo-VRExport addon](https://github.com/BerZerker96/Super-VRExport-Addon)**
> The preferred way to get **full-resolution SBS** out of **SuperDepth3D** and **Geo3D** into Osiris. Use it in place of older half-res export paths.

> **🕹️ 3DoF tracking alternative — [DS4WINDOWS — OSIRIS VR](https://github.com/BerZerker96/DS4WINDOWS---OSIRS-VR)**
> Another way to feed **3DoF head tracking** to games, alongside or instead of Osiris's built-in mouse/gamepad/UDP output.

---

## ✅ Requirements

Drop the two `.exe` files into one folder and run — there's no installer. You'll need:

- **A wired VR headset on an OpenXR runtime.** Osiris streams a shared GPU texture straight into your headset, which needs a **wired** PCVR connection (DisplayPort / HDMI, or a USB Link cable).
- **Windows 10 or 11 (64-bit).**
- **An active OpenXR runtime** (SteamVR, Pimax OpenXR, Oculus, etc.). Osiris talks to your headset through it — set your preferred one as the active OpenXR runtime first (see [Runtimes](#-runtimes-openxr-vs-steamvr)). Any PCVR setup already has this.
- **Up-to-date GPU drivers** (rendering uses Vulkan, included in current NVIDIA / AMD / Intel drivers). Developed and tested on an RTX 4080 + Pimax.
- **Microsoft Visual C++ Redistributable (x64), 2015–2022** — already on any gaming PC; grab `vc_redist.x64.exe` from Microsoft only if the app won't start on a fresh Windows install.
- **A 3D source** — a `geo-11` / `Katanga` mod, the [Super-VRExport / Geo-VRExport](https://github.com/BerZerker96/Super-VRExport-Addon) addon, or any SBS / TAB content via desktop capture.

> Keep a `presets/` folder next to the exes for saved configurations.

> ⚠️ **Run both `.exe` files as Administrator (recommended).** Right-click each → **Properties → Compatibility → "Run this program as an administrator"** (do it for **both** `osiris-vr-viewer.exe` and `osiris-gui.exe`). Windows blocks a normal-privilege program from sending input to a higher-privilege window, so without elevation the **global hotkeys**, **mouse emulation**, and **gamepad emulation** can silently fail to reach games that run elevated or grab input exclusively. Running elevated forces all three to work across **every** game, in **borderless *and* exclusive-fullscreen** alike.

> ⚠️ **Wired only — no wireless streaming.** Osiris relies on a shared-GPU-texture path that wireless streaming does **not** expose, so it **will not work over Virtual Desktop (or other wireless/streamed runtimes)**. Use a wired headset on a native or SteamVR OpenXR runtime.

---

<div align="center">

<img width="2477" height="1495" alt="2026-06-15 17_02_17-OSIRIS VR VIEWER" src="https://github.com/user-attachments/assets/7d55d5bc-f4c4-4f49-825a-dcb55101fba4" />


*The Osiris control panel*

</div>

---

## 🚀 At a glance

- 🎬 **7 stereo modes**, including a true Checkerboard demux for SuperDepth3D
- 🌐 **3 screen shapes** — sphere, box theatre, fisheye dome — with mesh extrusion
- 🧊 **Depth controls** — separation, convergence, dynamic depth, 10-layer depth layers
- 🪟 **Two parallax modes** — follow, or an off-axis "window" feel — plus subtle **Stable Lock**
- 🖱️ **Mouse emulation** and 🎮 **gamepad emulation** — control games with your head
- 📡 **6DoF tracking output** over UDP (OpenTrack format) for community 6DoF mods
- 🎯 **TrackIR / FreeTrack output** — drive native TrackIR games (DCS, Elite Dangerous, ETS2, Assetto Corsa, ARMA…) directly with your head, no UDP middleman
- 🎛️ **Rich image pipeline** — CAS, dehaze, sharpen, bicubic & Lanczos filters
- 🖥️ **Katanga ImGui** — the **full control panel floating inside VR**, mouse-driven, hotkey-toggled mid-game
- 🛟 **Self-healing capture** — instant desktop fallback and automatic recovery when a game hangs or exits
- ⌨️ **Fully rebindable hotkeys** + VR-controller binding, even while minimized
- 💾 **Live presets** the viewer hot-reloads

---

# ✨ Features

## 🖼️ Picture & Screen

### 📥 Input sources (auto-detected, in priority order)

1. **Katanga / geo-11** — the shared 3D texture from a geo-11 game, captured directly with no extra copy.
2. **Desktop Duplication** — captures the Windows desktop for non-Katanga sources (e.g. SuperDepth3D / Geo3D via ReShade, if you're not using the VRExport shared texture).

When a geo-11 game starts, the viewer **hot-swaps** to the full-res Katanga source automatically — no restart — and **reconnects seamlessly** if the game changes the shared texture mid-session. On game exit it falls back to the desktop on its own.

---

### 👓 Stereo modes

Mono · Half-SBS · Full-SBS (geo-11 mods) · Half-TAB · Full-TAB · Line-Interlaced (passive 3D TVs) · **Checkerboard 3D** (DLP / SuperDepth3D, with a parity-exact demux).

---

### 📐 Screen shapes

- 🌐 **Sphere** — curved screen with independent X/Y curvature (the default).
- 📦 **Box** — a six-sided theatre room with adjustable corner radius.
- 🐟 **Fisheye** — full dome projection.

Each shape has its own X/Y size, X/Y curvature, head-lock following, a **concave back-wall** control, and the full edge-stretch system below.

---

### 🎨 Image pipeline

- **Brightness / Contrast / Saturation** — the basics.
- **Sharpness** and a separate **Texture sharpener** for micro-detail.
- **Contrast Adaptive Sharpening (CAS)** — edge-aware sharpening (0–10).
- **Dehaze / Clarity** — local-contrast lift (0–10).
- **Katanga Filters** — a one-toggle "stronger image" set (extra CAS / dehaze / clarity) that only kicks in on a live Katanga source, stacked on top of your normal settings. Great for instantly waking up a dull game image; toggle it with a hotkey.
- **Resampling filters** — **Bilinear**, **Bicubic**, and **Lanczos**, each a blend slider. ⚠️ *Bicubic and especially Lanczos are GPU-heavy — see [Performance](#-performance--tuning).*
- **Flip / swap per eye** — fix mirrored or swapped sources.
- **Supersampling** — render above native for extra clarity. ⚠️ *Very heavy — recommended max **1.20**.*

---

### ↔️ Edge stretch, mesh extrusion & Hybrid Immersion

Fill the periphery beyond the source rectangle for a more immersive, wrap-around feel:

- **Expansion Stretch** — periphery pixels sample stretched content from the rim inward. Two sliders: how far the stretch reaches, and how deep into the image it pulls from.
- **Mesh Forward-Extrusion** — the periphery physically curls toward (or away from) you in 3D — a true wrap-around fishbowl effect. Strength + direction sliders.
- **Edge Stretch / Extend / Expand** — simpler mirror-and-extend fillers for the outer ring.
- **Hybrid Immersion** — an even rim-stretch with an optional **rear-360 wrap** (stretch, direction, dim, and motion-fade controls) for maximum coverage.

---

## 🎯 Depth, Parallax & Motion

### 🧊 Depth & stereo geometry

- **Separation** — how far apart the two eye images are (0–3). Higher = more 3D pop, lower = flatter.
- **Convergence** — slides the comfortable focal plane in or out of the screen.
- **Dynamic Depth** — links leaning in/out to convergence and separation (with optional **looming**) so the scene gently expands as you move. (Pauses while Stable Lock is on.)
- **Depth Layers** — a 10-layer “diorama” that gives flat 3D a soft sense of depth: ten layers each shift by a different amount as you sway. Choose the **band layout** — default **concentric rings** (centre → rim), **horizontal bands** (10 rows, delay cascading top→bottom), or **vertical columns** (10 side-by-side, delay cascading across) — all sharing the same follow-through delay and ground prior. The per-layer **follow-through delay** ripples motion through the layers like a cascade, and an **Invert delay** toggle flips which end leads — *inner closest* (delay spreads outward) or *outer closest* (delay spreads inward). A **Motion-reactive** toggle makes the warp pop while your head is moving and settle when you hold still. Moving **forward/back also zooms** the layers — leaning in magnifies, leaning back shrinks (a dolly-zoom tunnel), with the same delay rippling through it; a **Convex** control domes the centre of that zoom outward like a lens, intensifying as you lean in. Controls for strength, separation, delay, invert, band mode, motion-reactive, ground bias, horizon, perspective, in/out zoom, convex, falloff curve, and reach.

---

### 🪟 Simulated 6DoF (two parallax modes)

Head movement creates parallax on flat 3D content — no game support needed.

- **Default (follow)** — the screen moves with your head (classic parallax).
- **Off-axis "window"** — the screen acts like a fixed window: the deeper content sits behind the frame, the more it shifts as you move — a "looking through a window" feel. Adds window-depth, parallax, edge-falloff, and vertical-balance controls.

Both share movement amount, zoom amount, smoothing, and an auto-anchor so the view doesn't jump when you enable it.

---

### 🔒 Head-Lock & Stable Lock

- **Head-Lock** — pins the screen to your head on all axes (with averaged per-eye orientation to cancel HMD toe-in). Works with every shape and stereo mode.
- **Head-lock modes** — a dropdown picks how the lock behaves: **Default** (hard lock), **Delayed Lock** (the screen catches up after a tunable delay, so micro-jitters never reach it), or **Stable Lock** (below).
- **DeJitter (soft-lock spring)** — a spring-damper between your head and the screen with **stiffness** and **max-lag** sliders; absorbs head tremor while still following deliberate movement.
- **Stable Lock** — keeps the screen head-locked but adds a **subtle, fish-tank parallax** via dedicated **Parallax X/Y** and **Parallax Z** sliders, so it still feels anchored in space. Tuned with its own gentle scaling so the sliders cover a usable subtle→strong range.

---

### 🧭 Directional 6DoF (tilt & turn)

Turns head **rotation** (yaw / pitch / roll) into a small position shift with per-axis gains — a light "peek around the edges" effect layered on top of the parallax modes.

---

### 🌀 Motion & frame features *(experimental)*

> ⚠️ **These motion features are experimental.** They can smooth motion but may also add artefacts (ghosting, shimmer) or pacing hitches. Enable one at a time and turn off if you see issues.

- **Optical flow extrapolation** — motion extrapolation (sub-pixel, framerate-independent) that warps the image forward in time to fill judder on low-framerate content. Works on **every source** (Katanga, desktop, any stereo layout). Strength slider 0.3 (subtle) → 1.0 (full).
- **Temporal blend** — smooths frame-to-frame transitions.
- **Frame pacing** — submits within a target slice of the frame.
- **Pose prediction (ms)** — extra prediction to reduce drag/flicker on some headsets (see [Pimax notes](#-pimax-users--fixing-flicker)).

---

### ⚙️ Auto Adjust

Nudges the screen automatically **when head-lock turns on**, and reverts when it turns off — so your locked and free positions can each sit where you like. Independent toggles + values for **X, Y, Z, height, and roll**.

---

## 🎮 Controlling Games With Your Head

### 🖱️ Mouse emulation

Turn head movement into mouse-cursor motion for mouse-look games (Forza, Skyrim, Witcher 3, etc.). Four compatibility modes:

- **Relative** — reaches most modern FPS and racing sims.
- **Absolute** — for games that read the cursor position directly (e.g. Witcher 3 with Hardware Cursor off, older RPGs, point-and-click).
- **Both** *(default)* — sends both user-mode paths for maximum compatibility.
- **Interception** — driver-level input that works in **all** games, including ones that ignore user-mode injection. Requires the free **[Interception driver](https://github.com/oblitum/Interception/releases)** (one-time install + reboot).

> ⚠️ **Anti-cheat caution:** Interception (and ViGEm joystick emulation) are kernel-level input devices, and some anti-cheat systems flag or block them. If a protected game refuses to start or kicks you mid-session, **turn the emulation toggles off before launching the game** and enable them after it's running — or use a user-mode method (Relative/Absolute/Both) instead.

Sensitivity and speed sliders tune the response; a sub-pixel accumulator keeps slow movements from being lost.

---

### 🕹️ Joystick emulation

Turn head movement into a **virtual Xbox controller right stick** for gamepad-driven games. Requires the free **[ViGEmBus driver](https://github.com/nefarius/ViGEmBus/releases)** (one-time install). Two modes:

- **Relative-Delta** — how *fast* you turn sets the stick deflection. Best for **FPS / look-around**.
- **Joy-Look Continuous** — your head *angle* maps to a stick position. Best for **flight / driving**.

Tunable: sensitivity, deadzone, max angle, invert X/Y, smoothness, and X/Y speed.

---

### 📡 6DoF UDP output (for 6DoF mods)

Streams your head pose in **OpenTrack format** to any UDP listener, to drive community 6DoF mods (RE Requiem head-tracking, REFramework, or any OpenTrack-aware receiver).

> 🔗 This section is built specifically for the community 6DoF camera mods — see **[itsloopyo's mods](https://github.com/itsloopyo?tab=repositories)**, which it's designed to drive.

- Configurable IP and port (default `127.0.0.1:4242`).
- Per-axis flips (X, Y, Z, yaw, pitch, roll) to match game conventions.
- Independent rotational and position gains.
- Captures a reference pose on enable; packets carry deltas. Non-blocking, so it never stalls rendering.

---

### 🎯 TrackIR Game output (FreeTrack / NPClient)

Drives **native TrackIR games directly** — no OpenTrack or UDP in between — using your head pose. Osiris writes that pose into the standard **FreeTrack shared-memory block (`FT_SharedMem`)** that the whole TrackIR / FreeTrack-Enhanced library is built around — **DCS World, Elite Dangerous, Euro Truck Simulator 2, Assetto Corsa, ARMA, Falcon BMS, Everspace 2**, and the rest.

- Enabled by the **"TrackIR Game"** toggle in the **6DoF MODS** section (desktop GUI *and* the in-VR panel).
- **It redirects, it doesn't duplicate.** Turn it on and the UDP stream above is suppressed, so the same head pose goes to the game's TrackIR interface instead. Turn it off and the UDP path is exactly as before.
- Uses the **same per-axis flip / gain tuning** as the UDP output — if an axis points the wrong way in-game, flip it with the same controls. (Units are handled for you: rotation in radians, position in millimetres.)

> ⚠️ **The toggle alone is not enough — you also need the bridge DLL (one-time setup).** Games never read `FT_SharedMem` themselves; every TrackIR game loads a small **`NPClient64.dll`** and asks *it* for the data. Osiris ships its own bridge DLL for this — build it and drop it where the game looks (see **[TrackIR setup](#-trackir-setup-one-time)** below). It's a clean **ISC/MIT-licensed** build (based on OpenTrack's `npclient.c`, *not* the GPL FreeTrack DLL), so no OpenTrack install is required.

---

### 🎯 TrackIR setup (one-time)

The viewer publishes your head pose to shared memory the moment you flip **TrackIR Game** on. The bridge **`NPClient64.dll`** is what carries it the last step into the game. You build the DLL once, then place it where each game looks.

```
Osiris viewer  --writes-->  FT_SharedMem  --read by-->  NPClient64.dll  -->  game
```

**1 — take the trackir NPClient64.dll included in the main release


**2 — Put the DLL where the game looks.** There are two kinds of game:

- **Registry-based games — DCS, Elite, ETS2, Assetto, Falcon BMS, ARMA…**
  Keep the DLL in a folder (e.g. `C:\Osiris\trackir\`) and point one registry value at it:
  ```
  HKEY_CURRENT_USER\Software\NaturalPoint\NATURALPOINT\NPClient Location
      Path  (String)  =  C:\Osiris\trackir
  ```
  Set it once with `regedit` (or a `.reg` file); the game then loads your DLL from there.

- **Game-folder games — Everspace 2 (and other Unreal TrackIR-plugin titles)**
  These ignore the registry and load a copy bundled **inside the game**. Replace that copy:
  1. Open the game's TrackIR plugin folder, e.g.
     `…\steamapps\common\EVERSPACE™ 2\ES2\Plugins\TrackIR\…\NPClient\Win64\`
  2. **Rename the existing `NPClient64.dll` → `NPClient64.dll.bak`** (keep the backup).
  3. Drop **this** `NPClient64.dll` in its place.

**3 — Turn it on.** Flip **TrackIR Game** in Osiris, enable **TrackIR / head-tracking** in the game's own control options, and look around. If an axis is reversed, flip it with the per-axis controls — inverted pitch is common and expected.

> 📄 Full details, the data mapping, and licensing are in **`osiris-npclient/README.md`**.

> 🪪 **Why a separate DLL at all?** It's not a limitation of Osiris — *every* TrackIR tracker (real TrackIR, OpenTrack, vorpX) works this exact way, because the game is hard-wired to load `NPClient64.dll` and call it. Even plain OpenTrack needs this DLL copied into the Everspace 2 folder. Building your own just means you don't need OpenTrack to get it.

---

### 🔌 VR Data to UDP (FreePIE / VRCompanion)

A second, independent output that streams head (and per-controller left/right) data to companion apps like **[FreePIE](https://github.com/Ofisare/FreePIE)** and **[VRCompanion](https://github.com/Ofisare/VRCompanion)** — its own IP/port, per-axis flips, and gains — for setups that don't use the OpenTrack path above.

---

## 🛠️ Overlay, Hotkeys & Presets

### 🖥️ Katanga ImGui — the control panel *inside* VR

a **full Osiris control panel rendered inside the headset**, driven by your mouse. Press the hotkey to toggle it on/off **while you're in a Katanga full-res game** — no alt-tab, no desktop mirror, no leaving the game.

It mirrors essentially the whole desktop GUI, organised in four columns with collapsible advanced groups and **colour-coded section headers** so each feature group is easy to pick out at a glance:

- **Everything tunable mid-game** — stereo mode, screen shape, geometry (with curvature & concave), the full image pipeline (filters, CAS, dehaze, bicubic/Lanczos, Katanga Filters), simulated 6DoF with Directional 6DoF and Depth Layers, and the entire edge-stretch system (Hybrid, Mirror, Repeated, Expansion/Extrusion).
- **Full input & tracking controls in-VR** — **Mouse Emu** (sensitivity, speed, method dropdown, off-axis window tuning), **Joystick Emu** (sensitivity, speed X/Y, smoothness, deadzone, max angle, invert X/Y), and a dedicated **6DOF MODS** section with the UDP-stream toggle, all **six axis gain sliders** (yaw / pitch / roll / X / Y / Z), the **TrackIR Game** toggle, and VR-data-to-UDP — so you can set up and tune head-driven control without ever leaving the headset.
- **Top-bar buttons** — **Recenter**,**Screenshot**
- **Panel controls at the top** — resize the panel (overall size, width ×, height ×), move it (offset X/Y), and set its **distance** (0.5–5 m) from inside the panel itself, or from the desktop GUI's Katanga ImGui section.
- **HUD Mode** — on: the panel follows your head; off: it stays fixed in the room.

> ✅ **Works in any window mode.** Because the panel renders its own UI as a VR layer (it doesn't capture the desktop), it shows up even over exclusive-fullscreen games — only the optional "Show GUI with overlay" focus hand-off needs borderless.

---

### ⌨️ Global hotkeys

Every action is rebindable (click-to-bind), saved with presets, and works even when the GUI is minimized:

- **View:** cycle 3D mode, cycle screen shape, swap eyes
- **Position:** recenter, move X/Y/Z, zoom in/out, roll left/right
- **Modes:** toggle head-lock, simulated 6DoF, mouse emulation, joystick emulation, 6DoF UDP
- **Katanga:** toggle the **Katanga ImGui** in-VR panel, toggle Katanga Filters, **Force Desktop** (instantly drop to desktop, then auto-return to the game)
- **Misc:** screenshot, cycle preset, restart session

> 🎮 **VR controller binding:** these actions can also be triggered from your **VR controllers** in-headset, and those toggles reflect straight back into the GUI checkboxes, so the panel always shows the true state.

---

### 📍 Recenter, Roll & Screenshot

- **Recenter** — re-anchor the screen to your current head pose (button, hotkey, or tray).
- **Roll offset** — correct head-tilt bias around the forward axis.
- **Screenshot** — save the current left-eye view to a PNG in the presets folder.

---

### 💾 Presets

Save and load full configurations as JSON next to the viewer. `presets/default.json` is hot-reloaded by the running viewer the moment the GUI saves it, and old presets keep working in newer builds (every field has a default).

---

### 🎨 GUI Theme & Appearance

Restyle the control panel to taste — every option is saved with your config:

- **Theme** — a dropdown of **11 colour themes**: *Colored (default)*, **Dark Blue**, **Black**, **Red**, **Cyan**, **White**, **Orange**, **Yellow**, **Green**, **Purple**, and **Magenta**. The non-default themes recolour every section header and frame uniformly; *Colored* keeps each section's own accent colour.
- **Custom banner image** and **custom logo image** — drop in your own art at the top of the panel, each with a one-click **Reset** back to the bundled default.
- **Section background image** — paints a translucent image behind each individual panel section.
- **Overall background image** — a full-window backdrop behind the entire control panel.

When a background image is set, **every** section — including the Hotkeys grid and the GUI Theme panel itself — turns translucent so the image shows through, and the active theme colours apply across all of them.

---

## 🕹️ Runtimes: OpenXR vs SteamVR

Osiris is an **OpenXR** app, so it runs on whichever OpenXR runtime your headset uses:

- **Native runtime** (Pimax OpenXR, Oculus, etc.) — set it as the active OpenXR runtime, then launch Osiris.
- **SteamVR runtime** — set SteamVR as the current OpenXR runtime (SteamVR → Settings → OpenXR → "Set SteamVR as OpenXR Runtime"), then launch Osiris.

**Performance is near-identical either way** — the rendering path costs the same — so pick whichever is more stable for your headset.

> ⚠️ **Katanga + SteamVR:** Osiris itself runs the same on both, but **Katanga full-res capture tends to perform poorly under the SteamVR runtime specifically.** For Katanga full-res gaming, prefer your headset's **native OpenXR runtime**. SteamVR is still great for desktop/SBS content and headsets where it's the more stable choice.

---

## ⚡ Performance & Tuning

A few features are powerful but costly — add them deliberately:

- **Supersampling is heavy.** Above native, pixel cost climbs fast. **Recommended max: 1.20.** Beyond that you usually lose more frame time than you gain in clarity — lean on the sharpen/CAS pipeline instead.
- **Bicubic and Lanczos filters are demanding** (Lanczos most of all). Use them when you have GPU headroom; drop back to bilinear if you're frame-bound.
- **Optical flow, temporal blend, and frame pacing are experimental** and can cause artefacts or hitches. One at a time.
- **Costs stack.** High supersampling + Lanczos + CAS + flow together will tax even a strong GPU. If you drop frames, lower supersampling first, then filter quality, then the experimental motion features.

### 🟣 Pimax users — fixing flicker

If you get flicker / ATW drag on Pimax, try in order:

1. **Switch to the SteamVR OpenXR runtime** — often fixes Pimax flicker outright (non-Katanga content; see the Katanga note above).
2. **Lock to half framerate** — use Adaptive Half-Refresh (or the FPS limit) targeting a clean **90 Hz** or **120 Hz** half-rate for a stable cadence.
3. **Use the pose-prediction slider** — a few ms (≈8–10 ms is a good start for Pimax Crystal) reduces ATW flicker/drag.

---

## ⚙️ Build

Requires **Rust 1.74+** and the **Vulkan SDK** on Windows.

```sh
git clone https://github.com/<your-org>/osiris-vr-viewer
cd osiris-vr-viewer
build.bat clean      # or: cargo build --release
```

`build.bat` builds in release and renames the binaries to `osiris-vr-viewer.exe` and `osiris-gui.exe` (Cargo package names can't have spaces, so the rename happens after build). Output lands in `target/release/`.

---

## 🎮 Usage

1. Set your **OpenXR runtime** active (native or SteamVR — see [Runtimes](#-runtimes-openxr-vs-steamvr)).
2. Launch **`osiris-vr-viewer.exe`** (it appears in the tray) , the **`osiris-gui.exe`** control panel opens automatically too appears in task bar
3. Pick a stereo mode and screen shape, put on your headset — Osiris auto-detects the source and renders. Start your geo-11 / addon game and it hot-swaps to the full-res image.

**Tray menu:** Recenter, Screenshot, Toggle Head-Lock, Cycle Stereo Mode, Cycle Screen Shape, Quit.

**Mouse emulation:** enable it, start with **Both**, switch to **Relative** if the game over-rotates or **Absolute** if it ignores Relative, then tune sensitivity/speed.

**6DoF UDP (e.g. RE Requiem):** point the game's OpenTrack listener at `127.0.0.1:4242`, enable the UDP stream in the GUI, and flip any axis that points the wrong way.

**TrackIR games (DCS, Elite, ETS2, Assetto, Everspace 2…):** do the one-time **[TrackIR setup](#-trackir-setup-one-time)** (build the bridge `NPClient64.dll` and place it for your game), then enable **TrackIR Game** in the 6DoF MODS section and turn on TrackIR / head-tracking in the game's own options. Turning the toggle on redirects the head pose away from the UDP stream. Flip any axis that points the wrong way with the same per-axis controls.

---

## 📋 Tested

- ✅ Runtimes: SteamVR / OpenXR, Oculus / Quest Link (wired), Varjo, Pimax
- ✅ Mouse emulation: Skyrim VR (mod), Forza Horizon, Witcher 3 (HW cursor off — Absolute)
- ✅ 6DoF UDP: RE Requiem mod, REFramework
- ✅ Sources: geo-11 / Katanga (full-SBS), SuperDepth3D (checkerboard & VRExport SBS), Geo3D

---

## 🩺 Troubleshooting

Most issues come down to the **OpenXR runtime**, the **3D source**, or a game's **window mode**. Start here.

- **Reporting a bug?** Toggle **Debug** (in the GUI or the in-VR panel) to write `osiris-diagnostics.log` next to the exes and include it with `osiris.log`.
- **For Katanga full-res, use your headset's native OpenXR runtime** — the SteamVR runtime performs poorly with Katanga specifically (see [Runtimes](#-runtimes-openxr-vs-steamvr)).

### 🚫 The viewer won't start / "form factor … not available" / "device not available"

- Your **headset is off or asleep**, or **no OpenXR runtime is active**. Power on the headset, start your runtime (native or SteamVR), set it as the active OpenXR runtime, then relaunch.
- On a **fresh Windows install**, install the **[Microsoft Visual C++ Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe)**.

### 📡 Wireless / Virtual Desktop shows nothing

- Osiris needs a **wired** headset and a shared-texture path that wireless streaming (e.g. Virtual Desktop) doesn't expose. Use a wired PCVR connection or USB Link cable with your headset's native or SteamVR OpenXR runtime (see [Requirements](#-requirements)).

### ⬛ Black screen / no image in VR

- Confirm a **source is detected**: start your geo-11 / addon game, or enable desktop capture for ReShade sources. The viewer auto-detects in priority order (Katanga → Desktop).
- **Match the stereo mode to the source** (Full-SBS for geo-11, Checkerboard for SuperDepth3D DLP, etc.) — the wrong mode can look black, doubled, or flat.

### 👀 Image looks flat, doubled, or the eyes are swapped

- Pick the **correct stereo mode** for your source, and use the **Swap eyes** hotkey if the depth looks inverted.

### 🖥️ Katanga ImGui panel won't appear or won't respond

- Make sure it's **enabled** (Katanga ImGui section in the GUI) and bound to a **hotkey**, then toggle it in-game. The panel renders as its own VR layer, so it works in **any** window mode — only the optional "Show GUI with overlay" focus hand-off needs borderless.
- The panel is **mouse-driven**: move your physical mouse to move the panel cursor. If clicks go to the game instead, toggle the panel off/on with the hotkey to re-grab the cursor.

### 🟣 Flicker or ATW drag (especially Pimax)

- See [Pimax users — fixing flicker](#-pimax-users--fixing-flicker): try the SteamVR OpenXR runtime, lock to a clean half-refresh (90/120 Hz), and add a few ms of pose prediction.

### 🖱️ Mouse emulation doesn't move the game, or over-rotates

- Start with **Both**, switch to **Relative** if the game over-rotates, or **Absolute** if it ignores Relative (turn off Hardware Cursor in games like Witcher 3). For anti-cheat or stubborn games, use the **Interception** driver.
- **If a game ignores the cursor entirely** (especially elevated or fullscreen titles), **run both `.exe` files as Administrator** — Windows blocks unelevated input injection into elevated games (see [Requirements](#-requirements)).

### 🎮 Joystick emulation does nothing

- Install the free **[ViGEmBus driver](https://github.com/nefarius/ViGEmBus/releases)** (one-time install).
- If it still does nothing in a specific game, **run both `.exe` files as Administrator** so the virtual pad can reach elevated / fullscreen titles (see [Requirements](#-requirements)).

### ⌨️ Hotkeys do nothing in a game

- **Run both `.exe` files as Administrator.** Global hotkeys (and mouse / gamepad emulation) can't reach a game that runs at a higher privilege level than Osiris unless Osiris is elevated too. Right-click each exe → Properties → Compatibility → "Run this program as an administrator" (see [Requirements](#-requirements)).

### 🐌 Dropped frames / stutter

- Lower **supersampling** first (≤ **1.20**), then drop **Lanczos → Bicubic → Bilinear**, then disable the **experimental** motion features (optical flow, temporal blend, frame pacing) one at a time. See [Performance & Tuning](#-performance--tuning).

### 💾 Presets aren't applying

- Keep a **`presets/`** folder next to the exes. The running viewer hot-reloads `presets/default.json` the instant the GUI saves it.

---

## 🙏 Credits

- **[VRScreenCap](https://github.com/artumino/VRScreenCap)** by [@artumino](https://github.com/artumino) — the original architecture this fork is built on
- **[Katanga](https://github.com/bo3b/katanga)** by [@bo3b](https://github.com/bo3b) — the shared-texture VR streaming layer Osiris reads from
- **[geo-11 / HelixMod](https://helixmod.blogspot.com/2013/10/game-list-automatically-updated.html)** — stereoscopic 3D mod platform and per-game fixes
- **[Geo3D-Installer](https://github.com/Flugan/Geo3D-Installer)** by [@Flugan](https://github.com/Flugan)
- **[SuperDepth3D / Depth3D](https://github.com/BlueSkyDefender/Depth3D)** by [@BlueSkyDefender](https://github.com/BlueSkyDefender)
- **[wiz3D](https://github.com/effcol/wiz3D)** by [@effcol](https://github.com/effcol)
- **[OpenTrack](https://github.com/opentrack/opentrack)** — wire-format reference for the 6DoF UDP output and the FreeTrack / TrackIR shared-memory interface used by the TrackIR Game output

---

<img width="3420" height="4048" alt="osiris_screenshot_20260608_225040" src="https://github.com/user-attachments/assets/0f3375a7-cc8b-4822-a780-ebfeaf618b65" />
<img width="3420" height="4048" alt="osiris_screenshot_20260608_224408" src="https://github.com/user-attachments/assets/db22d488-5a47-4434-a025-2113cf873762" />
<img width="3420" height="4048" alt="osiris_screenshot_20260608_223635" src="https://github.com/user-attachments/assets/d8eb207c-bea3-45a2-bf47-4598e02d88f8" />
<img width="3420" height="4048" alt="osiris_screenshot_20260608_223440" src="https://github.com/user-attachments/assets/615176f6-1ddf-4a3c-8482-bb07168f618d" />
<img width="3271" height="3872" alt="osiris_screenshot_20260531_145827" src="https://github.com/user-attachments/assets/d919d41b-93e4-4b6a-97dc-8362976b331c" />



<div align="center">

🦀 Made with Rust • 🥽 Powered by OpenXR • ⚡ Rendered with wgpu

</div>
