// Hide the black console window on Windows for release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod deezer;
mod net;
mod theme;
mod ui;
mod widgets;
mod youtube;

fn main() -> eframe::Result {
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pulse")
            .with_app_id("pulse-player")
            .with_inner_size([1280.0, 800.0])
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
    // The shader compiler built into Windows, so no extra DLL has to ship with the .exe.
    setup.instance_descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    options.wgpu_options.wgpu_setup = WgpuSetup::CreateNew(setup);
}

/// Draws the window icon in code: a violet rounded square with sound bars.
fn app_icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let bars = [(14.0, 0.35), (24.0, 0.7), (34.0, 0.95), (44.0, 0.55), (54.0 - 4.0, 0.3)];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded square mask
            let r = 14.0;
            let cx = fx.clamp(r, n as f32 - r);
            let cy = fy.clamp(r, n as f32 - r);
            let inside = (fx - cx).powi(2) + (fy - cy).powi(2) <= r * r;
            if !inside {
                continue;
            }
            let t = fy / n as f32;
            let mut c = [
                (167.0 * (1.0 - t) + 124.0 * t) as u8,
                (139.0 * (1.0 - t) + 58.0 * t) as u8,
                (250.0 * (1.0 - t) + 237.0 * t) as u8,
            ];
            for (bx, h) in bars {
                let half = h * 20.0;
                if (fx - bx).abs() < 3.0 && (fy - 32.0).abs() < half {
                    c = [255, 255, 255];
                }
            }
            let i = (y * n + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}
