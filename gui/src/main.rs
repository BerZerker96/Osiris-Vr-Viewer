// Osiris VR Viewer — control panel.
//
// Restyle of the original GUI: same controls, same persistence model, same
// shared-memory channel — just laid out as a horizontal three-column
// dashboard with blue header bars per section and the project logo loaded
// from `assets/logo.ico`.
//
// Channels:
//   * `presets/default.json` next to the viewer exe — persistent storage,
//     hot-reloaded by the viewer's file watcher.
//   * Shared memory (`osiris_shared::LiveParamsMapping`) — realtime
//     overrides while sliders are dragged. Disabled on GUI exit so the
//     viewer falls back to the on-disk preset cleanly.
//
// All slider ranges in this file are the "× 5" widened ranges from the
// user spec.

#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod shm;
mod hotkeys;
#[cfg(target_os = "windows")]
mod low_level_hook;

use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use anyhow::Context;
use egui::{Color32, ColorImage, RichText, Rounding, Stroke, TextureHandle, Vec2};
use osiris_shared::{LiveParamsMapping, StereoModeIndex, StretchModeIndex, XrBackendIndex};
use serde::{Deserialize, Serialize};

const APP_TITLE: &str = "OSIRIS VR VIEWER";

// Logo bytes embedded at compile time. The ICO crate inside `image`
// happens to also handle the "single-large-bitmap-in-an-ICO" case our
// uploaded logo uses (192×90).
const LOGO_BYTES: &[u8] = include_bytes!("../../assets/logo.ico");
/// Banner background for the GUI title bar — a circuit-board image
/// stretched horizontally to fill the area behind the logo, title,
/// and the right-side action buttons. The PNG is loaded once at
/// startup and re-rendered each frame as a textured rect under the
/// widgets.
const BANNER_BYTES: &[u8] = include_bytes!("../../assets/banner.png");

// --- Theme constants (sampled from the reference mockup) ------------------

const COL_BG: Color32 = Color32::from_rgb(0x05, 0x07, 0x0E);
const COL_PANEL: Color32 = Color32::from_rgb(0x12, 0x18, 0x24);
const COL_PANEL_LIGHT: Color32 = Color32::from_rgb(0x1B, 0x22, 0x30);
const COL_HEADER_BG: Color32 = Color32::from_rgb(0x1F, 0x6F, 0xC8);
const COL_BLUE: Color32 = Color32::from_rgb(0x3A, 0x9D, 0xF5);
const COL_BLUE_DIM: Color32 = Color32::from_rgb(0x21, 0x6E, 0xB6);
const COL_TEXT: Color32 = Color32::from_rgb(0xE6, 0xEC, 0xF5);
const COL_TEXT_DIM: Color32 = Color32::from_rgb(0x96, 0xA0, 0xB0);
const COL_BORDER: Color32 = Color32::from_rgb(0x2A, 0x6F, 0xB6);
// Slider tracks: lighter than the panel so they're clearly visible
// against the dark dashboard background. Picked to read well behind
// the bright knob.
const COL_SLIDER_TRACK: Color32 = Color32::from_rgb(0x9A, 0xA6, 0xB6);
// Red accent palette. Used by the "Save default" button (the high-
// impact destructive action that writes over the auto-loaded preset)
// and reserved for any other buttons that need extra visibility.
const COL_RED: Color32 = Color32::from_rgb(0xE0, 0x3A, 0x3A);
const COL_RED_DIM: Color32 = Color32::from_rgb(0xB0, 0x2A, 0x2A);
#[allow(dead_code)]
const COL_RED_HOT: Color32 = Color32::from_rgb(0xFF, 0x55, 0x55);

// --- Config (mirrors viewer's AppConfig field-for-field) -----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct GuiConfig {
    // ---- Persisted GUI choices that map to viewer behaviour ----
    /// 3D format: which way the source is split into left/right eyes.
    /// Stored as the integer index from `StereoModeIndex` so the JSON
    /// is forward-compatible with new modes appended later.
    stereo_mode: u32,
    /// XR runtime to ask the viewer to use (OpenXR, OpenVR).
    xr_backend: u32,
    /// Whether GUI changes to toggles (swap_eyes, flip_x/y, head_lock,
    /// ambient) are pushed live to the viewer via shared memory.
    push_toggles: bool,
    /// Whether per-frame diagnostics are written to osiris-diagnostics.log.
    diag_mode: bool,
    /// Extra pose prediction offset in ms (0 = off, 8–10 = good for Pimax Crystal).
    pose_predict_ms: f32,
    /// EMA smoothing factor for pose velocity (0 = raw/snappy, 0.9 = heavy/stable).
    pose_smooth_alpha: f32,
    /// Pimax-only experimental: submit the flat depth layer so PimaxXR does
    /// planar positional reprojection (fixes low-FPS drag + left-eye divergence)
    /// instead of orientation-only ATW. Off by default; A/B test on-device.
    #[serde(default)]
    pimax_flat_depth: bool,
    /// Adaptive low-FPS prediction boost: extends the pose-prediction horizon
    /// when the runtime is frame-doubling (Pimax Smart Smoothing) so the pose
    /// lands mid-span instead of stale-by-a-frame. Needs pose predict on.
    #[serde(default)]
    lowfps_predict_boost: bool,
    /// Strength of the adaptive low-FPS boost (0.5 = span centre, 1.0 = span
    /// end). Higher = more aggressive drag compensation, more head-lead overshoot.
    #[serde(default = "default_lowfps_predict_strength")]
    lowfps_predict_strength: f32,
    /// VSync mode: 0=Default, 1=Off, 2=On, 3=Adaptive, 4=Adaptive Half Refresh
    #[serde(default)]
    vsync_mode: u32,
    /// Frame rate limit (fps). 0.0 = unlimited.
    #[serde(default)]
    fps_limit: f32,
    /// Frame pacing: enabled toggle.
    frame_pacing_enabled: bool,
    /// Frame pacing: target fraction of frame period for submission (0..0.95).
    frame_pacing_target: f32,
    /// Temporal blend: smooth transitions between frames.
    #[serde(default)]
    temporal_blend_enabled: bool,
    #[serde(default = "default_temporal_alpha")]
    temporal_blend_alpha: f32,
    /// Motion extrapolation (optical flow).
    #[serde(default)]
    flow_enabled: bool,
    #[serde(default = "default_flow_strength")]
    flow_strength: f32,
    /// #7: submit predicted (rendered) pose in the projection layer.
    #[serde(default)]
    submit_render_pose: bool,
    /// #6: equalize per-eye submit orientation (no divergent per-eye warp).
    #[serde(default)]
    stable_eye_submit: bool,
    /// Hold full refresh: min pacing lead + no low-FPS boost extension.
    #[serde(default)]
    hold_full_refresh: bool,
    /// Repeated method: stretch amount (0..30).
    repeat_stretch: f32,
    /// Repeated method: seam blend (0..1).
    repeat_blend: f32,
    /// Repeated method depth push (0=flat, 1=forward).
    #[serde(default)]
    repeat_depth: f32,
    #[serde(default)]
    repeat_size: f32,

    // ── Auto Adjust ──────────────────────────────────────────────────────
    #[serde(default)]
    auto_z_enabled: bool,
    #[serde(default)]
    auto_z_value: f32,
    #[serde(default)]
    auto_roll_enabled: bool,
    #[serde(default)]
    auto_roll_value: f32,
    #[serde(default)]
    auto_x_enabled: bool,
    #[serde(default)]
    auto_x_value: f32,
    #[serde(default)]
    auto_y_enabled: bool,
    #[serde(default)]
    auto_y_value: f32,
    #[serde(default)]
    auto_height_enabled: bool,
    #[serde(default = "default_sphere_y_169")]
    sphere_y_169: f32,
    #[serde(default = "default_sphere_y_43")]
    sphere_y_43: f32,
    #[serde(default = "default_sphere_y_219")]
    sphere_y_219: f32,
    /// FSR1 upscale factor (1.0=off, 1.3=Quality, 1.5=Balanced, 2.0=Performance). Requires restart.
    enhancement_quality: f32,
    /// RCAS sharpening strength (0=off, 2=max). Live-adjustable.
    rcas_sharpness: f32,

    // ---- Geometry / behaviour fields ----
    x_curvature: f32,
    y_curvature: f32,
    swap_eyes: bool,
    flip_x: bool,
    flip_y: bool,
    distance: f32,
    scale: f32,
    ambient: bool,
    head_lock: bool,
    edge_stretch: f32,
    /// Softness of the inner-edge transition (0 = hard cutover, 1 =
    /// fully smooth fade from the central image into the edge rays).
    edge_stretch_softness: f32,
    /// Gradual edge expand (0..1).
    edge_expand: f32,
    /// Sphere mode: angular half-width of the source-image cap (rad).
    sphere_x_size: f32,
    /// Sphere mode: angular half-height of the source-image cap (rad).
    sphere_y_size: f32,
    /// Sphere mode: 0..1 horizontal sphere curvature.
    sphere_x_curve: f32,
    /// Sphere mode: 0..1 vertical sphere curvature.
    sphere_y_curve: f32,
    /// Box mode: x scale multiplier (1.0 = source-aspect baseline).
    box_x_size: f32,
    /// Box mode: y scale multiplier.
    box_y_size: f32,
    /// Box mode: z (depth) scale multiplier.
    box_z_depth: f32,
    /// Box mode: 0..1 chamfer/rounding amount of the seams.
    box_corner_radius: f32,
    /// MeshExtension mode: x scale multiplier.
    mesh_ext_x_size: f32,
    /// MeshExtension mode: y scale multiplier.
    mesh_ext_y_size: f32,
    /// MeshExtension mode: z (depth) scale multiplier.
    mesh_ext_z_depth: f32,
    /// 0 = sphere mode, 1 = mesh-extension mode (deprecated, auto-upgrades
    /// to sphere on save), 2 = box mode.
    stretch_mode: u32,
    /// Supersampling factor for the OpenXR swapchain (0.5..3.0).
    /// 1.0 = native HMD resolution. Persisted in the preset.
    supersampling: f32,
    /// Katanga Filters: a second, stronger set of image adjustments. When
    /// enabled they are ADDED on top of the normal image sliders (applies to
    /// whatever is on screen, same as the normal filters). Sharpness fields go
    /// up to 10 for aggressive sharpening.
    katanga_filters_enabled: bool,
    katanga_sharpness: f32,
    katanga_texture_sharpness: f32,
    katanga_saturation: f32,
    katanga_contrast: f32,
    katanga_brightness: f32,
    brightness: f32,
    contrast: f32,
    saturation: f32,
    sharpness: f32,
    /// Contrast Adaptive Sharpening (0..10).
    #[serde(default)]
    cas: f32,
    /// Dehaze / Clarity (0..10).
    #[serde(default)]
    dehaze: f32,
    /// Katanga-specific CAS / Dehaze (0..10), added on top when Katanga
    /// filters are on.
    #[serde(default)]
    katanga_cas: f32,
    #[serde(default)]
    katanga_dehaze: f32,
    // ---- 0.6.0-dev fields ----
    /// Tight micro-detail unsharp mask (0..10). Targets small-texture
    /// detail; smaller kernel than `sharpness`.
    texture_sharpen: f32,
    /// Bilinear filter strength (0..1). 1 = native sampler bilinear,
    /// 0 = collapse toward nearest-neighbour.
    filter_bilinear: f32,
    /// Trilinear-style 9-tap filter (0..1). Smoothing without LOD.
    filter_trilinear: f32,
    /// Extend-based edge stretch (0..30). Companion to mirror-based
    /// `edge_stretch` — content flows out from the source edge by
    /// progressively sampling inward.
    edge_stretch_extend: f32,
    /// Extend-based edge stretch softness (0..1). Shapes the inward-walk
    /// curve from linear (0) to ease-in (1).
    edge_expand_extend: f32,
    /// Edge-stretch method toggles (image-1 layout): each classic method has its
    /// own on/off checkbox. When off, the method's driving values are pushed as 0
    /// so the effect is truly disabled (not just its sliders hidden).
    #[serde(default)]
    show_mirror_method: bool,
    #[serde(default)]
    show_repeated_method: bool,
    #[serde(default)]
    show_expansion_method: bool,
    #[serde(default)]
    show_extrusion_method: bool,
    /// Simulated 6DoF parallax — toggle.
    sim6dof_enabled: bool,
    /// Simulated 6DoF parallax intensity (0..10). 1.0 = head-to-screen
    /// 1:1 inverse.
    sim6dof_intensity: f32,
    /// Simulated 6DoF smoothing (0..1). Higher = more damped/smoother
    /// response. ~0.85 feels natural.
    sim6dof_smoothness: f32,
    /// Simulated 6DoF Z-axis (zoom) intensity (0..20). Independent
    /// of the X/Y movement slider so users can have, say, gentle
    /// side-to-side parallax with strong forward/back zoom.
    sim6dof_zoom_intensity: f32,
    /// IPD perspective multiplier (0..2). Stereoscopic perspective
    /// scaler — >1 makes objects appear closer/larger, <1 makes
    /// them appear farther/smaller. Lives at the top of the Image
    /// section in the GUI.
    ipd_perspective: f32,
    /// Stereo separation (0..3). Scales overall 3D disparity/pop. 1.0 = source
    /// as authored. Slider sits under IPD in the Image section.
    separation: f32,
    /// Stereo convergence (-1..1). Moves the zero-parallax (screen) plane in
    /// depth. 0 = neutral. Slider sits under Separation.
    convergence: f32,
    /// Dynamic-depth POP-OUT strength (0..4): forward/back -> convergence.
    #[serde(default = "default_one")]
    dyn_popout: f32,
    /// Dynamic-depth DEPTH-SCALE strength (0..4): forward/back -> separation.
    #[serde(default = "default_one")]
    dyn_depthscale: f32,
    /// Dynamic-depth LOOMING strength (0..1): forward/back -> optical expansion.
    #[serde(default)]
    dyn_looming: f32,
    /// Dynamic-depth toggle for simulated 6DoF (0/1 as bool).
    sim6dof_dynamic_depth: bool,
    /// Return-to-center spring for simulated 6DoF. When on, the anchor slowly
    /// relaxes toward the head so a sustained lean re-centers itself.
    sim6dof_spring: bool,
    /// Supersampling slider. When enabled (and the active source is
    /// Katanga), the viewer's shader takes a minimal fast path that
    /// bypasses sharpen, edge stretch, fisheye, filter blends, and
    /// curve mix — matching the original VRScreenCap baseline cost.
    katanga_perf_mode: bool,
    /// Expansion-stretch outer reach (0..1). New mesh-stretch edge
    /// mode — mesh extends physically toward 360°, fragments outside
    /// the cap sample stretched source content rather than mirrored
    /// edge pixels. 0 = off, 1 = full 360° wrap.
    expansion_outer: f32,
    /// Expansion-stretch seamlessness (0..1). 0 = stretch only the
    /// outermost edge pixel (jarring), 1 = stretch reaches deep into
    /// the source for a smooth continuous look.
    expansion_seamless: f32,
    /// Mesh forward-extrusion strength (0..3). New slider — physical
    /// mesh curl independent from the UV-walk that `expansion_outer`
    /// drives. 0 = no curl, higher = more fishbowl.
    #[serde(default)]
    extrusion_strength: f32,
    /// Mesh-extrusion direction. +1 = toward viewer (fishbowl),
    /// -1 = away from viewer.
    #[serde(default = "default_one")]
    extrusion_direction: f32,

    // Concave sliders — apply to Sphere / Fisheye / Box back wall.
    #[serde(default)]
    concave_strength: f32,
    #[serde(default = "default_concave_depth")]
    concave_depth: f32,
    #[serde(default = "default_concave_shape")]
    concave_shape: f32,
    #[serde(default = "default_concave_z")]
    concave_z0: f32,
    #[serde(default = "default_concave_z")]
    concave_z1: f32,
    #[serde(default = "default_concave_z")]
    concave_z2: f32,
    #[serde(default = "default_concave_z")]
    concave_z3: f32,
    #[serde(default = "default_concave_z")]
    concave_z4: f32,
    #[serde(default)]
    concave_dz0: f32,
    #[serde(default)]
    concave_dz1: f32,
    #[serde(default)]
    concave_dz2: f32,
    #[serde(default)]
    concave_dz3: f32,
    #[serde(default)]
    concave_dz4: f32,
    #[serde(default)]
    mirror_falloff: f32,
    #[serde(default)]
    extend_pull_blend: f32,
    #[serde(default)]
    expansion_smoothness: f32,
    #[serde(default)]
    filter_bicubic: f32,
    #[serde(default)]
    filter_lanczos: f32,
    #[serde(default)]
    headlock_jitter_deadzone: f32,
    #[serde(default)]
    headlock_jitter_smooth: f32,
    #[serde(default = "default_stable_lock_parallax")]
    stable_lock_parallax_xy: f32,
    #[serde(default = "default_stable_lock_parallax")]
    stable_lock_parallax_z: f32,
    #[serde(default = "default_true")]
    stable_lock_dir_enabled: bool,
    #[serde(default = "default_sl_dir_strength")]
    stable_lock_dir_strength: f32,
    /// 0=One Euro Filter, 1=Exponential smoothing, 2=Deadzone only
    #[serde(default)]
    headlock_jitter_method: u32,
    /// DeJitter: critically-damped soft-lock spring, composes
    /// with ALL headlock jitter methods (applied after them in the viewer).
    #[serde(default)]
    headlock_dejitter: bool,
    #[serde(default = "default_dejitter_stiffness")]
    headlock_dejitter_stiffness: f32,
    #[serde(default = "default_dejitter_max_lag")]
    headlock_dejitter_max_lag: f32,
    #[serde(default)] parallax_prediction: bool,
    #[serde(default = "default_parallax_prediction_amt")] parallax_prediction_amt: f32,
    #[serde(default)] pp_adaptive: bool,
    #[serde(default = "default_pp_deadband_deg")] pp_deadband_deg: f32,
    #[serde(default)] pp_accel: f32,
    #[serde(default)] pp_runtime_vel: bool,
    #[serde(default)] pp_photon_horizon: bool,
    #[serde(default)] pp_euro: bool,
    headlock_delay_ms: f32,
    #[serde(default)]
    sim6dof_zoom_only: bool,
    /// 6DoF translation mode: 0 = Default, 1 = Off-axis window.
    #[serde(default)]
    sim6dof_mode: u32,
    #[serde(default = "default_one")]
    offaxis_window_depth: f32,
    #[serde(default = "default_one")]
    offaxis_parallax: f32,
    #[serde(default = "default_one")]
    offaxis_edge_falloff: f32,
    #[serde(default = "default_one")]
    offaxis_vertical_balance: f32,

    // ── Depth Layers (VERSION 62) — radial multi-zone parallax ──
    /// Master toggle. When on, reveals the sliders below.
    #[serde(default)]
    dlayers_enabled: bool,
    /// Master parallax strength (0..3).
    #[serde(default = "default_one")]
    dlayers_strength: f32,
    /// Motion reactivity (Lighter): when on, the layer warp pops while the head
    /// moves and settles at rest. Off by default.
    #[serde(default)]
    dlayers_reactive_on: bool,
    /// Strength of the motion-reactive boost (0..2).
    #[serde(default = "default_dlayers_reactive_amt")]
    dlayers_reactive_amt: f32,
    /// Per-zone separation centre->rim (0..1). 0 = uniform, 1 = max differential.
    #[serde(default = "default_one")]
    dlayers_separation: f32,
    /// Follow-through delay (0..1). Centre stays crisp; rim trails.
    #[serde(default = "default_dlayers_delay")]
    dlayers_delay: f32,
    /// Invert delay direction. Off = inner/centre leads (delay ripples outward,
    /// inner is "closest"). On = outer/rim leads (ripples inward, outer closest).
    #[serde(default)]
    dlayers_invert: bool,
    #[serde(default)]
    dlayers_ground: f32,
    #[serde(default = "default_half")]
    dlayers_horizon: f32,
    #[serde(default)]
    dlayers_vp: f32,
    /// Radial profile curve (0.25..3).
    #[serde(default = "default_one")]
    dlayers_curve: f32,
    /// Lean in/out (zoom) deepening (0..2).
    #[serde(default = "default_one")]
    dlayers_zoom: f32,
    /// Convex dome on the zoom area (0..1). Bulges the centre like a lens;
    /// intensifies as you lean in to zoom.
    #[serde(default)]
    dlayers_convex: f32,
    /// Depth-layer band mode: 0=concentric, 1=horizontal bands, 2=vertical columns.
    #[serde(default)]
    dlayers_mode: u32,
    /// Depth reach / rim-fade start (0.05..1). Lower = depth near centre, eased
    /// out early (seam-free); higher = reaches further toward the rim.
    #[serde(default = "default_dlayers_edge")]
    dlayers_edge: f32,

    // ── Directional 6DoF (VERSION 63) — head rotation -> parallax ──
    #[serde(default)]
    dir6dof_enabled: bool,
    #[serde(default = "default_dir6dof_gain")]
    dir6dof_yaw: f32,
    #[serde(default = "default_dir6dof_gain")]
    dir6dof_pitch: f32,
    #[serde(default = "default_dir6dof_gain")]
    dir6dof_roll: f32,
    // ── Hybrid Immersion (VERSION 65) — even-ramp rim-stretch + rear-360 ──
    // All #[serde(default ...)] so older preset JSONs load cleanly.
    #[serde(default)]
    hybrid_enabled: bool,
    #[serde(default = "default_hybrid_center")]
    hybrid_center: f32,
    #[serde(default = "default_hybrid_fov_gain")]
    hybrid_fov_gain: f32,
    #[serde(default = "default_hybrid_ramp")]
    hybrid_ramp: f32,
    #[serde(default = "default_hybrid_softness")]
    hybrid_softness: f32,
    #[serde(default = "default_true")]
    hybrid_rear_enabled: bool,
    #[serde(default)]
    hybrid_rear_stretch: f32,
    #[serde(default)]
    hybrid_rear_direction: f32,
    #[serde(default)]
    hybrid_rear_dim: f32,
    #[serde(default)]
    hybrid_motion_fade: f32,
    #[serde(default = "default_hybrid_fov_gain")]
    hybrid_fov_gain_v: f32,
    #[serde(default = "default_hybrid_center")]
    hybrid_center_v: f32,
    #[serde(default = "default_hybrid_stretch_dir")]
    hybrid_stretch_dir: f32,
    #[serde(default = "default_hybrid_stretch_reach")]
    hybrid_stretch_reach: f32,
    #[serde(default = "default_true")]
    vr_hotkeys_enabled: bool,
    offset_x: f32,
    offset_y: f32,
    offset_z: f32,
    offset_roll: f32,
    config_file: Option<String>,
    /// Persistent global-hotkey assignments. Empty by default — user
    /// binds keys explicitly in the Hotkeys section of the GUI.
    #[serde(default)]
    hotkey_bindings: hotkeys::HotkeyBindings,
    /// Delivery method for global hotkeys: 0 = Default
    /// (RegisterHotKey + standard Windows path), 1 = Low-level
    /// hook (WH_KEYBOARD_LL — observes keys in the OS pipeline so
    /// hotkeys fire in many games that block normal hooks).
    /// 0 by default.
    #[serde(default)]
    hotkey_delivery_method: u32,

    // 0.6.0 head-tracking output features. Defaulted via #[serde(default)]
    // so older preset JSONs load cleanly.
    #[serde(default)]
    mouse_emu_enabled: bool,
    #[serde(default = "default_one")]
    mouse_emu_sensitivity: f32,
    #[serde(default = "default_one")]
    mouse_emu_speed: f32,
    /// Mouse emulation compatibility mode index. 0 = Relative SendInput
    /// only, 1 = Absolute SetCursorPos only, 2 = Both (default).
    #[serde(default = "default_compat")]
    mouse_emu_compat: u32,
    #[serde(default)]
    joy_emu_enabled: bool,
    #[serde(default)]
    joy_emu_mode: u32,
    #[serde(default = "default_joy_sensitivity")]
    joy_emu_sensitivity: f32,
    #[serde(default = "default_joy_deadzone")]
    joy_emu_deadzone: f32,
    #[serde(default = "default_joy_max_angle")]
    joy_emu_max_angle: f32,
    #[serde(default)]
    joy_emu_invert_x: bool,
    #[serde(default)]
    joy_emu_invert_y: bool,
    #[serde(default)]
    joy_emu_smoothness: f32,
    #[serde(default = "default_joy_speed")]
    joy_emu_speed_x: f32,
    #[serde(default = "default_joy_speed")]
    joy_emu_speed_y: f32,
    #[serde(default)]
    overlay_enabled: bool,
    #[serde(default)]
    panel_cursor_force: bool,
    #[serde(default)]
    panel_cursor_method: u32,
    panel_theme: u32,
    #[serde(default = "default_overlay_size")]
    overlay_size: f32,
    #[serde(default = "default_overlay_axis")]
    overlay_size_x: f32,
    #[serde(default = "default_overlay_axis")]
    overlay_size_y: f32,
    #[serde(default)]
    overlay_offset_x: f32,
    #[serde(default)]
    overlay_offset_y: f32,
    #[serde(default = "default_overlay_distance")]
    overlay_distance: f32,
    #[serde(default)]
    overlay_hud_mode: bool,
    #[serde(default = "default_overlay_res")]
    overlay_aspect: u32,
    #[serde(default = "default_overlay_transparency")]
    overlay_transparency: f32,
    /// When the overlay hotkey turns the overlay ON, also bring the GUI window
    /// to the foreground (so it appears inside the desktop-mirror overlay), and
    /// hand focus back to the game when the overlay is toggled OFF. This is
    /// GUI-process behaviour only — it is NOT sent to the viewer / wire format.
    #[serde(default)]
    overlay_show_gui: bool,
    /// GUI THEME settings (GUI-process only — not sent to the viewer/wire).
    /// Custom banner/logo image file paths (empty = use the bundled assets).
    #[serde(default)]
    custom_banner_path: String,
    #[serde(default)]
    custom_logo_path: String,
    /// false = original colored section theme; true = dark-blue headers/frames.
    /// Retained for back-compat with presets saved before `gui_theme_id`; on
    /// load, a stored `true` here promotes `gui_theme_id` to Dark Blue.
    #[serde(default)]
    gui_dark_theme: bool,
    /// Selected GUI theme: 0=Colored (default), 1=Dark Blue, 2=Black, 3=Red,
    /// 4=Cyan. Source of truth for the theme dropdown.
    #[serde(default)]
    gui_theme_id: u32,
    /// Optional background image painted behind EACH section's body (empty =
    /// none). When set, section fills go translucent so the image shows through.
    #[serde(default)]
    section_bg_path: String,
    /// Optional background image painted behind ALL sections, filling the whole
    /// panel (empty = none). Also makes section fills translucent so it shows.
    #[serde(default)]
    overall_bg_path: String,
    #[serde(default)]
    udp_6dof_enabled: bool,
    #[serde(default)]
    trackir_enabled: bool,
    #[serde(default)] trackir_flip_x: bool,
    #[serde(default)] trackir_flip_y: bool,
    #[serde(default)] trackir_flip_z: bool,
    #[serde(default)] trackir_flip_yaw: bool,
    #[serde(default)] trackir_flip_pitch: bool,
    #[serde(default)] trackir_flip_roll: bool,
    #[serde(default = "default_trackir_gain_z")] trackir_gain_z: f32,
    #[serde(default = "default_udp_port")]
    udp_6dof_port: u32,
    #[serde(default = "default_udp_ip")]
    udp_6dof_ip: String,
    #[serde(default)]
    udp_flip_x: bool,
    #[serde(default)]
    udp_flip_y: bool,
    #[serde(default)]
    udp_flip_z: bool,
    #[serde(default)]
    udp_flip_yaw: bool,
    #[serde(default)]
    udp_flip_pitch: bool,
    #[serde(default)]
    udp_flip_roll: bool,
    #[serde(default = "default_one")]
    udp_gain_yaw: f32,
    #[serde(default = "default_one")]
    udp_gain_pitch: f32,
    #[serde(default = "default_one")]
    udp_gain_roll: f32,
    #[serde(default = "default_one")]
    udp_gain_x: f32,
    #[serde(default = "default_one")]
    udp_gain_y: f32,
    #[serde(default = "default_one")]
    udp_gain_z: f32,

    // ─── VR Data to UDP (Stage 3) ──────────────────────────────
    #[serde(default)]
    vr_udp_enabled: bool,
    /// 0 = head only, 1 = controllers only, 2 = both.
    #[serde(default)]
    vr_udp_mode: u32,
    #[serde(default = "default_vr_udp_port")]
    vr_udp_port: u32,
    #[serde(default = "default_udp_ip")]
    vr_udp_ip: String,
    #[serde(default)]
    vr_udp_flip_x: bool,
    #[serde(default)]
    vr_udp_flip_y: bool,
    #[serde(default)]
    vr_udp_flip_z: bool,
    #[serde(default)]
    vr_udp_flip_yaw: bool,
    #[serde(default)]
    vr_udp_flip_pitch: bool,
    #[serde(default)]
    vr_udp_flip_roll: bool,
    #[serde(default = "default_one")]
    vr_udp_gain_yaw: f32,
    #[serde(default = "default_one")]
    vr_udp_gain_pitch: f32,
    #[serde(default = "default_one")]
    vr_udp_gain_roll: f32,
    #[serde(default = "default_one")]
    vr_udp_gain_x: f32,
    #[serde(default = "default_one")]
    vr_udp_gain_y: f32,
    #[serde(default = "default_one")]
    vr_udp_gain_z: f32,
    #[serde(default)]
    vr_udp_left_enabled: bool,
    #[serde(default)]
    vr_udp_right_enabled: bool,
    // Collapsible-group open/closed state (GUI-only; not in wire/uniform).
    // serde(default) so older presets load fine (all closed).
    #[serde(default)]
    grp_dyndepth_open: bool,
    #[serde(default)]
    grp_dlayers_open: bool,
    #[serde(default)]
    grp_concave_open: bool,
    #[serde(default)]
    grp_dir6dof_open: bool,
    #[serde(default)]
    grp_ipd_open: bool,
    /// Collapsed/open state of the "Experimental Features" section.
    #[serde(default)]
    grp_experimental_open: bool,
    // Collapsible section open/closed state (GUI-only). These ALWAYS default
    // CLOSED and never persist: skipped from preset (de)serialization, and egui
    // memory persistence is disabled (persist_egui_memory = false), so opening a
    // section never survives an app exit or gets baked into a saved preset.
    #[serde(skip, default = "collapsed_closed")]
    auto_adjust_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    gui_theme_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    hybrid_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    dejitter_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    stable_lock_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    curvature_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    parallax_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    sixdof_collapsed: bool,
    #[serde(skip, default = "collapsed_closed")]
    vr_udp_collapsed: bool,
    /// When true (default), saving over `default.json` also pushes the new
    /// values live to the viewer. The viewer's file-watcher reload of
    /// default.json can momentarily freeze the render loop on some setups, so
    /// this toggle lets the user save WITHOUT triggering that live reload.
    #[serde(default = "default_true")]
    reload_preset_after_save: bool,
}

fn default_vr_udp_port() -> u32 { 4243 }

fn default_one() -> f32 { 1.0 }
fn default_half() -> f32 { 0.5 }
/// Collapsible sections always default CLOSED and never persist their open
/// state (not in a preset, not across app exit). See persist_egui_memory.
fn collapsed_closed() -> bool { true }
fn default_07() -> f32 { 0.7 }
fn default_udp_port() -> u32 { 4242 }
fn default_udp_ip() -> String { "127.0.0.1".to_string() }
fn default_compat() -> u32 { 2 }
fn default_joy_sensitivity() -> f32 { 2.0 }
fn default_joy_deadzone() -> f32 { 0.015 }
fn default_joy_max_angle() -> f32 { 84.0 }
fn default_joy_speed() -> f32 { 1.0 }
fn default_overlay_size() -> f32 { 2.0 }
fn default_overlay_axis() -> f32 { 1.0 }
fn default_overlay_distance() -> f32 { 2.0 }
fn default_overlay_res() -> u32 { 0 }
fn default_overlay_transparency() -> f32 { 1.0 }
fn default_joy_sensitivity_rd() -> f32 { 0.3 }
fn default_concave_depth() -> f32 { 0.5 }
fn default_concave_shape() -> f32 { 0.5 }
fn default_concave_z() -> f32 { 1.0 }
fn default_true() -> bool { true }

fn serde_true() -> bool { true }
fn default_sphere_y_169() -> f32 { 0.50 }
fn default_sphere_y_43()  -> f32 { 0.60 }
fn default_sphere_y_219() -> f32 { 0.35 }
fn default_temporal_alpha() -> f32 { 0.8 }
fn default_flow_strength() -> f32 { 0.7 }
fn default_lowfps_predict_strength() -> f32 { 0.5 }
fn default_stable_lock_parallax() -> f32 { 0.5 }
fn default_dejitter_stiffness() -> f32 { 0.4 }
fn default_dejitter_max_lag() -> f32 { 8.0 }
fn default_parallax_prediction_amt() -> f32 { 0.5 }
fn default_pp_deadband_deg() -> f32 { 0.3 }
fn default_sl_dir_strength() -> f32 { 0.75 }
fn default_dlayers_delay() -> f32 { 0.2 }
fn default_trackir_gain_z() -> f32 { 1.0 }
fn default_dlayers_reactive_amt() -> f32 { 0.5 }
fn default_dlayers_edge() -> f32 { 0.5 }
fn default_dir6dof_gain() -> f32 { 0.5 }
fn default_hybrid_center() -> f32 { 0.40 }
fn default_hybrid_fov_gain() -> f32 { 1.35 }
fn default_hybrid_ramp() -> f32 { 1.0 }
fn default_hybrid_softness() -> f32 { 0.5 }
fn default_hybrid_stretch_dir() -> f32 { 0.0 }
fn default_hybrid_stretch_reach() -> f32 { 0.5 }

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            stereo_mode: StereoModeIndex::Mono as u32,
            xr_backend: XrBackendIndex::OpenXR as u32,
            push_toggles: true,
            diag_mode: false,
            pose_predict_ms: 0.0,
            pose_smooth_alpha: 0.75, // data-validated default: 6% better than 0.5 on Pimax Crystal
            pimax_flat_depth: false,
            lowfps_predict_boost: false,
            lowfps_predict_strength: 0.5,
            enhancement_quality: 0.0,
            vsync_mode: 0,
            fps_limit: 0.0,
            frame_pacing_enabled: true,
            frame_pacing_target: 0.45,
            temporal_blend_enabled: false,
            temporal_blend_alpha: 0.8,
            flow_enabled: false,
            flow_strength: 0.7,
            submit_render_pose: false,
            stable_eye_submit: false,
            hold_full_refresh: false,
            rcas_sharpness: 0.0,
            repeat_stretch: 0.0,
            repeat_size: 0.0,
            auto_z_enabled: false,
            auto_z_value: 0.0,
            auto_roll_enabled: false,
            auto_roll_value: 0.0,
            auto_x_enabled: false,
            auto_x_value: 0.0,
            auto_y_enabled: false,
            auto_y_value: 0.0,
            auto_height_enabled: false,
            sphere_y_169: 0.50,
            sphere_y_43:  0.60,
            sphere_y_219: 0.35,
            repeat_blend: 0.3,
            repeat_depth: 0.30,
            x_curvature: -0.5,
            y_curvature: -0.1,
            swap_eyes: true,
            flip_x: false,
            flip_y: false,
            distance: 20.0,
            scale: 300.0,
            ambient: false,
            head_lock: false,
            edge_stretch: 4.3,
            edge_stretch_softness: 0.0,
            edge_expand: 0.0,
            sphere_x_size: 1.0,
            sphere_y_size: 0.5,
            sphere_x_curve: 1.0,
            sphere_y_curve: 1.0,
            // Box defaults shrunk in 0.6.0 to ~45m half-edge at default
            // scale=300 (otherwise box mode swallows the user). See
            // AppConfig::default for the rationale.
            box_x_size: 0.3,
            box_y_size: 0.3,
            box_z_depth: 0.3,
            box_corner_radius: 0.0,
            mesh_ext_x_size: 1.0,
            mesh_ext_y_size: 1.0,
            mesh_ext_z_depth: 2.0,
            // 0.6.0: Sphere is the new default screen shape.
            stretch_mode: 0,
            supersampling: 1.35,
            katanga_filters_enabled: false,
            katanga_sharpness: 0.0,
            katanga_texture_sharpness: 0.0,
            katanga_saturation: 1.0,
            katanga_contrast: 1.0,
            katanga_brightness: 0.0,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.15,
            sharpness: 1.20,
            cas: 0.0,
            dehaze: 0.0,
            katanga_cas: 0.0,
            katanga_dehaze: 0.0,
            // 0.6.0 additions
            texture_sharpen: 0.0,
            filter_bilinear: 1.0,
            filter_trilinear: 0.0,
            edge_stretch_extend: 0.0,
            edge_expand_extend: 0.0,
            show_mirror_method: false,
            show_repeated_method: false,
            show_expansion_method: false,
            show_extrusion_method: false,
            sim6dof_enabled: false,
            sim6dof_intensity: 1.0,
            sim6dof_smoothness: 0.85,
            sim6dof_zoom_intensity: 1.0,
            ipd_perspective: 1.0,
            separation: 1.0,
            convergence: 0.0,
            dyn_popout: 1.0,
            dyn_depthscale: 1.0,
            dyn_looming: 0.0,
            sim6dof_dynamic_depth: false,
            sim6dof_spring: false,
            katanga_perf_mode: false,
            expansion_outer: 0.0,
            expansion_seamless: 0.0,
            extrusion_strength: 0.0,
            extrusion_direction: 1.0,
            concave_strength: 0.0,
            concave_depth: 0.5,
            concave_shape: 0.5,
            concave_z0: 1.0, concave_z1: 1.0, concave_z2: 1.0,
            concave_z3: 1.0, concave_z4: 1.0,
            concave_dz0: 0.0, concave_dz1: 0.0, concave_dz2: 0.0,
            concave_dz3: 0.0, concave_dz4: 0.0,
            mirror_falloff: 0.0,
            extend_pull_blend: 0.0,
            expansion_smoothness: 0.0,
            filter_bicubic: 0.0,
            filter_lanczos: 0.0,
            headlock_jitter_deadzone: 0.0,
            headlock_jitter_smooth: 0.0,
            stable_lock_parallax_xy: 0.5,
            stable_lock_parallax_z: 0.5,
            stable_lock_dir_enabled: true,
            stable_lock_dir_strength: 0.75,
            grp_dyndepth_open: false,
            grp_dlayers_open: false,
            grp_concave_open: false,
            grp_dir6dof_open: false,
            grp_ipd_open: false,
            grp_experimental_open: false,
            auto_adjust_collapsed: true,
            gui_theme_collapsed: true,
            hybrid_collapsed: true,
            dejitter_collapsed: true,
            stable_lock_collapsed: true,
            curvature_collapsed: true,
            parallax_collapsed: true,
            sixdof_collapsed: true,
            vr_udp_collapsed: true,
            reload_preset_after_save: true,
            headlock_jitter_method: 0,
            headlock_dejitter: false,
            parallax_prediction: false,
            parallax_prediction_amt: 0.5,
            pp_adaptive: false,
            pp_deadband_deg: 0.3,
            pp_accel: 0.0,
            pp_runtime_vel: false,
            pp_photon_horizon: false,
            pp_euro: false,
            headlock_dejitter_stiffness: 0.4,
            headlock_dejitter_max_lag: 8.0,
            headlock_delay_ms: 80.0,
            sim6dof_zoom_only: false,
            sim6dof_mode: 0,
            offaxis_window_depth: 1.0,
            offaxis_parallax: 1.0,
            offaxis_edge_falloff: 1.0,
            offaxis_vertical_balance: 1.0,
            dlayers_enabled: false,
            dlayers_strength: 1.0,
            dlayers_reactive_on: false,
            dlayers_reactive_amt: 0.5,
            dlayers_separation: 1.0,
            dlayers_delay: 0.2,
            dlayers_invert: false,
            dlayers_ground: 0.0,
            dlayers_horizon: 0.5,
            dlayers_vp: 0.0,
            dlayers_curve: 1.0,
            dlayers_zoom: 1.0,
            dlayers_convex: 0.0,
            dlayers_mode: 0,
            dlayers_edge: 0.5,
            dir6dof_enabled: false,
            dir6dof_yaw: 0.5,
            dir6dof_pitch: 0.5,
            dir6dof_roll: 0.5,
            hybrid_enabled: false,
            hybrid_center: 0.40,
            hybrid_fov_gain: 1.35,
            hybrid_ramp: 1.0,
            hybrid_softness: 0.5,
            hybrid_rear_enabled: true,
            hybrid_rear_stretch: 0.0,
            hybrid_rear_direction: 0.0,
            hybrid_rear_dim: 0.0,
            hybrid_motion_fade: 0.0,
            hybrid_fov_gain_v: 1.35,
            hybrid_center_v: 0.40,
            hybrid_stretch_dir: 0.0,
            hybrid_stretch_reach: 0.5,
            vr_hotkeys_enabled: true,
            offset_x: 0.0,
            offset_y: 0.0,
            offset_z: 0.0,
            offset_roll: 0.0,
            config_file: None,
            hotkey_bindings: hotkeys::HotkeyBindings::default(),
            hotkey_delivery_method: 0,
            mouse_emu_enabled: false,
            mouse_emu_sensitivity: 1.0,
            mouse_emu_speed: 1.0,
            mouse_emu_compat: 2,
            joy_emu_enabled: false,
            joy_emu_mode: 0,
            joy_emu_sensitivity: 2.0,
            joy_emu_deadzone: 0.015,
            joy_emu_max_angle: 84.0,
            joy_emu_invert_x: false,
            joy_emu_invert_y: false,
            joy_emu_smoothness: 0.0,
            joy_emu_speed_x: 1.0,
            joy_emu_speed_y: 1.0,
            overlay_enabled: false,
            panel_cursor_force: false,
            panel_cursor_method: 0,
            panel_theme: 0,
            overlay_size: 2.0,
            overlay_size_x: 1.0,
            overlay_size_y: 1.0,
            overlay_offset_x: 0.0,
            overlay_offset_y: 0.0,
            overlay_distance: 2.0,
            overlay_hud_mode: false,
            overlay_aspect: 0,
            overlay_transparency: 1.0,
            overlay_show_gui: false,
            custom_banner_path: String::new(),
            custom_logo_path: String::new(),
            gui_dark_theme: false,
            gui_theme_id: 0,
            section_bg_path: String::new(),
            overall_bg_path: String::new(),
            udp_6dof_enabled: false,
            trackir_enabled: false,
            trackir_flip_x: false,
            trackir_flip_y: false,
            trackir_flip_z: false,
            trackir_flip_yaw: false,
            trackir_flip_pitch: false,
            trackir_flip_roll: false,
            trackir_gain_z: 1.0,
            udp_6dof_port: 4242,
            udp_6dof_ip: "127.0.0.1".to_string(),
            udp_flip_x: false,
            udp_flip_y: false,
            udp_flip_z: false,
            udp_flip_yaw: false,
            udp_flip_pitch: false,
            udp_flip_roll: false,
            udp_gain_yaw: 1.0,
            udp_gain_pitch: 1.0,
            udp_gain_roll: 1.0,
            udp_gain_x: 1.0,
            udp_gain_y: 1.0,
            udp_gain_z: 1.0,
            vr_udp_enabled: false,
            vr_udp_mode: 0,
            vr_udp_port: 4243,
            vr_udp_ip: "127.0.0.1".to_string(),
            vr_udp_flip_x: false,
            vr_udp_flip_y: false,
            vr_udp_flip_z: false,
            vr_udp_flip_yaw: false,
            vr_udp_flip_pitch: false,
            vr_udp_flip_roll: false,
            vr_udp_gain_yaw: 1.0,
            vr_udp_gain_pitch: 1.0,
            vr_udp_gain_roll: 1.0,
            vr_udp_gain_x: 1.0,
            vr_udp_gain_y: 1.0,
            vr_udp_gain_z: 1.0,
            vr_udp_left_enabled: false,
            vr_udp_right_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PresetEnvelope {
    #[serde(default = "version_one")]
    version: u32,
    name: String,
    config: GuiConfig,
}

fn version_one() -> u32 {
    1
}

fn presets_dir_for(viewer_exe: &Path) -> PathBuf {
    viewer_exe
        .parent()
        .map(|p| p.join("presets"))
        .unwrap_or_else(|| PathBuf::from("presets"))
}

fn find_viewer_exe() -> PathBuf {
    let gui = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("osiris-gui.exe"));
    let parent = gui.parent().unwrap_or_else(|| Path::new("."));
    parent.join("osiris-vr-viewer.exe")
}

// --- App state ------------------------------------------------------------

struct OsirisGui {
    cfg: GuiConfig,
    preset_name: String,
    available_presets: Vec<String>,
    /// Index into available_presets for the CyclePreset hotkey.
    preset_cycle_idx: usize,
    viewer_exe: PathBuf,
    viewer_child: Option<Child>,
    /// PID of the spawned viewer, stored separately so we can call
    /// OpenProcess(PROCESS_TERMINATE, pid) at shutdown time. This works
    /// even when the child handle was created without PROCESS_TERMINATE
    /// rights (e.g. after UAC elevation changes the token integrity).
    viewer_pid: Option<u32>,
    writer: Arc<Mutex<Option<shm::LiveParamsWriter>>>,
    /// Shadow of `cfg` shared with the hotkey background thread. The
    /// hotkey thread mutates this when a hotkey fires (and writes SHM
    /// directly), so changes apply even when the GUI window is
    /// minimized. The main GUI thread syncs its `self.cfg` from this
    /// shadow on each `update()` to reflect background changes.
    shared_cfg: Arc<Mutex<GuiConfig>>,
    status: String,
    /// Instant when status was last set — used for a 2-second bright
    /// flash so hotkey feedback is clearly visible.
    status_updated: std::time::Instant,
    logo: Option<TextureHandle>,
    /// Banner image rendered as the title-bar background. Loaded once
    /// at startup; cheap to re-paint each frame as a textured rect.
    banner: Option<TextureHandle>,
    /// Optional per-section and overall background textures (None = unset).
    section_bg: Option<TextureHandle>,
    overall_bg: Option<TextureHandle>,
    /// Set true after the first frame applies any saved custom banner/logo
    /// (config is loaded from disk after `new()`, so we apply on first update).
    theme_assets_applied: bool,
    /// Set to true after we've pushed a quit signal to SHM so the
    /// quit handler doesn't keep firing every frame while the close
    /// is in flight.
    quit_pushed: bool,
    /// Global hotkey manager. Keys it tracks fire even when this
    /// window is minimized or another app has focus.
    hotkey_mgr: hotkeys::HotkeyManager,
    /// When the user clicks an "assign key" cell in the Hotkeys
    /// section, this stores which action is awaiting a keypress.
    /// Next key event captured by egui assigns and clears.
    capturing: Option<hotkeys::HotkeyAction>,
    /// Stage 4b: low-level hook worker stop-flag. Set to `true`
    /// to signal the worker to exit. Re-created when restarting.
    upstream_reader: Option<shm::UpstreamReader>,
    hook_stop: Option<Arc<AtomicBool>>,
    /// Shared bindings the hook worker reads each event.
    /// Updated by the GUI thread when user re-binds.
    hook_bindings: Arc<Mutex<hotkeys::HotkeyBindings>>,
    /// Last-seen delivery method so we can detect toggles.
    last_delivery_method: u32,
    /// Set by hotkey threads to signal main thread to do a full restart.
    hotkey_restart_flag: Arc<std::sync::atomic::AtomicBool>,
    /// Clone of the egui context, handed to the background hotkey threads so a
    /// fired Restart hotkey can `request_repaint()` and wake the UI loop
    /// immediately. Without this, the restart flag was only polled on the next
    /// natural egui repaint — which never comes while the window is minimized in
    /// VR — so the hotkey appeared to need a second press and felt slow vs. the
    /// in-window Restart button (a click is itself a repaint event).
    egui_ctx: egui::Context,
    /// Last-seen display scale factor (pixels_per_point). Used to detect a
    /// monitor resolution / DPI change at runtime, which otherwise corrupts the
    /// persisted window size and collapses the panel to a tiny square. None
    /// until the first frame establishes a baseline.
    last_scale_factor: Option<f32>,
    /// Frames to re-assert the default window size after a detected scale change
    /// (eframe can overwrite an immediate resize, so we hold the command for a
    /// few frames to win the race). 0 = idle.
    window_fix_frames: u32,
}

impl OsirisGui {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);

        // Decode the bundled logo. We try a few strategies because
        // `image`'s ICO decoder is finicky about non-standard ICO
        // payloads (in particular, single-entry 32-bpp BMP-inside-ICO
        // with negative-height bitmap headers like our 192×90 logo).
        // If everything fails we log loudly and carry on without a
        // logo — the GUI is still usable, just without the brand mark.
        let logo = load_logo_texture(&cc.egui_ctx);
        if logo.is_none() {
            log::warn!(
                "Logo did not load — see preceding errors. \
                 Title bar will render without an icon."
            );
        }
        let banner = load_banner_texture(&cc.egui_ctx);
        if banner.is_none() {
            log::warn!("Banner did not load — title bar will fall back to plain panel.");
        }

        let viewer_exe = find_viewer_exe();
        let writer = Arc::new(Mutex::new(match shm::LiveParamsWriter::new() {
            Ok(w) => Some(w),
            Err(err) => {
                log::warn!("Could not open live-params shared memory: {}", err);
                None
            }
        }));

        let shared_cfg = Arc::new(Mutex::new(GuiConfig::default()));

        let mut gui = Self {
            cfg: GuiConfig::default(),
            preset_name: "default".into(),
            available_presets: Vec::new(),
            preset_cycle_idx: 0,
            hotkey_restart_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            egui_ctx: cc.egui_ctx.clone(),
            last_scale_factor: None,
            window_fix_frames: 0,
            viewer_exe,
            viewer_child: None,
            viewer_pid: None,
            writer,
            shared_cfg,
            status: "Ready.".into(),
            status_updated: std::time::Instant::now(),
            logo,
            banner,
            section_bg: None,
            overall_bg: None,
            theme_assets_applied: false,
            quit_pushed: false,
            hotkey_mgr: hotkeys::HotkeyManager::new(),
            capturing: None,
            upstream_reader: shm::UpstreamReader::new().ok(),
            hook_stop: None,
            hook_bindings: Arc::new(Mutex::new(hotkeys::HotkeyBindings::default())),
            last_delivery_method: 0,
        };

        gui.refresh_preset_list();
        if let Err(err) = gui.load_preset("default") {
            log::info!("No default preset to pre-populate from: {}", err);
        }
        // After loading default preset, register any persisted hotkeys.
        gui.hotkey_mgr.sync(&gui.cfg.hotkey_bindings);
        // Sync shadow with loaded config so the hotkey thread starts
        // with the right values.
        if let Ok(mut shadow) = gui.shared_cfg.lock() {
            *shadow = gui.cfg.clone();
        }
        gui.push_to_shm(true);

        // Spawn the hotkey background thread. It owns clones of both
        // shared Arcs (writer + shadow config) and processes hotkey
        // events independently of the main UI thread, so hotkeys
        // continue working when the GUI window is minimized.
        spawn_hotkey_worker(
            gui.writer.clone(),
            gui.shared_cfg.clone(),
            gui.hotkey_mgr.id_to_action.clone(),
            gui.hotkey_restart_flag.clone(),
            gui.egui_ctx.clone(),
            gui.viewer_exe
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        );

        // Auto-launch the viewer alongside the GUI on startup. This makes
        // launching the GUI (or the viewer) a one-click experience — both
        // come up together regardless of which exe the user ran first.
        //
        // Recursion guard: if this GUI was itself launched by the viewer,
        // it sets `OSIRIS_GUI_PEER=1` on us, and we skip the spawn so we
        // don't end up with two viewers.
        if std::env::var_os("OSIRIS_GUI_PEER").is_some() {
            log::info!("Skipping viewer spawn: this GUI was launched by the viewer.");
        } else {
            gui.try_launch_viewer_silent();
        }

        gui
    }

    fn presets_dir(&self) -> PathBuf {
        presets_dir_for(&self.viewer_exe)
    }

    fn refresh_preset_list(&mut self) {
        let dir = self.presets_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                if e.path().extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        self.available_presets = names;
    }

    fn save_preset(&mut self, name: &str) -> anyhow::Result<PathBuf> {
        let dir = self.presets_dir();
        std::fs::create_dir_all(&dir).context("create presets dir")?;
        let path = dir.join(format!("{}.json", sanitise(name)));
        let envelope = PresetEnvelope {
            version: 1,
            name: sanitise(name),
            config: self.cfg.clone(),
        };
        let json = serde_json::to_string_pretty(&envelope)?;
        // Atomic write (temp + rename): the viewer watches default.json, so a
        // direct truncate-then-write could expose a partial file and fire
        // multiple watcher events. Rename is atomic and emits a single event.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("finalize {}", path.display()))?;
        self.status = format!("Saved preset to {}", path.display()); self.status_updated = std::time::Instant::now();
        Ok(path)
    }

    fn load_preset(&mut self, name: &str) -> anyhow::Result<()> {
        let dir = self.presets_dir();
        let path = dir.join(format!("{}.json", sanitise(name)));
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        if let Ok(env) = serde_json::from_slice::<PresetEnvelope>(&bytes) {
            self.cfg = env.config;
        } else {
            self.cfg = serde_json::from_slice::<GuiConfig>(&bytes)?;
        }
        // "Show GUI with overlay" was removed from the UI; force it off so a
        // stale `true` in an old preset can't keep the hidden behaviour alive.
        self.cfg.overlay_show_gui = false;
        // A loaded preset may carry different custom banner/logo paths (or none),
        // so re-apply the theme assets on the next frame.
        self.theme_assets_applied = false;
        self.status = format!("Loaded preset {}", path.display()); self.status_updated = std::time::Instant::now();
        self.push_to_shm(true);
        Ok(())
    }

    /// Delete a preset file. Currently unused — the GUI's delete
    /// button was removed in 0.6.0 to prevent accidental loss of
    /// saved configurations. Kept here so a future build can re-
    /// expose it (e.g. behind a confirmation dialog).
    #[allow(dead_code)]
    fn delete_preset(&mut self, name: &str) -> anyhow::Result<()> {
        let dir = self.presets_dir();
        let path = dir.join(format!("{}.json", sanitise(name)));
        if path.exists() {
            std::fs::remove_file(&path)?;
            self.status = format!("Deleted {}", path.display()); self.status_updated = std::time::Instant::now();
        }
        Ok(())
    }

    /// Launch the viewer process, marking it as a "spawned by GUI"
    /// peer so it skips its own auto-spawn-the-GUI step. Logs
    /// failures rather than writing them to the status bar — used
    /// both at GUI startup (where the user hasn't asked for anything
    /// explicitly) and after Restart.
    fn try_launch_viewer_silent(&mut self) {
        if self.is_viewer_running() {
            return;
        }
        if !self.viewer_exe.exists() {
            log::info!(
                "Auto-launch skipped: viewer not found at {}",
                self.viewer_exe.display()
            );
            return;
        }
        match std::process::Command::new(&self.viewer_exe)
            .env("OSIRIS_VIEWER_PEER", "1")
            .spawn()
        {
            Ok(child) => {
                log::info!(
                    "Auto-launched viewer at {} (pid {})",
                    self.viewer_exe.display(),
                    child.id()
                );
                self.viewer_pid = Some(child.id());
                self.viewer_child = Some(child);
            }
            Err(err) => {
                log::warn!(
                    "Auto-launch failed for {}: {}",
                    self.viewer_exe.display(),
                    err
                );
            }
        }
    }

    /// True if our previously-spawned viewer child is still alive.
    fn is_viewer_running(&mut self) -> bool {
        if let Some(child) = self.viewer_child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => false, // exited
                Ok(None) => true,     // still running
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Fill in the standard live-params payload that all `push_*`
    /// functions share. Keeping this in one place means a new field
    /// only needs to be wired up here, not across each of the four
    /// callers (full push, quit, screenshot, recenter).
    ///
    /// Takes `&GuiConfig` rather than `&self` so callers can hold a
    /// mutable borrow on `self.writer` simultaneously — `&self.cfg`
    /// and `&mut self.writer` are disjoint splits the compiler can
    /// see through, but `&self` paired with `&mut self.writer`
    /// trips E0502 because they alias the same struct.
    pub fn populate_live_params(cfg: &GuiConfig, p: &mut osiris_shared::LiveParams) {
        p.stereo_mode = cfg.stereo_mode;
        p.xr_backend = cfg.xr_backend;
        p.override_toggles = if cfg.push_toggles { 1 } else { 0 };
        p.distance = cfg.distance;
        p.scale = cfg.scale;
        p.x_curvature = cfg.x_curvature;
        p.y_curvature = cfg.y_curvature;
        p.offset_x = cfg.offset_x;
        p.offset_y = cfg.offset_y;
        p.offset_z = cfg.offset_z;
        p.offset_roll = cfg.offset_roll;
        p.edge_stretch = if cfg.show_mirror_method { cfg.edge_stretch } else { 0.0 };
        p.edge_stretch_softness = cfg.edge_stretch_softness;
        p.edge_expand = if cfg.show_mirror_method { cfg.edge_expand } else { 0.0 };
        p.sphere_x_size = cfg.sphere_x_size;
        p.sphere_y_size = cfg.sphere_y_size;
        p.sphere_x_curve = cfg.sphere_x_curve;
        p.sphere_y_curve = cfg.sphere_y_curve;
        p.box_x_size = cfg.box_x_size;
        p.box_y_size = cfg.box_y_size;
        p.box_z_depth = cfg.box_z_depth;
        p.box_corner_radius = cfg.box_corner_radius;
        // (mesh_ext_* fields exist on GuiConfig for preset round-trip,
        //  but were never part of the SHM wire format and certainly
        //  aren't there in 0.6.0 since MeshExtension is gone, so
        //  there's nothing to write here.)
        // 0.6.0: silently upgrade legacy MeshExtension (1) to Sphere
        // (0). Old preset files may still contain `1`; we don't want
        // to round-trip that into the live wire format.
        p.stretch_mode = if cfg.stretch_mode == 1 {
            0
        } else {
            cfg.stretch_mode
        };
        p.supersampling = cfg.supersampling;
        p.katanga_filters_enabled = cfg.katanga_filters_enabled as u32;
        p.katanga_sharpness = cfg.katanga_sharpness;
        p.katanga_texture_sharpness = cfg.katanga_texture_sharpness;
        p.katanga_saturation = cfg.katanga_saturation;
        p.katanga_contrast = cfg.katanga_contrast;
        p.katanga_brightness = cfg.katanga_brightness;
        p.brightness = cfg.brightness;
        p.contrast = cfg.contrast;
        p.saturation = cfg.saturation;
        p.sharpness = cfg.sharpness;
        p.cas = cfg.cas;
        p.dehaze = cfg.dehaze;
        p.katanga_cas = cfg.katanga_cas;
        p.katanga_dehaze = cfg.katanga_dehaze;
        p.swap_eyes = cfg.swap_eyes as u32;
        p.flip_x = cfg.flip_x as u32;
        p.flip_y = cfg.flip_y as u32;
        p.head_lock = cfg.head_lock as u32;
        p.ambient = cfg.ambient as u32;

        // 0.6.0 additions
        p.texture_sharpen = cfg.texture_sharpen;
        p.filter_bilinear = cfg.filter_bilinear;
        p.filter_trilinear = 0.0; // deprecated - replaced by bicubic/lanczos
        p.edge_stretch_extend = if cfg.show_repeated_method { cfg.edge_stretch_extend } else { 0.0 };
        p.edge_expand_extend = if cfg.show_repeated_method { cfg.edge_expand_extend } else { 0.0 };
        p.sim_6dof_enabled = if cfg.sim6dof_enabled { 1 } else { 0 };
        p.sim_6dof_amount = cfg.sim6dof_intensity;
        p.sim_6dof_smoothness = cfg.sim6dof_smoothness;
        p.sim_6dof_zoom_amount = cfg.sim6dof_zoom_intensity;
        p.ipd_perspective = cfg.ipd_perspective;
        p.separation = cfg.separation;
        p.convergence = cfg.convergence;
        p.dyn_popout = cfg.dyn_popout;
        p.dyn_depthscale = cfg.dyn_depthscale;
        p.dyn_looming = cfg.dyn_looming;
        p.sim6dof_dynamic_depth = if cfg.sim6dof_dynamic_depth { 1 } else { 0 };
        p.sim6dof_spring = if cfg.sim6dof_spring { 1.0 } else { 0.0 };
        p.katanga_perf_mode = if cfg.katanga_perf_mode { 1 } else { 0 };
        let mesh_on = cfg.show_expansion_method || cfg.show_extrusion_method;
        p.expansion_outer = if mesh_on { cfg.expansion_outer } else { 0.0 };
        p.expansion_seamless = if mesh_on { cfg.expansion_seamless } else { 0.0 };
        p.extrusion_strength = if mesh_on { cfg.extrusion_strength } else { 0.0 };
        p.extrusion_direction = cfg.extrusion_direction;
        p.concave_strength = cfg.concave_strength;
        p.concave_depth = cfg.concave_depth;
        p.concave_shape = cfg.concave_shape;
        // 3-zone UI: Centre=z0, Mid=z2, Rim=z4.
        // Interpolate z1 and z3 between their neighbours so the
        // 5-zone shader gets smooth transitions from 3 user controls.
        p.concave_z0 = cfg.concave_z0;
        p.concave_z1 = (cfg.concave_z0 + cfg.concave_z2) * 0.5;
        p.concave_z2 = cfg.concave_z2;
        p.concave_z3 = (cfg.concave_z2 + cfg.concave_z4) * 0.5;
        p.concave_z4 = cfg.concave_z4;
        // Per-zone depth sliders removed — all zero (depth controlled by master depth only).
        p.concave_dz0 = 0.0;
        p.concave_dz1 = 0.0;
        p.concave_dz2 = 0.0;
        p.concave_dz3 = 0.0;
        p.concave_dz4 = 0.0;
        p.mirror_falloff = cfg.mirror_falloff;
        p.extend_pull_blend = cfg.extend_pull_blend;
        p.expansion_smoothness = cfg.expansion_smoothness;
        p.filter_bicubic = cfg.filter_bicubic;
        p.filter_lanczos = cfg.filter_lanczos;
        p.headlock_jitter_deadzone = cfg.headlock_jitter_deadzone;
        p.headlock_jitter_smooth = cfg.headlock_jitter_smooth;
        p.stable_lock_parallax_xy = cfg.stable_lock_parallax_xy;
        p.stable_lock_parallax_z  = cfg.stable_lock_parallax_z;
        p.stable_lock_dir_enabled = if cfg.stable_lock_dir_enabled { 1 } else { 0 };
        p.stable_lock_dir_strength = cfg.stable_lock_dir_strength;
        p.headlock_jitter_method = cfg.headlock_jitter_method;
        p.headlock_dejitter = if cfg.headlock_dejitter { 1 } else { 0 };
        p.headlock_dejitter_stiffness = cfg.headlock_dejitter_stiffness;
        p.headlock_dejitter_max_lag = cfg.headlock_dejitter_max_lag;
        p.parallax_prediction = if cfg.parallax_prediction { 1 } else { 0 };
        p.parallax_prediction_amt = cfg.parallax_prediction_amt;
        p.pp_adaptive = if cfg.pp_adaptive { 1 } else { 0 };
        p.pp_deadband_deg = cfg.pp_deadband_deg;
        p.pp_accel = cfg.pp_accel;
        p.pp_runtime_vel = if cfg.pp_runtime_vel { 1 } else { 0 };
        p.pp_photon_horizon = if cfg.pp_photon_horizon { 1 } else { 0 };
        p.pp_euro = if cfg.pp_euro { 1 } else { 0 };
        p.headlock_delay_ms      = cfg.headlock_delay_ms.clamp(0.0, 500.0);
        p.sim6dof_zoom_only = if cfg.sim6dof_zoom_only { 1 } else { 0 };
        p.sim6dof_mode = cfg.sim6dof_mode;
        p.offaxis_window_depth = cfg.offaxis_window_depth;
        p.offaxis_parallax = cfg.offaxis_parallax;
        p.offaxis_edge_falloff = cfg.offaxis_edge_falloff;
        p.offaxis_vertical_balance = cfg.offaxis_vertical_balance;
        p.dlayers_enabled = if cfg.dlayers_enabled { 1 } else { 0 };
        p.dlayers_invert = if cfg.dlayers_invert { 1 } else { 0 };
        p.dlayers_strength = cfg.dlayers_strength;
        p.dlayers_reactive = if cfg.dlayers_reactive_on { cfg.dlayers_reactive_amt } else { 0.0 };
        p.dlayers_separation = cfg.dlayers_separation;
        p.dlayers_delay = cfg.dlayers_delay;
        p.dlayers_ground = cfg.dlayers_ground;
        p.dlayers_horizon = cfg.dlayers_horizon;
        p.dlayers_vp = cfg.dlayers_vp;
        p.dlayers_curve = cfg.dlayers_curve;
        p.dlayers_zoom = cfg.dlayers_zoom;
        p.dlayers_convex = cfg.dlayers_convex;
        p.dlayers_mode = cfg.dlayers_mode;
        p.dlayers_edge = cfg.dlayers_edge;
        p.dir6dof_enabled = if cfg.dir6dof_enabled { 1 } else { 0 };
        p.dir6dof_yaw = cfg.dir6dof_yaw;
        p.dir6dof_pitch = cfg.dir6dof_pitch;
        p.dir6dof_roll = cfg.dir6dof_roll;
        // ── Hybrid Immersion (VERSION 65) ──
        p.hybrid_enabled = if cfg.hybrid_enabled { 1 } else { 0 };
        p.hybrid_center = cfg.hybrid_center;
        p.hybrid_fov_gain = cfg.hybrid_fov_gain;
        p.hybrid_ramp = cfg.hybrid_ramp;
        p.hybrid_softness = cfg.hybrid_softness;
        p.hybrid_rear_enabled = if cfg.hybrid_rear_enabled { 1 } else { 0 };
        p.hybrid_rear_stretch = cfg.hybrid_rear_stretch;
        p.hybrid_rear_direction = cfg.hybrid_rear_direction;
        // V77: rear dim + motion-fade removed as features; fields kept as unused
        // pads (uniform stays 16-byte aligned). Always send 0.
        p.hybrid_rear_dim = 0.0;
        p.hybrid_motion_fade = 0.0;
        p.hybrid_fov_gain_v = cfg.hybrid_fov_gain_v;
        p.hybrid_center_v = cfg.hybrid_center_v;
        p.hybrid_stretch_dir = cfg.hybrid_stretch_dir;
        p.hybrid_stretch_reach = cfg.hybrid_stretch_reach;
        p.vr_hotkeys_enabled = if cfg.vr_hotkeys_enabled { 1 } else { 0 };
        // Head-tracking output features.
        p.mouse_emu_enabled = if cfg.mouse_emu_enabled { 1 } else { 0 };
        p.mouse_emu_sensitivity = cfg.mouse_emu_sensitivity;
        p.mouse_emu_speed = cfg.mouse_emu_speed;
        p.mouse_emu_compat = cfg.mouse_emu_compat;
        p.joy_emu_enabled     = if cfg.joy_emu_enabled { 1 } else { 0 };
        p.joy_emu_mode        = cfg.joy_emu_mode;
        p.joy_emu_sensitivity = cfg.joy_emu_sensitivity;
        p.joy_emu_deadzone    = cfg.joy_emu_deadzone;
        p.joy_emu_max_angle   = cfg.joy_emu_max_angle;
        p.joy_emu_invert_x    = if cfg.joy_emu_invert_x { 1 } else { 0 };
        p.joy_emu_invert_y    = if cfg.joy_emu_invert_y { 1 } else { 0 };
        p.joy_emu_smoothness  = cfg.joy_emu_smoothness;
        p.joy_emu_speed_x     = cfg.joy_emu_speed_x;
        p.joy_emu_speed_y     = cfg.joy_emu_speed_y;
        p.overlay_enabled  = if cfg.overlay_enabled { 1 } else { 0 };
        p.panel_cursor_force  = if cfg.panel_cursor_force { 1 } else { 0 };
        p.panel_cursor_method = cfg.panel_cursor_method.min(2);
        p.panel_theme         = cfg.panel_theme.min(4);
        p.overlay_size     = cfg.overlay_size;
        p.overlay_size_x   = cfg.overlay_size_x;
        p.overlay_size_y   = cfg.overlay_size_y;
        p.overlay_offset_x = cfg.overlay_offset_x;
        p.overlay_offset_y = cfg.overlay_offset_y;
        p.overlay_distance = cfg.overlay_distance;
        p.overlay_hud_mode = if cfg.overlay_hud_mode { 1 } else { 0 };
        p.overlay_aspect = cfg.overlay_aspect.min(2);
        p.overlay_transparency = cfg.overlay_transparency.clamp(0.0, 1.0);
        p.udp_6dof_enabled = if cfg.udp_6dof_enabled { 1 } else { 0 };
        p.trackir_enabled = if cfg.trackir_enabled { 1 } else { 0 };
        p.udp_6dof_port = cfg.udp_6dof_port;
        p.set_udp_ip_str(&cfg.udp_6dof_ip);
        p.udp_flip_x = if cfg.udp_flip_x { 1 } else { 0 };
        p.udp_flip_y = if cfg.udp_flip_y { 1 } else { 0 };
        p.udp_flip_z = if cfg.udp_flip_z { 1 } else { 0 };
        p.udp_flip_yaw = if cfg.udp_flip_yaw { 1 } else { 0 };
        p.trackir_flip_x = if cfg.trackir_flip_x { 1 } else { 0 };
        p.trackir_flip_y = if cfg.trackir_flip_y { 1 } else { 0 };
        p.trackir_flip_z = if cfg.trackir_flip_z { 1 } else { 0 };
        p.trackir_flip_yaw = if cfg.trackir_flip_yaw { 1 } else { 0 };
        p.trackir_flip_pitch = if cfg.trackir_flip_pitch { 1 } else { 0 };
        p.trackir_flip_roll = if cfg.trackir_flip_roll { 1 } else { 0 };
        p.trackir_gain_z = cfg.trackir_gain_z;
        p.udp_flip_pitch = if cfg.udp_flip_pitch { 1 } else { 0 };
        p.udp_flip_roll = if cfg.udp_flip_roll { 1 } else { 0 };
        p.udp_gain_yaw = cfg.udp_gain_yaw;
        p.udp_gain_pitch = cfg.udp_gain_pitch;
        p.udp_gain_roll = cfg.udp_gain_roll;
        p.udp_gain_x = cfg.udp_gain_x;
        p.udp_gain_y = cfg.udp_gain_y;
        p.udp_gain_z = cfg.udp_gain_z;
        // VR Data to UDP.
        p.vr_udp_enabled = if cfg.vr_udp_enabled { 1 } else { 0 };
        p.vr_udp_mode = cfg.vr_udp_mode;
        p.vr_udp_port = cfg.vr_udp_port;
        p.set_vr_udp_ip_str(&cfg.vr_udp_ip);
        p.vr_udp_flip_x = if cfg.vr_udp_flip_x { 1 } else { 0 };
        p.vr_udp_flip_y = if cfg.vr_udp_flip_y { 1 } else { 0 };
        p.vr_udp_flip_z = if cfg.vr_udp_flip_z { 1 } else { 0 };
        p.vr_udp_flip_yaw = if cfg.vr_udp_flip_yaw { 1 } else { 0 };
        p.vr_udp_flip_pitch = if cfg.vr_udp_flip_pitch { 1 } else { 0 };
        p.vr_udp_flip_roll = if cfg.vr_udp_flip_roll { 1 } else { 0 };
        p.vr_udp_gain_yaw = cfg.vr_udp_gain_yaw;
        p.vr_udp_gain_pitch = cfg.vr_udp_gain_pitch;
        p.vr_udp_gain_roll = cfg.vr_udp_gain_roll;
        p.vr_udp_gain_x = cfg.vr_udp_gain_x;
        p.vr_udp_gain_y = cfg.vr_udp_gain_y;
        p.vr_udp_gain_z = cfg.vr_udp_gain_z;
        p.vr_udp_left_enabled = if cfg.vr_udp_left_enabled { 1 } else { 0 };
        p.vr_udp_right_enabled = if cfg.vr_udp_right_enabled { 1 } else { 0 };
        p.diag_mode = if cfg.diag_mode { 1 } else { 0 };
        p.pose_predict_ms    = cfg.pose_predict_ms.clamp(0.0, 50.0);
        p.pose_smooth_alpha  = cfg.pose_smooth_alpha.clamp(0.0, 0.99);
        p.pimax_flat_depth   = if cfg.pimax_flat_depth { 1 } else { 0 };
        p.lowfps_predict_boost = if cfg.lowfps_predict_boost { 1 } else { 0 };
        p.lowfps_predict_strength = cfg.lowfps_predict_strength.clamp(0.0, 1.0);
        p.vsync_mode               = cfg.vsync_mode;
        p.fps_limit                = cfg.fps_limit.max(0.0);
        p.frame_pacing_enabled     = cfg.frame_pacing_enabled as u32;
        p.frame_pacing_target      = cfg.frame_pacing_target.clamp(0.0, 0.95);
        p.temporal_blend_enabled   = cfg.temporal_blend_enabled as u32;
        p.temporal_blend_alpha     = cfg.temporal_blend_alpha.clamp(0.5, 0.98);
        p.flow_enabled             = cfg.flow_enabled as u32;
        p.flow_strength            = cfg.flow_strength.clamp(0.1, 1.5);
        p.submit_render_pose       = cfg.submit_render_pose as u32;
        p.stable_eye_submit        = cfg.stable_eye_submit as u32;
        p.hold_full_refresh        = cfg.hold_full_refresh as u32;
        p.repeat_stretch     = if cfg.show_repeated_method { cfg.repeat_stretch.clamp(0.0, 30.0) } else { 0.0 };
        p.repeat_blend       = cfg.repeat_blend.clamp(0.0, 1.0);
        p.repeat_depth       = cfg.repeat_depth.clamp(0.01, 0.50);
        p.auto_z_enabled      = cfg.auto_z_enabled as u32;
        p.auto_z_value        = cfg.auto_z_value;
        p.auto_roll_enabled   = cfg.auto_roll_enabled as u32;
        p.auto_roll_value     = cfg.auto_roll_value;
        p.auto_x_enabled      = cfg.auto_x_enabled as u32;
        p.auto_x_value        = cfg.auto_x_value;
        p.auto_y_enabled      = cfg.auto_y_enabled as u32;
        p.auto_y_value        = cfg.auto_y_value;
        p.auto_height_enabled = cfg.auto_height_enabled as u32;
        p.sphere_y_169        = cfg.sphere_y_169.clamp(0.05, 3.0);
        p.sphere_y_43         = cfg.sphere_y_43.clamp(0.05, 3.0);
        p.sphere_y_219        = cfg.sphere_y_219.clamp(0.05, 3.0);
        p.enhancement_quality = cfg.enhancement_quality.clamp(0.0, 1.0);
        p.rcas_sharpness     = cfg.rcas_sharpness.clamp(0.0, 2.0);
    }

    fn push_to_shm(&mut self, enabled: bool) {
        let mut m = LiveParamsMapping::default();
        m.params.enabled = enabled as u32;
        Self::populate_live_params(&self.cfg, &mut m.params);
        write_shm(&self.writer, m);
        // Keep the shadow in lockstep with the master `cfg` so the
        // hotkey thread sees the latest values when a key fires.
        if let Ok(mut shadow) = self.shared_cfg.lock() {
            *shadow = self.cfg.clone();
        }
    }

    /// Stage 4b: start/stop the low-level keyboard hook worker
    /// based on `hotkey_delivery_method` changes. The worker only
    /// runs when method == 1 (Low-level hook). Default mode uses
    /// the `global_hotkey` thread spawned at startup.
    fn sync_hook_worker(&mut self) {
        let desired = self.cfg.hotkey_delivery_method;
        if desired == self.last_delivery_method {
            return;
        }
        // Stop any running worker first.
        if let Some(stop) = self.hook_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.last_delivery_method = desired;
        // When low-level hook is active, unregister Default-mode
        // (RegisterHotKey) bindings so they don't double-fire. When
        // we revert to Default mode, re-register from current
        // bindings.
        if desired == 1 {
            self.hotkey_mgr.sync(&hotkeys::HotkeyBindings::default());
        } else {
            self.hotkey_mgr.sync(&self.cfg.hotkey_bindings);
        }
        if desired == 1 {
            // Start a fresh worker. We dispatch through the existing
            // shared_cfg + writer path so a hook-detected hotkey
            // applies identically to a Default-detected one.
            #[cfg(target_os = "windows")]
            {
                let stop = Arc::new(AtomicBool::new(false));
                let stop_for_worker = stop.clone();
                let bindings = self.hook_bindings.clone();
                let shared_cfg = self.shared_cfg.clone();
                let writer = self.writer.clone();
                let restart_flag_for_hook = self.hotkey_restart_flag.clone();
                let egui_ctx_for_hook = self.egui_ctx.clone();
                let viewer_name_for_hook = self
                    .viewer_exe
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let dispatch: Arc<dyn Fn(hotkeys::HotkeyAction) + Send + Sync> =
                    Arc::new(move |action| {
                        // Same payload as spawn_hotkey_worker.
                        if let Ok(mut cfg) = shared_cfg.lock() {
                            apply_hotkey_to_cfg(action, &mut cfg);
                            let mut m = LiveParamsMapping::default();
                            m.params.enabled = 1;
                            match action {
                                hotkeys::HotkeyAction::Recenter => {
                                    m.params.recenter_request = 1;
                                }
                                hotkeys::HotkeyAction::Screenshot => {
                                    m.params.screenshot_request = 1;
                                }
                                hotkeys::HotkeyAction::ForceDesktop => {
                                    m.params.force_desktop_request = 1;
                                }
                                hotkeys::HotkeyAction::Restart => {
                                    // Kill the viewer NOW (off the UI thread) so it
                                    // dies the instant the key is pressed, matching
                                    // the in-window button. Main thread relaunches.
                                    immediate_kill_viewer(&viewer_name_for_hook);
                                    restart_flag_for_hook.store(true, std::sync::atomic::Ordering::Relaxed);
                                    // Wake the UI loop so update() runs the relaunch.
                                    egui_ctx_for_hook.request_repaint();
                                    return; // exit this closure invocation
                                }
                                _ => {}
                            }
                            OsirisGui::populate_live_params(&cfg, &mut m.params);
                            drop(cfg);
                            write_shm(&writer, m);
                        }
                    });
                if low_level_hook::start_worker(bindings, stop_for_worker, dispatch) {
                    self.hook_stop = Some(stop);
                } else {
                    // Worker failed to spawn — revert dropdown to
                    // Default so the user sees the state change.
                    log::warn!("Low-level hook worker did not start — reverting to Default.");
                    self.cfg.hotkey_delivery_method = 0;
                    self.last_delivery_method = 0;
                }
            }
        }
    }

    /// One-shot write that sets `quit_request = 1`. The viewer
    /// notices on its next frame's live-params poll and breaks its
    /// render loop. We send the rest of the params alongside the
    /// quit flag so the viewer doesn't see torn state on the way
    /// out (e.g. a stale stretch_mode from a previous build).
    fn push_quit_to_shm(&mut self) {
        let mut m = LiveParamsMapping::default();
        m.params.enabled = 1;
        m.params.quit_request = 1;
        // Also fill in the other fields so the viewer's last frame
        // before exiting reflects current GUI state. Cheap; the
        // viewer ignores most of this once it sees quit_request.
        Self::populate_live_params(&self.cfg, &mut m.params);
        write_shm(&self.writer, m);
    }

    /// Kill the viewer process reliably, including when it is running
    /// as administrator and this GUI is also elevated (or vice versa).
    ///
    /// Strategy (in order):
    /// 1. child.kill() — fast if we have the original handle with rights.
    /// 2. OpenProcess(PROCESS_TERMINATE, pid) — opens a fresh handle by
    ///    PID. Works when child handle lacks PROCESS_TERMINATE but our
    ///    process token grants SeDebugPrivilege (which admin gets).
    /// 3. taskkill /F /PID — fallback when (2) also fails. Works for
    ///    same-integrity-level scenarios.
    /// 4. taskkill /F /IM — last resort catch-all.
    ///
    /// After (1)+(2) we wait up to 2s for the process to actually exit
    /// before falling through to (3)+(4) so we don't leave a zombie.
    fn kill_viewer_process(&mut self) {
        // Step 1: kill via child handle.
        if let Some(mut child) = self.viewer_child.take() {
            let _ = child.kill();
            // Brief wait — the watchdog thread inside the viewer needs
            // ~100ms to notice GLOBAL_QUIT and call request_exit().
            // After request_exit(), OpenXR unwinds cleanly in <500ms.
            let _ = child.wait();
        }

        #[cfg(target_os = "windows")]
        if let Some(pid) = self.viewer_pid {
            use ::windows::Win32::System::Threading::{
                OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
            };
            use ::windows::Win32::Foundation::CloseHandle;

            // Step 2: try OpenProcess by PID for a fresh PROCESS_TERMINATE handle.
            let handle = unsafe {
                OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid)
            };
            // Valid if the call succeeded and the handle value is non-null.
            let handle_valid = handle.as_ref().map(|h| h.0 != 0).unwrap_or(false);

            if handle_valid {
                let h = handle.unwrap();
                unsafe {
                    use ::windows::Win32::System::Threading::TerminateProcess;
                    let _ = TerminateProcess(h, 1);
                    // Wait up to 2s for clean exit before fallthrough.
                    WaitForSingleObject(h, 2000);
                    let _ = CloseHandle(h);
                }
            }

            // Step 3: taskkill /F /PID — works for same-integrity scenarios
            // where OpenProcess returned a valid handle but TerminateProcess
            // returned access denied (rare, but possible with certain AppContainer configs).
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output();
        }

        // Step 4: taskkill /F /IM — last resort, catches any viewer we
        // didn't launch ourselves (peer-attached viewers, multiple instances).
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/IM", "osiris-vr-viewer.exe"])
                .creation_flags(0x08000000)
                .output();
        }
    }

    /// One-shot write that sets `screenshot_request = 1`. The viewer
    /// notices on its next frame, captures the left-eye image of the
    /// frame after that, writes a PNG to disk next to the exe, and
    /// resets the flag in shared memory so a subsequent click fires
    /// fresh.
    fn push_screenshot_request(&mut self) {
        if self.writer.lock().map(|g| g.is_none()).unwrap_or(true) {
            self.status = "Screenshot failed: no shared memory connection.".into(); self.status_updated = std::time::Instant::now();
            return;
        }
        let mut m = LiveParamsMapping::default();
        m.params.enabled = 1;
        m.params.screenshot_request = 1;
        // Send the full params snapshot so the viewer doesn't see
        // torn state if it just so happens to read this frame.
        Self::populate_live_params(&self.cfg, &mut m.params);
        write_shm(&self.writer, m);
        self.status = "Screenshot requested. Saving next to the .exe…".into(); self.status_updated = std::time::Instant::now();
    }

    /// One-shot write that sets `restart_session_request = 1`. The
    /// viewer notices, breaks its render loop with `restart_pending`,
    /// returns to launch()'s outer loop, and re-initialises the
    /// entire OpenXR session.
    /// Kill the viewer process and spawn a fresh one. Cleaner than
    /// the previous SHM-flag-based "soft restart" which re-init'd
    /// OpenXR within the same process — that approach occasionally
    /// hit runtime quirks (SteamVR session leftover state, swapchain
    /// re-creation timing). A hard process restart sidesteps all
    /// of that: the OS reclaims everything and the new viewer comes
    /// up clean.
    /// Kill the viewer process and spawn a fresh one. Works correctly when
    /// running as administrator — uses kill_viewer_process() which opens a
    /// fresh PROCESS_TERMINATE handle by PID, bypassing any rights issues
    /// with the original child handle.
    fn push_restart_request(&mut self) {
        // Feedback first so the GUI reflects the action immediately.
        self.status = "Restarting viewer…".into();
        self.status_updated = std::time::Instant::now();

        // 1. Push quit via SHM as a courtesy — the viewer's watchdog may pick
        //    it up and call request_exit() for a graceful OpenXR teardown in
        //    parallel. We do NOT wait for it: we hard-kill next.
        {
            let mut m = LiveParamsMapping::default();
            m.params.enabled = 1;
            m.params.quit_request = 1;
            write_shm(&self.writer, m);
        }

        // 2. Kill the viewer process IMMEDIATELY. TerminateProcess is
        //    synchronous, so the (possibly frozen) viewer disappears the
        //    instant the restart hotkey/button is pressed — this is what makes
        //    the restart feel immediate. Killing first also means we don't
        //    depend on the viewer being responsive enough to honour the SHM
        //    quit flag (it may be wedged, which is often why the user is
        //    restarting in the first place).
        self.kill_viewer_process();

        // 3. Settle so the OpenXR runtime detects the dead client and releases
        //    the session, GPU device, and SHM view before the fresh viewer
        //    tries to acquire them. Because we hard-kill (no graceful
        //    request_exit beforehand), the runtime has to notice the closed
        //    connection itself, so we keep a full ~300ms here — the same
        //    handle-release window the previous implementation used. We still
        //    save ~350ms by dropping the pre-kill graceful wait, which was
        //    useless anyway whenever the viewer was wedged (a frozen render
        //    loop never reads the SHM quit flag, so it never exits gracefully).
        std::thread::sleep(std::time::Duration::from_millis(300));

        // 4. CRUCIAL — clear quit_request and write fresh full-state, otherwise
        //    the fresh viewer reads SHM, sees quit_request=1, and exits at once.
        self.push_to_shm(true);

        // 5. Spawn fresh viewer.
        self.try_launch_viewer_silent();

        self.status = "Restarted viewer.".into();
        self.status_updated = std::time::Instant::now();
    }
    /// One-shot write that sets `recenter_request = 1`. The viewer
    /// notices and re-anchors the screen to the current head pose.
    fn push_recenter_request(&mut self) {
        if self.writer.lock().map(|g| g.is_none()).unwrap_or(true) {
            self.status = "Recenter failed: no shared memory connection.".into(); self.status_updated = std::time::Instant::now();
            return;
        }
        let mut m = LiveParamsMapping::default();
        m.params.enabled = 1;
        m.params.recenter_request = 1;
        // Same full-snapshot pattern as the other request helpers.
        Self::populate_live_params(&self.cfg, &mut m.params);
        write_shm(&self.writer, m);
        self.status = "Recentered.".into(); self.status_updated = std::time::Instant::now();
    }

    /// One-shot write that sets `force_desktop_request = 1`. The viewer drops to
    /// the desktop view immediately and applies a short (~800ms) skip-Katanga
    /// hold, then resumes normal probing — returning to Katanga on the next game.
    fn push_force_desktop_request(&mut self) {
        if self.writer.lock().map(|g| g.is_none()).unwrap_or(true) {
            self.status = "Force desktop failed: no shared memory connection.".into(); self.status_updated = std::time::Instant::now();
            return;
        }
        let mut m = LiveParamsMapping::default();
        m.params.enabled = 1;
        m.params.force_desktop_request = 1;
        Self::populate_live_params(&self.cfg, &mut m.params);
        write_shm(&self.writer, m);
        self.status = "Forced desktop view.".into(); self.status_updated = std::time::Instant::now();
    }

    /// Apply a fired hotkey on the main thread. Most actions toggle a
    /// config field or nudge a slider value; some send command pulses
    /// via SHM.
    ///
    /// This is the main-thread fast path: it updates a status-line
    /// message for visual feedback. The background hotkey worker
    /// thread also processes the same actions when the GUI window is
    /// minimized, but without status updates (no GUI to display them
    /// in). Both paths share the same `apply_hotkey_to_cfg` helper so
    /// behaviour stays in sync.
    fn apply_hotkey_action(&mut self, action: hotkeys::HotkeyAction) {
        use hotkeys::HotkeyAction as A;
        // Pulse-only commands take a separate path because they
        // write SHM with a flag we don't want to leave set.
        match action {
            A::Recenter => {
                self.push_recenter_request();
                return;
            }
            A::Screenshot => {
                self.push_screenshot_request();
                return;
            }
            A::ForceDesktop => {
                self.push_force_desktop_request();
                return;
            }
            A::Restart => {
                self.push_restart_request();
                return;
            }
            A::CyclePreset => {
                // Cycle through saved presets in alphabetical order.
                // Refreshes the list first so newly saved presets appear immediately.
                // Logic: load presets[idx] then advance idx.
                // Click 1 -> loads preset 1 (idx=0), idx becomes 1.
                // Click 2 -> loads preset 2 (idx=1), idx becomes 2 (or wraps).
                // Click N -> wraps back to preset 1.
                self.refresh_preset_list();
                let n = self.available_presets.len();
                if n == 0 {
                    self.status = "No saved presets to cycle".into();
                    self.status_updated = std::time::Instant::now();
                    return;
                }
                // Clamp in case presets were deleted since last cycle.
                if self.preset_cycle_idx >= n {
                    self.preset_cycle_idx = 0;
                }
                let idx = self.preset_cycle_idx;
                let name = self.available_presets[idx].clone();
                // Advance for next click before loading (so status shows correct position).
                self.preset_cycle_idx = (idx + 1) % n;
                match self.load_preset(&name) {
                    Ok(()) => {
                        self.status = format!("Preset: {} ({}/{})", name, idx + 1, n);
                        self.status_updated = std::time::Instant::now();
                    }
                    Err(e) => {
                        self.status = format!("Preset load failed: {}", e);
                        self.status_updated = std::time::Instant::now();
                    }
                }
                return;
            }
            _ => {}
        }
        // All other actions: mutate cfg, set status, push to SHM.
        apply_hotkey_to_cfg(action, &mut self.cfg);
        // Overlay→show-GUI: if the overlay HOTKEY toggled the overlay and the
        // toggle is on, bring this panel forward (ON) or return focus to the
        // game (OFF). Shares OVERLAY_SAVED_HWND with the worker path, so it
        // behaves the same whichever thread drained the event. No-op off-Windows
        // and for every other action.
        if matches!(action, A::ToggleKatangaOverlay) && self.cfg.overlay_show_gui {
            if self.cfg.overlay_enabled {
                overlay_show_gui_now();
            } else {
                overlay_restore_game_now();
            }
        }
        self.status = match action {
            A::Cycle3DMode => format!(
                "3D mode: {}",
                StereoModeIndex::from_u32(self.cfg.stereo_mode).label()
            ),
            A::CycleScreenShape => format!(
                "Screen shape: {}",
                StretchModeIndex::from_u32(self.cfg.stretch_mode).label()
            ),
            A::ToggleHeadlock => {
                if self.cfg.head_lock { "Head-lock: ON" } else { "Head-lock: OFF" }.into()
            }
            A::ToggleSim6dof => if self.cfg.sim6dof_enabled {
                "Simulated 6DoF: ON"
            } else {
                "Simulated 6DoF: OFF"
            }
            .into(),
            A::ZoomIn | A::ZoomOut => format!("Zoom: {:.1}", self.cfg.scale),
            A::OffsetZForward | A::OffsetZBackward => {
                format!("Z offset: {:.2}", self.cfg.offset_z)
            }
            A::OffsetXLeft | A::OffsetXRight => {
                format!("X offset: {:.2}", self.cfg.offset_x)
            }
            A::OffsetYUp | A::OffsetYDown => {
                format!("Y offset: {:.2}", self.cfg.offset_y)
            }
            A::RollOffsetLeft | A::RollOffsetRight => {
                format!(
                    "Roll: {:.1}°",
                    self.cfg.offset_roll * 180.0 / std::f32::consts::PI
                )
            }
            A::SwapEyes => if self.cfg.swap_eyes {
                "Swap eyes: ON"
            } else {
                "Swap eyes: OFF"
            }
            .into(),
            A::ToggleMouseEmu => if self.cfg.mouse_emu_enabled {
                "Mouse emulation: ON"
            } else {
                "Mouse emulation: OFF"
            }
            .into(),
            A::ToggleJoyEmu => if self.cfg.joy_emu_enabled {
                "Joystick emu: ON"
            } else {
                "Joystick emu: OFF"
            }
            .into(),
            A::ToggleKatangaOverlay => if self.cfg.overlay_enabled {
                "Katanga ImGui: ON"
            } else {
                "Katanga ImGui: OFF"
            }
            .into(),
            A::ToggleKatangaFilters => if self.cfg.katanga_filters_enabled {
                "Katanga Filters: ON"
            } else {
                "Katanga Filters: OFF"
            }
            .into(),
            A::ToggleUdp6dof => if self.cfg.udp_6dof_enabled {
                "6DoF UDP: ON"
            } else {
                "6DoF UDP: OFF"
            }
            .into(),
            // Pulse commands handled above.
            A::Recenter | A::Screenshot | A::Restart | A::ForceDesktop => self.status.clone(),
            A::CyclePreset => self.status.clone(), // handled in apply_hotkey_action
        }; self.status_updated = std::time::Instant::now();
        self.push_to_shm(true);
    }
}

/// Thread-safe SHM write helper. Locks the writer mutex briefly,
/// writes the mapping, releases. Used both by the GUI thread (when
/// sliders change) and by the hotkey background thread (when a global
/// hotkey fires while the window is minimized).
fn write_shm(
    writer: &Arc<Mutex<Option<shm::LiveParamsWriter>>>,
    mapping: LiveParamsMapping,
) {
    if let Ok(mut guard) = writer.lock() {
        if let Some(w) = guard.as_mut() {
            w.write(mapping);
        }
    }
}

// ── Overlay → Show GUI (Windows-only) ────────────────────────────────────
// When the Katanga overlay is toggled via its HOTKEY and "Show GUI with
// overlay" is enabled, bring the GUI window to the foreground so it appears
// inside the desktop-mirror overlay; toggling the overlay off restores focus
// to whatever window (the game) was in front. Implemented as free helpers plus
// one process-global slot so BOTH hotkey paths — the background worker and the
// main-thread `apply_hotkey_action` — share the same saved window, because
// either may drain a given hotkey event. No-ops on non-Windows. Touches the GUI
// process only: no viewer / wire / uniform changes, and nothing runs unless the
// overlay hotkey fires with the toggle on.
#[cfg(target_os = "windows")]
static OVERLAY_SAVED_HWND: std::sync::atomic::AtomicIsize =
    std::sync::atomic::AtomicIsize::new(0);
/// Selected GUI theme id, read inside `section_with_accent` (which all
/// section_* helpers funnel through). Set from cfg.gui_theme_id each frame.
/// 0=Colored (per-section accent), 1=Dark Blue, 2=Black, 3=Red, 4=Cyan.
static GUI_THEME_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// True when any background image (section or overall) is active this frame, so
/// `section_with_accent` makes its fills translucent to let the image show.
static GUI_BG_ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// TextureId of the per-section background image for this frame (None = unset).
    /// `egui::TextureId` is Copy, so a Cell is sufficient on the single UI thread.
    static SECTION_BG_TEX: std::cell::Cell<Option<egui::TextureId>> = std::cell::Cell::new(None);
    /// Per-section frame rects captured on the PREVIOUS frame, keyed by section
    /// title. Used to paint the per-section background sized to the section
    /// (immediate-mode can't know the rect before layout, and layout is static
    /// frame-to-frame, so a one-frame-old rect is seamless).
    static SECTION_RECTS: std::cell::RefCell<std::collections::HashMap<String, egui::Rect>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Map a theme id to (header, border, title) colours. Returns None for the
/// default "Colored" theme (0), where the caller keeps the per-section accent.
fn theme_palette(id: u32) -> Option<(Color32, Color32, Color32)> {
    match id {
        1 => Some((Color32::from_rgb(16, 30, 66), Color32::from_rgb(45, 80, 165), Color32::WHITE)), // Dark Blue
        2 => Some((Color32::from_rgb(8, 8, 8),   Color32::from_rgb(0, 0, 0),     Color32::WHITE)),  // Black
        3 => Some((Color32::from_rgb(150, 25, 25), Color32::from_rgb(150, 25, 25), Color32::WHITE)),// Red
        4 => Some((Color32::from_rgb(0, 150, 160), Color32::from_rgb(0, 150, 160), Color32::BLACK)),// Cyan
        5 => Some((Color32::from_rgb(236, 238, 242), Color32::from_rgb(255, 255, 255), Color32::BLACK)), // White
        6 => Some((Color32::from_rgb(200, 110, 20), Color32::from_rgb(200, 110, 20), Color32::WHITE)),   // Orange
        7 => Some((Color32::from_rgb(212, 180, 28), Color32::from_rgb(212, 180, 28), Color32::BLACK)),   // Yellow
        8 => Some((Color32::from_rgb(30, 130, 60), Color32::from_rgb(30, 130, 60), Color32::WHITE)),     // Green
        9 => Some((Color32::from_rgb(120, 55, 180), Color32::from_rgb(120, 55, 180), Color32::WHITE)),   // Purple
        10 => Some((Color32::from_rgb(185, 40, 150), Color32::from_rgb(185, 40, 150), Color32::WHITE)),  // Magenta
        _ => None, // 0 = Colored default
    }
}

/// Resolved per-section visuals after applying the active theme + background.
struct SectionVisuals {
    header_fill: Color32,
    border: Color32,
    title: Color32,
    frame_fill: Color32,
    body_fill: Color32,
}

/// Resolve a section's colours honoring the active theme (a non-default theme
/// overrides the per-section accent) and background state (fills go translucent
/// so a background image shows through). `default_*` are the section's own
/// accent colours, used only for theme 0 (Colored). Shared by `section_with_accent`
/// and the bespoke HOTKEYS / GUI THEME renderers so all three theme uniformly.
fn section_visuals(
    default_header: Color32,
    default_border: Color32,
    default_title: Color32,
    default_body: Color32,
) -> SectionVisuals {
    use std::sync::atomic::Ordering::Relaxed;
    let (header, border, title) = match theme_palette(GUI_THEME_ID.load(Relaxed)) {
        Some((h, b, t)) => (h, b, t),
        None => (default_header, default_border, default_title),
    };
    if GUI_BG_ACTIVE.load(Relaxed) {
        SectionVisuals {
            header_fill: Color32::from_rgba_unmultiplied(header.r(), header.g(), header.b(), 210),
            border,
            title,
            frame_fill: Color32::TRANSPARENT,
            body_fill: Color32::from_rgba_unmultiplied(
                default_body.r(), default_body.g(), default_body.b(), 150),
        }
    } else {
        SectionVisuals { header_fill: header, border, title, frame_fill: default_body, body_fill: default_body }
    }
}

/// Paint the per-section background image (if one is set) at the rect this
/// section occupied on the PREVIOUS frame, behind its content. Layout is static
/// frame-to-frame so the one-frame-old rect aligns. Call before drawing the frame.
fn paint_prev_section_bg(ui: &egui::Ui, title: &str) {
    if let Some(bg_id) = SECTION_BG_TEX.with(|c| c.get()) {
        if let Some(prev) = SECTION_RECTS.with(|m| m.borrow().get(title).copied()) {
            ui.painter().image(
                bg_id,
                prev,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }
}

/// Record this frame's section rect for next frame's background paint.
fn record_section_rect(title: &str, rect: egui::Rect) {
    SECTION_RECTS.with(|m| {
        m.borrow_mut().insert(title.to_string(), rect);
    });
}

#[cfg(target_os = "windows")]
struct OverlayFindWnd {
    pid: u32,
    found: isize,
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn overlay_find_gui_proc(
    hwnd: ::windows::Win32::Foundation::HWND,
    lparam: ::windows::Win32::Foundation::LPARAM,
) -> ::windows::Win32::Foundation::BOOL {
    use ::windows::Win32::Foundation::BOOL;
    use ::windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId};
    let ctx = &mut *(lparam.0 as *mut OverlayFindWnd);
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
    if pid == ctx.pid {
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            if title == APP_TITLE {
                ctx.found = hwnd.0;
                return BOOL(0); // found it — stop enumerating
            }
        }
    }
    BOOL(1) // keep enumerating
}

/// Find this process's main GUI window by title. Returns 0 if not found.
#[cfg(target_os = "windows")]
fn overlay_find_gui_hwnd() -> isize {
    use ::windows::Win32::Foundation::LPARAM;
    use ::windows::Win32::System::Threading::GetCurrentProcessId;
    use ::windows::Win32::UI::WindowsAndMessaging::EnumWindows;
    unsafe {
        let mut ctx = OverlayFindWnd { pid: GetCurrentProcessId(), found: 0 };
        let _ = EnumWindows(Some(overlay_find_gui_proc), LPARAM(&mut ctx as *mut _ as isize));
        ctx.found
    }
}

/// Bring a window to the foreground, restoring it if minimized. The synthetic
/// ALT tap satisfies Windows' foreground-lock rule so a background context is
/// allowed to change the foreground window.
#[cfg(target_os = "windows")]
fn overlay_force_foreground(hwnd_raw: isize) {
    use ::windows::Win32::Foundation::HWND;
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, SetWindowPos, ShowWindow,
        SET_WINDOW_POS_FLAGS, SW_RESTORE,
    };
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw);
        // Un-minimize first if needed.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Synthetic ALT tap relaxes Windows' foreground-lock timeout so the
        // SetForegroundWindow below is allowed from a background context.
        keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0); // ALT down
        keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0); // ALT up
        // Pull the window to the top of the Z-order with a brief topmost toggle.
        // This works reliably from a background thread without AttachThreadInput
        // (which isn't reachable in this windows-crate build) and causes no
        // minimize flash. Hardcoded Win32 values to avoid const-availability
        // issues: HWND_TOPMOST = -1, HWND_NOTOPMOST = -2,
        // SWP_NOSIZE (0x1) | SWP_NOMOVE (0x2) = 0x3.
        let swp = SET_WINDOW_POS_FLAGS(0x0003);
        let _ = SetWindowPos(hwnd, HWND(-1), 0, 0, 0, 0, swp);
        let _ = SetWindowPos(hwnd, HWND(-2), 0, 0, 0, 0, swp);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Overlay turned ON: remember the current foreground window (the game, unless
/// it is already us) and bring the GUI to the front.
#[cfg(target_os = "windows")]
fn overlay_show_gui_now() {
    use ::windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let gui = overlay_find_gui_hwnd();
    if gui == 0 {
        return;
    }
    let fg = unsafe { GetForegroundWindow() }.0;
    if fg != 0 && fg != gui {
        OVERLAY_SAVED_HWND.store(fg, std::sync::atomic::Ordering::Relaxed);
    }
    overlay_force_foreground(gui);
}

/// Overlay turned OFF: hand focus back to the saved (game) window, if any.
#[cfg(target_os = "windows")]
fn overlay_restore_game_now() {
    let saved = OVERLAY_SAVED_HWND.swap(0, std::sync::atomic::Ordering::Relaxed);
    if saved != 0 {
        overlay_force_foreground(saved);
    }
}

#[cfg(not(target_os = "windows"))]
fn overlay_show_gui_now() {}
#[cfg(not(target_os = "windows"))]
fn overlay_restore_game_now() {}

/// Spawn the hotkey background thread. It holds clones of the
/// SHM writer and shared config, polls global-hotkey events
/// continuously (independently of egui's update loop, which doesn't
/// run while the window is minimized), and applies fired actions
/// directly. This is the path that makes hotkeys work in-game.
///
/// Note: registration of hotkeys still happens on the main thread
/// (where `GlobalHotKeyManager` is created), because Win32's
/// `RegisterHotKey` requires the calling thread to have a message
/// pump. But event DELIVERY uses a global crossbeam channel, so the
/// background thread can drain it safely from anywhere.
/// Best-effort immediate termination of the viewer process by image name,
/// callable from any thread (takes no `&self`). Used by the restart-hotkey
/// paths so the (possibly frozen) viewer disappears the instant the key is
/// pressed — the same synchronous kill that makes the in-window Restart button
/// feel immediate — instead of waiting for the throttled UI loop to consume
/// the restart flag (which is what made the hotkey lag behind the button,
/// especially with the GUI minimized in VR). The main thread still performs
/// the relaunch via push_restart_request(), so its child-handle/PID bookkeeping
/// stays in sync and no second viewer is spawned. A kill on an already-dead
/// process is a harmless no-op, so the later main-thread kill is fine too.
#[cfg(target_os = "windows")]
fn immediate_kill_viewer(exe_name: &str) {
    if exe_name.is_empty() {
        return;
    }
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/IM", exe_name])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();
}
#[cfg(not(target_os = "windows"))]
fn immediate_kill_viewer(_exe_name: &str) {}

fn spawn_hotkey_worker(
    writer: Arc<Mutex<Option<shm::LiveParamsWriter>>>,
    shared_cfg: Arc<Mutex<GuiConfig>>,
    id_to_action: Arc<Mutex<std::collections::HashMap<u32, hotkeys::HotkeyAction>>>,
    restart_flag: Arc<std::sync::atomic::AtomicBool>,
    egui_ctx: egui::Context,
    viewer_exe_name: String,
) {
    std::thread::Builder::new()
        .name("osiris-hotkey-worker".into())
        .spawn(move || {
            let receiver = global_hotkey::GlobalHotKeyEvent::receiver();
            loop {
                // Drain any pending hotkey events. try_recv is
                // non-blocking; we sleep between polls to keep CPU
                // usage minimal (10 ms = up to 100 Hz response, plenty
                // for a key tap).
                while let Ok(evt) = receiver.try_recv() {
                    if evt.state != global_hotkey::HotKeyState::Pressed {
                        continue;
                    }
                    // Translate event id -> action via the shared
                    // map (populated by the main thread when bindings
                    // change).
                    let action = match id_to_action.lock() {
                        Ok(map) => map.get(&evt.id).copied(),
                        Err(_) => None,
                    };
                    let Some(action) = action else { continue };
                    // Apply the action to the shadow config.
                    if let Ok(mut cfg) = shared_cfg.lock() {
                        apply_hotkey_to_cfg(action, &mut cfg);
                        // Capture overlay→show-GUI intent while we still hold the
                        // lock; we act on it AFTER releasing the lock (the focus
                        // calls don't touch cfg). Some(true) = overlay just turned
                        // ON → foreground the GUI; Some(false) = just turned OFF →
                        // restore the game; None = not the overlay toggle / feature
                        // off → do nothing.
                        let overlay_focus: Option<bool> = if matches!(
                            action,
                            hotkeys::HotkeyAction::ToggleKatangaOverlay
                        ) && cfg.overlay_show_gui
                        {
                            Some(cfg.overlay_enabled)
                        } else {
                            None
                        };
                        // Push the updated config to SHM directly so
                        // the viewer picks up the change without
                        // needing the GUI's update() to run.
                        let mut m = LiveParamsMapping::default();
                        m.params.enabled = 1;
                        // Set the appropriate one-shot pulse for
                        // command-style actions.
                        match action {
                            hotkeys::HotkeyAction::Recenter => {
                                m.params.recenter_request = 1;
                            }
                            hotkeys::HotkeyAction::Screenshot => {
                                m.params.screenshot_request = 1;
                            }
                            // BUG FIX: this arm was missing on the Default
                            // (RegisterHotKey) delivery path — only the low-level
                            // hook path set the request, so the Force-desktop
                            // hotkey silently did nothing unless the user had
                            // switched delivery method.
                            hotkeys::HotkeyAction::ForceDesktop => {
                                m.params.force_desktop_request = 1;
                            }
                            hotkeys::HotkeyAction::Restart => {
                                // Kill the viewer NOW, off the UI thread, so it
                                // dies the instant the key is pressed — matching
                                // the in-window button's synchronous kill. The
                                // main thread still does the relaunch below.
                                immediate_kill_viewer(&viewer_exe_name);
                                restart_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                                // Wake the UI loop so update() runs push_restart_request()
                                // (which re-kills harmlessly, then settles + relaunches).
                                egui_ctx.request_repaint();
                                continue;
                            }
                            _ => {}
                        }
                        OsirisGui::populate_live_params(&cfg, &mut m.params);
                        // Drop the cfg lock before acquiring the
                        // writer lock to avoid lock-ordering bugs.
                        drop(cfg);
                        write_shm(&writer, m);
                        // Now that the lock is released and SHM is updated, apply
                        // the overlay→show-GUI focus change (Windows-only; no-op
                        // otherwise). Only set when the overlay hotkey fired with
                        // the toggle enabled, so nothing else is affected.
                        match overlay_focus {
                            Some(true) => overlay_show_gui_now(),
                            Some(false) => overlay_restore_game_now(),
                            None => {}
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })
        .expect("Failed to spawn hotkey worker thread");
}

/// Apply a hotkey action to a `GuiConfig` directly. Pure function —
/// no SHM I/O, no logging, just config mutation. Used by both the
/// main thread (when egui's update path is active) and the background
/// hotkey worker (when the GUI is minimized).
fn apply_hotkey_to_cfg(action: hotkeys::HotkeyAction, cfg: &mut GuiConfig) {
    use hotkeys::HotkeyAction as A;
    const ZOOM_STEP: f32 = 5.0;
    const POS_STEP: f32 = 10.0;
    match action {
        A::Cycle3DMode => {
            let cycle = [
                StereoModeIndex::Mono,
                StereoModeIndex::LineInterlaced,
                StereoModeIndex::Checkerboard,
                StereoModeIndex::Sbs,
                StereoModeIndex::Tab,
            ];
            let cur = StereoModeIndex::from_u32(cfg.stereo_mode);
            let cur_idx = cycle.iter().position(|m| *m == cur).unwrap_or(0);
            cfg.stereo_mode = cycle[(cur_idx + 1) % cycle.len()] as u32;
        }
        A::CycleScreenShape => {
            let cycle = [
                StretchModeIndex::Sphere,
                StretchModeIndex::Box,
                StretchModeIndex::Fisheye,
            ];
            let cur = StretchModeIndex::from_u32(cfg.stretch_mode);
            let cur_idx = cycle.iter().position(|m| *m == cur).unwrap_or(0);
            cfg.stretch_mode = cycle[(cur_idx + 1) % cycle.len()] as u32;
        }
        A::Recenter | A::Screenshot | A::Restart => {
            // Pulse-only commands — no config change, the caller
            // sets the pulse flag in the SHM mapping.
        }
        A::ToggleHeadlock => cfg.head_lock = !cfg.head_lock,
        A::ToggleSim6dof => cfg.sim6dof_enabled = !cfg.sim6dof_enabled,
        A::ZoomIn => cfg.scale = (cfg.scale + ZOOM_STEP).clamp(1.0, 1000.0),
        A::ZoomOut => cfg.scale = (cfg.scale - ZOOM_STEP).clamp(1.0, 1000.0),
        A::OffsetZForward => {
            cfg.offset_z = (cfg.offset_z + POS_STEP).clamp(-500.0, 500.0)
        }
        A::OffsetZBackward => {
            cfg.offset_z = (cfg.offset_z - POS_STEP).clamp(-500.0, 500.0)
        }
        A::SwapEyes => cfg.swap_eyes = !cfg.swap_eyes,
        A::CyclePreset => { /* handled in apply_hotkey_action, not here */ }
        A::OffsetXLeft => {
            cfg.offset_x = (cfg.offset_x - POS_STEP).clamp(-50.0, 50.0)
        }
        A::OffsetXRight => {
            cfg.offset_x = (cfg.offset_x + POS_STEP).clamp(-50.0, 50.0)
        }
        A::OffsetYUp => {
            cfg.offset_y = (cfg.offset_y + POS_STEP).clamp(-50.0, 50.0)
        }
        A::OffsetYDown => {
            cfg.offset_y = (cfg.offset_y - POS_STEP).clamp(-50.0, 50.0)
        }
        A::RollOffsetLeft => {
            // 2 degrees CCW per press, in radians. Range matches the
            // GUI slider's clamp (-π..π).
            const ROLL_STEP_RAD: f32 = 2.0 * std::f32::consts::PI / 180.0;
            cfg.offset_roll = (cfg.offset_roll - ROLL_STEP_RAD)
                .clamp(-std::f32::consts::PI, std::f32::consts::PI);
        }
        A::RollOffsetRight => {
            const ROLL_STEP_RAD: f32 = 2.0 * std::f32::consts::PI / 180.0;
            cfg.offset_roll = (cfg.offset_roll + ROLL_STEP_RAD)
                .clamp(-std::f32::consts::PI, std::f32::consts::PI);
        }
        A::ToggleMouseEmu => cfg.mouse_emu_enabled = !cfg.mouse_emu_enabled,
        A::ToggleJoyEmu => cfg.joy_emu_enabled = !cfg.joy_emu_enabled,
        A::ToggleKatangaOverlay => cfg.overlay_enabled = !cfg.overlay_enabled,
        A::ToggleKatangaFilters => cfg.katanga_filters_enabled = !cfg.katanga_filters_enabled,
        A::ToggleUdp6dof => cfg.udp_6dof_enabled = !cfg.udp_6dof_enabled,
        // Pulse/command actions carry no persistent cfg state.
        A::ForceDesktop => {}
    }
}

/// Load `LOGO_BYTES` into an egui texture, trying several strategies so a
/// finicky ICO format doesn't kill the brand mark silently.
///
/// Strategy:
///   1. Let `image` decode the ICO normally — works for most ICO files.
///   2. Manually parse the ICONDIRENTRY, find the largest entry, and try
///      to decode its payload as either PNG or BMP.
///   3. Final fallback: build a BMP-with-DIB header from the raw payload
///      and feed it to `image::load_from_memory_with_format(BMP)`. ICO
///      payloads are BMPs *without* the BITMAPFILEHEADER, so we synthesise
///      that 14-byte prefix and let the BMP decoder do the rest.
/// Load `BANNER_BYTES` (the title-bar background image) into an egui
/// texture. Plain decode — no chroma-keying since the banner is meant
/// to fill the rect opaquely. Returns None if decode fails (e.g. the
/// embedded asset got corrupted), in which case the title bar falls
/// back to a solid panel colour.
fn load_banner_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let img = image::load_from_memory(BANNER_BYTES).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let pixels = rgba.into_raw();
    let color_image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
    Some(ctx.load_texture(
        "title_banner",
        color_image,
        egui::TextureOptions::LINEAR,
    ))
}

/// Load an image from an arbitrary file path (PNG/JPG/BMP/ICO) into a texture.
/// Used by the GUI THEME section for custom banner/logo overrides. Returns
/// None on any read/decode failure so callers can fall back to the bundled art.
fn load_texture_from_file(ctx: &egui::Context, path: &str, name: &str) -> Option<TextureHandle> {
    if path.is_empty() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let pixels = rgba.into_raw();
    let color_image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
    Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
}

fn load_logo_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    fn upload(ctx: &egui::Context, img: image::DynamicImage) -> TextureHandle {
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut pixels = rgba.into_raw();
        // Chroma-key out near-white pixels so the logo blends into the
        // header bar instead of carrying a hard white box around it.
        // Threshold is high (235) to keep anti-aliased edges, with a soft
        // alpha falloff for pixels that are just *off* white.
        for chunk in pixels.chunks_exact_mut(4) {
            let r = chunk[0] as i32;
            let g = chunk[1] as i32;
            let b = chunk[2] as i32;
            let min_channel = r.min(g).min(b);
            if min_channel > 235 {
                // Fully white-ish: knock out completely.
                chunk[3] = 0;
            } else if min_channel > 200 {
                // Nearly white: linear ramp 200..235 -> 255..0 alpha so the
                // edge transitions smoothly to transparent.
                let t = ((235 - min_channel) * 255 / 35).clamp(0, 255) as u8;
                // Multiply existing alpha by the ramp so already-transparent
                // pixels stay transparent.
                chunk[3] = ((chunk[3] as u32 * t as u32) / 255) as u8;
            }
        }
        let color_image =
            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
        ctx.load_texture("osiris-logo", color_image, egui::TextureOptions::LINEAR)
    }

    // (1) Direct ICO.
    match image::load_from_memory_with_format(LOGO_BYTES, image::ImageFormat::Ico) {
        Ok(img) => return Some(upload(ctx, img)),
        Err(err) => log::info!("logo: ICO decoder failed ({}); trying fallback", err),
    }

    // (2) Manual: peek into the ICONDIR, take the largest entry's payload,
    //     try PNG and BMP decoders against it directly.
    if LOGO_BYTES.len() >= 6 {
        let count = u16::from_le_bytes([LOGO_BYTES[4], LOGO_BYTES[5]]) as usize;
        let mut best: Option<(usize, usize, u32)> = None; // (offset, size, area)
        for i in 0..count {
            let entry_off = 6 + i * 16;
            if entry_off + 16 > LOGO_BYTES.len() {
                break;
            }
            let e = &LOGO_BYTES[entry_off..entry_off + 16];
            let w = if e[0] == 0 { 256 } else { e[0] as u32 };
            let h = if e[1] == 0 { 256 } else { e[1] as u32 };
            let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
            let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
            let area = w * h;
            if best.map_or(true, |b| area > b.2) {
                best = Some((off, size, area));
            }
        }
        if let Some((off, size, _)) = best {
            if off + size <= LOGO_BYTES.len() {
                let payload = &LOGO_BYTES[off..off + size];
                // PNG-in-ICO has the standard PNG signature.
                if payload.starts_with(&[0x89, b'P', b'N', b'G']) {
                    if let Ok(img) =
                        image::load_from_memory_with_format(payload, image::ImageFormat::Png)
                    {
                        return Some(upload(ctx, img));
                    }
                }
                // (3) Synthesise a BMP file by prepending a BITMAPFILEHEADER
                //     to the ICO payload (which is a DIB without the file
                //     header). The dimensions field in the DIB header
                //     reports doubled height for ICO's mask AND-mask hack;
                //     we patch that here too.
                if let Some(bmp) = ico_payload_to_bmp(payload) {
                    if let Ok(img) =
                        image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
                    {
                        return Some(upload(ctx, img));
                    }
                }
            }
        }
    }

    log::warn!("logo: all decode strategies failed for assets/logo.ico");
    None
}

/// Convert an ICO payload (DIB header + pixel data, no file header) into
/// a complete BMP byte stream. ICO stores BMPs with a doubled-height DIB
/// header (the second half is the AND mask), so we patch the height back
/// to the real value before prepending the file header.
fn ico_payload_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    // BITMAPINFOHEADER layout: size(4) width(4) height(4) planes(2) bpp(2) ...
    if dib.len() < 16 {
        return None;
    }
    let dib_header_size = u32::from_le_bytes([dib[0], dib[1], dib[2], dib[3]]) as usize;
    if dib_header_size < 12 || dib_header_size > dib.len() {
        return None;
    }
    let width = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
    let height_raw = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
    // ICO doubles the DIB's height field (image + mask). For our
    // 32-bpp single-image logo there's no mask, but the doubled-height
    // convention is still what's encoded.
    let real_height = height_raw / 2;
    let bpp = u16::from_le_bytes([dib[14], dib[15]]) as u32;

    // Compute pixel data offset and size.
    let row_bytes = ((width.unsigned_abs() * bpp + 31) / 32) * 4;
    let pixel_size = row_bytes * real_height.unsigned_abs();

    let file_size = 14 + dib_header_size as u32 + pixel_size;
    let pixel_offset = 14 + dib_header_size as u32;

    let mut out = Vec::with_capacity(file_size as usize);
    // BITMAPFILEHEADER (14 bytes).
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved1
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved2
    out.extend_from_slice(&pixel_offset.to_le_bytes());
    // DIB header + pixel data, with patched height.
    out.extend_from_slice(dib);
    if out.len() >= 14 + 12 {
        let h_le = real_height.to_le_bytes();
        out[14 + 8..14 + 12].copy_from_slice(&h_le);
    }
    Some(out)
}

fn sanitise(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | ' ' => out.push(ch),
            _ => out.push('_'),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        "preset".to_string()
    } else {
        trimmed.to_string()
    }
}

// --- Theme ----------------------------------------------------------------

/// No-op accent scope. Was previously used to repaint slider knobs /
/// checkbox checks in red, but the user reverted that look. Kept as a
/// passthrough so existing callsites (`red_slider(...)`, `red_checkbox(...)`)
/// continue to compile without each one having to be rewritten.
fn red_accent_scope<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    body(ui)
}

/// Render a slider — kept as a wrapper so we can re-introduce a colour
/// accent later without touching every callsite.
fn red_slider<'a>(ui: &mut egui::Ui, slider: egui::Slider<'a>) -> egui::Response {
    ui.add(slider)
}
/// Render a slider with its label shown ABOVE it in bright bold text.
/// Usage: red_slider_labeled(ui, "My Label", egui::Slider::new(&mut val, 0.0..=1.0))
fn red_slider_labeled<'a>(ui: &mut egui::Ui, label: &str, slider: egui::Slider<'a>) -> egui::Response {
    ui.add_space(2.0);
    ui.label(RichText::new(label).color(Color32::from_rgb(215, 222, 245)).strong().size(12.0));
    ui.add(slider.text(""))
}

fn red_slider_colored<'a>(ui: &mut egui::Ui, label: &str, color: Color32, slider: egui::Slider<'a>) -> egui::Response {
    ui.add_space(2.0);
    ui.label(RichText::new(label).color(color).strong().size(12.0));
    ui.add(slider.text(""))
}

/// Render a checkbox whose CHECKED state stands out visually:
///   * The label text turns red when checked, so a quick scan of the
///     panel shows which toggles are active.
///   * The check mark stroke is bumped to bright red and slightly
///     thicker so it pops inside the box.
///
/// We don't try to recolour the box's background fill: egui reads
/// that from `widgets.inactive.bg_fill` / `widgets.hovered.bg_fill`,
/// which are global per-widget-state styles — patching them would
/// affect every other interactive widget in scope. The text-colour
/// approach is reliable and survives egui version bumps.
fn red_checkbox(ui: &mut egui::Ui, checked: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let widget_text: egui::WidgetText = text.into();
    let label = widget_text.text().to_string();
    let coloured_text = if *checked {
        egui::RichText::new(label).color(COL_RED).strong()
    } else {
        egui::RichText::new(label).color(COL_TEXT)
    };

    // Bump the check-mark stroke colour to red. Egui's checkbox
    // draws the tick using the widget's `fg_stroke`, picked from the
    // current interaction state (inactive / hovered / active). We
    // patch all three so the stroke stays red regardless of hover.
    let prev_visuals = ui.visuals().clone();
    {
        let v = ui.visuals_mut();
        let red_stroke = Stroke::new(2.0, COL_RED);
        v.widgets.inactive.fg_stroke = red_stroke;
        v.widgets.hovered.fg_stroke = red_stroke;
        v.widgets.active.fg_stroke = red_stroke;
    }
    let resp = ui.checkbox(checked, coloured_text);
    *ui.visuals_mut() = prev_visuals;
    resp
}

/// Like `red_checkbox` but greyed-out (non-interactive) when `enabled` is
/// false — for axis toggles that only apply while their parent feature is on.
/// Keeps the same red label + red tick-when-checked styling for consistency.
fn red_checkbox_enabled(ui: &mut egui::Ui, enabled: bool, checked: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| red_checkbox(ui, checked, text)).inner
}

/// Checkbox for the "Experimental Features" list: near-white label when OFF,
/// and fully RED when ON — label, check-mark stroke AND box fill all go red
/// (combining red_checkbox's red label/tick with the Frame-pacing toggles' red
/// box fill) so a glance shows what's active. Always shows `tooltip` on hover.
fn exp_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str, tooltip: &str) -> egui::Response {
    let on = *checked;
    let prev_visuals = ui.visuals().clone();
    if on {
        let v = ui.visuals_mut();
        let red_stroke = Stroke::new(2.0, COL_RED);
        v.widgets.inactive.fg_stroke = red_stroke;
        v.widgets.hovered.fg_stroke  = red_stroke;
        v.widgets.active.fg_stroke   = red_stroke;
        v.widgets.inactive.bg_fill = Color32::from_rgb(160, 20, 20);
        v.widgets.hovered.bg_fill  = Color32::from_rgb(200, 30, 30);
        v.widgets.active.bg_fill   = Color32::from_rgb(220, 40, 40);
    }
    let coloured_text = if on {
        RichText::new(label).color(COL_RED).strong()
    } else {
        RichText::new(label).color(COL_TEXT)
    };
    let resp = ui.checkbox(checked, coloured_text);
    *ui.visuals_mut() = prev_visuals;
    let tip = tooltip.to_string();
    resp.on_hover_ui(move |ui| {
        ui.label(RichText::new(tip).color(Color32::from_rgb(0, 220, 245)).size(15.0));
    })
}

fn configure_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = COL_BG;
    style.visuals.panel_fill = COL_BG;
    style.visuals.faint_bg_color = COL_PANEL;
    style.visuals.extreme_bg_color = COL_PANEL_LIGHT;

    style.visuals.widgets.inactive.bg_fill = COL_PANEL_LIGHT;
    style.visuals.widgets.inactive.weak_bg_fill = COL_PANEL_LIGHT;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, COL_TEXT);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, COL_BORDER);
    style.visuals.widgets.inactive.rounding = Rounding::same(3.0);

    style.visuals.widgets.hovered.bg_fill = COL_BLUE_DIM;
    style.visuals.widgets.hovered.weak_bg_fill = COL_BLUE_DIM;
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, COL_BLUE);
    style.visuals.widgets.hovered.rounding = Rounding::same(3.0);

    style.visuals.widgets.active.bg_fill = COL_BLUE;
    style.visuals.widgets.active.weak_bg_fill = COL_BLUE;
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, COL_BLUE);
    style.visuals.widgets.active.rounding = Rounding::same(3.0);

    style.visuals.widgets.open.bg_fill = COL_PANEL_LIGHT;
    style.visuals.widgets.open.weak_bg_fill = COL_PANEL_LIGHT;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0, COL_BORDER);
    style.visuals.widgets.open.fg_stroke = Stroke::new(1.0, COL_TEXT);

    style.visuals.selection.bg_fill = COL_BLUE;
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.hyperlink_color = COL_BLUE;
    style.visuals.window_stroke = Stroke::new(1.0, COL_BORDER);
    style.visuals.window_rounding = Rounding::same(4.0);

    // Slider track styling. egui draws sliders using two fills: the
    // "rail" (the line you drag along) comes from `extreme_bg_color`
    // for the unfilled portion, and `widgets.inactive.bg_fill` for
    // most of the track on hover. We override both with the lighter
    // grey so the track is legible against the dark panel.
    style.visuals.widgets.inactive.bg_fill = COL_SLIDER_TRACK;
    style.visuals.extreme_bg_color = COL_SLIDER_TRACK;

    // Fonts — bump every text style so the GUI is readable on a
    // typical 1080p / 1440p monitor without the user having to lean in.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional)),
        (TextStyle::Body,    FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        (TextStyle::Button,  FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Small,   FontId::new(12.0, FontFamily::Proportional)),
    ]
    .into();

    style.spacing.slider_width = 220.0;
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.item_spacing = Vec2::new(10.0, 6.0);
    style.spacing.interact_size = Vec2::new(40.0, 20.0);

    // Slider handle: red on hover.
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(220, 30, 30);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(220, 30, 30);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.5, Color32::from_rgb(255, 80, 80));
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(200, 20, 20);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(200, 20, 20);

    ctx.set_style(style);
}

/// Render a compact slider with its label ABOVE the track.
/// Label is bright+bold. Value is shown inline on the slider.
fn compact_slider(ui: &mut egui::Ui, label: &str, slider: egui::Slider<'_>) -> egui::Response {
    ui.add_space(2.0);
    ui.label(RichText::new(label).color(Color32::from_rgb(225, 230, 248)).strong().size(12.0));
    let r = ui.add(slider.text(""));
    r
}


#[allow(dead_code)] // kept for reuse; last call site replaced by inline styling
fn blue_accent<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    add_contents(ui)
}

/// Render bold text with a red->blue chromatic outline.
///
/// Implementation: lay out the text three times — once in red offset
/// up-and-left, once in blue offset down-and-right, and once in the
/// fill colour centred. The two coloured copies form a thick gradient-y
/// halo with red-to-blue running diagonally across the glyphs. The
/// fill copy on top sells the readability.
///
/// To get a "bold" look without depending on a separate bold font
/// family being available, we also draw the fill copy slightly thicker
/// by replicating it at small offsets in the same colour — this fattens
/// the strokes optically by ~1 pixel.
fn outlined_label(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    fill: Color32,
    outline_a: Color32, // red end of the gradient
    outline_b: Color32, // blue end of the gradient
) {
    let font = egui::FontId::new(size, egui::FontFamily::Proportional);
    let galley_a = ui.painter().layout_no_wrap(text.into(), font.clone(), outline_a);
    let galley_b = ui.painter().layout_no_wrap(text.into(), font.clone(), outline_b);
    let galley_fill = ui.painter().layout_no_wrap(text.into(), font, fill);

    // Reserve space slightly larger than the text to accommodate the
    // ~3px halo on each side.
    let text_size = galley_fill.size();
    let (rect, _resp) = ui.allocate_exact_size(
        egui::vec2(text_size.x + 6.0, text_size.y + 6.0),
        egui::Sense::hover(),
    );
    let origin = rect.left_top() + egui::vec2(3.0, 3.0);

    let painter = ui.painter();

    // Outline pass: paint the red copy at four offsets up-and-left, the
    // blue copy at four offsets down-and-right. This gives the gradient
    // appearance — left/top of letters reads red, right/bottom reads
    // blue, with the fill colour blending in the middle.
    for (dx, dy) in [(-2.0, -2.0), (-2.0, -1.0), (-1.0, -2.0), (-1.0, -1.0)] {
        painter.galley(origin + egui::vec2(dx, dy), galley_a.clone(), outline_a);
    }
    for (dx, dy) in [(2.0, 2.0), (2.0, 1.0), (1.0, 2.0), (1.0, 1.0)] {
        painter.galley(origin + egui::vec2(dx, dy), galley_b.clone(), outline_b);
    }

    // Fill pass: replicate at zero + 4 cardinal sub-pixel offsets to
    // visually thicken the strokes ("fake bold").
    for (dx, dy) in [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)] {
        painter.galley(origin + egui::vec2(dx, dy), galley_fill.clone(), fill);
    }
}

/// Box with a blue header bar above a body region. The whole thing has a
/// thin blue border to keep the panels feeling "carded" like the mockup.
fn section<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    section_with_accent(ui, title, COL_HEADER_BG, COL_BORDER, add_body)
}

/// Purple-accented variant of `section`. Header background and outer
/// border use a saturated purple instead of the default blue, so the
/// three new head-tracking sections (Simulated 6DoF, Mouse Emulation,
/// 6DoF MODS) stand out as a related family distinct from the
/// rendering-control sections above.
fn section_purple<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_PURPLE_HEADER: Color32 = Color32::from_rgb(0x6E, 0x4A, 0xC0);
    const COL_PURPLE_BORDER: Color32 = Color32::from_rgb(0x6E, 0x4A, 0xC0);
    section_with_accent(ui, title, COL_PURPLE_HEADER, COL_PURPLE_BORDER, add_body)
}


/// Red-themed section for the VR Data to UDP panel. Header fill is a
/// saturated red; border is the same shade for a clean thin red frame.
fn section_red<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_RED_HEADER: Color32 = Color32::from_rgb(0xC0, 0x35, 0x35);
    const COL_RED_BORDER: Color32 = Color32::from_rgb(0xC0, 0x35, 0x35);
    section_with_accent(ui, title, COL_RED_HEADER, COL_RED_BORDER, add_body)
}

fn section_green<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_GREEN_HEADER: Color32 = Color32::from_rgb(22, 100, 38);
    const COL_GREEN_BORDER: Color32 = Color32::from_rgb(65, 220, 90);
    section_with_accent(ui, title, COL_GREEN_HEADER, COL_GREEN_BORDER, add_body)
}

fn section_orange<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_ORANGE_HEADER: Color32 = Color32::from_rgb(100, 52, 12);
    const COL_ORANGE_BORDER: Color32 = Color32::from_rgb(240, 135, 25);
    section_with_accent(ui, title, COL_ORANGE_HEADER, COL_ORANGE_BORDER, add_body)
}

fn section_cyan<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_CYAN_HEADER: Color32 = Color32::from_rgb(10, 90, 105);
    const COL_CYAN_BORDER: Color32 = Color32::from_rgb(0, 220, 245);
    section_with_accent(ui, title, COL_CYAN_HEADER, COL_CYAN_BORDER, add_body)
}

fn section_pink<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_PINK_HEADER: Color32 = Color32::from_rgb(110, 25, 80);
    const COL_PINK_BORDER: Color32 = Color32::from_rgb(255, 120, 215);
    section_with_accent(ui, title, COL_PINK_HEADER, COL_PINK_BORDER, add_body)
}

fn section_magenta<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    const COL_MAG_HEADER: Color32 = Color32::from_rgb(100, 20, 100);
    const COL_MAG_BORDER: Color32 = Color32::from_rgb(230, 80, 230);
    section_with_accent(ui, title, COL_MAG_HEADER, COL_MAG_BORDER, add_body)
}

/// Shared implementation for `section` and `section_purple`. Keeps the
/// frame layout and padding identical between variants — only the
/// header fill and outer border colour differ.
fn section_with_accent<R>(
    ui: &mut egui::Ui,
    title: &str,
    header_color: Color32,
    border_color: Color32,
    add_body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // Stretch the section frame to fill the full available width in the column.
    ui.set_min_width(ui.available_width());
    // Resolve colours/fills via the shared helper (theme override + background
    // translucency). Default title is white for the standard accent sections.
    let v = section_visuals(header_color, border_color, Color32::WHITE, COL_PANEL);
    let (header_fill, border_color, title_color, frame_fill, body_fill) =
        (v.header_fill, v.border, v.title, v.frame_fill, v.body_fill);

    // Paint the per-section background BEHIND this section (sized to last frame's
    // rect), so it sits behind the translucent fills/content.
    paint_prev_section_bg(ui, title);

    let resp = egui::Frame::none()
        .fill(frame_fill)
        .stroke(Stroke::new(1.0, border_color))
        .rounding(Rounding::same(4.0))
        .inner_margin(0.0)
        .show(ui, |ui| {
            // Ensure inner content area also fills full width.
            ui.set_min_width(ui.available_width());
            egui::Frame::none()
                .fill(header_fill)
                .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(title.to_uppercase())
                            .color(title_color)
                            .strong()
                            .size(17.0),
                    );
                });
            egui::Frame::none()
                .fill(body_fill)
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| add_body(ui))
                .inner
        });

    // Record this frame's rect for next frame's background paint.
    record_section_rect(title, resp.response.rect);
    resp.inner
}

/// Yellow-themed Hotkeys section. Slim, compact layout with a
/// delivery-method dropdown at the top, one row per `HotkeyAction`
/// for keyboard binding, and a fixed VR-controller mapping legend
/// at the bottom.
///
/// Returns `true` if the user changed any binding so the caller knows
/// to mark the config dirty.
fn hotkeys_section(
    ui: &mut egui::Ui,
    bindings: &mut hotkeys::HotkeyBindings,
    capturing: &mut Option<hotkeys::HotkeyAction>,
    mgr: &mut hotkeys::HotkeyManager,
    delivery: &mut u32,
    vr_enabled: &mut bool,
) -> bool {
    const COL_HOTKEY_HEADER: Color32 = Color32::from_rgb(220, 180, 30);
    const COL_HOTKEY_BORDER: Color32 = Color32::from_rgb(220, 180, 30);
    const COL_HOTKEY_BG: Color32 = Color32::from_rgb(20, 22, 28);
    const COL_VR_HEADER: Color32 = Color32::from_rgb(60, 120, 200);

    // Honor the active GUI theme + background like the standard sections. In the
    // default "Colored" theme this keeps the yellow header/black title look.
    let v = section_visuals(COL_HOTKEY_HEADER, COL_HOTKEY_BORDER, Color32::BLACK, COL_HOTKEY_BG);
    paint_prev_section_bg(ui, "⌨ HOTKEYS");

    let mut changed = false;
    let resp = egui::Frame::none()
        .fill(v.frame_fill)
        .stroke(Stroke::new(1.0, v.border))
        .rounding(Rounding::same(4.0))
        .inner_margin(0.0)
        .show(ui, |ui| {
            // Header bar.
            egui::Frame::none()
                .fill(v.header_fill)
                .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("⌨ HOTKEYS")
                            .color(v.title)
                            .strong()
                            .size(15.0),
                    );
                });
            egui::Frame::none()
                .fill(v.body_fill)
                .inner_margin(egui::Margin::symmetric(6.0, 6.0))
                .show(ui, |ui| {
                    // Description + delivery method dropdown on one row.
                    ui.label(
                        RichText::new(
                            "Global hotkeys — fire even when minimized. \
                             Click to bind, ESC cancels, X clears.",
                        )
                        .color(Color32::from_rgb(170, 170, 170))
                        .size(10.0),
                    );
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Method:")
                                .color(Color32::from_rgb(200, 200, 200))
                                .size(11.0),
                        );
                        let labels = ["Default", "Raw Input (games)"];
                        let mut current = (*delivery).min(1);
                        egui::ComboBox::from_id_source("hotkey_delivery")
                            .selected_text(labels[current as usize])
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for (i, lbl) in labels.iter().enumerate() {
                                    if ui.selectable_value(&mut current, i as u32, *lbl).changed() {
                                        changed = true;
                                    }
                                }
                            });
                        if current != *delivery { *delivery = current; changed = true; }
                    });
                    ui.add_space(4.0);

                    // ── Keyboard bindings — 2-column compact grid ─────────────
                    // 18 actions split into left (9) and right (9) columns,
                    // rendered side-by-side to halve vertical scrolling.
                    // Each column: [90px label] [95px bind btn] [18px X]
                    let all_actions = hotkeys::HotkeyAction::ALL;
                    let half = (all_actions.len() + 1) / 2;
                    let left_col  = &all_actions[..half];
                    let right_col = &all_actions[half..];

                    // Macro renders one bind row inline — avoids closure capture
                    // conflicts between the immutable borrow (reading capturing/bindings
                    // for label state) and the mutable borrow (writing back on click).
                    macro_rules! bind_row {
                        ($ui:expr, $action:expr) => {{
                            let action = $action;
                            let mut bind_clicked  = false;
                            let mut clear_clicked = false;
                            $ui.horizontal(|ui| {
                                ui.add_sized(
                                    [100.0, 20.0],
                                    egui::Label::new(
                                        RichText::new(action.label())
                                            .color(Color32::WHITE)
                                            .strong()
                                            .size(11.0),
                                    ),
                                );
                                let is_cap = *capturing == Some(action);
                                let bound_key = bindings.label_for(action);
                                let is_bound = !bound_key.is_empty();
                                let lbl = if is_cap {
                                    "press key…".to_string()
                                } else if is_bound {
                                    bound_key
                                } else {
                                    "bind".into()
                                };
                                // Capturing: yellow bg + black text
                                // Bound:     yellow bg + red bold text  <- easy to spot at a glance
                                // Unbound:   dark grey bg + dim text
                                let btn_col = if is_cap || is_bound {
                                    Color32::from_rgb(220, 180, 30) // yellow
                                } else {
                                    Color32::from_rgb(40, 42, 55)   // dark grey
                                };
                                let txt_col = if is_cap {
                                    Color32::BLACK
                                } else if is_bound {
                                    Color32::from_rgb(200, 30, 30)  // red
                                } else {
                                    Color32::from_rgb(130, 130, 150) // dim
                                };
                                // Bound: 11.5pt bold — clearly readable, still fits 65px button.
                                // Capturing: 10.5pt normal — "press key…" is longer so keep smaller.
                                // Unbound: 10.0pt dim — de-emphasised.
                                let rich = if is_bound && !is_cap {
                                    RichText::new(lbl).color(txt_col).size(11.5).strong()
                                } else if is_cap {
                                    RichText::new(lbl).color(txt_col).size(10.5)
                                } else {
                                    RichText::new(lbl).color(txt_col).size(10.0)
                                };
                                let btn = egui::Button::new(rich)
                                .fill(btn_col)
                                .min_size(egui::vec2(65.0, 18.0));
                                if ui.add(btn).clicked() { bind_clicked = true; }
                                let x_btn = egui::Button::new(
                                    RichText::new("X").color(Color32::WHITE).size(10.0).strong(),
                                )
                                .fill(Color32::from_rgb(80, 30, 30))
                                .min_size(egui::vec2(14.0, 18.0));
                                if ui.add(x_btn).clicked() { clear_clicked = true; }
                            });
                            (bind_clicked, clear_clicked)
                        }};
                    }

                    // Render side-by-side: iterate row index, place left then right.
                    for row in 0..half {
                        ui.horizontal(|ui| {
                            // Left column action.
                            if let Some(&la) = left_col.get(row) {
                                let (b, c) = bind_row!(ui, la);
                                if b { *capturing = Some(la); }
                                if c {
                                    if bindings.bindings.contains_key(&la) {
                                        bindings.clear(la); mgr.sync(bindings); changed = true;
                                    }
                                    if *capturing == Some(la) { *capturing = None; }
                                }
                            }
                            // Thin separator between columns.
                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(4.0);
                            // Right column action.
                            if let Some(&ra) = right_col.get(row) {
                                let (b, c) = bind_row!(ui, ra);
                                if b { *capturing = Some(ra); }
                                if c {
                                    if bindings.bindings.contains_key(&ra) {
                                        bindings.clear(ra); mgr.sync(bindings); changed = true;
                                    }
                                    if *capturing == Some(ra) { *capturing = None; }
                                }
                            }
                        });
                    }

                    // ── VR Controller mapping ─────────────────────────────────
                    egui::Frame::none()
                        .fill(Color32::from_rgb(30, 32, 40))
                        .stroke(Stroke::new(1.0, COL_VR_HEADER))
                        .rounding(Rounding::same(3.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            // Header row: icon + title + ON/OFF toggle.
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("🎮 VR Controller")
                                        .color(Color32::from_rgb(100, 160, 255))
                                        .strong()
                                        .size(12.0),
                                );
                                let enable_text = if *vr_enabled { "ON" } else { "OFF" };
                                let btn_color = if *vr_enabled {
                                    Color32::from_rgb(30, 120, 30)
                                } else {
                                    Color32::from_rgb(120, 30, 30)
                                };
                                if ui.add(
                                    egui::Button::new(
                                        RichText::new(enable_text).color(Color32::WHITE).size(11.0),
                                    )
                                    .fill(btn_color)
                                    .min_size(egui::vec2(34.0, 18.0)),
                                ).clicked() {
                                    *vr_enabled = !*vr_enabled;
                                    changed = true;
                                }
                            });
                            // V77: only show the mapping list when the controller
                            // is ON, so the section is short when OFF.
                            if *vr_enabled {
                            ui.add_space(3.0);
                            // 14 mappings in rows of 2 pairs (4 visual cols).
                            // Compact: btn pill + 2px + [80px label] + 4px gap between pairs.
                            let mappings = [
                                ("A",         "Swap eyes"),
                                ("B",         "Recenter"),
                                ("R.Trig",    "Mouse emu"),
                                ("R.Grip",    "6DoF"),
                                ("R.^ ",      "Z+ fwd"),
                                ("R.v ",      "Z- back"),
                                ("R.< ",      "X- left"),
                                ("R.> ",      "X+ right"),
                                ("R.Clk",     "Cycle 3D"),
                                ("L.Trig",    "Roll left"),
                                ("L.Grip",    "Screenshot"),
                                ("X",         "Headlock"),
                                ("L.^ ",      "Y+ up"),
                                ("L.v ",      "Y- down"),
                            ];
                            for chunk in mappings.chunks(4) {
                                ui.horizontal(|ui| {
                                    for (btn_lbl, action_lbl) in chunk {
                                        egui::Frame::none()
                                            .fill(Color32::from_rgb(30, 80, 180))
                                            .rounding(egui::Rounding::same(8.0))
                                            .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(*btn_lbl)
                                                        .color(Color32::WHITE)
                                                        .strong()
                                                        .size(10.5),
                                                );
                                            });
                                        ui.add_space(2.0);
                                        ui.add_sized(
                                            [65.0, 16.0],
                                            egui::Label::new(
                                                RichText::new(*action_lbl)
                                                    .color(Color32::from_rgb(255, 210, 60))
                                                    .size(10.5),
                                            ),
                                        );
                                        ui.add_space(4.0);
                                    }
                                });
                            }
                            } // end if *vr_enabled (mapping list)
                        });
                });
        });
    record_section_rect("⌨ HOTKEYS", resp.response.rect);
    changed
}

// --- UI -------------------------------------------------------------------

impl eframe::App for OsirisGui {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // GUI THEME: publish the selected theme id + background state for
        // section_with_accent, and on the first frame (config is now loaded from
        // disk) apply any saved custom banner/logo/backgrounds, falling back to
        // the bundled art where a custom file is absent or fails.
        //
        // Back-compat: a preset saved before `gui_theme_id` existed only has the
        // old `gui_dark_theme` bool. If the id is still default (0) but the bool
        // is set, promote to Dark Blue (1) so old "dark" presets keep their look.
        if self.cfg.gui_theme_id == 0 && self.cfg.gui_dark_theme {
            self.cfg.gui_theme_id = 1;
        }
        GUI_THEME_ID.store(self.cfg.gui_theme_id, std::sync::atomic::Ordering::Relaxed);
        // A background is "active" only when a path is set AND its texture loaded.
        let bg_active = (!self.cfg.section_bg_path.is_empty() && self.section_bg.is_some())
            || (!self.cfg.overall_bg_path.is_empty() && self.overall_bg.is_some());
        GUI_BG_ACTIVE.store(bg_active, std::sync::atomic::Ordering::Relaxed);
        SECTION_BG_TEX.with(|c| c.set(self.section_bg.as_ref().map(|t| t.id())));
        if !self.theme_assets_applied {
            self.theme_assets_applied = true;
            // Always (re)assign both: use the custom file when a path is set and
            // loads, otherwise fall back to the bundled art. This also correctly
            // REVERTS to the bundled art when a loaded preset clears the path.
            self.banner = load_texture_from_file(ctx, &self.cfg.custom_banner_path, "title_banner")
                .or_else(|| load_banner_texture(ctx));
            self.logo = load_texture_from_file(ctx, &self.cfg.custom_logo_path, "osiris-logo")
                .or_else(|| load_logo_texture(ctx));
            // Background images have no bundled fallback — None when unset/failed.
            self.section_bg = load_texture_from_file(ctx, &self.cfg.section_bg_path, "section_bg");
            self.overall_bg = load_texture_from_file(ctx, &self.cfg.overall_bg_path, "overall_bg");
        }
        // ── Window-collapse fix (option 2) ────────────────────────────────
        // Changing the Windows desktop resolution fires a DPI/scale change.
        // With persist_window on, eframe restores the saved size reinterpreted
        // against the new scale, which can collapse the panel to a tiny square.
        // We watch the scale factor; when it changes we sanity-check the window
        // size and, if it has collapsed below our minimum (or the saved value
        // is degenerate), re-assert the default size. Normal restarts at the
        // same resolution keep the remembered size untouched (the check passes
        // and does nothing). The re-assert is held for a few frames because
        // eframe may overwrite a single-frame resize during the scale change.
        const DEFAULT_W: f32 = 1400.0;
        const DEFAULT_H: f32 = 720.0;
        const MIN_W: f32 = 1000.0;
        const MIN_H: f32 = 540.0;
        let scale = ctx.pixels_per_point();
        match self.last_scale_factor {
            None => { self.last_scale_factor = Some(scale); }
            Some(prev) => {
                if (scale - prev).abs() > 0.001 {
                    // Scale changed (likely a desktop resolution change). Check
                    // the current window size; if collapsed, schedule a reset.
                    self.last_scale_factor = Some(scale);
                    let size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
                    let collapsed = match size {
                        Some(sz) => sz.x < MIN_W - 1.0 || sz.y < MIN_H - 1.0
                            || !sz.x.is_finite() || !sz.y.is_finite(),
                        None => true, // unknown size -> treat as needing a reset
                    };
                    if collapsed {
                        self.window_fix_frames = 6;
                    }
                }
            }
        }
        if self.window_fix_frames > 0 {
            self.window_fix_frames -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                egui::vec2(DEFAULT_W, DEFAULT_H),
            ));
        }

        // Poll restart hotkey flag set by background hotkey threads.
        if self.hotkey_restart_flag.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.push_restart_request();
        }

        // Sync only the fields that background hotkey threads can mutate.
        // We do NOT copy the whole GuiConfig (which would clobber
        // hotkey_bindings and slider-in-progress values set this frame).
        if let Ok(shadow) = self.shared_cfg.try_lock() {
            self.cfg.stereo_mode         = shadow.stereo_mode;
            self.cfg.stretch_mode        = shadow.stretch_mode;
            self.cfg.head_lock           = shadow.head_lock;
            self.cfg.sim6dof_enabled     = shadow.sim6dof_enabled;
            self.cfg.scale               = shadow.scale;
            self.cfg.offset_z            = shadow.offset_z;
            self.cfg.offset_x            = shadow.offset_x;
            self.cfg.offset_y            = shadow.offset_y;
            self.cfg.offset_roll         = shadow.offset_roll;
            self.cfg.swap_eyes           = shadow.swap_eyes;
            self.cfg.mouse_emu_enabled   = shadow.mouse_emu_enabled;
            self.cfg.joy_emu_enabled     = shadow.joy_emu_enabled;
            self.cfg.overlay_enabled     = shadow.overlay_enabled;
            self.cfg.udp_6dof_enabled    = shadow.udp_6dof_enabled;
        }

        // Stage 4b: keep the low-level keyboard hook worker in sync
        // with the user's chosen delivery method. Cheap when no
        // change (a u32 compare).
        self.sync_hook_worker();
        // Poll upstream events from viewer so VR controller hotkey
        // changes are reflected in GUI checkboxes immediately.
        if let Some(reader) = self.upstream_reader.as_mut() {
            if let Some(bits) = reader.poll() {
                let was_mouse = self.cfg.mouse_emu_enabled;
                let was_6dof  = self.cfg.sim6dof_enabled;
                let was_lock  = self.cfg.head_lock;
                let was_swap  = self.cfg.swap_eyes;
                let was_udp   = self.cfg.udp_6dof_enabled;
                self.cfg.mouse_emu_enabled = (bits & 1) != 0;
                self.cfg.sim6dof_enabled   = (bits & 2) != 0;
                self.cfg.head_lock         = (bits & 4) != 0;
                self.cfg.swap_eyes         = (bits & 8) != 0;
                self.cfg.udp_6dof_enabled  = (bits & 16) != 0;
                // Update status bar so user sees what changed.
                let mut changes: Vec<&str> = Vec::new();
                if self.cfg.mouse_emu_enabled != was_mouse {
                    changes.push(if self.cfg.mouse_emu_enabled { "Mouse emu: ON" } else { "Mouse emu: OFF" });
                }
                if self.cfg.sim6dof_enabled != was_6dof {
                    changes.push(if self.cfg.sim6dof_enabled { "6DoF: ON" } else { "6DoF: OFF" });
                }
                if self.cfg.head_lock != was_lock {
                    changes.push(if self.cfg.head_lock { "Headlock: ON" } else { "Headlock: OFF" });
                }
                if self.cfg.swap_eyes != was_swap {
                    changes.push(if self.cfg.swap_eyes { "Swap eyes: ON" } else { "Swap eyes: OFF" });
                }
                if self.cfg.udp_6dof_enabled != was_udp {
                    changes.push(if self.cfg.udp_6dof_enabled { "6DoF UDP: ON" } else { "6DoF UDP: OFF" });
                }
                if !changes.is_empty() {
                    self.status = changes.join(" | "); self.status_updated = std::time::Instant::now();
                }
            }
        }
        // Push current bindings into the worker's shared snapshot
        // each frame so re-bindings take effect immediately.
        if let Ok(mut b) = self.hook_bindings.lock() {
            *b = self.cfg.hotkey_bindings.clone();
        }
        // If the user clicked the window's X (or system close), send
        // a quit signal to the viewer through shared memory AND
        // forcibly terminate any viewer process. The SHM signal alone
        // proved unreliable: by the time the viewer's render loop
        // polls, the GUI process may have exited and Windows may have
        // unmapped the shared view before the write was visible. So
        // we do the SHM write (the polite shutdown path) AND kill
        // the viewer process by handle and name (the hard kill path).
        //
        // We check `close_requested()` first so this fires once on
        // the close attempt rather than every frame.
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.quit_pushed {
            self.push_quit_to_shm();
            self.quit_pushed = true;
            // Give the viewer's watchdog thread time to see quit_request,
            // set GLOBAL_QUIT, and call request_exit() to unblock xrWaitFrame.
            // This avoids racing TerminateProcess against xrWaitFrame which
            // can leave the OpenXR runtime in a dirty state on next launch.
            std::thread::sleep(std::time::Duration::from_millis(200));
            self.kill_viewer_process();
        }

        // ----- Global hotkeys -------------------------------------------
        // The background hotkey worker thread (spawned in App::new)
        // applies hotkey actions independently — this is what makes
        // hotkeys work when the window is minimized. It mutates the
        // shared shadow config and writes SHM directly without our
        // main thread's involvement.
        //
        // Here we:
        //   1. Copy any hotkey-induced changes from the shadow into
        //      our `self.cfg` so the GUI sliders reflect them.
        //   2. Drain any events that landed on our thread first
        //      (the channel is global; whichever thread polls first
        //      gets the event). Apply via `apply_hotkey_action` for
        //      its status-line side effects.
        //
        // `self.cfg` and the shadow may briefly disagree mid-frame,
        // but they reconverge here at the top of every update.
        if let Ok(shadow) = self.shared_cfg.lock() {
            // Cheap field-by-field copy — we don't replace the
            // whole struct because some fields (preset_name etc) are
            // GUI-thread-owned and the shadow doesn't track them.
            self.cfg.stereo_mode = shadow.stereo_mode;
            self.cfg.stretch_mode = shadow.stretch_mode;
            self.cfg.head_lock = shadow.head_lock;
            self.cfg.sim6dof_enabled = shadow.sim6dof_enabled;
            self.cfg.swap_eyes = shadow.swap_eyes;
            self.cfg.scale = shadow.scale;
            self.cfg.offset_x = shadow.offset_x;
            self.cfg.offset_y = shadow.offset_y;
            self.cfg.offset_z = shadow.offset_z;
        }
        let fired = self.hotkey_mgr.poll();
        for action in fired {
            self.apply_hotkey_action(action);
        }
        // If we're capturing a key for a binding, look at egui's key
        // events to see what the user pressed. We capture FIRST key
        // event, assign it, then re-sync the manager.
        if let Some(action) = self.capturing {
            let captured: Option<egui::Key> = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Key {
                        key, pressed: true, ..
                    } = e
                    {
                        Some(*key)
                    } else {
                        None
                    }
                })
            });
            if let Some(k) = captured {
                if k == egui::Key::Escape {
                    // ESC cancels capture without binding.
                    self.capturing = None;
                } else if let Some(code) = hotkeys::egui_key_to_code(k) {
                    self.cfg.hotkey_bindings.set(action, code);
                    self.capturing = None;
                    self.hotkey_mgr.sync(&self.cfg.hotkey_bindings);
                    self.push_to_shm(true);
                    ctx.request_repaint();
                }
            }
        }

        // Request continuous repainting so hotkey events are picked up
        // promptly even if the user isn't moving the mouse over our
        // window. (egui only schedules repaints on input by default.)
        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        let mut changed = false;

        // ----- Top bar: banner background + logo + title + buttons --------
        // The banner image (color.png — circuit board art) is painted
        // as the panel background, with logo/title/buttons layered on
        // top. If the banner failed to load, we fall back to the
        // plain panel fill so the layout still works.
        egui::TopBottomPanel::top("title_bar")
            .exact_height(76.0)
            .frame(
                egui::Frame::none()
                    .fill(COL_PANEL)
                    .stroke(Stroke::new(1.0, COL_BORDER)),
            )
            .show(ctx, |ui| {
                // Paint banner image stretched to fill the entire
                // title-bar rect BEFORE laying out widgets. Widgets
                // are drawn on top in subsequent paint calls.
                if let Some(tex) = &self.banner {
                    let rect = ui.available_rect_before_wrap();
                    let painter = ui.painter();
                    painter.image(
                        tex.id(),
                        rect,
                        egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(1.0, 1.0),
                        ),
                        Color32::WHITE,
                    );
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    if let Some(tex) = &self.logo {
                        let aspect = {
                            let s = tex.size_vec2();
                            s.x / s.y.max(1.0)
                        };
                        let target_h = 64.0;
                        // No filled frame around the logo anymore —
                        // it sits directly on the banner so the
                        // circuit-board art shows through any
                        // transparent edges.
                        ui.image((tex.id(), egui::vec2(target_h * aspect, target_h)));
                    }
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.add_space(4.0);
                        // Title: white bold with red->blue chromatic
                        // outline matching the spec.
                        outlined_label(
                            ui,
                            APP_TITLE,
                            32.0,
                            Color32::WHITE,
                            Color32::from_rgb(0xE0, 0x1A, 0x1A), // red
                            Color32::from_rgb(0x1A, 0x6E, 0xE0), // blue
                        );
                        {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("***").color(Color32::from_rgb(220, 40, 40)).size(13.0));
                                let link_response = ui.add(
                                    egui::Label::new(
                                        RichText::new("BerZerker96")
                                            .size(17.0)
                                            .color(Color32::from_rgb(0, 210, 220))
                                            .strong()
                                    ).sense(egui::Sense::click()),
                                );
                                if link_response.clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/BerZerker96?tab=repositories"));
                                }
                                ui.label(RichText::new("***").color(Color32::from_rgb(220, 40, 40)).size(13.0));
                            });
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        // Screenshot button (rightmost — first in
                        // right_to_left ordering).
                        let screenshot_btn = egui::Button::new(
                            RichText::new("📷 Screenshot")
                                .color(Color32::WHITE)
                                .size(13.0),
                        )
                        .fill(Color32::from_rgb(0x2A, 0x6E, 0xC0))
                        .rounding(Rounding::same(4.0));
                        if ui.add(screenshot_btn).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                            "Save the current left-eye VR view to a PNG file saved next to the viewer's .exe (osiris_screenshot_<timestamp>.png).",
                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).clicked() {
                            self.push_screenshot_request();
                        }

                        ui.add_space(4.0);
                        // Restart-screen button. Tears down and
                        // recreates the OpenXR session.
                        let restart_btn = egui::Button::new(
                            RichText::new("Restart")
                                .color(Color32::WHITE)
                                .size(13.0),
                        )
                        .fill(Color32::from_rgb(0x6E, 0x4A, 0xC0))
                        .rounding(Rounding::same(4.0));
                        if ui.add(restart_btn).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                            "Restart the OpenXR session (full re-init of the VR view).",
                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).clicked() {
                            self.push_restart_request();
                        }

                        ui.add_space(4.0);
                        // Recenter button. Re-anchors the screen to
                        // the current head pose.
                        let recenter_btn = egui::Button::new(
                            RichText::new("🎯 Recenter")
                                .color(Color32::WHITE)
                                .size(13.0),
                        )
                        .fill(Color32::from_rgb(0x4A, 0xA0, 0x6E))
                        .rounding(Rounding::same(4.0));
                        if ui.add(recenter_btn).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                            "Re-anchor the VR screen to your current head pose.",
                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).clicked() {
                            self.push_recenter_request();
                        }

                        ui.add_space(4.0);
                        // Debug diagnostics toggle.
                        // Red button + red checkbox when ON, orange button + grey when OFF.
                        let (btn_fill, _dot_color) = if self.cfg.diag_mode {
                            (Color32::from_rgb(180, 30, 30), Color32::from_rgb(220, 60, 60))
                        } else {
                            (Color32::from_rgb(0xC0, 0x6A, 0x00), Color32::from_rgb(130, 130, 130))
                        };
                        let debug_btn = egui::Button::new(
                            RichText::new("🔬 Debug")
                                .color(Color32::WHITE)
                                .size(13.0),
                        )
                        .fill(btn_fill)
                        .rounding(Rounding::same(4.0));
                        let resp = ui.add(debug_btn).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                            "Enable per-frame diagnostics logging to osiris-diagnostics.log.\nRed = logging ON.  Orange = OFF."
                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                        if resp.clicked() {
                            self.cfg.diag_mode = !self.cfg.diag_mode;
                            changed = true;
                        }
                        // Checkbox — turns red (checkmark visible) when debug on
                        let mut diag_check = self.cfg.diag_mode;
                        {
                            let mut style = ui.style_mut().clone();
                            if self.cfg.diag_mode {
                                style.visuals.widgets.inactive.fg_stroke =
                                    egui::Stroke::new(2.0, Color32::from_rgb(220, 60, 60));
                                style.visuals.selection.stroke =
                                    egui::Stroke::new(2.0, Color32::from_rgb(220, 60, 60));
                                style.visuals.selection.bg_fill =
                                    Color32::from_rgb(180, 30, 30);
                            }
                            ui.set_style(style);
                        }
                        let chk_resp = ui.add(egui::Checkbox::new(&mut diag_check, ""));
                        ui.reset_style();
                        if chk_resp.changed() {
                            self.cfg.diag_mode = diag_check;
                            changed = true;
                        }


                    });
                });
            });

        // ----- Bottom status bar ------------------------------------------
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(28.0)
            .frame(egui::Frame::none().fill(COL_PANEL))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    // Flash yellow for 2 s after a hotkey fires, then dim.
                    let age = self.status_updated.elapsed().as_secs_f32();
                    let color = if age < 2.0 {
                        let t = (age / 2.0).clamp(0.0, 1.0);
                        let r = (255.0 * (1.0 - t * 0.6)) as u8;
                        let g = (220.0 * (1.0 - t * 0.6)) as u8;
                        let b = (80.0 * (1.0 - t)) as u8;
                        egui::Color32::from_rgb(r, g, b)
                    } else {
                        COL_TEXT_DIM
                    };
                    ui.label(RichText::new(&self.status).color(color));
                    if age < 2.0 { ctx.request_repaint(); }
                });
            });

        // ----- Main area: three horizontal columns ------------------------
        // Scrollbar: wider and solid. Only geometry changes — no color overrides.
        ctx.style_mut(|s| {
            s.spacing.scroll.bar_width        = 14.0;
            s.spacing.scroll.bar_inner_margin = 2.0;
            s.spacing.scroll.bar_outer_margin = 0.0;
            s.spacing.scroll.floating         = false;
            s.visuals.extreme_bg_color        = Color32::from_rgb(0x12, 0x18, 0x24);
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(COL_BG).inner_margin(6.0))
            .show(ctx, |ui| {
                // Overall background image: painted first (behind everything) so
                // all sections/content draw on top. Fills the visible panel; the
                // scrolling content moves over it like a fixed backdrop. None =
                // unchanged look (opaque COL_BG frame fill shows as before).
                if let Some(bg) = &self.overall_bg {
                    let r = ui.max_rect();
                    ui.painter().image(
                        bg.id(),
                        r,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // We let egui distribute width naturally via ui.columns.
                    // Responsive three-column layout.
                    // Each column gets exactly 1/3 of available width
                    // so they shrink with the window instead of overlapping.

                    ui.columns(4, |cols| {
                        cols[0].vertical(|ui| {

                            // 0.6.0 layout: top section of column 0 is
                            // split horizontally into two sub-columns
                            // so the user's three "viewer setup"
                            // sections (3D Mode, Behaviour, Presets)
                            // sit next to each other rather than
                            // stacked. 3D Mode is taller (combo +
                            // 3 checkboxes) so it fills the left
                            // sub-column on its own; Behaviour
                            // (small) and Presets (medium) stack
                            // together in the right sub-column.
                            ui.columns(2, |sub| {
                                // Pin each sub-column to its allotted width up front.
                                // egui::columns sizes children equally, but a child
                                // whose CONTENT is wider than its share forces the
                                // whole `columns` (and thus column 1) wider. The
                                // Behaviour section's headlock sliders (DeJitter /
                                // Stable Lock) have value-boxes that, when shown,
                                // exceeded the share and made the column visibly jump
                                // wider. set_max_width clamps the content so the
                                // layout width is identical whether the sliders are
                                // expanded or collapsed.
                                let sub_w = sub[0].available_width();
                                sub[0].set_max_width(sub_w);
                                sub[1].set_max_width(sub[1].available_width());
                                sub[0].vertical(|ui| {
                                    ui.set_max_width(sub_w);
                                    section(ui, "🎬 3D Mode", |ui| {
                                        // 3D Mode section sits alone in the
                                        // left sub-col of column 0 row 0.
                                        // Spacing tuned so this section's
                                        // vertical height roughly matches
                                        // the Behaviour + Presets stack
                                        // next to it — fills the empty
                                        // area beneath the dropdown that
                                        // used to be visible.
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new("Stereo format")
                                                .color(COL_TEXT_DIM)
                                                .size(12.5),
                                        );
                                        let mut stereo =
                                            StereoModeIndex::from_u32(self.cfg.stereo_mode);
                                        egui::ComboBox::from_id_source("stereo_combo")
                                            .selected_text(stereo.label())
                                            .width(220.0)
                                            .show_ui(ui, |ui| {
                                                for m in [
                                                    StereoModeIndex::Mono,
                                                    StereoModeIndex::LineInterlaced,
                                                    StereoModeIndex::Checkerboard,
                                                    StereoModeIndex::Sbs,
                                                    StereoModeIndex::Tab,
                                                    StereoModeIndex::FullSbs,
                                                    StereoModeIndex::FullTab,
                                                ] {
                                                    if ui
                                                        .selectable_value(&mut stereo, m, m.label())
                                                        .changed()
                                                    {
                                                        changed = true;
                                                    }
                                                }
                                            });
                                        if stereo as u32 != self.cfg.stereo_mode {
                                            self.cfg.stereo_mode = stereo as u32;
                                            changed = true;
                                        }
                                        ui.label(
                                            RichText::new(match StereoModeIndex::from_u32(self.cfg.stereo_mode) {
                                                StereoModeIndex::Mono => "Single image, no stereoscopy.",
                                                StereoModeIndex::Sbs => "Half-width side-by-side (most 3D BluRay rips).",
                                                StereoModeIndex::FullSbs => "Full-width side-by-side (game mods like geo-11).",
                                                StereoModeIndex::Tab => "Half-height top-and-bottom.",
                                                StereoModeIndex::FullTab => "Full-height top-and-bottom.",
                                                StereoModeIndex::LineInterlaced => "Alternating scanlines (passive 3D TVs).",
                                                StereoModeIndex::Checkerboard => "Checkerboard 3D (DLP alternating pixel format).",
                                            })
                                            .color(COL_TEXT_DIM)
                                            .size(10.0),
                                        );
                                        ui.add_space(18.0);
                                        ui.separator();
                                        ui.add_space(10.0);
                                        ui.label(
                                            RichText::new("Stereo corrections")
                                                .color(COL_TEXT_DIM)
                                                .size(12.5),
                                        );
                                        ui.add_space(4.0);
                                        if red_checkbox(ui, &mut self.cfg.swap_eyes, "Swap eyes")
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        ui.label(
                                            RichText::new("Toggle if depth looks inverted (close objects appear far).")
                                                .color(COL_TEXT_DIM)
                                                .size(10.0),
                                        );
                                        ui.add_space(12.0);
                                    });
                                });
                                sub[1].vertical(|ui| {
                                    ui.set_max_width(sub_w);
                                    section(ui, "⚙ Behaviour", |ui| {
                                        // Behaviour is the NARROW half of column 1's
                                        // 2-way split, but the global slider track is
                                        // 220px (sized for the wide columns). At 220px
                                        // the headlock sliders (label + track + value
                                        // box) overflow this column's width and push
                                        // the value boxes off-screen / widen the
                                        // column. Shrink the track locally so every
                                        // slider here fits regardless of expand state.
                                        ui.spacing_mut().slider_width = 150.0;
                                        if red_checkbox(ui, &mut self.cfg.head_lock, "Head-lock screen")
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                        if red_checkbox(
                                            ui,
                                            &mut self.cfg.push_toggles,
                                            "Push toggles to viewer in realtime",
                                        )
                                        .changed()
                                        {
                                            changed = true;
                                        }
                                        // ── Head-lock mode ───────────────────────────────
                                        ui.label(RichText::new("Head-lock mode:")
                                            .color(COL_TEXT_DIM).size(12.5));
                                        let mode_labels = ["Default", "Delayed Lock", "Stable Lock"];
                                        let mut jm = self.cfg.headlock_jitter_method.min(2) as usize;
                                        egui::ComboBox::from_id_source("jitter_method")
                                            .selected_text(mode_labels[jm])
                                            .show_ui(ui, |ui| {
                                                for (i, label) in mode_labels.iter().enumerate() {
                                                    if ui.selectable_value(&mut jm, i, *label).changed() {
                                                        changed = true;
                                                    }
                                                }
                                            });
                                        if jm as u32 != self.cfg.headlock_jitter_method {
                                            self.cfg.headlock_jitter_method = jm as u32;
                                            // Delayed Lock (1) and Stable Lock (2) are head-lock modes —
                                            // they only do anything while the screen is head-locked, and
                                            // Stable Lock's parallax branch requires head_lock to be on.
                                            // Auto-enable it so picking the mode actually engages it
                                            // (otherwise the Parallax X/Y & Z sliders silently do nothing
                                            // and motion falls back to the regular sim6dof amount).
                                            if jm >= 1 {
                                                self.cfg.head_lock = true;
                                            }
                                            changed = true;
                                        }
                                        // ── DeJitter (works alongside ALL modes above) ──
                                        // Always-visible DeJitter collapsible; the on/off toggle
                                        // now lives inside it as the first row.
                                        let _g_dj = egui::CollapsingHeader::new(RichText::new("DeJitter")
                                                    .color(Color32::WHITE).strong().size(12.5))
                                                .id_source("grp_dejitter").default_open(!self.cfg.dejitter_collapsed).show(ui, |ui| {
                                            if red_checkbox(ui, &mut self.cfg.headlock_dejitter, "Enabled")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                "Soft lock: the screen PURSUES your head through a\ncritically-damped spring instead of being rigidly bolted to it.\nTracker noise becomes smooth drift (zero shimmer), fast turns leave\nthe screen lagging a few degrees with pleasant inertia, and it\nsettles with no overshoot. Composes with every mode above\n(Default / Delayed / Stable Lock).").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_slider_labeled(ui, "DeJitter stiffness",
                                                egui::Slider::new(
                                                    &mut self.cfg.headlock_dejitter_stiffness,
                                                    0.05..=10.0_f32,
                                                ).step_by(0.05).fixed_decimals(2))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "How quickly the screen catches up to your head.\nLow = floaty/cinematic, 0.40 = balanced default, higher = tighter/snappier (up to 10).").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_slider_labeled(ui, "DeJitter max lag (°)",
                                                egui::Slider::new(
                                                    &mut self.cfg.headlock_dejitter_max_lag,
                                                    1.0..=30.0_f32,
                                                ).step_by(1.0).fixed_decimals(0))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Fast head turns can never leave the screen further behind\nthan this many degrees — it gets pulled along.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            }); // end DeJitter collapsing
                                        { let o = _g_dj.openness > 0.5; if (!o) != self.cfg.dejitter_collapsed { self.cfg.dejitter_collapsed = !o; changed = true; } }
                                        // ── Parallax Prediction — its OWN collapsible, placed below the
                                        // DeJitter section so it is always reachable and applies to EVERY
                                        // lock mode (isolated post-stage on the final head-lock pose). Same
                                        // header/persistence style as the DeJitter & Stable-Lock groups.
                                        ui.add_space(4.0);
                                        let _g_pp = egui::CollapsingHeader::new(RichText::new("PARALLAX PREDICTION")
                                                .color(Color32::WHITE).strong().size(12.5))
                                            .id_source("grp_parallax").default_open(!self.cfg.parallax_collapsed).show(ui, |ui| {
                                            if red_checkbox(ui, &mut self.cfg.parallax_prediction, "Parallax Prediction")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Predicts where your head is turning and glides the screen there,\nramping every correction instead of snapping. Smooths the jitter\nand double-motion you get when head-lock and head-tracking run at\nonce. Works on top of any lock mode.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_slider_labeled(ui, "Parallax Prediction amount",
                                                egui::Slider::new(
                                                    &mut self.cfg.parallax_prediction_amt,
                                                    0.0..=1.0_f32,
                                                ).step_by(0.01).fixed_decimals(2))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Strength of the predict-and-glide effect.\nLow = subtle, high = stronger smoothing with more look-ahead.\nBack off if fast turns start to overshoot or swim.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            // PP #3 — adaptive smoothing + deadband (CPU-only; only acts while
                                            // Parallax Prediction is on).
                                            if red_checkbox(ui, &mut self.cfg.pp_adaptive, "Adaptive smoothing")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Smooths hard when your head is nearly still (kills shimmer) and\neases off automatically as you turn faster (stays responsive).\nAlso enables the deadband below.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if self.cfg.pp_adaptive {
                                                if red_slider_labeled(ui, "Deadband (°)",
                                                    egui::Slider::new(
                                                        &mut self.cfg.pp_deadband_deg,
                                                        0.0..=2.0_f32,
                                                    ).step_by(0.05).fixed_decimals(2))
                                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                        "Head movements smaller than this are ignored, so a still head\nreads rock-steady. Too high makes slow turns feel sticky.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                    .changed() { changed = true; }
                                            }
                                            // PP #4 — acceleration-aware prediction (CPU-only; only acts while
                                            // Parallax Prediction is on).
                                            if red_slider_labeled(ui, "Acceleration prediction",
                                                egui::Slider::new(
                                                    &mut self.cfg.pp_accel,
                                                    0.0..=1.0_f32,
                                                ).step_by(0.01).fixed_decimals(2))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Lets the prediction account for how fast your turn is speeding up\nor slowing down — curves the guess through the start and end of\nturns. Helps fast stops; too high can overshoot.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_checkbox(ui, &mut self.cfg.pp_runtime_vel, "Runtime velocity (A)")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Drive the prediction from the headset runtime's own angular velocity\ninstead of estimating it by differencing poses. Lower latency and less\nnoise at once. Falls back automatically if the runtime reports none.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_checkbox(ui, &mut self.cfg.pp_photon_horizon, "Photon horizon (B)")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Extend the look-ahead by one measured display frame (motion-to-photon)\nso the guess also covers the real frame latency, not just the smoothing.\nBounded by the same overshoot cap.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_checkbox(ui, &mut self.cfg.pp_euro, "Euler (1-Euro) filter")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Speed-adaptive velocity smoothing: heavy when your head is near-still\n(kills jitter), light when you turn fast (no lag). The standard fix for\nthe jitter-vs-lag tradeoff. Replaces the fixed smoothing.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            }); // end Parallax Prediction collapsing
                                        { let o = _g_pp.openness > 0.5; if (!o) != self.cfg.parallax_collapsed { self.cfg.parallax_collapsed = !o; changed = true; } }
                                        if self.cfg.headlock_jitter_method == 1 {
                                            if red_slider_labeled(ui, "Delay (ms)",
                                                egui::Slider::new(
                                                    &mut self.cfg.headlock_delay_ms,
                                                    0.0..=500.0_f32,
                                                ).step_by(5.0).fixed_decimals(0))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "How many milliseconds the screen lags behind your head.
                                                     0 = no delay (instant follow).
                                                     80 = default, subtle cinematic feel.
                                                     200-400 = very delayed, content floats into position.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                        }
                                        if self.cfg.headlock_jitter_method == 2 {
                                            let _g_sl = egui::CollapsingHeader::new(RichText::new("STABLE LOCK SLIDERS")
                                                    .color(Color32::WHITE).strong().size(12.5))
                                                .id_source("grp_stablelock").default_open(!self.cfg.stable_lock_collapsed).show(ui, |ui| {
                                            ui.add_space(2.0);
                                            if red_slider_labeled(ui, "Parallax X/Y",
                                                egui::Slider::new(
                                                    &mut self.cfg.stable_lock_parallax_xy,
                                                    0.0..=50.0_f32,
                                                ).step_by(0.1).fixed_decimals(1))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Lateral (left/right) + vertical (up/down) parallax as you lean —
works like Simulated 6DoF intensity, exclusive to Stable Lock.
0 = none, ~0.5 = moderate, higher = stronger screen shift.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if red_slider_labeled(ui, "Parallax Z",
                                                egui::Slider::new(
                                                    &mut self.cfg.stable_lock_parallax_z,
                                                    0.0..=50.0_f32,
                                                ).step_by(0.1).fixed_decimals(1))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Forward/back lean parallax — zoom-like depth feel.
0 = none, ~0.5 = moderate, higher = stronger.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            // ── Stable Lock DIRECTIONAL parallax (rotation-driven) ──
                                            if red_checkbox(ui, &mut self.cfg.stable_lock_dir_enabled, "Directional parallax (rotation)")
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                    "Rotation-driven look-around parallax — Stable Lock's own\nversion of Simulated 6DoF's Directional feature, with its own\nstrength below. Exclusive to Stable Lock; the Directional 6DOF\nMods section is unaffected. Off = only the translation\nParallax X/Y and Z sliders above move the screen.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                            if self.cfg.stable_lock_dir_enabled {
                                                if red_slider_labeled(ui, "Directional strength",
                                                    egui::Slider::new(
                                                        &mut self.cfg.stable_lock_dir_strength,
                                                        0.0..=30.0_f32,
                                                    ).step_by(0.05).fixed_decimals(2))
                                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                        "How much the screen shifts as you TURN your head.\n0 = none, 0.75 = previous default feel, higher = stronger.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                    .changed() { changed = true; }
                                            }
                                            }); // end Stable Lock collapsing
                                            { let o = _g_sl.openness > 0.5; if (!o) != self.cfg.stable_lock_collapsed { self.cfg.stable_lock_collapsed = !o; changed = true; } }
                                        }
                                    });
                                    ui.add_space(4.0);

                            // 0.6.0 layout: Presets moved here from
                            // column 2 (which is now Simulated 6DoF).
                            // Compact form: one row for the name +
                            // primary save/load buttons, a dropdown for
                            // the saved-presets list instead of the old
                            // expanded vertical list. The dropdown
                            // saves a lot of vertical space which this
                            // narrower column needs.
                                });  // close sub[1].vertical
                            });      // close ui.columns(2)
                            ui.add_space(4.0);

                                                        section(ui, "💾 Presets", |ui| {
                                ui.label(
                                    RichText::new("`default` auto-loads on startup")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                ui.add_space(4.0);

                                // ── Row 1: Name field — centered, brighter background ──
                                ui.horizontal(|ui| {
                                    let available = ui.available_width();
                                    let field_w = 160.0;
                                    let label_w = 42.0;
                                    let pad = ((available - field_w - label_w) / 2.0).max(0.0);
                                    ui.add_space(pad);
                                    ui.label(RichText::new("Name:").strong().size(12.0));
                                    // Brighter text-edit background
                                    ui.visuals_mut().extreme_bg_color = Color32::from_rgb(45, 55, 80);
                                    ui.add(egui::TextEdit::singleline(&mut self.preset_name).desired_width(field_w));
                                });
                                ui.add_space(6.0);

                                // ── Rows 2+3: 3-column grid ───────────────────────
                                // Shared widths so buttons align across rows:
                                //   col0: Save (top) same width as preset chip (bottom)
                                //   col1: Save default == Load width
                                //   col2: Refresh == Overwrite width
                                let presets_snap = self.available_presets.clone();
                                // Measure Save default button width for col1 alignment
                                const SD_W: f32 = 110.0; // Save default approx width
                                const OW_W: f32 = 90.0;  // Overwrite approx width
                                const SAVE_W: f32 = 60.0; // Save button width
                                ui.columns(3, |cols| {
                                    // ── Col 0: Save (cyan) / preset chip ─────────
                                    cols[0].vertical_centered(|ui| {
                                        // Save — cyan idle, red hover
                                        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::from_rgb(10, 90, 105);
                                        ui.visuals_mut().widgets.inactive.bg_fill      = Color32::from_rgb(10, 90, 105);
                                        ui.visuals_mut().widgets.inactive.bg_stroke    = egui::Stroke::new(1.0, Color32::from_rgb(0, 220, 245));
                                        ui.visuals_mut().widgets.hovered.weak_bg_fill  = Color32::from_rgb(180, 20, 20);
                                        ui.visuals_mut().widgets.hovered.bg_fill       = Color32::from_rgb(180, 20, 20);
                                        ui.visuals_mut().widgets.hovered.bg_stroke     = egui::Stroke::new(1.0, Color32::from_rgb(255, 80, 80));
                                        if ui.add_sized(
                                            [SAVE_W, 24.0],
                                            egui::Button::new(RichText::new("Save").strong().size(12.0).color(Color32::WHITE)),
                                        ).clicked() {
                                            let nm = self.preset_name.clone();
                                            match self.save_preset(&nm) {
                                                Ok(_) => self.refresh_preset_list(),
                                                Err(e) => { self.status = format!("Save failed: {}", e); self.status_updated = std::time::Instant::now(); }
                                            }
                                        }
                                        ui.add_space(6.0);
                                        // Preset name chip — slightly wider than Save
                                        if let Some(nm) = presets_snap.first() {
                                            ui.allocate_ui(egui::vec2(SAVE_W + 12.0, 26.0), |ui| {
                                                egui::Frame::none()
                                                    .fill(Color32::from_rgb(30, 60, 140))
                                                    .stroke(egui::Stroke::new(1.5, Color32::from_rgb(200, 40, 40)))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .rounding(Rounding::same(3.0))
                                                    .show(ui, |ui| {
                                                        ui.set_min_width(SAVE_W + 12.0);
                                                        ui.label(RichText::new(nm).size(13.0).strong().color(Color32::WHITE));
                                                    });
                                            });
                                        }
                                    });

                                    // ── Col 1: Save default / Load (same width) ──
                                    cols[1].vertical_centered(|ui| {
                                        ui.visuals_mut().widgets.inactive.weak_bg_fill = COL_RED;
                                        ui.visuals_mut().widgets.inactive.bg_fill      = COL_RED;
                                        ui.visuals_mut().widgets.inactive.bg_stroke    = egui::Stroke::new(1.0, COL_RED_DIM);
                                        ui.visuals_mut().widgets.hovered.weak_bg_fill  = Color32::from_rgb(180, 20, 20);
                                        ui.visuals_mut().widgets.hovered.bg_fill       = Color32::from_rgb(180, 20, 20);
                                        ui.visuals_mut().widgets.hovered.bg_stroke     = egui::Stroke::new(1.0, Color32::from_rgb(255, 80, 80));
                                        if ui.add_sized(
                                            [SD_W, 24.0],
                                            egui::Button::new(egui::RichText::new("Save default").color(Color32::WHITE).strong()),
                                        ).clicked() {
                                            match self.save_preset("default") {
                                                Ok(_) => {
                                                    self.refresh_preset_list();
                                                    // Saving as "default" auto-applies to the running viewer
                                                    // ONLY when reload-after-save is enabled. With it off, the
                                                    // file is written but not pushed live — avoiding the
                                                    // viewer's watcher reload (which can freeze the render
                                                    // loop on some setups).
                                                    if self.cfg.reload_preset_after_save {
                                                        self.push_to_shm(true);
                                                        self.status = "Saved as default & applied.".to_string();
                                                    } else {
                                                        self.status = "Saved as default (no live reload).".to_string();
                                                    }
                                                    self.status_updated = std::time::Instant::now();
                                                }
                                                Err(e) => { self.status = format!("Save failed: {}", e); self.status_updated = std::time::Instant::now(); }
                                            }
                                        }
                                        ui.add_space(6.0);
                                        // Load — same width as Save default
                                        if let Some(nm) = presets_snap.first().cloned() {
                                            ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::from_rgb(30, 120, 50);
                                            ui.visuals_mut().widgets.inactive.bg_fill      = Color32::from_rgb(30, 120, 50);
                                            ui.visuals_mut().widgets.inactive.bg_stroke    = egui::Stroke::new(1.0, Color32::from_rgb(60, 180, 80));
                                            ui.visuals_mut().widgets.hovered.weak_bg_fill  = Color32::from_rgb(180, 20, 20);
                                            ui.visuals_mut().widgets.hovered.bg_fill       = Color32::from_rgb(180, 20, 20);
                                            ui.visuals_mut().widgets.hovered.bg_stroke     = egui::Stroke::new(1.0, Color32::from_rgb(255, 80, 80));
                                            if ui.add_sized(
                                                [SD_W, 24.0],
                                                egui::Button::new(RichText::new("Load").color(Color32::WHITE).strong().size(12.0))
                                                    .rounding(Rounding::same(3.0)),
                                            ).clicked() {
                                                if let Err(e) = self.load_preset(&nm) {
                                                    self.status = format!("Load failed: {}", e);
                                                    self.status_updated = std::time::Instant::now();
                                                }
                                            }
                                        }
                                    });

                                    // ── Col 2: Refresh (cyan) / Overwrite ────────
                                    cols[2].vertical_centered(|ui| {
                                        // Refresh  — cyan idle, red hover
                                        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::from_rgb(10, 90, 105);
                                        ui.visuals_mut().widgets.inactive.bg_fill      = Color32::from_rgb(10, 90, 105);
                                        ui.visuals_mut().widgets.inactive.bg_stroke    = egui::Stroke::new(1.0, Color32::from_rgb(0, 220, 245));
                                        ui.visuals_mut().widgets.hovered.weak_bg_fill  = Color32::from_rgb(180, 20, 20);
                                        ui.visuals_mut().widgets.hovered.bg_fill       = Color32::from_rgb(180, 20, 20);
                                        ui.visuals_mut().widgets.hovered.bg_stroke     = egui::Stroke::new(1.0, Color32::from_rgb(255, 80, 80));
                                        if ui.add_sized(
                                            [OW_W, 24.0],
                                            egui::Button::new(RichText::new("Refresh").size(14.0).color(Color32::WHITE)),
                                        )
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new("Refresh list").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                            .clicked()
                                        {
                                            self.refresh_preset_list(); self.status = "Preset list refreshed.".to_string(); self.status_updated = std::time::Instant::now();
                                        }
                                        ui.add_space(6.0);
                                        // Overwrite button under refresh
                                        if let Some(nm) = presets_snap.first().cloned() {
                                            ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::from_rgb(100, 20, 150);
                                            ui.visuals_mut().widgets.inactive.bg_fill     = Color32::from_rgb(100, 20, 150);
                                            ui.visuals_mut().widgets.inactive.bg_stroke   = egui::Stroke::new(1.0, Color32::from_rgb(200, 100, 255));
                                            ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::from_rgb(180, 20, 20);
                                            ui.visuals_mut().widgets.hovered.bg_fill      = Color32::from_rgb(180, 20, 20);
                                            ui.visuals_mut().widgets.hovered.bg_stroke    = egui::Stroke::new(1.0, Color32::from_rgb(255, 150, 150));
                                            ui.visuals_mut().widgets.hovered.fg_stroke    = egui::Stroke::new(1.5, Color32::from_rgb(255, 215, 0));
                                            let ow_btn = egui::Button::new(RichText::new("Overwrite").color(Color32::from_rgb(255, 215, 0)).strong().size(12.0))
                                                .rounding(Rounding::same(3.0));
                                            if ui.add(ow_btn)
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new("Save current settings over this preset").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .clicked()
                                            {
                                                match self.save_preset(&nm) {
                                                    Ok(_) => { self.status = format!("Overwritten: {}", nm); self.status_updated = std::time::Instant::now(); self.refresh_preset_list(); }
                                                    Err(e) => { self.status = format!("Overwrite failed: {}", e); self.status_updated = std::time::Instant::now(); }
                                                }
                                            }
                                        }
                                    });
                                });
                                ui.add_space(4.0);
                                // Saved count label
                                if !self.available_presets.is_empty() {
                                    ui.label(RichText::new(format!("Saved presets ({}):", self.available_presets.len())).color(COL_TEXT_DIM).size(12.5));
                                }
                                ui.add_space(4.0);
                                ui.separator();
                                // Reload-after-save toggle: when OFF, "Save default" writes the file
                                // but does NOT push live to the viewer, avoiding the watcher reload
                                // that can freeze the render loop on some setups.
                                if red_checkbox(ui, &mut self.cfg.reload_preset_after_save, "Reload preset after save")
                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "When ON, saving over the default preset immediately applies it to the running viewer.
Turn OFF if saving the default causes the viewer to freeze — the file is still written, just not hot-reloaded (restart or Load to apply).").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed() { changed = true; }
                                ui.add_space(2.0);

                            });
                            ui.add_space(4.0);

                            section(ui, "📐 Geometry", |ui| {
                                // Stretch-mode selector — first control in
                                // the geometry tab so the user picks how the
                                // peripheral effect works before adjusting
                                // anything else. Two implementations:
                                //   - Sphere: rays project onto an inside-
                                //     out sphere mesh wrapping the user.
                                //   - Mesh extension: the screen mesh
                                //     itself grows outward, with peripheral
                                //     UVs remapped to stretched edge pixels.
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Screen Shape:").color(Color32::from_rgb(255, 220, 0)).strong());
                                    let mut sm = StretchModeIndex::from_u32(
                                        self.cfg.stretch_mode,
                                    );
                                    egui::ComboBox::from_id_source("stretch_mode_combo")
                                        .selected_text(sm.label())
                                        .show_ui(ui, |ui| {
                                            // 0.6.0: MeshExtension removed.
                                            // Sphere is the new default and
                                            // appears first; Box is for
                                            // users who want hard edges;
                                            // Fisheye is the new cinematic
                                            // ultrawide curved panel.
                                            for m in [
                                                StretchModeIndex::Sphere,
                                                StretchModeIndex::Box,
                                                StretchModeIndex::Fisheye,
                                            ] {
                                                if ui
                                                    .selectable_value(&mut sm, m, m.label())
                                                    .changed()
                                                {
                                                    changed = true;
                                                }
                                            }
                                        });
                                    if sm as u32 != self.cfg.stretch_mode {
                                        self.cfg.stretch_mode = sm as u32;
                                        changed = true;
                                    }
                                });
                                if red_slider_labeled(ui, "Distance (m)", egui::Slider::new(&mut self.cfg.distance, 1.0..=4000.0)
                                            )
                                    .changed()
                                {
                                    changed = true;
                                }
                                ui.label(RichText::new("Offsets (m)").color(Color32::from_rgb(255, 220, 0)).strong());
                                if red_slider(ui, egui::Slider::new(
                                            &mut self.cfg.offset_x,
                                            -50.0..=50.0,
                                        )
                                        .text("X"))
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider(ui, egui::Slider::new(
                                            &mut self.cfg.offset_y,
                                            -50.0..=50.0,
                                        )
                                        .text("Y"))
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider(ui, egui::Slider::new(
                                            &mut self.cfg.offset_z,
                                            -500.0..=500.0,
                                        )
                                        .text("Z"))
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider_labeled(ui, "Roll (rad)", egui::Slider::new(
                                            &mut self.cfg.offset_roll,
                                            -std::f32::consts::PI..=std::f32::consts::PI,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                // Sphere-mode sliders. All four are
                                // greyed out when not in sphere mode.
                                // Sphere width/height/curve sliders also
                                // drive fisheye mode (which uses the same
                                // ray-sphere shader path), just with a
                                // 1.5× wider horizontal extent baked in
                                // by the fragment shader. So enable them
                                // for stretch_mode 0 (Sphere) and 3
                                // (Fisheye).
                                let in_sphere_mode = self.cfg.stretch_mode == 0
                                    || self.cfg.stretch_mode == 3
                ;
                                // ── Sphere / Fisheye sliders (hidden on other shapes) ──
                                if in_sphere_mode {
                                    // Yellow "Curvature" collapse toggle — folds away
                                    // ONLY these four sphere shaping sliders.
                                    let _g_cv = egui::CollapsingHeader::new(RichText::new("CURVATURE")
                                            .color(Color32::from_rgb(255, 215, 0)).strong().size(12.5))
                                        .id_source("grp_curvature").default_open(!self.cfg.curvature_collapsed).show(ui, |ui| {
                                    ui.label(RichText::new("Sphere width (rad)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.sphere_x_size, 0.05..=3.0).text("")).changed() { changed = true; }
                                    ui.label(RichText::new("Sphere height (rad)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.sphere_y_size, 0.05..=3.0).text("")).changed() { changed = true; }
                                    ui.label(RichText::new("Sphere X curve").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.sphere_x_curve, 0.0..=1.0).text("")).changed() { changed = true; }
                                    ui.label(RichText::new("Sphere Y curve").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.sphere_y_curve, 0.0..=1.0).text("")).changed() { changed = true; }
                                    }); // end Curvature collapsing
                                    { let o = _g_cv.openness > 0.5; if (!o) != self.cfg.curvature_collapsed { self.cfg.curvature_collapsed = !o; changed = true; } }
                                }

                                // ── Box sliders (hidden on other shapes) ──
                                let in_box_mode = self.cfg.stretch_mode == 2;
                                if in_box_mode {
                                    ui.label(RichText::new("Box X size").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.box_x_size, 0.1..=5.0).text("")).changed() { changed = true; }
                                    ui.label(RichText::new("Box Y size").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.box_y_size, 0.1..=5.0).text("")).changed() { changed = true; }
                                    ui.label(RichText::new("Box depth").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.box_z_depth, 0.1..=5.0).text("")).changed() { changed = true; }
                                    ui.label(RichText::new("Box corner round").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    if ui.add(egui::Slider::new(&mut self.cfg.box_corner_radius, 0.0..=1.0).text("")).changed() { changed = true; }
                                }
                                ui.separator();

                                // -------- 5-zone concave bowl --------
                                // Works in Sphere / Fisheye / Box.
                                // Master strength + global depth + shape
                                // control the overall bowl. Per-zone
                                // depths (z0=centre, z4=rim) let you
                                // sculpt each ring independently. A
                                // Catmull-Rom spline blends between
                                // zones — no sharp kinks.
                                let _g_concave = egui::CollapsingHeader::new(
                                    RichText::new("CONCAVE")
                                        .color(Color32::from_rgb(255, 220, 0))
                                        .strong()
                                        .size(16.0),
                                )
                                    .id_source("grp_concave")
                                    .default_open(self.cfg.grp_concave_open)
                                    .show(ui, |ui| {
                                macro_rules! concave_slider {
                                    ($field:expr, $label:expr, $max:expr) => {{
                                        ui.add_space(1.0);
                                        ui.label(RichText::new($label).color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                        let r = ui.add(egui::Slider::new($field, -$max..=$max).text(""));
                                        if r.changed() { changed = true; }
                                    }};
                                }
                                concave_slider!(&mut self.cfg.concave_strength, "Master strength", 4.0_f32);
                                concave_slider!(&mut self.cfg.concave_depth,    "Master depth",    4.0_f32);
                                concave_slider!(&mut self.cfg.concave_shape,    "Shape (para->sphere)", 1.0_f32);
                                ui.label(RichText::new("Per-zone UV pinch (Centre / Mid / Rim):")
                                    .color(COL_TEXT_DIM).size(11.5));
                                concave_slider!(&mut self.cfg.concave_z0, "Zone: Centre", 8.0_f32);
                                concave_slider!(&mut self.cfg.concave_z2, "Zone: Mid",    8.0_f32);
                                concave_slider!(&mut self.cfg.concave_z4, "Zone: Rim",    8.0_f32);
                                });
                                { let o = _g_concave.openness > 0.5; if o != self.cfg.grp_concave_open { self.cfg.grp_concave_open = o; changed = true; } }
                                ui.separator();


                            });
                        
                            

                            // ── Katanga Desktop Overlay ───────────────────────────

                        });
                        cols[1].vertical(|ui| {

                            section(ui, "🖼 Image", |ui| {
                                // Pose prediction — reduces ATW drag/flicker on
                                // canted-display headsets (Pimax Crystal etc.).
                                // 0 = off. Try 8–10 ms on Pimax Crystal.
                                if red_slider_labeled(
                                    ui,
                                    "Pose predict (ms)",
                                    egui::Slider::new(&mut self.cfg.pose_predict_ms, 0.0..=30.0)
                                        .step_by(0.5)
                                        .fixed_decimals(1),
                                )
                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "Extra pose prediction added on top of the runtime's own.\n\
                                     Reduces drag/flicker caused by ATW correcting 10ms of\n\
                                     head motion between render submit and actual display.\n\n\
                                     Pimax Crystal (OpenXR):  8–10 ms  <- biggest benefit\n\
                                     SteamVR (any headset):   4–8 ms   <- moderate benefit\n\
                                     Meta/Oculus (OpenXR):    0–4 ms   <- ASW already good\n\
                                     WMR / Mixed Reality:     4–8 ms\n\
                                     Varjo:                   0–4 ms   <- already excellent\n\n\
                                     0 = disabled (safe default — no risk of overpredict).",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                .changed()
                                {
                                    changed = true;
                                }
                                // Velocity smoothing (EMA alpha). Only visible when
                                // pose prediction is active.
                                let predict_on = self.cfg.pose_predict_ms > 0.0;
                                ui.add_enabled_ui(predict_on, |ui| {
                                    if red_slider_labeled(
                                        ui,
                                        "Predict smooth",
                                        egui::Slider::new(&mut self.cfg.pose_smooth_alpha, 0.0..=0.95)
                                            .step_by(0.05)
                                            .fixed_decimals(2),
                                    )
                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Velocity smoothing for pose prediction (EMA alpha).\n\
                                         0.0 = raw/snappy — reacts instantly but can jitter.\n\
                                         0.5 = balanced — good for most headsets (default).\n\
                                         0.9 = heavy smoothing — very stable, slower to react.\n\n\
                                         SteamVR: use 0.6-0.8 (its PDT batching makes raw\n\
                                         velocity noisy — more smoothing compensates).\n\
                                         Pimax Crystal: 0.4-0.6 works well.\n\
                                         Only active when Pose predict > 0.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed()
                                    {
                                        changed = true;
                                    }
                                });

                                // ── Experimental Features (collapsible) ───────
                                // Pimax depth, low-FPS prediction boost, frame
                                // pacing, temporal blend and optical-flow extrap.
                                // — every experimental motion/reprojection toggle
                                // in one place. Each checkbox is near-white when
                                // OFF and fully red (box + tick + label) when ON,
                                // and shows a tooltip on hover.
                                let _g_exp = egui::CollapsingHeader::new(
                                    RichText::new("Experimental Features")
                                        .color(Color32::WHITE).size(15.0).strong(),
                                )
                                .id_source("grp_experimental")
                                .default_open(self.cfg.grp_experimental_open)
                                .show(ui, |ui| {
                                    // 1. Pimax planar depth reprojection
                                    if exp_checkbox(ui, &mut self.cfg.pimax_flat_depth,
                                        "Pimax planar depth reprojection",
                                        "Pimax only. Submits a flat depth plane at the screen\n\
                                         distance so PimaxXR reprojects translation (not just\n\
                                         rotation) at low FPS — aims to kill the drag and the\n\
                                         left-eye flicker on canted Crystal displays. Pairs with the\n\
                                         low-FPS boost: the runtime reprojects translation while\n\
                                         the boost leads the rendered pose.\n\n\
                                         If the view garbles or goes black, turn this OFF.\n\
                                         No effect on non-Pimax runtimes.")
                                        .changed() { changed = true; }

                                    // 2. Adaptive low-FPS prediction boost (needs pose predict)
                                    let lowfps_enabled = self.cfg.pose_predict_ms > 0.0;
                                    ui.add_enabled_ui(lowfps_enabled, |ui| {
                                        if exp_checkbox(ui, &mut self.cfg.lowfps_predict_boost,
                                            "Adaptive low-FPS prediction boost",
                                            "Needs Pose predict > 0. When the headset is\n\
                                             frame-doubling at low FPS (e.g. Pimax Smart\n\
                                             Smoothing holding each frame for two refresh\n\
                                             periods), the rendered pose goes stale by a frame\n\
                                             on the second period — that's the drag. It pushes\n\
                                             the prediction toward the middle of the doubled\n\
                                             span to even out that error. Detects the doubling\n\
                                             from the reported period OR the real frame cadence.\n\
                                             Adds nothing at full frame rate.")
                                            .changed() { changed = true; }
                                        if self.cfg.lowfps_predict_boost {
                                            if red_slider_labeled(ui, "Predict strength",
                                                egui::Slider::new(&mut self.cfg.lowfps_predict_strength, 0.0..=1.0)
                                                    .step_by(0.05).fixed_decimals(2))
                                                .on_hover_ui(|ui| { ui.label(egui::RichText::new("How far across the doubled display span to lead the pose.\n0.50 = middle of the span (balanced error across both refreshes).\n1.00 = far end (fully compensates the last refresh, but over-leads\nthe first — can look like head-lead 'swimming'). Raise toward 1.0\nif drag still shows; back off if the image starts to swim.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                                .changed() { changed = true; }
                                        }
                                    });

                                    // 3. Frame pacing + slider
                                    if exp_checkbox(ui, &mut self.cfg.frame_pacing_enabled,
                                        "Frame pacing",
                                        "Sleeps after GPU submit to land each frame at a target\n\
                                         point before the predicted display time. Smooths\n\
                                         compositor queueing / frame delivery — a latency and\n\
                                         pacing optimiser, not a head-motion reprojection fix.")
                                        .changed() { changed = true; }
                                    if self.cfg.frame_pacing_enabled {
                                        if red_slider_labeled(ui, "Pacing target",
                                            egui::Slider::new(&mut self.cfg.frame_pacing_target, 0.10..=0.90)
                                                .step_by(0.01).fixed_decimals(2))
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new("Fraction of the frame period used for rendering.\n0.45 = submit at 45% of the frame budget (recommended).\nLower = more safety margin, higher = less latency.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                            .changed() { changed = true; }
                                    }

                                    // 4. Temporal blend + slider
                                    if exp_checkbox(ui, &mut self.cfg.temporal_blend_enabled,
                                        "Temporal blend",
                                        "Mixes the current frame with the previous frame to\n\
                                         smooth flicker, noise and judder in the SOURCE image.\n\
                                         Trades sharpness for smoothing (ghosting on motion).\n\
                                         Smooths content — not head-motion reprojection.")
                                        .changed() { changed = true; }
                                    if self.cfg.temporal_blend_enabled {
                                        if red_slider_labeled(ui, "Blend (1.0=off  0.5=max)",
                                            egui::Slider::new(&mut self.cfg.temporal_blend_alpha, 0.5..=0.98)
                                                .step_by(0.01).fixed_decimals(2))
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new("Temporal blend — mixes the current frame with the previous frame\nto smooth out flickering, noise, and framerate judder.\n\nAlpha = how much of the current frame is kept:\n0.98 = very subtle ghosting. 0.80 = moderate (recommended).\n0.65 = strong smoothing, visible motion blur.\n0.50 = maximum — heavy film-grain smoothing.\n\nHistory is neighbourhood-clamped (TAA-style) to stay ghost-free on\nmotion. Higher = sharper, lower = smoother.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                            .changed() { changed = true; }
                                    }

                                    // 5. Optical flow extrapolation + slider
                                    if exp_checkbox(ui, &mut self.cfg.flow_enabled,
                                        "Optical flow extrapolation",
                                        "Predicts where pixels move between game frames and\n\
                                         warps the image forward — reduces judder on low\n\
                                         framerate SOURCE content (30fps games in VR).\n\
                                         Smooths content — not head-motion reprojection.")
                                        .changed() { changed = true; }
                                    if self.cfg.flow_enabled {
                                        if red_slider_labeled(ui, "Extrapolation strength",
                                            egui::Slider::new(&mut self.cfg.flow_strength, 0.1..=1.5)
                                                .step_by(0.05).fixed_decimals(2))
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new("Optical flow extrapolation — predicts where pixels are moving\nbetween game frames and warps the image forward in time.\nReduces judder on low-framerate content (30fps games in VR).\n\n0.3 = subtle smoothing. 0.7 = balanced (recommended).\n1.0 = full extrapolation. 1.5 = overshoots (ghosting).\n\nBest used with source framerates below 60fps.\nDisable if you see warping artifacts.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                            .changed() { changed = true; }
                                    }

                                    // ── Submit-path reprojection toggles (v84) ──
                                    if exp_checkbox(ui, &mut self.cfg.submit_render_pose,
                                        "Submit predicted pose to layer",
                                        "Submits the SAME (predicted) pose the frame was\n\
                                         rendered with into the projection layer, instead of\n\
                                         the raw runtime pose. Matches xrLocateViews to\n\
                                         xrEndFrame, so the compositor reprojects from the\n\
                                         true render pose. Makes prediction self-correcting\n\
                                         and removes the render/submit gap that grows with\n\
                                         Pose predict. Off = raw pose (original behaviour).")
                                        .changed() { changed = true; }
                                    if exp_checkbox(ui, &mut self.cfg.stable_eye_submit,
                                        "Stable per-eye submit",
                                        "Forces both eyes to submit with the SAME orientation\n\
                                         (averaged head orientation; per-eye IPD position kept)\n\
                                         so the runtime can't apply divergent per-eye warp —\n\
                                         targets one-eye (left-eye) shimmer on canted displays.\n\
                                         Small geometric tradeoff on heavily canted optics.\n\
                                         Off = true per-eye pose (original behaviour).")
                                        .changed() { changed = true; }
                                    if exp_checkbox(ui, &mut self.cfg.hold_full_refresh,
                                        "Hold full refresh",
                                        "Asserts full-rate operation: submits at minimum\n\
                                         pacing lead (freshest pose) and disables the low-FPS\n\
                                         boost horizon extension. Use when the viewer holds\n\
                                         native refresh so the runtime never reprojects.\n\
                                         Off = normal pacing + boost (original behaviour).")
                                        .changed() { changed = true; }
                                });
                                { let o = _g_exp.openness > 0.5; if o != self.cfg.grp_experimental_open { self.cfg.grp_experimental_open = o; changed = true; } }

                                ui.separator();
                                // 0.6.0: IPD perspective slider lives at
                                // the top of the Image section. Tweaks
                                // the per-eye horizontal offset in the
                                // stereo split, making objects appear
                                // closer/larger (>1) or farther/smaller
                                // (<1). Stays at 1.0 = neutral by
                                // default. Particularly useful for SBS
                                // and TAB content where the source's
                                // baked-in IPD doesn't match the user's
                                // headset.
                                let mut _ipd_job = egui::text::LayoutJob::default();
                                {
                                    let w = egui::TextFormat { font_id: egui::FontId::proportional(15.0), color: Color32::WHITE, ..Default::default() };
                                    let r = egui::TextFormat { font_id: egui::FontId::proportional(15.0), color: Color32::from_rgb(235, 60, 60), ..Default::default() };
                                    _ipd_job.append("IPD ", 0.0, w.clone());
                                    _ipd_job.append("/", 0.0, r.clone());
                                    _ipd_job.append(" SEPARATION ", 0.0, w.clone());
                                    _ipd_job.append("/", 0.0, r);
                                    _ipd_job.append(" CONVERGENCE", 0.0, w);
                                }
                                let _g_ipd = egui::CollapsingHeader::new(_ipd_job)
                                    .id_source("grp_ipd").default_open(self.cfg.grp_ipd_open).show(ui, |ui| {
                                ui.label(
                                    RichText::new("IPD: higher = closer, lower = farther")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                if red_slider_labeled(ui, "IPD perspective", egui::Slider::new(
                                            &mut self.cfg.ipd_perspective,
                                            0.0..=2.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                // Separation: scales overall 3D disparity (depth
                                // "pop"). 1.0 = source as authored, higher = more
                                // pronounced depth, lower = flatter.
                                ui.label(
                                    RichText::new("Separation: overall 3D depth strength")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                if red_slider_labeled(ui, "Separation", egui::Slider::new(
                                            &mut self.cfg.separation,
                                            0.0..=3.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                // Convergence: moves the zero-parallax (screen)
                                // plane. >0 pushes scene back / more pops toward
                                // you; <0 pulls it forward. 0 = neutral.
                                ui.label(
                                    RichText::new("Convergence: screen-plane depth (0 = neutral)")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                if red_slider_labeled(ui, "Convergence", egui::Slider::new(
                                            &mut self.cfg.convergence,
                                            -1.0..=1.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                }); // end IPD/Separation/Convergence collapsing
                                { let o = _g_ipd.openness > 0.5; if o != self.cfg.grp_ipd_open { self.cfg.grp_ipd_open = o; changed = true; } }
                                ui.separator();
                                if red_slider_colored(ui, "Brightness",    Color32::from_rgb(0, 220, 245), egui::Slider::new(
                                            &mut self.cfg.brightness,
                                            -2.0..=2.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider_colored(ui, "Contrast",      Color32::from_rgb(0, 220, 245), egui::Slider::new(&mut self.cfg.contrast, 0.0..=2.0)
                                            )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider_colored(ui, "Saturation",    Color32::from_rgb(0, 220, 245), egui::Slider::new(
                                            &mut self.cfg.saturation,
                                            0.0..=2.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider_colored(ui, "Sharpness",     Color32::from_rgb(0, 220, 245), egui::Slider::new(&mut self.cfg.sharpness, 0.0..=10.0)
                                            )
                                    .changed()
                                {
                                    changed = true;
                                }
                                // Texture Sharpen — a tighter (half-pixel
                                // kernel, 9-tap diagonal) unsharp mask
                                // aimed at small texture detail like
                                // skin pores, fabric weave, or hair.
                                // Less halo than the broader Sharpness
                                // slider; better for adding bite to
                                // game-rendered textures.
                                if red_slider_colored(ui, "Texture sharpen", Color32::from_rgb(0, 220, 245), egui::Slider::new(
                                            &mut self.cfg.texture_sharpen,
                                            0.0..=10.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                // Contrast Adaptive Sharpening — adaptive sharpen
                                // that backs off near strong edges to avoid halos.
                                if red_slider_colored(ui, "Contrast Adaptive Sharpening", Color32::from_rgb(0, 220, 245), egui::Slider::new(
                                            &mut self.cfg.cas,
                                            0.0..=10.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                // Dehaze — local-contrast enhancement that adds
                                // punch/depth and cuts through flat or hazy images.
                                if red_slider_colored(ui, "Dehaze", Color32::from_rgb(0, 220, 245), egui::Slider::new(
                                            &mut self.cfg.dehaze,
                                            0.0..=10.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Filtering")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                if red_slider_colored(ui, "Bilinear",      Color32::from_rgb(255, 220, 0), egui::Slider::new(
                                            &mut self.cfg.filter_bilinear,
                                            0.0..=4.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider_colored(ui, "Bicubic",       Color32::from_rgb(255, 220, 0), egui::Slider::new(
                                            &mut self.cfg.filter_bicubic,
                                            0.0..=2.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }
                                if red_slider_colored(ui, "Lanczos",       Color32::from_rgb(255, 220, 0), egui::Slider::new(
                                            &mut self.cfg.filter_lanczos,
                                            0.0..=2.0,
                                        )
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }

                                // ── Supersampling ──────────────────────────────────────
                                ui.separator();
                                ui.label(
                                    RichText::new("Supersampling: higher = sharper, more GPU. Restart after changing")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                if red_slider_colored(ui, "Supersampling ×", Color32::from_rgb(220, 60, 60), egui::Slider::new(
                                            &mut self.cfg.supersampling,
                                            0.5..=2.0,
                                        )
                                        .step_by(0.05)
                                        )
                                    .changed()
                                {
                                    changed = true;
                                }

                                // ── Katanga Filters ─────────────────────────
                                // A second, stronger set of image adjustments.
                                // When enabled, these are ADDED on top of the
                                // normal image sliders (apply to whatever is on
                                // screen, same as the normal filters). The two
                                // sharpness sliders go to 10 for aggressive
                                // sharpening. Slider labels are cyan; the frame
                                // around the toggle is yellow; the toggle itself
                                // uses red_checkbox so it goes red (text + tick)
                                // when on, like the others. Tooltip matches the
                                // other GUI tooltips (cyan, size 16).
                                ui.add_space(8.0);
                                let kat_cyan = Color32::from_rgb(0, 210, 230);
                                let kat_yellow = Color32::from_rgb(255, 225, 0);
                                egui::Frame::none()
                                    .stroke(Stroke::new(1.5, kat_yellow))
                                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                    .rounding(Rounding::same(4.0))
                                    .show(ui, |ui| {
                                        // red_checkbox: red text + red tick when
                                        // toggled on, matching the other toggles.
                                        if red_checkbox(
                                            ui,
                                            &mut self.cfg.katanga_filters_enabled,
                                            RichText::new("Katanga Filters")
                                                .strong().size(14.0),
                                        )
                                        .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                            "Katanga Filters — a second, stronger set of image \
                                             adjustments layered ON TOP of the normal image sliders \
                                             above.\n\n\
                                             • They apply to whatever is on screen (desktop and \
                                             Katanga games alike), just like the normal filters.\n\
                                             • Sharpness and Texture Sharpness go up to 10 (vs the \
                                             normal 0–2) for aggressive sharpening; Texture \
                                             Sharpness targets fine detail.\n\
                                             • Sharpness/Brightness ADD to the base sliders; \
                                             Saturation/Contrast MULTIPLY them.\n\
                                             • Costs the same as the normal filters (no extra GPU \
                                             work) and adds nothing when this toggle is off.\n\
                                             • Bind a key to 'Toggle Katanga Filters' in the \
                                             Hotkeys panel to flip these on/off instantly.",
                                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                        .changed() {
                                            changed = true;
                                        }
                                    });

                                if self.cfg.katanga_filters_enabled {
                                    ui.add_space(4.0);
                                    if red_slider_colored(ui, "Katanga Sharpness", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_sharpness, 0.0..=10.0))
                                        .changed() { changed = true; }
                                    if red_slider_colored(ui, "Katanga Texture Sharpness", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_texture_sharpness, 0.0..=10.0))
                                        .changed() { changed = true; }
                                    if red_slider_colored(ui, "Katanga Saturation", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_saturation, 0.0..=2.0))
                                        .changed() { changed = true; }
                                    if red_slider_colored(ui, "Katanga Contrast", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_contrast, 0.0..=2.0))
                                        .changed() { changed = true; }
                                    if red_slider_colored(ui, "Katanga Brightness", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_brightness, -2.0..=2.0))
                                        .changed() { changed = true; }
                                    if red_slider_colored(ui, "Katanga Contrast Adaptive Sharpening", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_cas, 0.0..=10.0))
                                        .changed() { changed = true; }
                                    if red_slider_colored(ui, "Katanga Dehaze", kat_cyan,
                                        egui::Slider::new(&mut self.cfg.katanga_dehaze, 0.0..=10.0))
                                        .changed() { changed = true; }
                                }



                                ui.add_space(6.0);
                                // Reset image button: dark-blue base (just above
                                // the background), red on hover. Patch the button
                                // widget fills locally (restore after) so we don't
                                // affect other widgets.
                                let reset_clicked = {
                                    let prev = ui.visuals().clone();
                                    {
                                        let v = ui.visuals_mut();
                                        // Resting (inactive) = dark blue, slightly
                                        // lighter than the near-black background
                                        // (COL_BG 0x05,0x07,0x0E), white text.
                                        let dark_blue = Color32::from_rgb(0x1A, 0x33, 0x66);
                                        v.widgets.inactive.weak_bg_fill = dark_blue;
                                        v.widgets.inactive.bg_fill = dark_blue;
                                        v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
                                        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2E, 0x55, 0x9E));
                                        // Hover = red fill, white text.
                                        v.widgets.hovered.weak_bg_fill = COL_RED;
                                        v.widgets.hovered.bg_fill = COL_RED;
                                        v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
                                        v.widgets.hovered.bg_stroke = Stroke::new(1.0, COL_RED);
                                        // Active (pressed) = brighter red.
                                        v.widgets.active.weak_bg_fill = COL_RED_HOT;
                                        v.widgets.active.bg_fill = COL_RED_HOT;
                                        v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
                                        v.widgets.active.bg_stroke = Stroke::new(1.0, COL_RED_HOT);
                                    }
                                    let clicked = ui.button("Reset image").clicked();
                                    *ui.visuals_mut() = prev;
                                    clicked
                                };
                                if reset_clicked {
                                    self.cfg.brightness = 0.0;
                                    self.cfg.contrast = 1.0;
                                    self.cfg.saturation = 1.0;
                                    self.cfg.sharpness = 0.0;
                                    self.cfg.texture_sharpen = 0.0;
                                    self.cfg.cas = 0.0;
                                    self.cfg.dehaze = 0.0;
                                    self.cfg.filter_bilinear = 1.0;
                                    self.cfg.filter_trilinear = 0.0;
                                    self.cfg.filter_bicubic = 0.0;
                                    self.cfg.filter_lanczos = 0.0;
                                    self.cfg.ipd_perspective = 1.0;
                                    self.cfg.separation = 1.0;
                                    self.cfg.convergence = 0.0;
                                    self.cfg.katanga_filters_enabled = false;
                                    self.cfg.katanga_sharpness = 0.0;
                                    self.cfg.katanga_texture_sharpness = 0.0;
                                    self.cfg.katanga_saturation = 1.0;
                                    self.cfg.katanga_contrast = 1.0;
                                    self.cfg.katanga_brightness = 0.0;
                                    changed = true;
                                }
                            });
                            ui.add_space(4.0);

                            // (Behaviour, Simulated 6DoF, and Presets
                            //  moved to columns 0 and 2 in the 0.6.0
                            //  layout pass.)

                            // ---------- Edge Stretch section ----------
                            // Four complementary sliders, organised into
                            // two strategies:
                            //
                            //   MIRROR BASED — clamps the boundary pixel
                            //     and smears it outward. Looks like
                            //     vertical/horizontal streaks; great for
                            //     clean horizons, awful for clouds and
                            //     organic content.
                            //
                            //   EXTEND BASED (NEW) — samples the source
                            //     progressively starting from inside the
                            //     image, so peripheral content visually
                            //     continues outward like a real
                            //     extension of the scene. Better for
                            //     clouds, faces, organic shapes.
                            //
                            // Both can be combined for a hybrid look.
                        
                            
                        });
                        cols[2].vertical(|ui| {

                            section_purple(ui, "🎢 Simulated 6DoF", |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(
                                    RichText::new(
                                        "Fake parallax for flat 3D. Use Zoom-only for headlock.",
                                    )
                                    .color(COL_TEXT_DIM)
                                    .size(13.5),
                                );
                                ui.add_space(4.0);
                                // ── 6DoF mode dropdown (top of section) ──────
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("6DoF mode").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let mode_label = if self.cfg.sim6dof_mode == 1 { "Off-axis (window)" } else { "Default" };
                                    egui::ComboBox::from_id_source("sim6dof_mode_combo")
                                        .selected_text(mode_label)
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_value(&mut self.cfg.sim6dof_mode, 0u32, "Default").changed() { changed = true; }
                                            if ui.selectable_value(&mut self.cfg.sim6dof_mode, 1u32, "Off-axis (window)").changed() { changed = true; }
                                        });
                                }).response.on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "How head movement is applied.\n\
                                     • Default: the screen follows your head (the original parallax).\n\
                                     • Off-axis (window): the screen acts like a fixed window into the scene — leaning changes your viewing angle, like looking through a real window. Most noticeable on the curved screen and with bigger leans. No holes, no HUD issues. Shows extra fine-tune sliders below.\n\
                                     Dynamic Depth and the movement controls apply in BOTH modes.",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                ui.add_space(4.0);
                                if red_checkbox(
                                    ui,
                                    &mut self.cfg.sim6dof_enabled,
                                    "Enable simulated 6DoF",
                                )
                                .changed()
                                {
                                    changed = true;
                                }
                                if red_checkbox(
                                    ui,
                                    &mut self.cfg.sim6dof_zoom_only,
                                    "Zoom-only (headlock compatible)",
                                )
                                .changed()
                                {
                                    changed = true;
                                }

                                // ── Group 1: Movement ────────────────────────
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("MOVEMENT")
                                        .color(COL_BLUE).strong().size(11.0),
                                );
                                ui.label(RichText::new("Movement amount").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                let resp = ui.add_enabled(
                                    self.cfg.sim6dof_enabled && !self.cfg.sim6dof_zoom_only,
                                    egui::Slider::new(
                                        &mut self.cfg.sim6dof_intensity,
                                        0.0..=20.0,
                                    )
                                    .text(""),
                                );
                                if resp.changed() { changed = true; }

                                ui.label(RichText::new("Zoom amount").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                let resp = ui.add_enabled(
                                    self.cfg.sim6dof_enabled,
                                    egui::Slider::new(
                                        &mut self.cfg.sim6dof_zoom_intensity,
                                        0.0..=20.0,
                                    )
                                    .text(""),
                                );
                                if resp.changed() { changed = true; }

                                ui.label(RichText::new("Motion smoothness").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                let resp = ui.add_enabled(
                                    self.cfg.sim6dof_enabled,
                                    egui::Slider::new(
                                        &mut self.cfg.sim6dof_smoothness,
                                        0.0..=0.99,
                                    )
                                    .text(""),
                                );
                                if resp.changed() { changed = true; }

                                // ── Group: Off-axis WINDOW fine-tune (only in mode 1) ──
                                if self.cfg.sim6dof_mode == 1 {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.label(
                                        RichText::new("WINDOW (off-axis)")
                                            .color(COL_BLUE).strong().size(11.0),
                                    );
                                    ui.label(
                                        RichText::new("Fine-tune the window feel. All applied on top of Movement + Dynamic Depth.")
                                            .color(COL_TEXT_DIM).size(11.5),
                                    );

                                    ui.label(RichText::new("Window depth").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.offaxis_window_depth, 0.2..=4.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "How far behind the frame the scene sits. Higher = deeper into the room, stronger window feel and more parallax per head move.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Parallax strength").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.offaxis_parallax, 0.0..=4.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "How much the view shifts for a given head movement (the lean-in/zoom axis), independent of window depth.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Edge falloff").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.offaxis_edge_falloff, 0.0..=2.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Biases the lateral effect toward the screen edges — the 'looking around the frame' quality. 1.0 = neutral.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Vertical balance").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.offaxis_vertical_balance, 0.0..=2.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Up/down response vs left/right. Below 1 damps vertical bob; above 1 emphasises it. 1.0 = balanced.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }
                                }

                                // ── Group 2: Dynamic Depth ───────────────────
                                ui.add_space(8.0);
                                ui.separator();
                                // Grey out + disable the three sub-feature collapsibles
                                // entirely whenever Simulated 6DoF is off (they all
                                // depend on it).
                                let _sim6_on = self.cfg.sim6dof_enabled;
                                ui.add_enabled_ui(_sim6_on, |ui| {
                                let _g_dyn = egui::CollapsingHeader::new(RichText::new("DYNAMIC DEPTH")
                                        .color(Color32::from_rgb(185, 130, 255)).strong().size(15.0))
                                    .id_source("grp_dyndepth").default_open(self.cfg.grp_dyndepth_open).show(ui, |ui| {
                                ui.label(
                                    RichText::new("Lean toward the scene -> it pops out and gains depth, like a real dolly-in.")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                ui.add_space(2.0);
                                ui.add_enabled_ui(self.cfg.sim6dof_enabled, |ui| {
                                    if red_checkbox(
                                        ui,
                                        &mut self.cfg.sim6dof_dynamic_depth,
                                        "Enable dynamic depth",
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Couples stereo depth to FORWARD/BACK head movement so approaching the scene feels like real VR depth.\n\
                                         • Pop-out (convergence): lean IN -> the scene pops out toward you (zero-parallax plane pulls closer); lean out -> it recedes.\n\
                                         • Depth scale (separation): lean IN -> the world gains overall depth/roundness, like UEVR's Depth Scale; lean out -> flattens.\n\
                                         Both ride the same smoothed head signal as the parallax, so they stay in lock-step.\n\
                                         Sideways movement is left to the screen parallax alone (coupling global disparity to lateral motion is a known motion-sickness source on flat stereo, so it's intentionally not done).\n\
                                         Comfort-tuned: dead-zone ignores jitter, small leans give the most effect, big/fast moves ease off. Needs simulated 6DoF enabled.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed() {
                                        changed = true;
                                    }
                                });
                                ui.add_enabled_ui(self.cfg.sim6dof_enabled, |ui| {
                                    if red_checkbox(
                                        ui,
                                        &mut self.cfg.sim6dof_spring,
                                        "Return-to-center spring",
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Slowly relaxes the simulated-6DoF anchor toward your head: a brief lean still parallaxes, but a SUSTAINED offset eases back to centre on its own — no manual recenter. Gentle, and leaves fast motion untouched. Needs simulated 6DoF enabled.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed() {
                                        changed = true;
                                    }
                                });
                                // Two independent strengths, shown only when dynamic
                                // depth is ON. Both driven by forward/back (approach).
                                if self.cfg.sim6dof_dynamic_depth {
                                    ui.label(RichText::new("Pop-out (convergence)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dyn_popout, 0.0..=4.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "How strongly leaning IN pops the scene out toward you (convergence). 0 = off.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if resp.changed() { changed = true; }

                                    ui.label(RichText::new("Depth scale (separation)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dyn_depthscale, 0.0..=4.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "How strongly leaning IN deepens overall depth/roundness (separation). 0 = off.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if resp.changed() { changed = true; }

                                    ui.label(RichText::new("Dolly looming (optical expansion)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dyn_looming, 0.0..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Leaning IN gently magnifies the whole view (optical expansion), the way things grow as you approach -- the strongest motion-in-depth cue, so it makes the pop-out/depth-scale read as a real dolly instead of fighting it. The zoom is identical for both eyes, so it adds NO stereo disparity and can't break 3D. Rides the same smoothed lean signal as pop-out/depth-scale, comfort-capped and snapped to neutral when still. 0 = off (default); try 0.3-0.5 first.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if resp.changed() { changed = true; }

                                    ui.label(
                                        RichText::new("Comfort-tuned: small leans give the most effect; big/fast moves ease off. Sideways movement uses the screen parallax only.")
                                            .color(COL_TEXT_DIM).size(11.5),
                                    );
                                }
                                }); // end Dynamic Depth collapsing
                                { let o = _g_dyn.openness > 0.5; if o != self.cfg.grp_dyndepth_open { self.cfg.grp_dyndepth_open = o; changed = true; } }

                                // ── Group 3: Depth Layers ────────────────────
                                ui.add_space(8.0);
                                ui.separator();
                                let _g_dl = egui::CollapsingHeader::new(RichText::new("DEPTH LAYERS")
                                        .color(Color32::from_rgb(185, 130, 255)).strong().size(15.0))
                                    .id_source("grp_dlayers").default_open(self.cfg.grp_dlayers_open).show(ui, |ui| {
                                ui.label(
                                    RichText::new("5 concentric zones parallax by different amounts as you sway — a soft diorama depth.")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                ui.add_space(2.0);
                                ui.add_enabled_ui(self.cfg.sim6dof_enabled, |ui| {
                                    if red_checkbox(
                                        ui,
                                        &mut self.cfg.dlayers_enabled,
                                        "Enable depth layers",
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "5-zone radial parallax. The centre stays the responsive anchor; outer zones move more and trail slightly (follow-through), giving a soft sense of depth across the screen surface. Works in BOTH Default and Off-axis modes. Needs simulated 6DoF enabled.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed() {
                                        changed = true;
                                    }
                                });
                                // Sliders revealed only when the toggle is on.
                                if self.cfg.dlayers_enabled {
                                    ui.label(RichText::new("Strength").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_strength, 0.0..=3.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Master amount of the layered parallax. 0 = none.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.add_space(4.0);
                                    if red_checkbox(ui, &mut self.cfg.dlayers_reactive_on, "Motion-reactive (pop on head movement)")
                                        .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                            "Layers intensify while you move or turn your head (translation + yaw/pitch/roll) and settle at rest. Adds dynamic depth feel without true 6DoF.",
                                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                        .changed() { changed = true; }
                                    if self.cfg.dlayers_reactive_on {
                                        ui.label(RichText::new("Reactivity").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                        let r = ui.add(
                                            egui::Slider::new(&mut self.cfg.dlayers_reactive_amt, 0.0..=2.0).text(""),
                                        ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                            "How much head motion boosts the layer warp. Higher = stronger pop while moving.",
                                        ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                        if r.changed() { changed = true; }
                                    }
                                    ui.add_space(4.0);

                                    ui.label(RichText::new("Separation").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_separation, 0.0..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "How much MORE the rim moves than the centre — the differential that reads as depth. 0 = whole surface moves together; 1 = centre still, rim moves most.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Delay (follow-through)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_delay, 0.0..=5.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Per-layer lag across the 10 concentric layers. The leading end (set by Invert) stays crisp (locked to your head); the far end trails by this much, so motion ripples through the layers for an organic 'give'. Too high = jelly / underwater feel.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    let r = red_checkbox_enabled(ui, self.cfg.sim6dof_enabled, &mut self.cfg.dlayers_invert, "Invert delay (outer leads / outer closest)").on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Flips which end of the 10-layer cascade is the responsive anchor.\nOff = inner/centre leads, delay ripples OUTWARD — the inner layer reads as closest.\nOn = outer/rim leads, delay ripples INWARD — the outer layer reads as closest.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    // Band MODE: two mutually-exclusive toggles (off = concentric).
                                    {
                                        let mut horiz = self.cfg.dlayers_mode == 1;
                                        let mut vert  = self.cfg.dlayers_mode == 2;
                                        let mut grid  = self.cfg.dlayers_mode == 3;
                                        let rh = red_checkbox_enabled(ui, self.cfg.sim6dof_enabled, &mut horiz, "Horizontal bands (10 rows)")
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                "Swap the concentric rings for 10 horizontal bands stacked top→bottom. The delay cascades down the screen and the Ground bias still applies. Mutually exclusive with Vertical columns.",
                                            ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                        let rv = red_checkbox_enabled(ui, self.cfg.sim6dof_enabled, &mut vert, "Vertical columns (10)")
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                "Swap the concentric rings for 10 vertical columns side-by-side. The delay cascades across the screen. Mutually exclusive with Horizontal bands.",
                                            ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                        let rg = red_checkbox_enabled(ui, self.cfg.sim6dof_enabled, &mut grid, "Grid / cubes (intersection)")
                                            .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                                "Combine bands AND columns into a 2D cell grid: horizontal position drives the sideways parallax, vertical position drives the up/down parallax, so each cell reacts on its own. The most granular, per-region depth — strongest simulated-6DoF feel. Mutually exclusive with the others.",
                                            ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                        if rh.changed() { self.cfg.dlayers_mode = if horiz { 1 } else { 0 }; changed = true; }
                                        if rv.changed() { self.cfg.dlayers_mode = if vert  { 2 } else { 0 }; changed = true; }
                                        if rg.changed() { self.cfg.dlayers_mode = if grid  { 3 } else { 0 }; changed = true; }
                                    }

                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_ground, 0.0..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Ground-plane prior: the lower image (ground = near) parallaxes MORE than the upper (sky = far), pivoting at the horizon. The classic scene-shape cue — try 0.4-0.7 for driving/walking games. 0 = off.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Horizon height").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_horizon, 0.0..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Where the horizon sits, measured from the image bottom (0.5 = middle). Pivot for Ground bias and anchor for Perspective. Lower it for racing dashboards, raise it for flight.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Perspective (vanishing point)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_vp, 0.0..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Re-centres the layer depth on the horizon point instead of the screen centre, so corridors and roads layer along perspective: distant centre stays still, near edges sweep. 0 = classic radial.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Falloff curve").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_curve, 0.25..=3.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Shapes how the per-zone motion grows from centre to rim. Low = gentle near centre then ramps; high = most of the motion at the rim. 1.0 = even.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Zoom (lean in/out)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_zoom, 0.0..=2.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Forward/back lean ZOOMS the layers: leaning in magnifies (the rings expand about the centre), leaning back shrinks them — a dolly-zoom tunnel. The per-ring Delay + Invert ripple through it just like the sway parallax. It also deepens the side-to-side movement. 0 = no in/out zoom (sway only).",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Convex (zoom bulge)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_convex, 0.0..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Bulges the centre of the screen outward like a magnifier/lens — the area the in/out zoom magnifies. A small dome is always present, and it intensifies as you lean IN to zoom into the area. 0 = flat (off). Pairs with Zoom (lean in/out).",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Depth reach").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dlayers_edge, 0.05..=1.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Where the radial depth taper starts (rim-fade). Lower keeps depth near the centre and eases it out early — removes the edge seam; higher lets depth reach further toward the rim.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }
                                }
                                }); // end Depth Layers collapsing
                                { let o = _g_dl.openness > 0.5; if o != self.cfg.grp_dlayers_open { self.cfg.grp_dlayers_open = o; changed = true; } }

                                ui.add_space(8.0);
                                ui.separator();
                                let _g_dir = egui::CollapsingHeader::new(RichText::new("DIRECTIONAL 6DoF (tilt & turn)")
                                        .color(Color32::from_rgb(185, 130, 255)).strong().size(15.0))
                                    .id_source("grp_dir6dof").default_open(self.cfg.grp_dir6dof_open).show(ui, |ui| {
                                ui.label(
                                    RichText::new("Turning, looking up/down, or tilting your head adds a small parallax — comfortable.")
                                        .color(COL_TEXT_DIM).size(12.5),
                                );
                                ui.add_space(2.0);
                                ui.add_enabled_ui(self.cfg.sim6dof_enabled, |ui| {
                                    if red_checkbox(
                                        ui,
                                        &mut self.cfg.dir6dof_enabled,
                                        "Enable directional 6DoF",
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Head yaw/pitch/roll nudge the scene as extra motion parallax, smoothed in lock-step with the positional 6DoF. Needs simulated 6DoF enabled.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed() {
                                        changed = true;
                                    }
                                });
                                if self.cfg.dir6dof_enabled {
                                    ui.label(RichText::new("Yaw (turn L/R)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dir6dof_yaw, 0.0..=5.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Turning your head left/right slides the scene laterally, as if peeking around. 0 = ignore yaw.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Pitch (look up/down)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dir6dof_pitch, 0.0..=5.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Looking up/down nudges the scene vertically. 0 = ignore pitch.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }

                                    ui.label(RichText::new("Roll (tilt)").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                    let r = ui.add_enabled(
                                        self.cfg.sim6dof_enabled,
                                        egui::Slider::new(&mut self.cfg.dir6dof_roll, 0.0..=5.0).text(""),
                                    ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Tilting your head ear-to-shoulder shifts the scene laterally in the tilt direction. 0 = ignore roll.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                    if r.changed() { changed = true; }
                                }
                                }); // end Directional 6DoF collapsing
                                { let o = _g_dir.openness > 0.5; if o != self.cfg.grp_dir6dof_open { self.cfg.grp_dir6dof_open = o; changed = true; } }
                                }); // end sim6dof-gated collapsibles

                            });
                            ui.add_space(4.0);

                            // -------- Mouse Emulation -------------------
                            // Head pose drives the OS mouse cursor via
                            // Win32 SendInput on the viewer side. Lets
                            // games that only support mouse-look (no
                            // gamepad/joystick yaw axis — Forza is the
                            // canonical case) be controlled by head
                            // movement for 3DoF camera control.
                            section_cyan(ui, "🖱 Mouse Emulation", |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(
                                    RichText::new(
                                        "Head turn -> mouse cursor for 3DoF camera control in games",
                                    )
                                    .color(COL_TEXT_DIM)
                                    .size(12.5),
                                );
                                ui.add_space(4.0);
                                if red_checkbox(
                                    ui,
                                    &mut self.cfg.mouse_emu_enabled,
                                    "Enable mouse emulation",
                                )
                                .changed()
                                {
                                    changed = true;
                                }

                                if self.cfg.mouse_emu_enabled {  // hide options when toggle off
                                ui.label(RichText::new("Sensitivity").color(Color32::from_rgb(215,222,245)).strong().size(12.0));

                                let resp = ui.add_enabled(
                                    self.cfg.mouse_emu_enabled,
                                    egui::Slider::new(
                                        &mut self.cfg.mouse_emu_sensitivity,
                                        0.0..=2.0,
                                    )
                                    .text(""),
                                );
                                if resp.changed() { changed = true; }

                                ui.label(RichText::new("Mouse speed").color(Color32::from_rgb(215,222,245)).strong().size(12.0));

                                let resp = ui.add_enabled(
                                    self.cfg.mouse_emu_enabled,
                                    egui::Slider::new(
                                        &mut self.cfg.mouse_emu_speed,
                                        0.0..=2.0,
                                    )
                                    .text(""),
                                );
                                if resp.changed() { changed = true; }

                                // Compatibility-mode picker. Lets the
                                // user match the injection style to
                                // the target game's input model — the
                                // single biggest factor in whether
                                // mouse emulation works at all in a
                                // given title.
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Compatibility mode")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                let labels = [
                                    "Relative (SendInput)",
                                    "Absolute (SetCursorPos)",
                                    "Both (max user-mode)",
                                    "Interception (driver — works in ALL games)",
                                ];
                                let cur = (self.cfg.mouse_emu_compat as usize).min(3);
                                let mut new_idx = cur;
                                ui.add_enabled_ui(self.cfg.mouse_emu_enabled, |ui| {
                                    egui::ComboBox::from_id_source("mouse_emu_compat_mode")
                                        .selected_text(labels[cur])
                                        .show_ui(ui, |ui| {
                                            for (i, lbl) in labels.iter().enumerate() {
                                                ui.selectable_value(&mut new_idx, i, *lbl);
                                            }
                                        });
                                });
                                if new_idx != cur {
                                    self.cfg.mouse_emu_compat = new_idx as u32;
                                    changed = true;
                                }
                                ui.label(
                                    RichText::new(match self.cfg.mouse_emu_compat {
                                        0 => "Best for: modern FPS, raw-input games (Forza, racing/flight sims)",
                                        1 => "Best for: cursor-polling games (Witcher 3 hardware-cursor-OFF, older RPGs)",
                                        2 => "Tries both user-mode paths each frame. Use this if unsure which mode the game needs.",
                                        _ => "Kernel driver (all games incl. anti-cheat).",
                                    })
                                    .color(COL_TEXT_DIM)
                                    .size(11.5),
                                );
                                // Interception driver hyperlink (shown when compat=3 selected)
                                if self.cfg.mouse_emu_compat == 3 {
                                    ui.hyperlink_to(
                                        RichText::new("⬇ Download Interception Driver")
                                            .color(Color32::from_rgb(100, 180, 255))
                                            .size(11.0),
                                        "https://github.com/oblitum/Interception/releases",
                                    );
                                }
                                }  // end hide-when-off
                            });
                            ui.add_space(4.0);

                            // -------- 6DOF MODS UDP Network -------------
                            // Head pose streamed to a target IP:port as
                            // 48-byte OpenTrack-format UDP packets at
                            // VR refresh rate. Drives community 6DoF
                            // mods (RE Requiem, etc.) that listen on
                            // OpenTrack's wire format.


                            // ── Joystick Emulation ────────────────────────────────
                            section_green(ui, "🎮 Joystick Emulation", |ui| {
                                ui.set_min_width(ui.available_width());
                                // ViGEmBus requirement notice
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        RichText::new("Requires ViGEmBus driver:  ")
                                            .color(Color32::from_rgb(255, 200, 60))
                                            .size(11.0),
                                    );
                                    ui.hyperlink_to(
                                        RichText::new("Download ViGEmBus")
                                            .color(Color32::from_rgb(100, 200, 255))
                                            .size(11.0),
                                        "https://github.com/nefarius/ViGEmBus/releases",
                                    );
                                });
                                // Enable toggle
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.joy_emu_enabled, "Enable").changed() {
                                        changed = true;
                                    }
                                    ui.label(
                                        RichText::new("Maps head rotation -> Xbox right stick")
                                            .color(COL_TEXT_DIM).size(12.5),
                                    );
                                });
                                ui.add_space(4.0);

                                // All controls below are disabled when joy emu is off.
                                if self.cfg.joy_emu_enabled {  // hide options when toggle off
                                ui.add_enabled_ui(self.cfg.joy_emu_enabled, |ui| {

                                // Mode dropdown
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Mode:").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let modes = ["Relative-Delta", "Joy-Look Continuous"];
                                    let current_mode = self.cfg.joy_emu_mode.min(1) as usize;
                                    egui::ComboBox::from_id_source("joy_mode")
                                        .selected_text(modes[current_mode])
                                        .show_ui(ui, |ui| {
                                            for (i, label) in modes.iter().enumerate() {
                                                if ui.selectable_label(current_mode == i, *label).clicked() {
                                                    self.cfg.joy_emu_mode = i as u32;
                                                    changed = true;
                                                }
                                            }
                                        });
                                });
                                ui.add_space(2.0);
                                // Mode description
                                let mode_desc = if self.cfg.joy_emu_mode == 1 {
                                    "Joy-Look: head angle -> stick position. Best for flight/driving."
                                } else {
                                    "Relative-Delta: head rotation speed -> stick velocity. Best for FPS."
                                };
                                ui.label(RichText::new(mode_desc).color(COL_TEXT_DIM).size(12.0));
                                ui.add_space(6.0);

                                // Sensitivity slider
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Sensitivity").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let r = ui.add(egui::Slider::new(&mut self.cfg.joy_emu_sensitivity, 0.01f32..=2.0)
                                        .step_by(0.01).fixed_decimals(2));
                                    if r.changed() { changed = true; }
                                });
                                ui.label(RichText::new("Joy-Look default: 1.0  ·  Relative-Delta default: 0.3").color(COL_TEXT_DIM).size(12.0));

                                // Deadzone slider
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Deadzone   ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let r = ui.add(egui::Slider::new(&mut self.cfg.joy_emu_deadzone, 0.0f32..=0.5)
                                        .step_by(0.005).fixed_decimals(3));
                                    if r.changed() { changed = true; }
                                });

                                // Max Angle (Joy-Look only)
                                if self.cfg.joy_emu_mode == 1 {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Max Angle°").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.joy_emu_max_angle, 10.0f32..=180.0)
                                            .step_by(1.0).fixed_decimals(0).suffix("°"));
                                        if r.changed() { changed = true; }
                                    });
                                }
                                ui.add_space(4.0);

                                // Smoothness slider (both modes)
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Smoothness ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let r = ui.add(egui::Slider::new(&mut self.cfg.joy_emu_smoothness, 0.0f32..=0.95)
                                        .step_by(0.01).fixed_decimals(2));
                                    if r.changed() { changed = true; }
                                });
                                ui.label(RichText::new("0 = instant · 0.9 = very smooth").color(COL_TEXT_DIM).size(12.0));
                                ui.add_space(4.0);

                                // Per-axis speed
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Speed X  ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let r = ui.add(egui::Slider::new(&mut self.cfg.joy_emu_speed_x, -10.0f32..=10.0)
                                        .step_by(0.1).fixed_decimals(1));
                                    if r.changed() { changed = true; }
                                });
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Speed Y  ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let r = ui.add(egui::Slider::new(&mut self.cfg.joy_emu_speed_y, -10.0f32..=10.0)
                                        .step_by(0.1).fixed_decimals(1));
                                    if r.changed() { changed = true; }
                                });
                                ui.label(RichText::new("Negative speed reverses axis · 0 = disabled").color(COL_TEXT_DIM).size(12.0));
                                ui.add_space(4.0);

                                // Invert axes
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.joy_emu_invert_x, "Invert X").changed() { changed = true; }
                                    ui.add_space(12.0);
                                    if red_checkbox(ui, &mut self.cfg.joy_emu_invert_y, "Invert Y").changed() { changed = true; }
                                });

                                }); // end add_enabled_ui
                                }  // end hide-when-off
                            });

                            section_pink(ui, "📡 6DOF MODS", |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.hyperlink_to("6DoF Mods by itsloopyo", "https://github.com/itsloopyo/itsloopyo");
                                });
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new("UDP over Network")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                ui.label(
                                    RichText::new(
                                        "Stream head pose as OpenTrack 48-byte UDP \
                                         packets to a 6DoF mod listener",
                                    )
                                    .color(COL_TEXT_DIM)
                                    .size(12.5),
                                );
                                ui.add_space(4.0);
                                if red_checkbox(
                                    ui,
                                    &mut self.cfg.udp_6dof_enabled,
                                    "Enable 6DoF UDP stream",
                                )
                                .changed()
                                {
                                    changed = true;
                                }
                                // TrackIR redirects the 6DoF stream, so it only makes sense
                                // while the stream is on. If the stream is off, drop TrackIR
                                // too (its toggle is hidden below) so it can't stay silently
                                // active with no visible control.
                                if !self.cfg.udp_6dof_enabled && self.cfg.trackir_enabled {
                                    self.cfg.trackir_enabled = false;
                                    changed = true;
                                }
                                if self.cfg.udp_6dof_enabled {
                                let _g_6dof = egui::CollapsingHeader::new(RichText::new("6DoF SETTINGS")
                                        .color(Color32::WHITE).strong().size(13.5))
                                    .id_source("grp_6dof").default_open(!self.cfg.sixdof_collapsed).show(ui, |ui| {
                                ui.add_space(4.0);
                                // TrackIR Game — only visible while the 6DoF MODS UDP
                                // stream is enabled. Full description is on hover.
                                if red_checkbox(
                                    ui,
                                    &mut self.cfg.trackir_enabled,
                                    "TrackIR Game",
                                )
                                .on_hover_ui(|ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "Redirect head movement to a TrackIR/FreeTrack game \
                                             via shared memory (\"FT_SharedMem\") instead of UDP. \
                                             When on, the UDP stream is suppressed. Enable TrackIR \
                                             in the game. Use the TrackIR axis flips below (separate \
                                             from the 6DoF-mod flips); rotational gains are shared.",
                                        )
                                        .color(Color32::from_rgb(0, 220, 245))
                                        .size(15.0),
                                    );
                                })
                                .changed()
                                {
                                    changed = true;
                                }
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [60.0, 20.0],
                                        egui::Label::new(
                                            RichText::new("IP")
                                                .color(COL_TEXT_DIM)
                                                .size(13.5),
                                        ),
                                    );
                                    let resp = ui.add_enabled(
                                        self.cfg.udp_6dof_enabled,
                                        egui::TextEdit::singleline(&mut self.cfg.udp_6dof_ip)
                                            .desired_width(140.0)
                                            .hint_text("127.0.0.1"),
                                    );
                                    if resp.changed() { changed = true; }
                                });
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [60.0, 20.0],
                                        egui::Label::new(
                                            RichText::new("Port")
                                                .color(COL_TEXT_DIM)
                                                .size(13.5),
                                        ),
                                    );
                                    let resp = ui.add_enabled(
                                        self.cfg.udp_6dof_enabled,
                                        egui::DragValue::new(&mut self.cfg.udp_6dof_port)
                                            .clamp_range(1..=65535u32)
                                            .speed(1.0),
                                    );
                                    if resp.changed() { changed = true; }
                                });

                                // Per-axis flip toggles. Default
                                // OpenVR->OpenTrack inversion is
                                // already baked in for pitch/roll/Y;
                                // these toggles let the user OVERRIDE
                                // that default if their target game
                                // expects different sign conventions.
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Axis flips")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                ui.horizontal(|ui| {
                                    if red_checkbox_enabled(ui, self.cfg.udp_6dof_enabled, &mut self.cfg.udp_flip_x, "X").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.udp_6dof_enabled, &mut self.cfg.udp_flip_y, "Y").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.udp_6dof_enabled, &mut self.cfg.udp_flip_z, "Z").changed() { changed = true; }
                                });
                                ui.horizontal(|ui| {
                                    if red_checkbox_enabled(ui, self.cfg.udp_6dof_enabled, &mut self.cfg.udp_flip_yaw, "Yaw").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.udp_6dof_enabled, &mut self.cfg.udp_flip_pitch, "Pitch").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.udp_6dof_enabled, &mut self.cfg.udp_flip_roll, "Roll").changed() { changed = true; }
                                });

                                // TrackIR-ONLY axis flips — independent of the
                                // 6DoF-mod flips above so the two never fight.
                                // Active only while TrackIR Game is on.
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("TrackIR axis flips (independent)")
                                        .color(Color32::from_rgb(0, 220, 245))
                                        .size(12.5),
                                );
                                ui.horizontal(|ui| {
                                    if red_checkbox_enabled(ui, self.cfg.trackir_enabled, &mut self.cfg.trackir_flip_x, "X").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.trackir_enabled, &mut self.cfg.trackir_flip_y, "Y").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.trackir_enabled, &mut self.cfg.trackir_flip_z, "Z (lean in/out)").changed() { changed = true; }
                                });
                                ui.horizontal(|ui| {
                                    if red_checkbox_enabled(ui, self.cfg.trackir_enabled, &mut self.cfg.trackir_flip_yaw, "Yaw").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.trackir_enabled, &mut self.cfg.trackir_flip_pitch, "Pitch").changed() { changed = true; }
                                    if red_checkbox_enabled(ui, self.cfg.trackir_enabled, &mut self.cfg.trackir_flip_roll, "Roll").changed() { changed = true; }
                                });

                                // TrackIR-ONLY Z (lean) gain — amplifies the small
                                // physical forward/back lean for TrackIR, independent
                                // of the 6DoF-mod gains. Active only with TrackIR on.
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("TrackIR Z gain (lean in/out)")
                                        .color(Color32::from_rgb(0, 220, 245))
                                        .size(12.5),
                                );
                                let resp = ui.add_enabled(
                                    self.cfg.trackir_enabled,
                                    egui::Slider::new(&mut self.cfg.trackir_gain_z, 0.0..=10.0)
                                        .text("Z"),
                                ).on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "Boosts the forward/back lean sent to TrackIR only. Physical lean is small, so raise this if a game's lean/zoom feels too weak. 1.0 = neutral. Does not affect the 6DoF-mod (UDP) output.",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                if resp.changed() { changed = true; }

                                // Rotational gains (yaw/pitch/roll
                                // multipliers). 1.0 = neutral, 0.5 =
                                // halve sensitivity, 2.0 = double.
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Rotational gains")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                let resp = ui.add_enabled(
                                    self.cfg.udp_6dof_enabled,
                                    egui::Slider::new(&mut self.cfg.udp_gain_yaw, 0.0..=3.0)
                                        .text("Yaw"),
                                );
                                if resp.changed() { changed = true; }
                                let resp = ui.add_enabled(
                                    self.cfg.udp_6dof_enabled,
                                    egui::Slider::new(&mut self.cfg.udp_gain_pitch, 0.0..=3.0)
                                        .text("Pitch"),
                                );
                                if resp.changed() { changed = true; }
                                let resp = ui.add_enabled(
                                    self.cfg.udp_6dof_enabled,
                                    egui::Slider::new(&mut self.cfg.udp_gain_roll, 0.0..=3.0)
                                        .text("Roll"),
                                );
                                if resp.changed() { changed = true; }

                                // Position gains (x/y/z multipliers).
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Position gains")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                let resp = ui.add_enabled(
                                    self.cfg.udp_6dof_enabled,
                                    egui::Slider::new(&mut self.cfg.udp_gain_x, 0.0..=3.0)
                                        .text("X"),
                                );
                                if resp.changed() { changed = true; }
                                let resp = ui.add_enabled(
                                    self.cfg.udp_6dof_enabled,
                                    egui::Slider::new(&mut self.cfg.udp_gain_y, 0.0..=3.0)
                                        .text("Y"),
                                );
                                if resp.changed() { changed = true; }
                                let resp = ui.add_enabled(
                                    self.cfg.udp_6dof_enabled,
                                    egui::Slider::new(&mut self.cfg.udp_gain_z, 0.0..=3.0)
                                        .text("Z"),
                                );
                                if resp.changed() { changed = true; }
                                }); // end 6DoF SETTINGS collapsible
                                } // end if udp_6dof_enabled (hide options when off)
                            });
                            ui.add_space(4.0);

                            // ---- VR Data to UDP ----------------------
                            section_red(ui, "🎮 VR Data to UDP", |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Route VR data to other apps:").color(COL_TEXT_DIM).size(11.5));
                                });
                                ui.horizontal(|ui| {
                                    ui.hyperlink_to("FreePIE", "https://github.com/Ofisare/FreePIE");
                                    ui.label(RichText::new("  ").size(10.0));
                                    ui.hyperlink_to("VRCompanion", "https://github.com/Ofisare/VRCompanion");
                                });
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_enabled, "Enable").changed() { changed = true; }
                                    let mut current = self.cfg.vr_udp_mode.min(2);
                                    let labels = ["Head only","Controllers only","Both"];
                                    egui::ComboBox::from_id_source("vr_udp_mode")
                                        .selected_text(if self.cfg.vr_udp_enabled { labels[current as usize] } else { "— disabled —" })
                                        .width(120.0)
                                        .show_ui(ui, |ui| {
                                            for (i, label) in labels.iter().enumerate() {
                                                if ui.selectable_value(&mut current, i as u32, *label).changed() { changed = true; }
                                            }
                                        });
                                    if current != self.cfg.vr_udp_mode { self.cfg.vr_udp_mode = current; changed = true; }
                                });
                                if self.cfg.vr_udp_enabled {
                                let _g_vrudp = egui::CollapsingHeader::new(RichText::new("VR DATA SETTINGS")
                                        .color(Color32::WHITE).strong().size(13.5))
                                    .id_source("grp_vrudp").default_open(!self.cfg.vr_udp_collapsed).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("IP:");
                                    let r = ui.add_enabled(self.cfg.vr_udp_enabled, egui::TextEdit::singleline(&mut self.cfg.vr_udp_ip).desired_width(100.0));
                                    if r.changed() { changed = true; }
                                    ui.label("Port:");
                                    let r = ui.add_enabled(self.cfg.vr_udp_enabled, egui::DragValue::new(&mut self.cfg.vr_udp_port).clamp_range(1..=65535u32).speed(1.0));
                                    if r.changed() { changed = true; }
                                });
                                ui.label(RichText::new("Flip:").color(COL_TEXT_DIM).size(11.5));
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_flip_x, "X").changed() { changed = true; }
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_flip_y, "Y").changed() { changed = true; }
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_flip_z, "Z").changed() { changed = true; }
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_flip_yaw, "Yaw").changed() { changed = true; }
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_flip_pitch, "Pitch").changed() { changed = true; }
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_flip_roll, "Roll").changed() { changed = true; }
                                });
                                ui.label(RichText::new("Rot gains:").color(COL_TEXT_DIM).size(11.5));
                                let en = self.cfg.vr_udp_enabled;
                                ui.label(RichText::new("Yaw").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                if ui.add_enabled(en, egui::Slider::new(&mut self.cfg.vr_udp_gain_yaw,   0.0..=3.0).text("")).changed()   { changed = true; }
                                ui.label(RichText::new("Pitch").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                if ui.add_enabled(en, egui::Slider::new(&mut self.cfg.vr_udp_gain_pitch, 0.0..=3.0).text("")).changed() { changed = true; }
                                ui.label(RichText::new("Roll").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                if ui.add_enabled(en, egui::Slider::new(&mut self.cfg.vr_udp_gain_roll,  0.0..=3.0).text("")).changed()  { changed = true; }
                                ui.label(RichText::new("Pos gains:").color(COL_TEXT_DIM).size(11.5));
                                ui.label(RichText::new("X").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                if ui.add_enabled(en, egui::Slider::new(&mut self.cfg.vr_udp_gain_x, 0.0..=3.0).text("")).changed() { changed = true; }
                                ui.label(RichText::new("Y").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                if ui.add_enabled(en, egui::Slider::new(&mut self.cfg.vr_udp_gain_y, 0.0..=3.0).text("")).changed() { changed = true; }
                                ui.label(RichText::new("Z").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                if ui.add_enabled(en, egui::Slider::new(&mut self.cfg.vr_udp_gain_z, 0.0..=3.0).text("")).changed() { changed = true; }
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_left_enabled, "Left ctrl").changed()  { changed = true; }
                                    if red_checkbox(ui, &mut self.cfg.vr_udp_right_enabled, "Right ctrl").changed() { changed = true; }
                                });
                                }); // end VR DATA SETTINGS collapsible
                                } // end if vr_udp_enabled (hide options when off)
                            });
                            ui.add_space(4.0);
                        
                            
                        });
                        cols[3].vertical(|ui| {
                            section(ui, "↔ Edge Stretch", |ui| {
                                ui.label(
                                    RichText::new(
                                        "Extends the picture outward to fill your periphery.",
                                    )
                                    .color(COL_TEXT_DIM)
                                    .size(11.5),
                                );
                                ui.add_space(3.0);
                                // ── HYBRID IMMERSION (VERSION 65) — first option ──
                                // Even-ramp rim-stretch (crisp centre, outer band
                                // magnified outward, same-direction, seamless) +
                                // rear-360 mirror. Sphere screen shape only. Sits
                                // ON TOP of the classic sliders below (extra layer).
                                ui.label(
                                    RichText::new("HYBRID IMMERSION (Sphere only)")
                                        .color(Color32::from_rgb(120, 220, 140))
                                        .strong()
                                        .size(13.0),
                                );
                                ui.label(
                                    RichText::new(
                                        "Crisp centre, outer rim stretches outward (same-direction, seamless), rear fills 360 by mirror.",
                                    )
                                    .color(Color32::from_rgb(150, 200, 165))
                                    .size(11.0),
                                );
                                if red_checkbox(ui, &mut self.cfg.hybrid_enabled, "Enable Hybrid Immersion").changed()
                                {
                                    changed = true;
                                }
                                let _g_hy = egui::CollapsingHeader::new(RichText::new("HYBRID SLIDERS")
                                        .color(Color32::WHITE).strong().size(13.5))
                                    .id_source("grp_hybrid").default_open(!self.cfg.hybrid_collapsed).show(ui, |ui| {
                                if self.cfg.hybrid_enabled {
                                    if red_slider_labeled(ui, "Crisp centre — horizontal", egui::Slider::new(
                                                &mut self.cfg.hybrid_center, 0.05..=0.95,
                                            )).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Crisp centre — vertical", egui::Slider::new(
                                                &mut self.cfg.hybrid_center_v, 0.05..=0.95,
                                            )).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Rim gain — horizontal (L/R)", egui::Slider::new(
                                                &mut self.cfg.hybrid_fov_gain, 1.0..=10.0,
                                            )).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Rim gain — vertical (top/bot)", egui::Slider::new(
                                                &mut self.cfg.hybrid_fov_gain_v, 1.0..=10.0,
                                            )).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Stretch ramp (1 = even)", egui::Slider::new(
                                                &mut self.cfg.hybrid_ramp, 0.25..=4.0,
                                            )).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Edge softness", egui::Slider::new(
                                                &mut self.cfg.hybrid_softness, 0.0..=1.0,
                                            )).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Rim stretch direction (out <-> fwd)", egui::Slider::new(
                                                &mut self.cfg.hybrid_stretch_dir, -1.0..=1.0,
                                            )).on_hover_ui(|ui| { ui.label(egui::RichText::new("Which way the stretched outer rim moves in DEPTH: -1 = pushed outward/away, 0 = angular stretch only (current default), +1 = pulled FORWARD toward you. Sphere mode only.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() { changed = true; }
                                    if red_slider_labeled(ui, "Rim stretch reach / forward cutoff", egui::Slider::new(
                                                &mut self.cfg.hybrid_stretch_reach, 0.0..=1.0,
                                            )).on_hover_ui(|ui| { ui.label(egui::RichText::new("CUTOFF for how far the forward stretch eats into the screen: 0 = ONLY the outer rim bends forward (the whole main screen stays flat), 0.5 = moderate, 1 = the bend reaches far in toward the centre. Turn it DOWN to make only the outer rim stretch forward. The outer edge stays pinned at the rim, so the rear-360 mirror always moves forward with the rim and the seamless 360 seam never breaks.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() { changed = true; }
                                    ui.separator();
                                    if red_checkbox(ui, &mut self.cfg.hybrid_rear_enabled, "Rear 360 fill (mirror)").changed()
                                    {
                                        changed = true;
                                    }
                                    if self.cfg.hybrid_rear_enabled {
                                        if red_slider_labeled(ui, "Rear stretch", egui::Slider::new(
                                                    &mut self.cfg.hybrid_rear_stretch, 0.0..=1.0,
                                                )).on_hover_ui(|ui| { ui.label(egui::RichText::new("Stretches the mirrored rear content outward across the rear region (like EXPANSION's reach). 0 = mirror sampled 1:1; higher = content spread further out.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() { changed = true; }
                                        if red_slider_labeled(ui, "Rear direction", egui::Slider::new(
                                                    &mut self.cfg.hybrid_rear_direction, -1.0..=1.0,
                                                )).on_hover_ui(|ui| { ui.label(egui::RichText::new("Moves the rear mesh along the view axis: +1 = forward toward you (rear looms in), -1 = outward (pushed away), 0 = flat on the sphere. Uses vertex displacement (VHT-style).").color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() { changed = true; }
                                    }
                                }
                                }); // end Hybrid collapsing
                                { let o = _g_hy.openness > 0.5; if (!o) != self.cfg.hybrid_collapsed { self.cfg.hybrid_collapsed = !o; changed = true; } }

                                ui.separator();
                                ui.label(
                                    RichText::new("— Classic edge stretch (layers on top) —")
                                        .color(Color32::from_rgb(170, 170, 170))
                                        .size(11.0),
                                );
                                if red_checkbox(ui, &mut self.cfg.show_mirror_method,
                                    "Mirror").on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "Fills the empty edge by reflecting the outermost pixels back inward. Cheapest method, but the single repeated edge pixel can look like streaks when stretched far. Good for small gaps.",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() {
                                    changed = true;
                                }
                                if self.cfg.show_mirror_method {
                                    if red_slider_labeled(ui, "Edge stretch (mirror)", egui::Slider::new(
                                                &mut self.cfg.edge_stretch,
                                                0.0..=30.0,
                                            )
                                            )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    if red_slider_labeled(ui, "Edge expand (mirror)", egui::Slider::new(
                                                &mut self.cfg.edge_expand,
                                                0.0..=2.0,
                                            )
                                            )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                }

                                ui.add_space(6.0);
                                // ── REPEATED METHOD ───────────────────────────────────────
                                if red_checkbox(ui, &mut self.cfg.show_repeated_method,
                                    "Repeated").on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "Fills the empty edge by repeating the outer strip of the source image in the same direction (no mirror flip). Avoids the mirrored-seam look; content tiles outward instead. Good when the edge has a continuous texture.",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() {
                                    changed = true;
                                }
                                if self.cfg.show_repeated_method {
                                    if red_slider_labeled(
                                        ui,
                                        "Repeat stretch",
                                        egui::Slider::new(&mut self.cfg.repeat_stretch, 0.0..=30.0)
                                            .step_by(0.5),
                                    )
                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "How far the repeated edge strip stretches outward toward 360°.
                                         The outermost 30% of each side is repeated in the same
                                         direction as the source — not mirrored, not reversed.
                                         0 = off. Higher = fills more of the peripheral zone.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed()
                                    {
                                        changed = true;
                                    }
                                    if red_slider_labeled(
                                        ui,
                                        "Repeat blend",
                                        egui::Slider::new(&mut self.cfg.repeat_blend, 0.0..=1.0)
                                            .step_by(0.05).fixed_decimals(2),
                                    )
                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                        "Blend width at the seam between the real screen and the
                                         repeated strip. 0 = hard cut (visible seam).
                                         0.3 = soft crossfade (default). 1.0 = very wide blend.
                                         Adjust until the join is invisible.",
                                    ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                    .changed()
                                    {
                                        changed = true;
                                    }
                                }
                                // "Copy depth" slider hidden from the GUI per user request.
                                // The underlying `repeat_depth` value is left untouched (keeps
                                // its saved/default value) so the shader and preset wire format
                                // are unaffected — we simply don't expose the control anymore.
                                // To restore it, un-comment the block below.
                                /*
                                if red_slider_labeled(
                                    ui,
                                    "Copy depth",
                                    egui::Slider::new(&mut self.cfg.repeat_depth, 0.01..=0.50)
                                        .step_by(0.01).fixed_decimals(2),
                                )
                                .on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "How far inward from each screen edge the repeated area is copied from.\n\
                                     0.10 = thin strip (10% from edge) — very edge-like content.\n\
                                     0.30 = default — 30% inward, more varied content.\n\
                                     0.50 = halfway to centre — widest variety.",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); })
                                .changed()
                                {
                                    changed = true;
                                }
                                */
                                ui.add_space(6.0);
                                if red_checkbox(ui, &mut self.cfg.show_expansion_method,
                                    "Expansion / Extrusion").on_hover_ui(|ui| { ui.label(egui::RichText::new(
                                    "Physically deforms the screen mesh instead of just filling pixels. EXPANSION stretches the mesh outward (Reach = how far, Smoothness = how gradual); EXTRUSION curls it along the view axis (Strength = how much, Direction = +1 toward viewer, -1 away). They share one mesh displacement, so they're a single toggle and work together. Best, most seamless fill — needs Sphere/Fisheye.",
                                ).color(Color32::from_rgb(0, 220, 245)).size(15.0)); }).changed() {
                                    // These two effects share the same vertex-shader
                                    // mesh displacement and depend on each other, so
                                    // they're a single toggle now. Keep the extrusion
                                    // flag in lock-step so populate_live_params gating
                                    // (which still reads show_extrusion_method) follows.
                                    self.cfg.show_extrusion_method = self.cfg.show_expansion_method;
                                    changed = true;
                                }
                                if self.cfg.show_expansion_method {
                                    self.cfg.show_extrusion_method = true;
                                    if red_slider_labeled(ui, "Expansion reach", egui::Slider::new(
                                                &mut self.cfg.expansion_outer,
                                                0.0..=3.0,
                                            )
                                            )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    if red_slider_labeled(ui, "Expansion seamlessness", egui::Slider::new(
                                                &mut self.cfg.expansion_seamless,
                                                0.0..=3.0,
                                            )
                                            )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    if red_slider_labeled(ui, "Extrusion strength", egui::Slider::new(
                                                &mut self.cfg.extrusion_strength,
                                                0.0..=3.0,
                                            )
                                            )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                    if red_slider_labeled(ui, "Extrusion direction", egui::Slider::new(
                                                &mut self.cfg.extrusion_direction,
                                                -1.0..=1.0,
                                            )
                                            )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                } else {
                                    self.cfg.show_extrusion_method = false;
                                }
                            });
                            ui.add_space(4.0);
                            ui.add_space(4.0);
                            section_magenta(ui, "🖥 Katanga ImGui", |ui| {
                                ui.add_space(4.0);
ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.overlay_enabled, "Enable").changed() {
                                        changed = true;
                                    }
                                    ui.label(RichText::new("In-VR control panel").color(COL_TEXT_DIM).size(12.5));
                                });
                                if self.cfg.overlay_enabled {  // hide options when toggle off
                                ui.add_space(6.0);

                                // ── Force panel cursor — shown ONLY while the Katanga
                                // ImGui main toggle above is enabled. Makes the panel
                                // reticle work in mouselook games that lock/recenter the
                                // OS cursor (otherwise it sticks in the centre). ──
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.panel_cursor_force, "Force panel cursor").changed() {
                                        changed = true;
                                    }
                                    ui.label(RichText::new("works in all games").color(COL_TEXT_DIM).size(12.5));
                                });
                                if self.cfg.panel_cursor_force {
                                    let labels = [
                                        "Relative (physical motion)",
                                        "Absolute (OS cursor)",
                                        "Both (physical motion)",
                                    ];
                                    let cur = (self.cfg.panel_cursor_method as usize).min(2);
                                    let mut new_idx = cur;
                                    egui::ComboBox::from_id_source("panel_cursor_method_combo")
                                        .selected_text(labels[cur])
                                        .show_ui(ui, |ui| {
                                            for (i, lbl) in labels.iter().enumerate() {
                                                ui.selectable_value(&mut new_idx, i, *lbl);
                                            }
                                        });
                                    if new_idx != cur {
                                        self.cfg.panel_cursor_method = new_idx as u32;
                                        changed = true;
                                    }
                                }
                                // ── In-VR panel THEME selector (recolors the VR panel) ──
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Panel theme").color(Color32::from_rgb(215, 222, 245)).size(12.5));
                                    let theme_labels = [
                                        "Colored (default)",
                                        "Dark Blue (white headers)",
                                        "Black (red headers)",
                                        "Cyan headers",
                                        "Light (black headers)",
                                    ];
                                    let cur = (self.cfg.panel_theme as usize).min(4);
                                    let mut new_idx = cur;
                                    egui::ComboBox::from_id_source("panel_theme_combo")
                                        .selected_text(theme_labels[cur])
                                        .show_ui(ui, |ui| {
                                            for (i, lbl) in theme_labels.iter().enumerate() {
                                                ui.selectable_value(&mut new_idx, i, *lbl);
                                            }
                                        });
                                    if new_idx != cur {
                                        self.cfg.panel_theme = new_idx as u32;
                                        changed = true;
                                    }
                                });
                                ui.add_space(4.0);

                                ui.add_enabled_ui(self.cfg.overlay_enabled, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Size (m)   ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_size, 0.5f32..=5.0)
                                            .step_by(0.1).fixed_decimals(1).suffix(" m"));
                                        if r.changed() { changed = true; }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Width ×    ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_size_x, 0.25f32..=3.0)
                                            .step_by(0.05).fixed_decimals(2));
                                        if r.changed() { changed = true; }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Height ×   ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_size_y, 0.25f32..=3.0)
                                            .step_by(0.05).fixed_decimals(2));
                                        if r.changed() { changed = true; }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Offset X   ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_offset_x, -3.0f32..=3.0)
                                            .step_by(0.05).fixed_decimals(2));
                                        if r.changed() { changed = true; }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Offset Y   ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_offset_y, -3.0f32..=3.0)
                                            .step_by(0.05).fixed_decimals(2));
                                        if r.changed() { changed = true; }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Distance   ").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                        let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_distance, 0.5f32..=5.0)
                                            .step_by(0.1).fixed_decimals(1).suffix(" m"));
                                        if r.changed() { changed = true; }
                                    });
                                ui.add_space(4.0);
                                // Panel is a fixed 4:3 — the aspect selector was
                                // removed (changing it rebuilt the overlay swapchain,
                                // which froze the viewer).
                                // Opacity slider (floating translucent panel).
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Opacity").color(Color32::from_rgb(215, 222, 245)).size(12.0));
                                    let r = ui.add(egui::Slider::new(&mut self.cfg.overlay_transparency, 0.0f32..=1.0)
                                        .step_by(0.05).fixed_decimals(2));
                                    if r.changed() { changed = true; }
                                });
                                ui.label(RichText::new("1.00 = solid, lower = see-through to the scene.").color(COL_TEXT_DIM).size(12.0));
                                ui.add_space(4.0);
                                // HUD mode toggle
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.overlay_hud_mode, "HUD Mode").changed() {
                                        changed = true;
                                    }
                                    ui.label(
                                        RichText::new(
                                            if self.cfg.overlay_hud_mode {
                                                "Panel follows your head  (VIEW space)"
                                            } else {
                                                "Panel fixed in room  (LOCAL space)"
                                            }
                                        )
                                        .color(COL_TEXT_DIM).size(12.5)
                                    );
                                });
                                }); // end add_enabled_ui
                                }  // end hide-when-off
                            });

                            ui.add_space(4.0);
                            // ---- GUI THEME ----------------------------------
                            // Grey header/frame + purple title (a fixed style so
                            // it stays readable regardless of the chosen theme).
                            {
                                const GREY_HEADER: Color32 = Color32::from_rgb(72, 72, 80);
                                const GREY_BORDER: Color32 = Color32::from_rgb(120, 120, 132);
                                const PURPLE_TITLE: Color32 = Color32::from_rgb(196, 140, 255);
                                // Honor the active theme + background like every
                                // other section. Default "Colored" keeps the grey
                                // frame + purple title.
                                let v = section_visuals(GREY_HEADER, GREY_BORDER, PURPLE_TITLE, COL_PANEL);
                                ui.set_min_width(ui.available_width());
                                paint_prev_section_bg(ui, "🎨 GUI THEME");
                                let resp = egui::Frame::none()
                                    .fill(v.frame_fill)
                                    .stroke(Stroke::new(1.0, v.border))
                                    .rounding(Rounding::same(4.0))
                                    .inner_margin(0.0)
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        egui::Frame::none()
                                            .fill(v.header_fill)
                                            .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(RichText::new("🎨 GUI THEME").color(v.title).strong().size(17.0));
                                            });
                                        egui::Frame::none()
                                            .fill(v.body_fill)
                                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                            .show(ui, |ui| {
                                                let _g_gt = egui::CollapsingHeader::new(RichText::new("GUI THEME")
                                                        .color(Color32::WHITE).strong().size(13.5))
                                                    .id_source("grp_guitheme").default_open(!self.cfg.gui_theme_collapsed).show(ui, |ui| {
                                                ui.label(RichText::new("Theme").color(COL_TEXT).strong().size(12.0));
                                                let mut theme = self.cfg.gui_theme_id;
                                                let theme_name = |id: u32| match id {
                                                    1 => "Dark Blue",
                                                    2 => "Black",
                                                    3 => "Red",
                                                    4 => "Cyan",
                                                    5 => "White",
                                                    6 => "Orange",
                                                    7 => "Yellow",
                                                    8 => "Green",
                                                    9 => "Purple",
                                                    10 => "Magenta",
                                                    _ => "Colored (default)",
                                                };
                                                egui::ComboBox::from_id_source("gui_theme_combo")
                                                    .selected_text(theme_name(theme))
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(&mut theme, 0, "Colored (default)");
                                                        ui.selectable_value(&mut theme, 1, "Dark Blue");
                                                        ui.selectable_value(&mut theme, 2, "Black");
                                                        ui.selectable_value(&mut theme, 3, "Red");
                                                        ui.selectable_value(&mut theme, 4, "Cyan");
                                                        ui.selectable_value(&mut theme, 5, "White");
                                                        ui.selectable_value(&mut theme, 6, "Orange");
                                                        ui.selectable_value(&mut theme, 7, "Yellow");
                                                        ui.selectable_value(&mut theme, 8, "Green");
                                                        ui.selectable_value(&mut theme, 9, "Purple");
                                                        ui.selectable_value(&mut theme, 10, "Magenta");
                                                    });
                                                if theme != self.cfg.gui_theme_id {
                                                    self.cfg.gui_theme_id = theme;
                                                    // Keep the legacy bool in sync so older
                                                    // viewers/tools reading it stay consistent.
                                                    self.cfg.gui_dark_theme = theme == 1;
                                                    changed = true;
                                                }
                                                ui.add_space(8.0);
                                                ui.label(RichText::new("Custom banner image").color(COL_TEXT).strong().size(12.0));
                                                ui.horizontal(|ui| {
                                                    if ui.button("Set banner…").clicked() {
                                                        if let Some(p) = rfd::FileDialog::new()
                                                            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "ico"])
                                                            .pick_file()
                                                        {
                                                            let path = p.to_string_lossy().to_string();
                                                            self.banner = load_texture_from_file(ui.ctx(), &path, "title_banner")
                                                                .or_else(|| load_banner_texture(ui.ctx()));
                                                            self.cfg.custom_banner_path = path;
                                                            changed = true;
                                                        }
                                                    }
                                                    if ui.button("Reset").clicked() {
                                                        self.cfg.custom_banner_path.clear();
                                                        self.banner = load_banner_texture(ui.ctx());
                                                        changed = true;
                                                    }
                                                });
                                                if !self.cfg.custom_banner_path.is_empty() {
                                                    ui.label(RichText::new(self.cfg.custom_banner_path.clone()).color(COL_TEXT_DIM).size(10.0));
                                                }
                                                ui.add_space(6.0);
                                                ui.label(RichText::new("Custom logo image").color(COL_TEXT).strong().size(12.0));
                                                ui.horizontal(|ui| {
                                                    if ui.button("Set logo…").clicked() {
                                                        if let Some(p) = rfd::FileDialog::new()
                                                            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "ico"])
                                                            .pick_file()
                                                        {
                                                            let path = p.to_string_lossy().to_string();
                                                            self.logo = load_texture_from_file(ui.ctx(), &path, "osiris-logo")
                                                                .or_else(|| load_logo_texture(ui.ctx()));
                                                            self.cfg.custom_logo_path = path;
                                                            changed = true;
                                                        }
                                                    }
                                                    if ui.button("Reset").clicked() {
                                                        self.cfg.custom_logo_path.clear();
                                                        self.logo = load_logo_texture(ui.ctx());
                                                        changed = true;
                                                    }
                                                });
                                                if !self.cfg.custom_logo_path.is_empty() {
                                                    ui.label(RichText::new(self.cfg.custom_logo_path.clone()).color(COL_TEXT_DIM).size(10.0));
                                                }
                                                ui.add_space(6.0);
                                                ui.label(RichText::new("Section background image").color(COL_TEXT).strong().size(12.0))
                                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new("Painted behind each section. When set, section panels go translucent so the image shows through.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                                ui.horizontal(|ui| {
                                                    if ui.button("Set section bg…").clicked() {
                                                        if let Some(p) = rfd::FileDialog::new()
                                                            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "ico"])
                                                            .pick_file()
                                                        {
                                                            let path = p.to_string_lossy().to_string();
                                                            self.section_bg = load_texture_from_file(ui.ctx(), &path, "section_bg");
                                                            self.cfg.section_bg_path = path;
                                                            changed = true;
                                                        }
                                                    }
                                                    if ui.button("Reset").clicked() {
                                                        self.cfg.section_bg_path.clear();
                                                        self.section_bg = None;
                                                        changed = true;
                                                    }
                                                });
                                                if !self.cfg.section_bg_path.is_empty() {
                                                    ui.label(RichText::new(self.cfg.section_bg_path.clone()).color(COL_TEXT_DIM).size(10.0));
                                                }
                                                ui.add_space(6.0);
                                                ui.label(RichText::new("Overall background image").color(COL_TEXT).strong().size(12.0))
                                                    .on_hover_ui(|ui| { ui.label(egui::RichText::new("Painted behind all sections, filling the whole panel as a fixed backdrop.").color(Color32::from_rgb(0, 220, 245)).size(15.0)); });
                                                ui.horizontal(|ui| {
                                                    if ui.button("Set overall bg…").clicked() {
                                                        if let Some(p) = rfd::FileDialog::new()
                                                            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "ico"])
                                                            .pick_file()
                                                        {
                                                            let path = p.to_string_lossy().to_string();
                                                            self.overall_bg = load_texture_from_file(ui.ctx(), &path, "overall_bg");
                                                            self.cfg.overall_bg_path = path;
                                                            changed = true;
                                                        }
                                                    }
                                                    if ui.button("Reset").clicked() {
                                                        self.cfg.overall_bg_path.clear();
                                                        self.overall_bg = None;
                                                        changed = true;
                                                    }
                                                });
                                                if !self.cfg.overall_bg_path.is_empty() {
                                                    ui.label(RichText::new(self.cfg.overall_bg_path.clone()).color(COL_TEXT_DIM).size(10.0));
                                                }
                                                }); // end GUI Theme collapsing
                                                { let o = _g_gt.openness > 0.5; if (!o) != self.cfg.gui_theme_collapsed { self.cfg.gui_theme_collapsed = !o; changed = true; } }
                                            });
                                    });
                                record_section_rect("🎨 GUI THEME", resp.response.rect);
                            }
                            ui.add_space(4.0);
                            section_cyan(ui, "⚙ Auto Adjust", |ui| {
                                ui.set_min_width(ui.available_width() * 0.96);
                                let _g_aa = egui::CollapsingHeader::new(RichText::new("AUTO ADJUST")
                                        .color(Color32::WHITE).strong().size(13.5))
                                    .id_source("grp_autoadjust").default_open(!self.cfg.auto_adjust_collapsed).show(ui, |ui| {
                                // ── Z offset on headlock ─────────────────────────────────
                                ui.label(
                                    RichText::new("Z Offset on Headlock")
                                        .color(Color32::from_rgb(0x00, 0xBF, 0xC8))
                                        .strong()
                                        .size(12.0),
                                );
                                ui.label(
                                    RichText::new("Adds a Z offset when headlock is ON, reverts when OFF.")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.auto_z_enabled, "Enable").changed() {
                                        changed = true;
                                    }
                                    ui.label(RichText::new("Value (m)").color(Color32::from_rgb(215,222,245)).size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.auto_z_enabled,
                                        egui::DragValue::new(&mut self.cfg.auto_z_value)
                                            .speed(0.5)
                                            .clamp_range(-500.0_f32..=500.0_f32)
                                            .fixed_decimals(1),
                                    );
                                    if resp.changed() { changed = true; }
                                });
                                ui.add_space(4.0);

                                // ── Roll offset on headlock ──────────────────────────────
                                ui.label(
                                    RichText::new("Roll Offset on Headlock")
                                        .color(Color32::from_rgb(0x00, 0xBF, 0xC8))
                                        .strong()
                                        .size(12.0),
                                );
                                ui.label(
                                    RichText::new("Adds a roll offset when headlock is ON, reverts when OFF.")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.auto_roll_enabled, "Enable").changed() {
                                        changed = true;
                                    }
                                    ui.label(RichText::new("Value (rad)").color(Color32::from_rgb(215,222,245)).size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.auto_roll_enabled,
                                        egui::DragValue::new(&mut self.cfg.auto_roll_value)
                                            .speed(0.01)
                                            .clamp_range(-3.14f32..=3.14f32)
                                            .fixed_decimals(3),
                                    );
                                    if resp.changed() { changed = true; }
                                });
                                ui.add_space(6.0);

                                // ── X and Y Offsets on Headlock ──────────────────────────
                                ui.colored_label(Color32::from_rgb(120, 190, 255), "X Offset on Headlock");
                                ui.label(RichText::new("Adds an X offset when headlock is ON, reverts when OFF.")
                                    .color(COL_TEXT_DIM).size(12.5));
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.auto_x_enabled, "Enable").changed() {
                                        changed = true;
                                    }
                                    ui.label(RichText::new("Value (m)").color(Color32::from_rgb(215,222,245)).size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.auto_x_enabled,
                                        egui::DragValue::new(&mut self.cfg.auto_x_value)
                                            .speed(0.5)
                                            .clamp_range(-500.0..=500.0)
                                            .fixed_decimals(1),
                                    );
                                    if resp.changed() { changed = true; }
                                });
                                ui.add_space(4.0);

                                ui.colored_label(Color32::from_rgb(120, 190, 255), "Y Offset on Headlock");
                                ui.label(RichText::new("Adds a Y offset when headlock is ON, reverts when OFF.")
                                    .color(COL_TEXT_DIM).size(12.5));
                                ui.horizontal(|ui| {
                                    if red_checkbox(ui, &mut self.cfg.auto_y_enabled, "Enable").changed() {
                                        changed = true;
                                    }
                                    ui.label(RichText::new("Value (m)").color(Color32::from_rgb(215,222,245)).size(12.0));
                                    let resp = ui.add_enabled(
                                        self.cfg.auto_y_enabled,
                                        egui::DragValue::new(&mut self.cfg.auto_y_value)
                                            .speed(0.5)
                                            .clamp_range(-500.0..=500.0)
                                            .fixed_decimals(1),
                                    );
                                    if resp.changed() { changed = true; }
                                });
                                ui.add_space(6.0);

                                // ── Sphere height by aspect ratio ────────────────────────
                                ui.label(
                                    RichText::new("Sphere Height by Aspect Ratio")
                                        .color(Color32::from_rgb(0x00, 0xBF, 0xC8))
                                        .strong()
                                        .size(12.0),
                                );
                                ui.label(
                                    RichText::new("Auto-adjusts sphere/box height when source ratio is detected.")
                                        .color(COL_TEXT_DIM)
                                        .size(12.5),
                                );
                                if red_checkbox(ui, &mut self.cfg.auto_height_enabled, "Enable auto height").changed() {
                                    changed = true;
                                }
                                ui.add_enabled_ui(self.cfg.auto_height_enabled, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("16:9").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                        let r = ui.add(
                                            egui::DragValue::new(&mut self.cfg.sphere_y_169)
                                                .speed(0.01).clamp_range(0.05_f32..=3.0_f32).fixed_decimals(2),
                                        );
                                        if r.changed() { changed = true; }
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("4:3").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                        let r = ui.add(
                                            egui::DragValue::new(&mut self.cfg.sphere_y_43)
                                                .speed(0.01).clamp_range(0.05_f32..=3.0_f32).fixed_decimals(2),
                                        );
                                        if r.changed() { changed = true; }
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("21:9").color(Color32::from_rgb(215,222,245)).strong().size(12.0));
                                        let r = ui.add(
                                            egui::DragValue::new(&mut self.cfg.sphere_y_219)
                                                .speed(0.01).clamp_range(0.05_f32..=3.0_f32).fixed_decimals(2),
                                        );
                                        if r.changed() { changed = true; }
                                    });
                                });
                                }); // end Auto Adjust collapsing
                                { let o = _g_aa.openness > 0.5; if (!o) != self.cfg.auto_adjust_collapsed { self.cfg.auto_adjust_collapsed = !o; changed = true; } }
                            });
                            ui.add_space(4.0);
                            // -------- Hotkeys (under Edge Stretch) --------
                            // Constrain to 60% of column width so it matches
                            // the sections above (Image, Filtering, Edge Stretch).
                            ui.allocate_ui(
                                egui::vec2(ui.available_width(), f32::INFINITY),
                                |ui| {
                                    if hotkeys_section(
                                        ui,
                                        &mut self.cfg.hotkey_bindings,
                                        &mut self.capturing,
                                        &mut self.hotkey_mgr,
                                        &mut self.cfg.hotkey_delivery_method,
                                        &mut self.cfg.vr_hotkeys_enabled,
                                    ) {
                                        changed = true;
                                    }
                                }
                            );
                        });
                    });
                });
            });

        if changed {
            self.push_to_shm(true);
        }
    }

    // Collapsible sections must ALWAYS start closed and never remember their
    // open state across an app exit. eframe's default is to persist egui memory
    // (which includes every CollapsingHeader's open/closed flag) to disk on exit
    // and restore it on launch; that is what made an opened section reappear open
    // next time. Returning false disables egui-memory persistence entirely. This
    // does NOT affect persist_window (OS window size/position lives in a separate
    // key and is still restored).
    fn persist_egui_memory(&self) -> bool { false }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        // Disable SHM so the viewer falls back to the on-disk preset.
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(writer) = guard.as_mut() {
                writer.disable();
            }
        }
        // Safety net: if the window was closed via OS (Alt+F4, taskbar right-click,
        // etc.) without going through the close_requested path, make sure any
        // running viewer is killed. kill_viewer_process() is idempotent —
        // calling it twice is harmless (second call finds no process to kill).
        if !self.quit_pushed {
            self.push_quit_to_shm();
            std::thread::sleep(std::time::Duration::from_millis(150));
            self.kill_viewer_process();
        }
    }
}

/// Bytes of the .ico file embedded into the binary at compile time.
/// Used both as the Windows .exe resource icon (via `build.rs` +
/// `winres`) and as the live window/taskbar icon (decoded at startup
/// in `main` and passed to `ViewportBuilder::with_icon`).
const APP_ICON_BYTES: &[u8] = include_bytes!("../app-icon.ico");

/// Decode `APP_ICON_BYTES` into an `egui::IconData` for the window
/// chrome. Returns `None` if decoding fails — eframe falls back to its
/// own default icon, which is fine.
fn load_window_icon() -> Option<egui::IconData> {
    // Try the canonical ICO decode first.
    let img = match image::load_from_memory_with_format(
        APP_ICON_BYTES,
        image::ImageFormat::Ico,
    ) {
        Ok(img) => img,
        Err(err) => {
            log::warn!("Window icon: ICO decode failed ({}), trying BMP fallback", err);
            // Same fallback as the logo loader: many ICO payloads are
            // BMP-with-DIB-header-no-file-header. Reuse the same helpers.
            if APP_ICON_BYTES.len() < 22 {
                return None;
            }
            let off =
                u32::from_le_bytes(APP_ICON_BYTES[18..22].try_into().ok()?) as usize;
            let size =
                u32::from_le_bytes(APP_ICON_BYTES[14..18].try_into().ok()?) as usize;
            let payload = APP_ICON_BYTES.get(off..off + size)?;
            let bmp = ico_payload_to_bmp(payload)?;
            image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?
        }
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    })
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    // Build the viewport with the window icon if we can decode it.
    let mut viewport = egui::ViewportBuilder::default()
        // Slightly wider than 0.6.0 (1400) to fit the new Hotkeys
        // column to the right of Mouse Emu / 6DOF MODS, while
        // staying compact. eframe restores user's last-used size
        // via the `persistence` feature on subsequent launches.
        .with_inner_size([1400.0, 720.0])
        .with_min_inner_size([1000.0, 540.0])
        .with_title(APP_TITLE);
    if let Some(icon) = load_window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        // Persist window position and size across launches. Storage lives
        // under the OS-standard config dir for "OsirisVRViewer".
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(|cc| Box::new(OsirisGui::new(cc))),
    )
}
