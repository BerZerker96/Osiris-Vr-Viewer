//! Shared types used by both the Osiris VR Viewer and its GUI.
//!
//! The binaries communicate through two channels:
//!
//!   1. **JSON file** (`presets/default.json` next to the exe) — persistent
//!      source of truth. The GUI writes it on save / autosave; the viewer
//!      hot-reloads via its file watcher.
//!
//!   2. **Shared memory** (this crate's `LiveParams`) — realtime overrides
//!      while the GUI is open. The viewer polls once per frame so slider
//!      drags feel instantaneous instead of waiting for the file watcher.
//!
//! When the GUI closes, the shared-memory `enabled` flag goes to 0 and the
//! viewer falls back to whatever the on-disk preset says. That keeps the
//! viewer fully usable as a standalone tool with just the tray menu.

use serde::{Deserialize, Serialize};

/// Name of the shared-memory mapping. Changes here MUST be paired across
/// both binaries — that's exactly why this constant lives in the shared
/// crate. The `Local\` prefix makes the mapping per-user-session on
/// Windows; both processes have to be running as the same user.
pub const SHM_NAME: &str = "Local\\OsirisVRViewerLiveParams";

/// Bytes reserved in the mapping. Must be at least
/// `size_of::<LiveParamsMapping>()`. We round up generously so future
/// fields don't break wire compat with installed GUIs.
pub const SHM_SIZE: usize = 1024;

/// Magic value used to detect a stale or unwritten mapping. The viewer
/// only trusts the mapping if `magic == LIVE_MAGIC && version == LIVE_VERSION`.
pub const LIVE_MAGIC: u32 = 0x4F53_5256; // 'OSRV' in ASCII
/// Bumped to 13 in 0.6.0-dev: removed MeshExtension stretch_mode (default
/// is now Sphere); added extend-based edge stretch (`edge_stretch_extend`,
/// `edge_expand_extend`), texture sharpen (`texture_sharpen`), bilinear /
/// trilinear filter sliders (`filter_bilinear`, `filter_trilinear`), and
/// simulated 6DoF parallax fields (`sim_6dof_enabled`, `sim_6dof_amount`,
/// `sim_6dof_smoothness`). Old writers/readers will reject the mapping
/// cleanly via the version check rather than reading garbage values.
pub const LIVE_VERSION: u32 = 95;

/// Realtime parameters the GUI streams to the viewer. POD so it can be
/// memcpy'd into a memory mapping with `bytemuck::bytes_of_mut`.
///
/// Field order must remain stable across versions; bump `LIVE_VERSION`
/// whenever it changes. Changes in semantics (without changing layout)
/// should also bump the version.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LiveParams {
    /// Increments on every write. The viewer only re-applies if it
    /// changes, avoiding redundant uniform uploads when the GUI is idle.
    ///
    /// Stored as two u32 halves (lo, hi) instead of a single u64 to keep
    /// the struct's alignment at 4 bytes. With u64 first, the struct
    /// alignment becomes 8 bytes, which forces trailing padding when
    /// the field-data size isn't a multiple of 8 — and bytemuck::Pod
    /// rejects implicit padding. Splitting seq into two u32s makes the
    /// whole struct 4-byte aligned and pad-free.
    pub seq_lo: u32,
    pub seq_hi: u32,

    /// 1 when the GUI is actively driving values, 0 when not. The viewer
    /// ignores everything else if this is 0.
    pub enabled: u32,
    /// Stereo mode index — see `StereoModeIndex` below.
    pub stereo_mode: u32,
    /// XR backend index — see `XrBackendIndex` below. Currently informational
    /// only (OpenVR backend lands in Phase 5).
    pub xr_backend: u32,
    /// 1 if the viewer should treat any of the boolean toggles below as
    /// authoritative; 0 to keep using whatever the file preset says.
    pub override_toggles: u32,

    // --- Geometry sliders ---
    pub distance: f32,
    pub scale: f32,
    pub x_curvature: f32,
    pub y_curvature: f32,
    pub offset_x: f32, // reserved; positions screen relative to anchor
    pub offset_y: f32,
    pub offset_z: f32,
    pub edge_stretch: f32,
    /// Softness of the inner-edge transition between central image and
    /// peripheral rays. 0.0 = hard cutover, 1.0 = wide smooth fade.
    pub edge_stretch_softness: f32,

    // --- Sphere-mode sliders (only used when `stretch_mode == Sphere`) ---
    /// Angular half-width of the source-image cap painted onto the
    /// sphere's front, in radians. Larger = source image covers more
    /// horizontal sphere area.
    pub sphere_x_size: f32,
    /// Angular half-height of the source-image cap.
    pub sphere_y_size: f32,
    /// 0..1 — horizontal sphere curvature. 0 = sphere flattens
    /// horizontally (image looks flat across width), 1 = full sphere
    /// curve.
    pub sphere_x_curve: f32,
    /// 0..1 — vertical sphere curvature. Same idea but for the Y axis.
    pub sphere_y_curve: f32,

    // --- Box-mode sliders (only used when `stretch_mode == Box`) ---
    /// Multiplicative scale on the box's X extent on top of the
    /// auto-aspect baseline. 1.0 = use the source's natural aspect for
    /// the front face. Larger values = wider box.
    pub box_x_size: f32,
    /// Multiplicative scale on the box's Y extent.
    pub box_y_size: f32,
    /// Multiplicative scale on the box's Z (depth) extent.
    pub box_z_depth: f32,
    /// 0..1 — chamfer/rounding amount of the seams between the cube's
    /// faces. 0 = sharp cube, 1 = unit sphere. Intermediate values give
    /// a chamfered cube where the centres of the faces stay flat and
    /// the edges curve smoothly into the next face.
    pub box_corner_radius: f32,

    // --- Texture detail sharpen (high-frequency unsharp mask) ---
    /// Fine-detail texture sharpening (0..10). Separate from the broad
    /// `sharpness` slider — uses a tighter kernel radius to target
    /// small texture detail rather than broad edge contrast.
    pub texture_sharpen: f32,

    // --- Filter mode: 0 = bilinear (default), 1 = trilinear (mipmap blend) ---
    /// 0 = bilinear (no mip blend), 1 = trilinear (smooth mip blend),
    /// 2 = nearest-neighbour (raw pixels). Implemented as shader-side
    /// multi-sample blending on top of the base GPU bilinear sampler.
    pub filter_mode: u32,

    // --- Extend-based edge stretch (new mode, complementary to mirror) ---
    /// Extend-based edge stretch amount (0..30). Instead of mirroring
    /// the edge pixel outward, this mode progressively samples further
    /// INTO the source image — each step outside the screen boundary
    /// samples one step further inward — so the content appears to
    /// "flow" outward rather than smearing a 1D edge ray. Same visual
    /// idea as the right panel of the reference image.
    pub extend_stretch: f32,
    /// Softness of the extend-based stretch transition (0..1).
    pub extend_softness: f32,

    // --- Simulated 6DoF ---
    /// Enable simulated 6DoF parallax (DeoVR-style). When enabled,
    /// the screen translates opposite to the HMD's translational
    /// movement, scaled by `sim6dof_intensity`, creating a convincing
    /// depth parallax illusion without actual depth data.
    pub sim6dof_enabled: u32,
    /// Scale of the simulated parallax motion. 1.0 = 1:1 inverse
    /// translation of the screen. Range 0..10.
    pub sim6dof_intensity: f32,
    /// Smoothness / lag of the parallax response. 0 = instant (no
    /// smoothing), 1 = heavily smoothed. Range 0..1.
    pub sim6dof_smoothness: f32,

    // --- Image sliders ---
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub sharpness: f32,

    // --- Toggles (only honoured if override_toggles == 1) ---
    pub swap_eyes: u32,
    pub flip_x: u32,
    pub flip_y: u32,
    pub head_lock: u32,
    pub ambient: u32,

    /// Screen-shape choice. See `StretchModeIndex`.
    /// 0 = Sphere (default since 0.6.0; inside-out sphere wraps the
    /// user), 2 = Box (inside-out cube). Value 1 is reserved for
    /// the deprecated MeshExtension mode and is silently mapped to
    /// Sphere on read.
    pub stretch_mode: u32,

    /// If set to 1 by the GUI (e.g. via its "Quit" button), the viewer
    /// will treat it as a manual Quit command on the next frame and
    /// shut down. Reset by the viewer to 0 on shutdown so a stale
    /// flag in shared memory doesn't cause an immediate exit on the
    /// next launch.
    pub quit_request: u32,

    /// Supersampling factor for the OpenXR swapchain. 1.0 = native HMD
    /// resolution. The viewer recreates its swapchain when this
    /// changes; while the viewer is running, changes mid-session may
    /// not take effect until the next session restart.
    pub supersampling: f32,

    /// Set to 1 by the GUI to request a screenshot. The viewer captures
    /// the next rendered frame, writes it to disk, and resets this flag
    /// to 0 so successive sets fire as fresh requests.
    pub screenshot_request: u32,

    /// Set to 1 by the GUI to request the OpenXR session restart
    /// (swapchain teardown + recreation). The viewer notices and
    /// breaks its render loop with a flag that triggers a full
    /// re-init. Resets to 0 after action.
    pub restart_session_request: u32,

    /// Set to 1 by the GUI to request a recenter (re-anchor the
    /// screen to the current head pose, similar to the tray
    /// "Recenter" command). Resets to 0 after action.
    pub recenter_request: u32,

    /// Gradual edge stretch (0..1). When > 0, the peripheral region
    /// outside the source plays back the source progressively from
    /// edge inward — each step outside samples further into the
    /// source — so content continues outward rather than collapsing
    /// to a 1D ray. Doubles as the trailing-padding slot.
    pub edge_expand: f32,

    // ------------------------------------------------------------------
    // 0.6.0-dev additions (LIVE_VERSION 14). Append-only; do not reorder.
    // ------------------------------------------------------------------
    /// Extend-based edge stretch (companion to `edge_stretch`).
    /// Where the existing `edge_stretch` mirrors the boundary pixel
    /// outward (vertical streaks), this samples the source
    /// progressively starting from inside the image so peripheral
    /// content visually continues outward — clouds keep developing
    /// shape, edges blend rather than smear. Range 0..30 (same as
    /// `edge_stretch` so the two can be dialled in together).
    pub edge_stretch_extend: f32,

    /// Companion to `edge_stretch_extend` — gradual variant that
    /// works in INWARD walking units (analogous to `edge_expand`),
    /// for finer control of how much the source is consumed by the
    /// extending periphery.
    pub edge_expand_extend: f32,

    /// Bilinear filter strength 0..1. 0 = identity (sampler default),
    /// 1 = full bilinear (current behaviour). Lets the user dial
    /// down filtering for a sharper but blockier look.
    pub filter_bilinear: f32,

    /// Trilinear-style filter strength 0..1. Since the source texture
    /// has no mipmaps, this is implemented as a multi-tap weighted
    /// blend in the shader (gives the smoothness benefit of tri
    /// without the LOD selection). 0 = off, 1 = full effect.
    pub filter_trilinear: f32,

    /// Simulated 6DoF parallax — when enabled, the screen translates
    /// in the OPPOSITE direction to head movement so the user
    /// perceives a parallax effect, similar to DeoVR's basic 6DoF
    /// fallback for non-volumetric content.
    pub sim_6dof_enabled: u32,
    /// Amount of fake parallax. 1.0 = head moves 1cm right, screen
    /// moves 1cm left. Range 0..20 lets the user crank it to taste.
    pub sim_6dof_amount: f32,
    /// Exponential smoothing factor 0..1 for the parallax response.
    /// 0 = follow head exactly each frame (jittery), 1 = no movement
    /// (fully damped). Useful range ~0.7..0.95 for a natural feel.
    pub sim_6dof_smoothness: f32,
    /// Z-axis (zoom) intensity multiplier — independent slider so
    /// users can dial up zoom-in/out from leaning forward/back
    /// without having to also crank X/Y parallax. Multiplies on top
    /// of `sim_6dof_amount`. Range 0..20. Default 1.0 keeps existing
    /// behaviour.
    pub sim_6dof_zoom_amount: f32,

    /// IPD perspective multiplier (0..2). Scales per-eye horizontal
    /// stereo offset to make stereoscopic content feel closer/larger
    /// (>1) or farther/smaller (<1). Lives at the top of the GUI's
    /// Image section.
    pub ipd_perspective: f32,

    /// Katanga VR Performance toggle. 1 = enabled (apply fast path
    /// in shader for Katanga sources only). 0 = off (default).
    /// Plumbed from GUI → SHM → AppConfig.katanga_perf_mode →
    /// uniform.
    pub katanga_perf_mode: u32,

    /// Expansion-stretch outer reach (0..1). New mesh-stretch mode.
    pub expansion_outer: f32,
    /// Expansion-stretch seamlessness (0..1). Controls how much of
    /// the source content the stretch consumes.
    pub expansion_seamless: f32,
    /// Roll offset in radians. Rotates screen content around the
    /// forward (Z) axis to correct head tilt. Works with all screen
    /// modes and with head-lock enabled.
    pub offset_roll: f32,

    // ------------------------------------------------------------------
    // 0.6.0 additions (LIVE_VERSION 21): head-tracking output features.
    // ------------------------------------------------------------------
    /// Mouse Emulation toggle. 1 = enabled. When on, the viewer reads
    /// head pose every frame and emits relative mouse motion via
    /// Win32 `SendInput`, scaled by `mouse_emu_sensitivity` and
    /// `mouse_emu_speed`. Lets games that only support mouse-look
    /// (e.g. Forza) be controlled by head movement.
    pub mouse_emu_enabled: u32,
    /// How responsive the mouse cursor is to head ROTATION. 0..2
    /// range in the GUI. 1.0 = neutral. Higher = bigger cursor moves
    /// per degree of head turn.
    pub mouse_emu_sensitivity: f32,
    /// Overall mouse-motion speed multiplier on top of sensitivity.
    /// Tunes the absolute pixel rate. 0..2 in the GUI. 1.0 = neutral.
    pub mouse_emu_speed: f32,
    /// Mouse emulation compatibility mode.
    /// 0 = Relative SendInput only (raw-input games, modern FPS).
    /// 1 = Absolute SetCursorPos only (cursor-polling games).
    /// 2 = Both (default; max compatibility, covers Witcher 3 etc.).
    pub mouse_emu_compat: u32,

    /// 6DoF UDP toggle. 1 = enabled. When on, the viewer sends head
    /// pose at full VR refresh rate to a UDP target as 48-byte
    /// OpenTrack-format packets (6 little-endian f64: x, y, z (cm),
    /// yaw, pitch, roll (deg)). Used by community headtracking
    /// integration scripts (e.g. RE Requiem mod, OpenTrack-aware
    /// games).
    pub udp_6dof_enabled: u32,
    /// Destination UDP port. Stored as u32 for alignment; valid range
    /// 1..65535. Default 4242 matches OpenTrack convention.
    pub udp_6dof_port: u32,
    /// Destination IP, stored as a 16-byte null-terminated ASCII
    /// string (max "255.255.255.255" = 15 chars + null). Fixed-size
    /// because the SHM mapping is a POD struct — variable-length
    /// data isn't allowed.
    pub udp_6dof_ip: [u8; 16],

    // -- 6DoF UDP per-axis flip flags (1 = invert that axis) --
    pub udp_flip_x: u32,
    pub udp_flip_y: u32,
    pub udp_flip_z: u32,
    pub udp_flip_yaw: u32,
    pub udp_flip_pitch: u32,
    pub udp_flip_roll: u32,
    // -- 6DoF UDP rotational gains (multipliers; 1.0 = neutral) --
    pub udp_gain_yaw: f32,
    pub udp_gain_pitch: f32,
    pub udp_gain_roll: f32,
    // -- 6DoF UDP position gains (multipliers; 1.0 = neutral) --
    pub udp_gain_x: f32,
    pub udp_gain_y: f32,
    pub udp_gain_z: f32,

    /// Mesh forward-extrusion strength (0..3). When > 0 the
    /// peripheral vertices of the ray sphere are pulled along the
    /// world-space forward axis by an amount proportional to this
    /// value times `extrusion_direction`. 0 = no mesh deformation
    /// (planar wrap-around sphere); higher = more curl. Independent
    /// of `expansion_outer` so users can tune mesh curve and UV
    /// reach separately. Closer to VHT-style fishbowl curve when
    /// pushed up.
    pub extrusion_strength: f32,
    /// Mesh-extrusion direction (-1..+1). +1 = pull toward viewer
    /// (default, fishbowl curve). -1 = push away from viewer
    /// (anti-fishbowl, periphery recedes into distance). 0 = no
    /// effect regardless of strength.
    pub extrusion_direction: f32,

    // -- Concave back-wall sliders. Apply to Sphere / Fisheye / Box.
    /// Concave strength (0..2). 0 = flat screen.
    pub concave_strength: f32,
    /// Concave depth multiplier (0..2).
    pub concave_depth: f32,
    /// Concave shape (0..1). 0 = parabolic, 1 = spherical.
    pub concave_shape: f32,
    /// Mirror-stretch falloff curve (0..2).
    pub mirror_falloff: f32,
    /// Extend pull blend (0..1).
    pub extend_pull_blend: f32,
    /// Expansion smoothness (0..2).
    pub expansion_smoothness: f32,
    pub concave_z0: f32, pub concave_z1: f32, pub concave_z2: f32,
    pub concave_z3: f32, pub concave_z4: f32,
    pub concave_dz0: f32, pub concave_dz1: f32, pub concave_dz2: f32,
    pub concave_dz3: f32, pub concave_dz4: f32,
    pub filter_bicubic: f32,
    pub filter_lanczos: f32,
    pub headlock_jitter_deadzone: f32,
    pub headlock_jitter_smooth: f32,
    /// Stable Lock X/Y parallax — lateral/vertical head movement counter. 0 = off.
    pub stable_lock_parallax_xy: f32,
    /// Stable Lock Z parallax — forward/back head movement (zoom feel). 0 = off.
    pub stable_lock_parallax_z: f32,
    /// Stable Lock DIRECTIONAL parallax toggle (LIVE_VERSION 69): rotation-
    /// driven look-around parallax with its own strength, exclusive to the
    /// Stable Lock head-lock method and isolated from dir6dof_*/sim6dof_*.
    pub stable_lock_dir_enabled: u32,
    /// Directional strength 0–10 (×1.5 internal scale; 0.75 ≈ previous feel).
    pub stable_lock_dir_strength: f32,
    /// 0=One Euro, 1=EMA, 2=Deadzone
    pub headlock_jitter_method: u32,
    // ── DeJitter (LIVE_VERSION 69): vorpX-style soft-lock spring ──────────
    /// 1 = the head-locked screen PURSUES the head pose through a
    /// critically-damped rotational spring instead of being rigidly bolted to
    /// it. Applied AFTER whichever headlock jitter method (0/1/2) runs, so it
    /// composes with all of them. Tracker noise comes out as sub-pixel smooth
    /// drift, fast turns leave the screen lagging a bounded few degrees behind
    /// (pleasant inertia) and it settles with zero overshoot.
    pub headlock_dejitter: u32,
    /// Spring stiffness 0.05–1.0: how quickly the screen catches up.
    /// Low = floaty/cinematic, high = tight. ~0.4 ≈ vorpX default feel.
    pub headlock_dejitter_stiffness: f32,
    /// Maximum rotational lag in DEGREES (1–30). Fast head turns can never
    /// leave the screen further behind than this; it is pulled along.
    pub headlock_dejitter_max_lag: f32,
    /// Delayed lock: how many milliseconds the screen lags behind head rotation.
    /// 0 = off. Range 0..500ms. Method=1 activates this.
    pub headlock_delay_ms: f32,
    /// Parallax Prediction: isolated post-stage on the headlock pose that
    /// velocity-predicts the orientation forward and continuously blends
    /// (ramps) toward it, so per-frame corrections glide instead of snapping
    /// — smooths jitter / double-motion when headlock + head-tracking run.
    pub parallax_prediction: u32,
    /// Strength 0..1: scales the look-ahead + smoothing time constant.
    pub parallax_prediction_amt: f32,
    /// PP #3 Adaptive smoothing toggle: heavy smoothing when the head is near
    /// still (kills shimmer), lighter as you turn faster (stays responsive);
    /// also enables the rotational deadband below. CPU-only; PP-gated.
    pub pp_adaptive: u32,
    /// PP #3 Deadband (degrees): sub-threshold per-frame corrections are held
    /// (absorbs tracker micro-jitter). Only active when pp_adaptive != 0.
    pub pp_deadband_deg: f32,
    /// PP #4 Acceleration prediction 0..1: adds a 2nd-order term so the guess
    /// curves through the start/end of turns (less swim on fast stops).
    pub pp_accel: f32,
    pub sim6dof_zoom_only: u32,
    pub vr_hotkeys_enabled: u32,

    // ─── VR Data to UDP (Stage 3) ──────────────────────────────
    // Sends OpenTrack 48-byte packets of head pose + optionally
    // L/R controller poses to an external app for routing.
    /// 1 = section enabled (master on/off).
    pub vr_udp_enabled: u32,
    /// 0 = head only, 1 = controllers only, 2 = both.
    pub vr_udp_mode: u32,
    /// Destination UDP port (1..65535).
    pub vr_udp_port: u32,
    /// Destination IP, 16-byte null-terminated ASCII.
    pub vr_udp_ip: [u8; 16],
    /// Per-axis flip flags (1 = invert).
    pub vr_udp_flip_x: u32,
    pub vr_udp_flip_y: u32,
    pub vr_udp_flip_z: u32,
    pub vr_udp_flip_yaw: u32,
    pub vr_udp_flip_pitch: u32,
    pub vr_udp_flip_roll: u32,
    /// Rotational gains (1.0 = neutral).
    pub vr_udp_gain_yaw: f32,
    pub vr_udp_gain_pitch: f32,
    pub vr_udp_gain_roll: f32,
    /// Position gains (1.0 = neutral).
    pub vr_udp_gain_x: f32,
    pub vr_udp_gain_y: f32,
    pub vr_udp_gain_z: f32,
    /// 1 = send left controller pose.
    pub vr_udp_left_enabled: u32,
    /// 1 = send right controller pose.
    pub vr_udp_right_enabled: u32,
    /// 1 = enable per-frame diagnostics logging to osiris-diagnostics.log.
    /// When 0 the log file is not written and there is no performance cost.
    pub diag_mode: u32,

    // ── v39 additions ──────────────────────────────────────────────────────
    /// Pose prediction offset in milliseconds added on top of the runtime's
    /// own prediction. Positive values look further into the future, reducing
    /// what ATW has to correct between render and display.
    /// 0.0 = runtime prediction only (default).
    /// Recommended starting point for Pimax Crystal: 8.0–10.0 ms.
    pub pose_predict_ms: f32,

    // ── v40 additions ──────────────────────────────────────────────────────
    /// Velocity smoothing factor (0.0–1.0, EMA alpha on angular velocity).
    /// 0.0 = no smoothing (raw single-frame velocity, can be jittery).
    /// 0.5 = moderate smoothing — good balance for most headsets.
    /// 0.9 = heavy smoothing — very stable but slower to react to fast motion.
    /// Only active when pose_predict_ms > 0.
    pub pose_smooth_alpha: f32,

    // ── Frame pacing ───────────────────────────────────────────────────────
    /// Frame pacing enabled. When true, the viewer sleeps after GPU submit
    /// to target a consistent submission time within the frame period.
    /// This reduces compositor queue variance and ATW correction frequency.
    pub vsync_mode: u32,
    pub fps_limit: f32,
    pub frame_pacing_enabled: u32,
    /// Target submission point as a fraction of the frame period (0.0..0.95).
    /// 0.0 = submit immediately (old behaviour, maximum pose freshness risk).
    /// 0.45 = submit ~5ms before display at 90Hz (recommended default).
    /// 0.85 = very late submission (freshest pose, needs fast GPU).
    pub frame_pacing_target: f32,
    /// Temporal blend: 0=off, 1=on
    pub temporal_blend_enabled: u32,
    pub temporal_blend_alpha: f32,
    pub flow_enabled: u32,
    pub flow_strength: f32,

    // ── v42 additions ──────────────────────────────────────────────────────
    /// Quality Enhancement level (0.0 = off, 1.0 = maximum).
    /// Drives a 3-pass image enhancement pipeline on the final rendered output:
    ///   Pass 1: Lanczos edge enhancement (amplifies real edges, suppresses flat)
    ///   Pass 2: Local contrast boost (enhances micro-contrast, VR sharpness cue)
    ///   Pass 3: RCAS adaptive sharpening (noise-adaptive edge sharpening)
    /// Higher = strictly better at every level. 0.0 = zero overhead (skip all passes).
    /// Live-adjustable, no restart needed.
    pub enhancement_quality: f32,

    /// Independent RCAS sharpening strength added on top of enhancement_quality.
    /// 0.0 = only quality-driven RCAS. 2.0 = maximum additional sharpening.
    /// Live-adjustable.
    pub rcas_sharpness: f32,

    // ── v43 additions ──────────────────────────────────────────────────────
    /// Repeated method: how much the edge strips are stretched outward (0..30).
    pub repeat_stretch: f32,
    /// Repeated method: seam blend width (0=hard, 1=wide smooth crossfade).
    pub repeat_blend: f32,
    /// Repeated method: Z-depth push toward viewer (0=flat, 1=full forward push). 0..1.
    pub repeat_depth: f32,
    pub repeat_size: f32,

    // ── Auto Adjust ──────────────────────────────────────────────────────────
    /// When 1, apply auto_z_offset to offset_z while headlock is active.
    pub auto_z_enabled:      u32,
    /// Z offset value to add when headlock is active.
    pub auto_z_value:        f32,
    /// When 1, apply auto_roll_value to offset_roll while headlock is active.
    pub auto_roll_enabled:   u32,
    /// Roll offset value to add when headlock is active.
    pub auto_roll_value:     f32,
    /// When 1, apply auto_x_value to offset_x while headlock is active.
    pub auto_x_enabled:      u32,
    /// X offset value to add when headlock is active.
    pub auto_x_value:        f32,
    /// When 1, apply auto_y_value to offset_y while headlock is active.
    pub auto_y_enabled:      u32,
    /// Y offset value to add when headlock is active.
    pub auto_y_value:        f32,
    /// When 1, set sphere_y_size automatically based on detected aspect ratio.
    pub auto_height_enabled: u32,
    /// Sphere height preset for 16:9 sources.
    pub sphere_y_169:        f32,
    /// Sphere height preset for 4:3 sources.
    pub sphere_y_43:         f32,
    /// Sphere height preset for 21:9 sources.
    pub sphere_y_219:        f32,

    // ── Joystick Emulation ────────────────────────────────────────────────
    /// 1 = joystick emulation active (requires ViGEmBus driver).
    pub joy_emu_enabled:     u32,
    /// 0 = Relative-Delta, 1 = Joy-Look Continuous.
    pub joy_emu_mode:        u32,
    /// Sensitivity (0.1..5.0).
    pub joy_emu_sensitivity: f32,
    /// Deadzone fraction (0.0..0.5).
    pub joy_emu_deadzone:    f32,
    /// Max angle degrees for Joy-Look mode (10..180).
    pub joy_emu_max_angle:   f32,
    /// 1 = invert X axis output.
    pub joy_emu_invert_x:    u32,
    /// 1 = invert Y axis output.
    pub joy_emu_invert_y:    u32,
    /// Smoothness EMA alpha (0.0 = off, 0.95 = heavy).
    pub joy_emu_smoothness:  f32,
    /// Per-axis speed multiplier X (-10..10). Negative = reversed.
    pub joy_emu_speed_x:     f32,
    /// Per-axis speed multiplier Y (-10..10). Negative = reversed.
    pub joy_emu_speed_y:     f32,
    /// 1 = redirect 6DoF head pose to a TrackIR/FreeTrack game via the
    /// FreeTrack shared-memory interface ("FT_SharedMem") INSTEAD of the
    /// UDP ip:port. When 0, the 6DoF UDP path is unchanged. Repurposes a
    /// former padding slot, so the wire layout/size is identical to older
    /// builds (old configs read 0 here = disabled). (LIVE_VERSION 83.)
    pub trackir_enabled:     u32,
    // -- TrackIR-ONLY per-axis flip flags (1 = invert). Separate from the
    // -- 6DoF-mod (udp_flip_*) flags so TrackIR sign tweaks never disturb a
    // -- UDP mod the user already tuned, and vice-versa.
    pub trackir_flip_x:      u32,
    pub trackir_flip_y:      u32,
    pub trackir_flip_z:      u32,
    pub trackir_flip_yaw:    u32,
    pub trackir_flip_pitch:  u32,
    pub trackir_flip_roll:   u32,
    /// TrackIR-only Z (lean in/out) gain. Independent of udp_gain_z so the
    /// small physical lean can be amplified for TrackIR without touching the
    /// 6DoF-mod tuning. 1.0 = neutral.
    pub trackir_gain_z:      f32,
    /// Depth-Layers motion reactivity (Lighter dynamic feature). 0 = off.
    /// >0 = the layer warp intensifies while the head moves (translation +
    /// yaw/pitch/roll velocity) and settles at rest. Repurposed from _pad_joy1;
    /// the slot's 0.0 default = off, so the wire stays version-compatible.
    pub dlayers_reactive:    f32,

    // ── Katanga Desktop Overlay ───────────────────────────────────────────
    /// 1 = show desktop mirror overlay in VR.
    pub overlay_enabled:     u32,
    /// Width of the overlay quad in metres (0.5..5.0).
    pub overlay_size:        f32,
    /// Distance in front of the user in metres (0.5..5.0).
    pub overlay_distance:    f32,
    /// Depth-Layers band MODE: 0 = concentric rings (default), 1 = 10 horizontal
    /// bands (by vertical position), 2 = 10 vertical columns (by horizontal
    /// position). All modes share the same delay cascade + ground prior.
    /// Repurposed from _pad_ov0 so the wire size is unchanged.
    pub dlayers_mode:        u32,
    /// 1 = HUD mode: overlay follows head (VIEW space), 0 = fixed in room (LOCAL space).
    pub overlay_hud_mode:    u32,
    /// In-VR panel ASPECT (was overlay_resolution). 0=16:9 (default), 1=4:3, 2=1:1.
    /// Drives the overlay texture dims AND the quad's world aspect together.
    pub overlay_aspect:      u32,
    /// In-VR panel opacity 0.0 (fully see-through) .. 1.0 (normal). Repurposed
    /// from _pad_ov2 so the wire layout is unchanged. Scales imgui style alpha.
    pub overlay_transparency: f32,
    /// Depth-Layers CONVEX dome strength (0..1). Bulges the centre (the area the
    /// dolly-zoom magnifies) like a lens; intensifies as you lean in to zoom.
    /// Repurposed from _pad_ov3 (a former overlay pad) so the wire size is
    /// unchanged; logically it belongs with the Depth-Layers controls.
    pub dlayers_convex:      f32,

    // ── Katanga Filters (LIVE_VERSION 56) ─────────────────────────────────
    // A second, STRONGER set of image adjustments. When enabled they are
    // combined on top of the normal image sliders and apply to whatever is on
    // screen (desktop or Katanga), exactly like the normal filters. Named
    // "Katanga Filters" for the UI; the sharpness fields go up to 10 (vs the
    // global 0..2) for aggressive sharpening.
    /// 1 = apply the Katanga Filters below (combined on top of base filters).
    pub katanga_filters_enabled:  u32,
    /// Broad unsharp-mask strength added on top of base sharpness, 0..10.
    pub katanga_sharpness:        f32,
    /// Fine-detail (texture) unsharp strength added on top, 0..10.
    pub katanga_texture_sharpness: f32,
    /// Saturation multiplier, 0..2 (1 = unchanged).
    pub katanga_saturation:       f32,
    /// Contrast multiplier around mid-grey, 0..2 (1 = unchanged).
    pub katanga_contrast:         f32,
    /// Additive brightness, -2..2 (0 = unchanged).
    pub katanga_brightness:       f32,
    pub hybrid_stretch_reach:     f32,   // was _pad_kf0; 0..1 forward-bend reach
    pub sim6dof_spring:           f32,   // was _pad_kf1; 0/1 return-to-center spring

    // ── Manual Force-Desktop hotkey (LIVE_VERSION 57) ─────────────────────
    /// Set to 1 by the GUI when the user presses the "Force desktop" hotkey.
    /// The viewer drops to the desktop view immediately (this frame) and applies
    /// a SHORT skip-Katanga hold (~800ms) so it doesn't instantly re-grab, then
    /// resumes normal probing — so it returns to Katanga on the next game just
    /// like the auto game-exit fallback, only with a much shorter hold. Resets
    /// to 0 after the viewer acks.
    pub force_desktop_request:    u32,

    // ── Stereo separation & convergence (LIVE_VERSION 58) ─────────────────
    /// Stereo SEPARATION: multiplies the per-eye horizontal disparity, i.e. the
    /// interaxial-distance analog for pre-rendered SBS/TAB content. 1.0 = source
    /// as authored; >1 = stronger depth/pop (objects gain roundness and stand
    /// out more); <1 = flatter. Range 0..3. Independent of convergence.
    pub separation:               f32,
    /// Stereo CONVERGENCE: shifts the two eye images in opposite directions by a
    /// constant, moving the zero-parallax (screen) plane in depth. 0 = neutral;
    /// >0 pushes the scene back (more pops out toward you / negative parallax);
    /// <0 pulls it forward. Range -1..1 (scaled to a small UV shift internally).
    pub convergence:              f32,
    /// Dynamic-depth POP-OUT strength (0..4): drives CONVERGENCE from forward/
    /// back head motion. Leaning toward the scene pulls the zero-parallax plane
    /// closer so content pops out toward you — the move-toward-object cue. Only
    /// used when sim6dof_dynamic_depth != 0. (Was dynamic_depth_strength.)
    pub dyn_popout:               f32,
    /// Dynamic-depth DEPTH-SCALE strength (0..4): drives SEPARATION (overall
    /// stereo depth/roundness, like UEVR's Depth Scale) from forward/back head
    /// motion. Leaning in deepens the world's roundness; leaning out flattens it.
    /// Forward/back axis only (the comfortable axis). Only used when
    /// sim6dof_dynamic_depth != 0.
    pub dyn_depthscale:           f32,
    /// Toggle (0/1): when 1 AND simulated 6DoF is active, forward/back head
    /// movement drives convergence (pop-out) and separation (depth scale) so
    /// approaching the scene feels like real depth. Lateral movement is handled
    /// purely by the mesh parallax (no global-disparity coupling — that axis is
    /// a known motion-sickness source for flat stereo).
    pub sim6dof_dynamic_depth:    u32,

    // ── Contrast Adaptive Sharpening + Dehaze (LIVE_VERSION 59) ───────────
    /// Contrast Adaptive Sharpening (0..10). AMD-CAS-style adaptive sharpen:
    /// sharpens more in low-contrast areas and less near strong edges to avoid
    /// over-sharpening halos. 0 = off. Regular (non-Katanga) value.
    pub cas:                      f32,
    /// Dehaze / Clarity (0..10). Local-contrast enhancement: boosts mid-scale
    /// contrast to cut through flat/hazy images and add punch and depth. 0 = off.
    pub dehaze:                   f32,
    /// Katanga-specific CAS (0..10), added on top of `cas` when Katanga filters
    /// are enabled.
    pub katanga_cas:              f32,
    /// Katanga-specific Dehaze (0..10), added on top of `dehaze` when Katanga
    /// filters are enabled.
    pub katanga_dehaze:           f32,

    // ── Simulated-6DoF mode + Off-axis "window" tuning (VERSION 61) ──
    /// 6DoF translation MODE: 0 = Default (the screen follows the head — the
    /// original mesh-translation parallax). 1 = Off-axis "window" (the screen
    /// behaves like a fixed window into the scene; head movement changes the
    /// viewing perspective rather than sliding the panel). Other sim6dof fields
    /// (intensity, smoothness, dynamic depth) still apply in BOTH modes.
    pub sim6dof_mode:             u32,
    /// Off-axis: how far "behind" the frame the scene sits (0.2..4.0, neutral
    /// 1.0). Larger = scene set deeper into the room → more parallax per unit of
    /// head movement → stronger window feel. The main character control.
    pub offaxis_window_depth:     f32,
    /// Off-axis: parallax strength multiplier (0..4, neutral 1.0). Scales how
    /// much the view shifts for a given head movement, independent of depth.
    pub offaxis_parallax:         f32,
    /// Off-axis: edge falloff / frame influence (0..2, neutral 1.0). Biases the
    /// effect toward the screen edges (the "looking around the frame" quality).
    pub offaxis_edge_falloff:     f32,
    /// Off-axis: vertical response balance vs horizontal (0..2, neutral 1.0).
    /// <1 damps up/down bob relative to left/right lean; >1 emphasises it.
    pub offaxis_vertical_balance: f32,

    // ── Depth Layers: radial multi-zone head-coupled parallax (VERSION 62) ──
    /// Master toggle (0/1). When on, the screen's 10 concentric radial layers
    /// (centre → rim) parallax by different amounts as the head moves, with a
    /// per-layer follow-through delay, on top of WHICHEVER sim6dof mode is active
    /// (Default translate or Off-axis window). Hole-free: it warps the single
    /// screen surface, never reconstructs per-pixel depth.
    pub dlayers_enabled:          u32,
    /// Invert delay direction (0/1). 0 = the CENTRE/inner layer is the responsive
    /// anchor and the delay ripples OUTWARD (inner reads as closest). 1 = the
    /// RIM/outer layer leads and the delay ripples INWARD (outer reads as
    /// closest). Flips which end of the 10-layer cascade is "near".
    /// (Repurposed from the unused `dlayers_count`; wire layout unchanged.)
    pub dlayers_invert:           u32,
    /// Master parallax strength (0..3, neutral 1.0). Scales the whole effect.
    pub dlayers_strength:         f32,
    /// Per-zone separation (0..3, neutral 1.0). How much MORE the rim moves than
    /// the centre — the differential that makes it read as depth (0 = uniform).
    pub dlayers_separation:       f32,
    /// Follow-through delay (0..1). Per-layer lag across the 10 concentric layers:
    /// the leading end (set by `dlayers_invert`) is the responsive anchor (least
    /// lag) and the opposite end trails by this much, so motion ripples through
    /// the layers. 0 = all layers move together (no lag). Adds the organic
    /// "give"; too high = jelly/water feel.
    pub dlayers_delay:            f32,
    /// Radial profile curve (0.25..3, neutral 1.0). Shapes how the per-zone gain
    /// grows from centre to rim (low = gentle near centre then ramps; high =
    /// most of the motion concentrated at the rim).
    pub dlayers_curve:            f32,
    /// In/out zoom + deepen (0..2, neutral 1.0). Forward/back head lean drives a
    /// per-ring radial dolly-zoom (lean in magnifies, lean back shrinks) that
    /// ripples through the layers via the same Delay + Invert, and also deepens
    /// the lateral parallax. 0 = no in/out zoom (sway/bob only).
    pub dlayers_zoom:             f32,
    /// Depth reach / rim-fade start (0.05..1, neutral 0.5). Sets where the radial
    /// depth taper begins: lower keeps depth near the centre and eases it out
    /// early (seam-free); higher lets it reach further toward the rim. (VERSION 66)
    pub dlayers_edge:             f32,

    // ── Directional 6DoF: head yaw/pitch/roll → motion parallax (VERSION 63) ──
    /// Master toggle (0/1). When on, head ROTATION (turning, looking up/down,
    /// tilting) adds a small directional parallax shift to the screen ON TOP of
    /// the positional sim6dof, so the scene responds to which way you turn/tilt,
    /// not just how you translate. Implemented purely as an extra mesh offset
    /// (motion parallax — the comfortable cue for non-forward motion), so it is
    /// hole-free and does NOT touch binocular disparity. Default off.
    pub dir6dof_enabled:          u32,
    /// Yaw gain (0..5). Turning the head left/right nudges the scene laterally,
    /// as if peeking around. 0 = ignore yaw.
    pub dir6dof_yaw:              f32,
    /// Pitch gain (0..5). Looking up/down nudges the scene vertically. 0 = off.
    pub dir6dof_pitch:            f32,
    /// Roll gain (0..5). Tilting the head ear-to-shoulder shifts the scene
    /// laterally in the tilt direction. 0 = off.
    pub dir6dof_roll:             f32,
    // ── Hybrid Immersion (LIVE_VERSION 65) — even-ramp rim-stretch + rear-360 ──
    // New "Hybrid Immersion" edge-fill method (Sphere mode only this version).
    // Keeps the inner `hybrid_center` fraction of the screen 1:1, ramps the
    // outer band outward (even/linear by default), and fills the rear 360 by
    // mirroring. Composites ON TOP of the classic edge sliders (extra layer),
    // and is fully gated by `hybrid_enabled` so OFF = byte-identical render.
    /// Master toggle (0/1).
    pub hybrid_enabled:           u32,
    /// Crisp-centre fraction kept 1:1 (0.05..0.95). 0.40 = inner 40% kept,
    /// outer 30%-per-side is the stretch band.
    pub hybrid_center:            f32,
    /// FOV gain / how far the rim reaches outward, HORIZONTAL (1.0..10.0).
    pub hybrid_fov_gain:          f32,
    /// Stretch ramp exponent (0.25..4.0). 1.0 = even/linear.
    pub hybrid_ramp:              f32,
    /// Corner micro-blur where magnification is highest (0..1).
    pub hybrid_softness:          f32,
    /// Rear-360 mirror fill toggle (0/1). On by default.
    pub hybrid_rear_enabled:      u32,
    /// Where the rear mirror begins past the cap (0..1).
    pub hybrid_rear_stretch:        f32,
    /// Rear mirror blur (0..1). 0 = sharp (default per spec).
    pub hybrid_rear_direction:         f32,
    /// Rear mirror dim (0..1). 0 = full brightness (default per spec).
    pub hybrid_rear_dim:          f32,
    /// Motion-adaptive rear fade (0..1). 0 = off.
    pub hybrid_motion_fade:       f32,
    /// FOV gain VERTICAL — top/bottom rim reach (1.0..10.0), independent of
    /// horizontal so the top/bottom can be pushed further toward the mirror.
    pub hybrid_fov_gain_v:        f32,
    /// Crisp-centre fraction kept 1:1, VERTICAL (0.05..0.95).
    pub hybrid_center_v:          f32,
    // ── Hybrid rim stretch DIRECTION (LIVE_VERSION 67) ── append-only ──
    /// -1 = rim pushed outward/away, 0 = angular stretch only (default),
    /// +1 = rim pulled forward toward the viewer. Sphere mode only.
    pub hybrid_stretch_dir:       f32,
    // ── Dynamic-depth LOOMING (LIVE_VERSION 68) ── append-only ──
    /// Optical-expansion strength (0..1): forward/back lean → a gentle clip-space
    /// magnification (the dominant motion-in-depth cue), in lock-step with pop-out
    /// and depth-scale. 0 = off (default).
    pub dyn_looming:              f32,
    // ── In-VR (Katanga ImGui) panel size (LIVE_VERSION 76) ────────────────────
    // Independent X/Y stretch multipliers on the overlay quad. `overlay_size`
    // remains the overall width in metres (16:9-derived height); these scale that
    // base per-axis so the panel can be made wider/taller in VR. Append-only.
    pub overlay_size_x: f32,
    pub overlay_size_y: f32,
    /// In-VR overlay panel position offset (metres). X = right, Y = up.
    pub overlay_offset_x: f32,
    pub overlay_offset_y: f32,
    // ── Depth Layers scene priors (LIVE_VERSION 79) ───────────────────────────
    // Ground-plane bias: >0 makes lower image (ground) parallax MORE than upper
    // (sky) — the classic 2D→3D "ground is near" prior. 0 = off (radial only).
    pub dlayers_ground: f32,
    // Horizon height measured from the image BOTTOM (0 = bottom, 1 = top).
    // Pivot for the ground prior and anchor for the vanishing point.
    pub dlayers_horizon: f32,
    // Vanishing-point perspective: blends the layer radius metric from
    // screen-centre toward the horizon point, so corridors/roads layer along
    // perspective instead of a flat vignette. 0 = off.
    pub dlayers_vp: f32,

    // ── v80 addition ───────────────────────────────────────────────────────
    /// Pimax-only: submit the (flat) depth layer so PimaxXR does PLANAR
    /// positional reprojection at the screen plane instead of orientation-only
    /// ATW. 0 = off (today's behaviour: depth skipped on Pimax). 1 = on. Aimed
    /// at the low-FPS positional drag + canted-display left-eye divergence; it's
    /// runtime-specific and untestable off-device, so it's a user A/B toggle.
    pub pimax_flat_depth: u32,

    // ── v81 addition ───────────────────────────────────────────────────────
    /// Adaptive low-FPS prediction boost. When 1 (and pose predict is on), the
    /// viewer measures its real frame cadence and, when it detects the runtime
    /// is frame-doubling (e.g. Pimax Smart Smoothing holding each frame across
    /// two refresh periods at low FPS), extends the pose-prediction horizon to
    /// land in the MIDDLE of the doubled display span instead of its start.
    /// This cancels the second-period staleness that reads as positional drag.
    /// Bounded and self-gating (adds nothing at full rate). 0 = off (default).
    pub lowfps_predict_boost: u32,

    // ── v82 addition ───────────────────────────────────────────────────────
    /// Strength of the adaptive low-FPS boost: how far across the doubled
    /// display span to push the pose prediction when the runtime is frame-
    /// doubling. 0.5 = the centre of the span (balanced error across both
    /// refreshes), 1.0 = the far end (fully compensates the stale last refresh,
    /// at the cost of over-leading the first). Only matters when
    /// `lowfps_predict_boost` is on. Default 0.5.
    pub lowfps_predict_strength: f32,

    // ── Submit-path reprojection toggles (LIVE_VERSION 84). Append-only. ──
    /// #7: submit the predicted (rendered) pose in the projection layer instead
    /// of the raw locate_views pose, so the runtime reprojects from the true
    /// render pose (matches xrLocateViews↔xrEndFrame). 0 = off (default).
    pub submit_render_pose: u32,
    /// #6: equalize the submitted per-eye orientation (both eyes share the
    /// averaged head orientation; per-eye IPD position preserved) so the runtime
    /// cannot apply divergent per-eye rotational warp. 0 = off (default).
    pub stable_eye_submit: u32,
    /// Hold full refresh: assert full-rate operation — minimum pacing lead
    /// (freshest pose) and no low-FPS boost horizon extension. 0 = off (default).
    pub hold_full_refresh: u32,
    // ── Parallax Prediction options A/B/F (LIVE_VERSION 93). Append-only. ──
    /// PP option A: feed the runtime's sensor-fused angular velocity
    /// (XrSpaceVelocity) into the predictor instead of the finite-difference
    /// estimate (lower latency + lower noise). 0 = off (default).
    pub pp_runtime_vel: u32,
    /// PP option B: add the measured display period (motion-to-photon) to the
    /// prediction lead so it covers the real frame latency. 0 = off (default).
    pub pp_photon_horizon: u32,
    /// PP option F: 1-Euro adaptive velocity filter (speed-scaled smoothing)
    /// in place of the fixed 0.20 EMA. 0 = off (default).
    pub pp_euro: u32,
    /// Force the in-VR (Katanga) panel reticle to read physical mouse motion so
    /// it works in mouselook games that lock/recenter the OS cursor. 0 = off.
    pub panel_cursor_force: u32,
    /// Panel cursor read method: 0 = Relative (physical motion),
    /// 1 = Absolute (OS cursor), 2 = Both. Default 0.
    pub panel_cursor_method: u32,
    /// In-VR panel theme: 0=Colored,1=DarkBlue,2=Black,3=Cyan,4=Light. CPU-only.
    pub panel_theme: u32,
}

impl LiveParams {
    /// Combine the two seq halves into a u64 for monotonic comparison.
    /// Stored split (lo, hi) only to keep the struct 4-byte aligned;
    /// callers conceptually treat seq as a single u64.
    pub fn seq(&self) -> u64 {
        ((self.seq_hi as u64) << 32) | (self.seq_lo as u64)
    }
    /// Set the combined seq from a u64.
    pub fn set_seq(&mut self, v: u64) {
        self.seq_lo = v as u32;
        self.seq_hi = (v >> 32) as u32;
    }

    /// Decode the UDP destination IP from its fixed-size byte form
    /// to a Rust string. Stops at the first null byte. Returns "" if
    /// the bytes are non-ASCII.
    pub fn udp_ip_str(&self) -> String {
        let mut end = self.udp_6dof_ip.len();
        for (i, &b) in self.udp_6dof_ip.iter().enumerate() {
            if b == 0 {
                end = i;
                break;
            }
        }
        std::str::from_utf8(&self.udp_6dof_ip[..end])
            .unwrap_or("")
            .to_string()
    }

    /// Pack a Rust IP string into the fixed-size byte form. Truncates
    /// to 15 chars (leaving room for the null) and zero-pads.
    pub fn set_udp_ip_str(&mut self, ip: &str) {
        self.udp_6dof_ip = [0u8; 16];
        let bytes = ip.as_bytes();
        let n = bytes.len().min(15);
        self.udp_6dof_ip[..n].copy_from_slice(&bytes[..n]);
    }

    /// Decode the VR-Data UDP destination IP.
    pub fn vr_udp_ip_str(&self) -> String {
        let mut end = self.vr_udp_ip.len();
        for (i, &b) in self.vr_udp_ip.iter().enumerate() {
            if b == 0 {
                end = i;
                break;
            }
        }
        std::str::from_utf8(&self.vr_udp_ip[..end])
            .unwrap_or("")
            .to_string()
    }

    /// Pack a Rust IP string into vr_udp_ip's fixed-size byte form.
    pub fn set_vr_udp_ip_str(&mut self, ip: &str) {
        self.vr_udp_ip = [0u8; 16];
        let bytes = ip.as_bytes();
        let n = bytes.len().min(15);
        self.vr_udp_ip[..n].copy_from_slice(&bytes[..n]);
    }
}

impl Default for LiveParams {
    fn default() -> Self {
        Self {
            seq_lo: 0,
            seq_hi: 0,
            enabled: 0,
            stereo_mode: StereoModeIndex::Mono as u32,
            xr_backend: XrBackendIndex::OpenXR as u32,
            override_toggles: 0,
            distance: 20.0,
            scale: 40.0,
            x_curvature: 0.4,
            y_curvature: 0.08,
            offset_x: 0.0,
            offset_y: 0.0,
            offset_z: 0.0,
            edge_stretch: 0.0,
            edge_stretch_softness: 0.5,
            // Default sphere cap = ~26° half-width × ~17° half-height,
            // a natural-feeling 50° wide × 33° tall content area when
            // the user is at typical viewing distance.
            sphere_x_size: 1.0,
            sphere_y_size: 0.5,
            sphere_x_curve: 1.0,
            sphere_y_curve: 1.0,
            box_x_size: 1.0,
            box_y_size: 1.0,
            box_z_depth: 1.0,
            box_corner_radius: 0.0,
            texture_sharpen: 0.0,
            filter_mode: 0,
            extend_stretch: 0.0,
            extend_softness: 0.0,
            sim6dof_enabled: 0,
            sim6dof_intensity: 1.0,
            sim6dof_smoothness: 0.3,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            sharpness: 0.0,
            swap_eyes: 1,
            flip_x: 0,
            flip_y: 0,
            head_lock: 0,
            ambient: 0,
            stretch_mode: 0,
            quit_request: 0,
            supersampling: 1.0,
            screenshot_request: 0,
            restart_session_request: 0,
            recenter_request: 0,
            edge_expand: 0.0,
            // 0.6.0-dev additions
            edge_stretch_extend: 0.0,
            edge_expand_extend: 0.0,
            filter_bilinear: 1.0,
            filter_trilinear: 0.0,
            sim_6dof_enabled: 0,
            sim_6dof_amount: 1.0,
            sim_6dof_smoothness: 0.85,
            sim_6dof_zoom_amount: 1.0,
            ipd_perspective: 1.0,
            katanga_perf_mode: 0,
            expansion_outer: 0.0,
            expansion_seamless: 0.0,
            offset_roll: 0.0,
            // 0.6.0 head-tracking output features
            mouse_emu_enabled: 0,
            mouse_emu_sensitivity: 1.0,
            mouse_emu_speed: 1.0,
            mouse_emu_compat: 2,
            udp_6dof_enabled: 0,
            udp_6dof_port: 4242,
            udp_6dof_ip: {
                // "127.0.0.1" + null padding to 16 bytes.
                let mut a = [0u8; 16];
                let s = b"127.0.0.1";
                let mut i = 0;
                while i < s.len() {
                    a[i] = s[i];
                    i += 1;
                }
                a
            },
            udp_flip_x: 0,
            udp_flip_y: 0,
            udp_flip_z: 0,
            udp_flip_yaw: 0,
            udp_flip_pitch: 0,
            udp_flip_roll: 0,
            udp_gain_yaw: 1.0,
            udp_gain_pitch: 1.0,
            udp_gain_roll: 1.0,
            udp_gain_x: 1.0,
            udp_gain_y: 1.0,
            udp_gain_z: 1.0,
            extrusion_strength: 0.0,
            extrusion_direction: 1.0,
            concave_strength: 0.0,
            concave_depth: 0.5,
            concave_shape: 0.5,
            mirror_falloff: 0.0,
            extend_pull_blend: 0.0,
            expansion_smoothness: 0.0,
            concave_z0: 1.0, concave_z1: 1.0, concave_z2: 1.0,
            concave_z3: 1.0, concave_z4: 1.0,
            concave_dz0: 0.0, concave_dz1: 0.0, concave_dz2: 0.0,
            concave_dz3: 0.0, concave_dz4: 0.0,
            filter_bicubic: 0.0,
            filter_lanczos: 0.0,
            headlock_jitter_deadzone: 0.0,
            headlock_jitter_smooth: 0.0,
            stable_lock_parallax_xy: 0.5,
            stable_lock_parallax_z: 0.5,
            stable_lock_dir_enabled: 1,
            stable_lock_dir_strength: 0.75,
            headlock_jitter_method: 0,
            headlock_dejitter: 0,
            headlock_dejitter_stiffness: 0.4,
            headlock_dejitter_max_lag: 8.0,
            headlock_delay_ms: 80.0,
            parallax_prediction: 0,
            parallax_prediction_amt: 0.5,
            pp_adaptive: 0,
            pp_deadband_deg: 0.3,
            pp_accel: 0.0,
            sim6dof_zoom_only: 0,
            vr_hotkeys_enabled: 1,
            vr_udp_enabled: 0,
            vr_udp_mode: 0,
            vr_udp_port: 4243,
            vr_udp_ip: {
                let mut buf = [0u8; 16];
                let s = b"127.0.0.1";
                buf[..s.len()].copy_from_slice(s);
                buf
            },
            vr_udp_flip_x: 0,
            vr_udp_flip_y: 0,
            vr_udp_flip_z: 0,
            vr_udp_flip_yaw: 0,
            vr_udp_flip_pitch: 0,
            vr_udp_flip_roll: 0,
            vr_udp_gain_yaw: 1.0,
            vr_udp_gain_pitch: 1.0,
            vr_udp_gain_roll: 1.0,
            vr_udp_gain_x: 1.0,
            vr_udp_gain_y: 1.0,
            vr_udp_gain_z: 1.0,
            vr_udp_left_enabled: 0,
            vr_udp_right_enabled: 0,
            diag_mode: 0,
            pose_predict_ms: 0.0,
            pose_smooth_alpha: 0.75,
            vsync_mode: 0,
            fps_limit: 0.0,
            frame_pacing_enabled: 1,
            frame_pacing_target: 0.45,
            temporal_blend_enabled: 0,
            temporal_blend_alpha: 0.8,
            flow_enabled: 0,
            flow_strength: 0.7,
            enhancement_quality: 0.0,
            rcas_sharpness: 0.0,
            repeat_stretch: 0.0,
            repeat_blend: 0.3,
            repeat_depth: 0.30,
            repeat_size: 0.0,
            auto_z_enabled:      0,
            auto_z_value:        0.0,
            auto_roll_enabled:   0,
            auto_roll_value:     0.0,
            auto_x_enabled:      0,
            auto_x_value:        0.0,
            auto_y_enabled:      0,
            auto_y_value:        0.0,
            auto_height_enabled: 0,
            sphere_y_169:        0.5,
            sphere_y_43:         0.6,
            sphere_y_219:        0.35,
            joy_emu_enabled:     0,
            joy_emu_mode:        0,
            joy_emu_sensitivity: 2.0,
            joy_emu_deadzone:    0.015,
            joy_emu_max_angle:   84.0,
            joy_emu_invert_x:    0,
            joy_emu_invert_y:    0,
            joy_emu_smoothness:  0.0,
            trackir_enabled:     0,
            trackir_flip_x:      0,
            trackir_flip_y:      0,
            trackir_flip_z:      0,
            trackir_flip_yaw:    0,
            trackir_flip_pitch:  0,
            trackir_flip_roll:   0,
            trackir_gain_z:      1.0,
            joy_emu_speed_x:     1.0,
            joy_emu_speed_y:     1.0,
            dlayers_reactive:    0.0,
            overlay_enabled:     0,
            overlay_size:        2.0,
            overlay_distance:    2.0,
            dlayers_mode:        0,
            overlay_hud_mode:    0,
            overlay_aspect:      0,
            overlay_transparency: 1.0,
            dlayers_convex:      0.0,

            // Katanga Filters — off by default; neutral values.
            katanga_filters_enabled:   0,
            katanga_sharpness:         0.0,
            katanga_texture_sharpness: 0.0,
            katanga_saturation:        1.0,
            katanga_contrast:          1.0,
            katanga_brightness:        0.0,
            hybrid_stretch_reach:      0.5,
            sim6dof_spring:             0.0,

            force_desktop_request:    0,
            separation:               1.0,
            convergence:              0.0,
            dyn_popout:               1.0,
            dyn_depthscale:           1.0,
            sim6dof_dynamic_depth:    0,
            cas:                       0.0,
            dehaze:                    0.0,
            katanga_cas:               0.0,
            katanga_dehaze:            0.0,
            sim6dof_mode:              0,    // Default (follow-the-head)
            offaxis_window_depth:      1.0,
            offaxis_parallax:          1.0,
            offaxis_edge_falloff:      1.0,
            offaxis_vertical_balance:  1.0,
            dlayers_enabled:           0,    // off — additive/reversible
            dlayers_invert:            0,    // 0 = inner closest (delay ripples outward)
            dlayers_strength:          1.0,
            dlayers_separation:        1.0,
            dlayers_delay:             0.2,
            dlayers_curve:             1.0,
            dlayers_zoom:              1.0,
            dlayers_edge:              0.5,
            dir6dof_enabled:           0,
            dir6dof_yaw:               0.5,
            dir6dof_pitch:             0.5,
            dir6dof_roll:              0.5,
            hybrid_enabled:            0,
            hybrid_center:             0.40,
            hybrid_fov_gain:           1.35,
            hybrid_ramp:               1.0,
            hybrid_softness:           0.5,
            hybrid_rear_enabled:       1,
            hybrid_rear_stretch:         0.0,
            hybrid_rear_direction:          0.0,
            hybrid_rear_dim:           0.0,
            hybrid_motion_fade:        0.0,
            hybrid_fov_gain_v:         1.35,
            hybrid_center_v:           0.40,
            hybrid_stretch_dir:        0.0,
            dyn_looming:               0.0,
            overlay_size_x: 1.0,
            overlay_size_y: 1.0,
            overlay_offset_x: 0.0,
            overlay_offset_y: 0.0,
            dlayers_ground: 0.0,
            dlayers_horizon: 0.5,
            dlayers_vp: 0.0,
            pimax_flat_depth: 0,
            lowfps_predict_boost: 0,
            lowfps_predict_strength: 0.5,
            submit_render_pose: 0,
            stable_eye_submit: 0,
            hold_full_refresh: 0,
            pp_runtime_vel: 0,
            pp_photon_horizon: 0,
            pp_euro: 0,
            panel_cursor_force: 0,
            panel_cursor_method: 0,
            panel_theme: 0,
        }
    }
}

/// Wrapper actually stored in the shared mapping. Includes the magic+version
/// header so the viewer can ignore the mapping safely if it was written by
/// an incompatible build.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LiveParamsMapping {
    pub magic: u32,
    pub version: u32,
    pub _pad: [u32; 2],
    pub params: LiveParams,
}

impl Default for LiveParamsMapping {
    fn default() -> Self {
        Self {
            magic: LIVE_MAGIC,
            version: LIVE_VERSION,
            _pad: [0; 2],
            params: LiveParams::default(),
        }
    }
}

/// Stereo mode index used in the wire format. Kept separate from the
/// viewer's `StereoMode` enum so the shared crate doesn't need to depend
/// on the viewer crate.
///
/// Order MUST stay stable. Append-only.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StereoModeIndex {
    Mono = 0,
    Sbs = 1,
    Tab = 2,
    FullSbs = 3,
    FullTab = 4,
    LineInterlaced = 5,
    Checkerboard = 6,
}

impl StereoModeIndex {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Sbs,
            2 => Self::Tab,
            3 => Self::FullSbs,
            4 => Self::FullTab,
            5 => Self::LineInterlaced,
            6 => Self::Checkerboard,
            _ => Self::Mono,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Sbs => "Half-SBS",
            Self::Tab => "Half-TAB",
            Self::FullSbs => "Full-SBS",
            Self::FullTab => "Full-TAB",
            Self::LineInterlaced => "Line Interlaced",
            Self::Checkerboard => "Checkerboard 3D",
        }
    }
}

/// XR backend index. OpenVR is reserved for Phase 5; the GUI shows the
/// toggle today and the viewer logs its choice without acting on it yet.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum XrBackendIndex {
    OpenXR = 0,
    OpenVR = 1,
}

impl XrBackendIndex {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::OpenVR,
            _ => Self::OpenXR,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::OpenXR => "OpenXR",
            Self::OpenVR => "OpenVR (preview)",
        }
    }
}

/// Wire format for the `stretch_mode` field of `LiveParams`. Selects
/// which screen shape mode runs.
///
/// `Sphere` (default): inside-out sphere mesh wrapping the user.
/// The `sphere_curve` slider controls curvature.
///
/// `Box`: Inside-out cube. Front wall = source image; other 5 walls
/// show stretched edge pixels with chamferable seams.
///
/// `Fisheye` (new in 0.6.0): a horizontally elongated curved panel
/// that bends outward at the corners, like a cinematic ultrawide
/// projector screen. Reuses the sphere's angular extent + fragment
/// shader, but with a wider default horizontal angle and adjustable
/// per-axis curvature (sphere_x_curve / sphere_y_curve double as the
/// fisheye curve sliders). Source UV maps linearly across the panel
/// so there's no lens distortion in the centre — only at the rim
/// does the geometry curl outward.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StretchModeIndex {
    Sphere = 0,
    /// Inside-out box. Viewer at the centre. Front wall shows the
    /// source image; the other 5 walls show stretched edge pixels.
    /// The cube's seams can be chamfered for a smoother corner look.
    Box = 2,
    /// Fisheye / cinematic ultrawide curved screen. Wider horizontal
    /// extent than sphere mode by default, with adjustable curvature
    /// on both axes. Edge stretch behaves like the sphere mode's.
    Fisheye = 3,
}

impl StretchModeIndex {
    pub fn from_u32(v: u32) -> Self {
        match v {
            2 => Self::Box,
            3 => Self::Fisheye,
            _ => Self::Sphere,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sphere => "Sphere (360°)",
            Self::Box => "Box (cube)",
            Self::Fisheye => "Fisheye (curved ultrawide)",
        }
    }
}

// ───────────────────────────────────────────────────────────────
// Upstream events channel  (viewer → GUI)
// ───────────────────────────────────────────────────────────────
// A small SEPARATE named-memory segment that flows the OTHER
// direction: the viewer writes current toggle states every frame
// so the GUI can sync its checkboxes even when VR controller
// hotkeys change them.

// "Local\" prefix works across all session/elevation boundaries that
// Osiris targets (same desktop session, viewer and GUI both running
// as the same user, optionally elevated). "Global\" requires the
// SeCreateGlobalPrivilege privilege which is not granted by default
// even to elevated processes in modern Windows → HRESULT 0x80070005
// "Access is denied." Using "Local\" avoids that restriction while
// still being visible to both the viewer and the GUI (same session).
pub const UPSTREAM_NAME: &str = "Local\\OsirisUpstreamEvents";
pub const UPSTREAM_SIZE: usize = 64;
pub const UPSTREAM_MAGIC: u32 = 0x4F535549; // "OSUI"
pub const UPSTREAM_VERSION: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UpstreamEvents {
    pub magic: u32,
    pub version: u32,
    /// Monotonic counter. Incremented whenever any toggle state
    /// changes. GUI compares to its last-seen value.
    pub seq: u32,
    /// Packed toggle bits (viewer-side state):
    ///   bit 0 = mouse_emu_enabled
    ///   bit 1 = sim6dof_enabled
    ///   bit 2 = head_lock
    ///   bit 3 = swap_eyes
    ///   bit 4 = udp_6dof_enabled
    pub toggle_bits: u32,
    /// Last controller button pressed during VR-binding capture.
    /// 0 = none, else the wire byte from ControllerButton.
    pub last_button: u8,
    /// Which HotkeyAction the capture was for (wire byte + 1, 0=none).
    pub capture_action: u8,
    pub _pad: [u8; 2],
    pub _reserved: [u32; 10],
}

impl Default for UpstreamEvents {
    fn default() -> Self {
        Self {
            magic: UPSTREAM_MAGIC,
            version: UPSTREAM_VERSION,
            seq: 0,
            toggle_bits: 0,
            last_button: 0,
            capture_action: 0,
            _pad: [0; 2],
            _reserved: [0; 10],
        }
    }
}
