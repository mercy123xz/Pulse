//! Build script: makes the Windows `.exe` self-contained.
//!
//! Rust programs built on Windows normally need Microsoft's `vcruntime140.dll`, which comes
//! with the "Visual C++ Redistributable" that not every PC has. This bakes that runtime into
//! `pulse.exe` instead, so people can download the .exe and double-click it, nothing else.
//! The rest of the C runtime (the "Universal CRT") is part of Windows 10/11 itself, so it
//! stays a normal system DLL.
//!
//! Same technique as the `static_vcruntime` crate (MIT / Apache-2.0), written inline.

fn main() {
    // Only applies to the Microsoft (MSVC) toolchain, which is what Windows builds use.
    // On Linux / macOS this script does nothing.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

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
