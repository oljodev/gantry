// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Before the webview is created, and before any thread exists to race with (`linux.rs`).
    #[cfg(target_os = "linux")]
    {
        use gantry_app_lib::linux::{DMABUF, dmabuf_value, nvidia_driver};

        let set = std::env::var(DMABUF).ok();
        if let Some(value) = dmabuf_value(set.as_deref(), nvidia_driver()) {
            eprintln!("{DMABUF}={value}: the NVIDIA driver's DMA-BUF path leaves the window blank");
            // SAFETY: the first statement of `main`, on the only thread there is. The rule the
            // 2024 edition enforces is that nothing else may be reading the environment at the
            // same time, and here nothing else exists yet.
            unsafe { std::env::set_var(DMABUF, value) };
        }
    }
    gantry_app_lib::run()
}
