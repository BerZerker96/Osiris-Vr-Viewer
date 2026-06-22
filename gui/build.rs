// Build script: embed the app icon into the Windows .exe so File
// Explorer, the taskbar, and Alt-Tab show our icon instead of the
// generic Rust binary icon.

fn main() {
    println!("cargo:rerun-if-changed=gui.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("gui.ico");
        if let Err(err) = res.compile() {
            eprintln!("cargo:warning=winres icon embed failed: {}", err);
        }
    }
}
