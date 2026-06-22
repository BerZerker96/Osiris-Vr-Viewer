//! Shared-memory writer for the Osiris GUI.
//!
//! Mirrors the viewer's reader in `osiris-vr-viewer/src/utils/live_params.rs`.
//! The GUI is the *writer*: it bumps the sequence counter on every change.
//! The viewer's reader only re-applies values when the seq actually moved,
//! so idle GUI = zero per-frame work in the viewer.

#[cfg(target_os = "windows")]
mod imp {
    use anyhow::{anyhow, Context};
    use osiris_shared::{LiveParamsMapping, LIVE_MAGIC, LIVE_VERSION, SHM_NAME, SHM_SIZE};
    use std::ffi::CString;
    use windows::core::PCSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows::Win32::System::Memory::{
        CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_READ, FILE_MAP_WRITE,
        MEMORYMAPPEDVIEW_HANDLE, PAGE_READWRITE,
    };

    pub struct LiveParamsWriter {
        _file_handle: HANDLE,
        view: MEMORYMAPPEDVIEW_HANDLE,
        seq: u64,
    }

    // SAFETY: Win32 HANDLEs and memory-mapped view pointers are safe to
    // use from multiple threads, as long as access is serialised by the
    // caller (we wrap in a Mutex when sharing). The handles aren't
    // bound to any thread-local state.
    unsafe impl Send for LiveParamsWriter {}

    impl LiveParamsWriter {
        pub fn new() -> anyhow::Result<Self> {
            let name = CString::new(SHM_NAME).context("Bad SHM name")?;
            // Same call as the viewer: returns a handle to the existing
            // mapping if it's already there, creates it otherwise. Whoever
            // came up first "wins"; both peers see the same memory.
            let file_handle = unsafe {
                CreateFileMappingA(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    0,
                    SHM_SIZE as u32,
                    PCSTR(name.as_ptr() as *const u8),
                )
            }
            .map_err(|e| anyhow!("CreateFileMapping failed: {:?}", e))?;
            if file_handle.is_invalid() {
                return Err(anyhow!("CreateFileMapping returned an invalid handle"));
            }

            let view = unsafe {
                MapViewOfFile(file_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, SHM_SIZE)
            }
            .map_err(|e| {
                unsafe {
                    let _ = CloseHandle(file_handle);
                }
                anyhow!("MapViewOfFile failed: {:?}", e)
            })?;
            if view.is_invalid() {
                unsafe {
                    let _ = CloseHandle(file_handle);
                }
                return Err(anyhow!("MapViewOfFile returned an invalid view"));
            }

            log::info!("Opened live-params shared memory (writer side)");
            Ok(Self {
                _file_handle: file_handle,
                view,
                seq: 0,
            })
        }

        /// Write the params block with a fresh sequence number.
        ///
        /// Note: this is NOT safe against torn reads in a strict sense —
        /// the viewer's reader uses a volatile read with a seq check, so a
        /// transiently-torn frame just gets skipped. That's good enough
        /// for slider updates at 60 Hz.
        pub fn write(&mut self, mut mapping: LiveParamsMapping) {
            self.seq = self.seq.wrapping_add(1);
            mapping.magic = LIVE_MAGIC;
            mapping.version = LIVE_VERSION;
            mapping.params.set_seq(self.seq);
            let dst = self.view.0 as *mut LiveParamsMapping;
            if !dst.is_null() {
                unsafe { std::ptr::write_volatile(dst, mapping) };
            }
        }

        /// Mark the channel as inactive. Called on GUI exit so the viewer
        /// stops applying overrides and falls back to the on-disk preset.
        pub fn disable(&mut self) {
            let dst = self.view.0 as *mut LiveParamsMapping;
            if dst.is_null() {
                return;
            }
            unsafe {
                let mut current = std::ptr::read_volatile(dst);
                current.params.enabled = 0;
                self.seq = self.seq.wrapping_add(1);
                current.params.set_seq(self.seq);
                std::ptr::write_volatile(dst, current);
            }
        }
    }

    impl Drop for LiveParamsWriter {
        fn drop(&mut self) {
            self.disable();
            unsafe {
                let _ = UnmapViewOfFile(self.view);
                let _ = CloseHandle(self._file_handle);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use osiris_shared::LiveParamsMapping;
    pub struct LiveParamsWriter;
    impl LiveParamsWriter {
        pub fn new() -> anyhow::Result<Self> {
            anyhow::bail!("Live-params shared memory is Windows-only for now");
        }
        pub fn write(&mut self, _mapping: LiveParamsMapping) {}
        pub fn disable(&mut self) {}
    }
}

pub use imp::LiveParamsWriter;

// ─── Upstream events reader (viewer → GUI) ──────────────────────
// Reads the toggle-state echo written by the viewer so VR controller
// hotkey changes are reflected in the GUI checkboxes.

#[cfg(target_os = "windows")]
mod upstream_imp {
    use anyhow::{anyhow, Context};
    use osiris_shared::{UpstreamEvents, UPSTREAM_MAGIC, UPSTREAM_NAME, UPSTREAM_SIZE};
    use std::ffi::CString;
    use windows::core::PCSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows::Win32::System::Memory::{
        CreateFileMappingA, MapViewOfFile, UnmapViewOfFile, FILE_MAP_READ, FILE_MAP_WRITE,
        MEMORYMAPPEDVIEW_HANDLE, PAGE_READWRITE,
    };

    pub struct UpstreamReader {
        _file_handle: HANDLE,
        view: MEMORYMAPPEDVIEW_HANDLE,
        last_seq: u32,
    }

    unsafe impl Send for UpstreamReader {}

    impl UpstreamReader {
        pub fn new() -> anyhow::Result<Self> {
            let name = CString::new(UPSTREAM_NAME).context("Bad upstream SHM name")?;
            let file_handle = unsafe {
                CreateFileMappingA(
                    INVALID_HANDLE_VALUE,
                    None,
                    PAGE_READWRITE,
                    0,
                    UPSTREAM_SIZE as u32,
                    PCSTR(name.as_ptr() as *const u8),
                )
            }
            .map_err(|e| anyhow!("CreateFileMapping (upstream reader) failed: {:?}", e))?;
            if file_handle.is_invalid() {
                return Err(anyhow!("CreateFileMapping (upstream reader) invalid handle"));
            }
            let view = unsafe {
                MapViewOfFile(file_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, UPSTREAM_SIZE)
            }
            .map_err(|e| {
                unsafe { let _ = CloseHandle(file_handle); }
                anyhow!("MapViewOfFile (upstream reader) failed: {:?}", e)
            })?;
            Ok(Self { _file_handle: file_handle, view, last_seq: 0 })
        }

        /// Poll for new toggle state. Returns `Some(toggle_bits)` if
        /// the viewer wrote new data since the last call, `None` otherwise.
        pub fn poll(&mut self) -> Option<u32> {
            let src = self.view.0 as *const UpstreamEvents;
            if src.is_null() { return None; }
            let ev = unsafe { std::ptr::read_volatile(src) };
            if ev.magic != UPSTREAM_MAGIC { return None; }
            if ev.seq == self.last_seq { return None; }
            self.last_seq = ev.seq;
            Some(ev.toggle_bits)
        }
    }

    impl Drop for UpstreamReader {
        fn drop(&mut self) {
            unsafe {
                let _ = UnmapViewOfFile(self.view);
                let _ = CloseHandle(self._file_handle);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod upstream_imp {
    pub struct UpstreamReader;
    impl UpstreamReader {
        pub fn new() -> anyhow::Result<Self> {
            anyhow::bail!("Upstream events SHM is Windows-only");
        }
        pub fn poll(&mut self) -> Option<u32> { None }
    }
}

pub use upstream_imp::UpstreamReader;
