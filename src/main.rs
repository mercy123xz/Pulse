// Hide the black console window on Windows for release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod app;
mod artcache;
mod audio;
mod audiocache;
mod backdrop;
mod backup;
mod crash;
mod deezer;
mod diag;
mod discord;
mod downloads;
mod dsp;
mod fixes;
mod i18n;
mod import;
mod itunes;
mod local;
mod lyrics;
mod net;
mod perf;
mod report;
mod settings;
mod share;
mod single;
mod suggest;
mod tidal;
mod theme;
mod themes;
mod ui;
mod update;
mod widgets;
mod youtube;

/// Windows slows down programs that are minimized or in the background ("Efficiency
/// mode"), and that can starve the threads that download and decode the music: it stalls
/// as if paused. Ask Windows to leave Pulse at full speed.
#[cfg(windows)]
fn no_power_throttling() {
    #[repr(C)]
    struct PowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }
    const PROCESS_POWER_THROTTLING: i32 = 4; // ProcessPowerThrottling
    const EXECUTION_SPEED: u32 = 0x1;
    const IGNORE_TIMER_RESOLUTION: u32 = 0x4;
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn SetProcessInformation(process: isize, class: i32, info: *const PowerThrottlingState, size: u32) -> i32;
    }
    // Control a setting with its state bit off: throttling turned off for that setting.
    let state = PowerThrottlingState { version: 1, control_mask: EXECUTION_SPEED | IGNORE_TIMER_RESOLUTION, state_mask: 0 };
    // SAFETY: passes a valid struct of the size we give; a failure (older Windows) is harmless.
    let ok = unsafe { SetProcessInformation(GetCurrentProcess(), PROCESS_POWER_THROTTLING, &state, std::mem::size_of::<PowerThrottlingState>() as u32) };
    if ok == 0 {
        crash::note("Couldn't turn off Windows' power throttling for Pulse");
    }
}
#[cfg(not(windows))]
fn no_power_throttling() {}

fn main() -> eframe::Result {
    // A crash leaves a report (on this PC only) that Pulse offers to copy next time.
    crash::install();
    // Already open: that window comes to the front instead.
    if !single::claim() {
        return Ok(());
    }
    no_power_throttling();
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pulse")
            .with_app_id("pulse-player")
            .with_inner_size([1280.0, std::env::var("PULSE_DEBUG_HEIGHT").ok().and_then(|h| h.parse().ok()).unwrap_or(800.0)])
            .with_min_inner_size([980.0, 620.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    #[cfg(windows)]
    use_directx(&mut options);
    eframe::run_native("Pulse", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

/// Windows only: draw with DirectX 12 instead of OpenGL, so Pulse opens on any
/// Windows 10/11 PC, even one without a proper graphics driver (wgpu then uses
/// Windows' built-in software renderer).
#[cfg(windows)]
fn use_directx(options: &mut eframe::NativeOptions) {
    use eframe::egui_wgpu::{WgpuSetup, WgpuSetupCreateNew, wgpu};
    options.renderer = eframe::Renderer::Wgpu;
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    // A music player doesn't need the gaming GPU: prefer the battery-friendly one.
    setup.power_preference = wgpu::PowerPreference::LowPower;
    // DirectX 12 only. Left to choose, wgpu sometimes picks Vulkan, and some Vulkan drivers
    // crash Pulse. Windows' software renderer (WARP) is DirectX 12 too, so PCs without a
    // graphics driver still open.
    setup.instance_descriptor.backends = wgpu::Backends::DX12;
    // The shader compiler built into Windows, so no extra DLL has to ship with the .exe.
    setup.instance_descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    if !crate::settings::hardware_acceleration_enabled() {
        // "Hardware acceleration" off: draw with Windows' built-in software renderer (WARP).
        setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters: &[wgpu::Adapter], _surface: Option<&wgpu::Surface<'_>>| {
            adapters
                .iter()
                .find(|a| a.get_info().device_type == wgpu::DeviceType::Cpu)
                .or_else(|| adapters.first())
                .cloned()
                .ok_or_else(|| "No graphics adapter found".to_string())
        }));
    }
    options.wgpu_options.wgpu_setup = WgpuSetup::CreateNew(setup);
}

/// The window and taskbar icon: the same picture as the .exe icon.
fn app_icon() -> egui::IconData {
    let png = include_bytes!("../assets/icon/pulse-256.png");
    match image::load_from_memory(png) {
        Ok(img) => {
            let img = img.into_rgba8();
            egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}
