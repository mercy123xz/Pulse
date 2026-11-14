//! The window's title bar is drawn by Windows, white by default: a bright strip above a
//! dark app. This paints it in the theme's own colours instead, and again whenever the
//! theme changes: the exact sidebar and text colours on Windows 11, and Windows' dark (or
//! light) title bar on Windows 10, which can't take exact colours.

use egui::Color32;

/// Matches the title bar to the theme. Cheap enough for every frame: Windows is only asked
/// when the colours change.
pub fn match_theme(frame: &eframe::Frame, ctx: &egui::Context) {
    let fill = crate::theme::parts().sidebar.fill.unwrap_or(crate::theme::pal().sidebar);
    let text = crate::theme::pal().text;
    let key = egui::Id::new("titlebar_colours");
    // Also a few times while the window settles in after opening: the windowing library
    // sets the system's look on the new window, and that mustn't win.
    let frame_nr = ctx.cumulative_frame_nr();
    let settling = frame_nr < 240 && frame_nr % 40 == 0;
    if !settling && ctx.data(|d| d.get_temp::<(Color32, Color32)>(key)) == Some((fill, text)) {
        return;
    }
    if paint(frame, fill, text) {
        ctx.data_mut(|d| d.insert_temp(key, (fill, text)));
    }
}

/// How bright a colour looks (0 black, 1 white).
fn brightness(c: Color32) -> f32 {
    (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0
}

#[cfg(windows)]
fn paint(frame: &eframe::Frame, fill: Color32, text: Color32) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, value: *const core::ffi::c_void, size: u32) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> isize;
        fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    }
    // Windows 10 keeps drawing the old title bar until the window is next activated or
    // deactivated, so it's told it was (twice, ending as it really is). Posted, not sent:
    // we're in the middle of drawing a frame here.
    const WM_NCACTIVATE: u32 = 0x0086;
    const USE_IMMERSIVE_DARK_MODE: u32 = 20; // Windows 10 1809 and later
    const BORDER_COLOR: u32 = 34; // Windows 11 and later (older ones ignore these)
    const CAPTION_COLOR: u32 = 35;
    const TEXT_COLOR: u32 = 36;

    let debug = std::env::var_os("PULSE_DEBUG_TITLEBAR").is_some();
    let handle = match frame.window_handle() {
        Ok(h) => h,
        Err(e) => {
            if debug {
                eprintln!("titlebar: no window handle: {e:?}");
            }
            return false;
        }
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        if debug {
            eprintln!("titlebar: not a Win32 handle: {:?}", handle.as_raw());
        }
        return false;
    };
    let hwnd = h.hwnd.get();
    let colorref = |c: Color32| c.r() as u32 | (c.g() as u32) << 8 | (c.b() as u32) << 16;
    let set = |attribute: u32, value: u32| {
        // SAFETY: a live window of ours, and a 4-byte value as each of these attributes takes.
        let hr = unsafe { DwmSetWindowAttribute(hwnd, attribute, (&value as *const u32).cast(), 4) };
        if debug {
            eprintln!("titlebar: hwnd {hwnd:#x} attribute {attribute} = {value:#x}: hr {hr:#x}");
        }
        hr
    };
    set(USE_IMMERSIVE_DARK_MODE, u32::from(brightness(fill) < 0.5));
    set(CAPTION_COLOR, colorref(fill));
    set(TEXT_COLOR, colorref(text));
    set(BORDER_COLOR, colorref(fill));
    // SAFETY: our own window; these only make it redraw its title bar.
    unsafe {
        let active = GetForegroundWindow() == hwnd;
        PostMessageW(hwnd, WM_NCACTIVATE, usize::from(!active), 0);
        PostMessageW(hwnd, WM_NCACTIVATE, usize::from(active), 0);
    }
    true
}

#[cfg(not(windows))]
fn paint(_frame: &eframe::Frame, _fill: Color32, _text: Color32) -> bool {
    true // the window manager draws the title bar there
}

#[cfg(test)]
mod tests {
    #[test]
    fn tells_dark_from_light() {
        assert!(super::brightness(egui::Color32::from_rgb(18, 18, 22)) < 0.5);
        assert!(super::brightness(egui::Color32::from_rgb(245, 245, 247)) > 0.5);
    }
}
