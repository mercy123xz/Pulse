//! Build script: makes the Windows `.exe` self-contained.
//!
//! Rust programs built on Windows normally need Microsoft's `vcruntime140.dll`, which comes
//! with the "Visual C++ Redistributable" that not every PC has. This bakes that runtime into
//! `pulse.exe` instead, so people can download the .exe and double-click it, nothing else.
//! The rest of the C runtime (the "Universal CRT") is part of Windows 10/11 itself, so it
//! stays a normal system DLL.
//!
//! Same technique as the `static_vcruntime` crate (MIT / Apache-2.0), written inline.
//!
//! It also puts Pulse's icon and version details into the .exe, so File Explorer, the
//! taskbar and the installer show the Pulse logo.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/pulse.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/pulse.ico")
            .set("ProductName", "Pulse")
            .set("FileDescription", "Pulse music player")
            .set("CompanyName", "Pulse")
            .set("LegalCopyright", "Pulse contributors");
        if let Err(e) = res.compile() {
            // A missing resource compiler shouldn't stop the build; the .exe just has no icon.
            println!("cargo:warning=Couldn't add the icon to the .exe: {e}");
        }
    }

    // Only applies to the Microsoft (MSVC) toolchain, which is what Windows builds use.
    // On Linux / macOS this script does nothing.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    // An 8 MB stack for the window's thread (Windows gives 1 MB by default, Linux 8 MB), so a
    // deep page can't overflow it.
    println!("cargo:rustc-link-arg-bins=/STACK:8388608");

    // Stop the linker from picking the DLL versions of the Visual C++ runtime...
    for lib in [
        "libvcruntimed.lib",
        "vcruntime.lib",
        "vcruntimed.lib",
        "libcmtd.lib",
        "msvcrt.lib",
        "msvcrtd.lib",
        "libucrt.lib",
        "libucrtd.lib",
    ] {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:{lib}");
    }
    // ...and use the built-in (static) versions, plus Windows' own Universal CRT.
    for lib in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
        println!("cargo:rustc-link-arg=/DEFAULTLIB:{lib}");
    }
}
