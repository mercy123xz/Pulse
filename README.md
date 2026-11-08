<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/logo-for-dark-backgrounds.svg">
    <img src="assets/logo/logo-for-light-backgrounds.svg" alt="Pulse" height="72">
  </picture>
</p>

<p align="center">
  <b>A fast, native music player for Windows, written in Rust.</b><br>
  Stream free, full-length music from independent artists on <a href="https://audius.co">Audius</a>.<br>
  One <code>.exe</code>: no installer, no account, nothing else to download.
</p>

<p align="center">
  <a href="https://github.com/mercy123xz/Pulse/releases/latest"><b>⬇ Download for Windows</b></a>
  &nbsp;·&nbsp; <a href="#features">Features</a>
  &nbsp;·&nbsp; <a href="#keyboard-shortcuts">Shortcuts</a>
  &nbsp;·&nbsp; <a href="#building-from-source">Build from source</a>
</p>

<!-- Add a screenshot: save it as assets/screenshot.png, then delete these comment markers.
<p align="center"><img src="assets/screenshot.png" alt="Pulse playing music" width="900"></p>
-->

## Features

**Discover**
- **Home:** trending playlists, this week's top tracks, up-and-coming underground tracks and quick genre picks
- **Trending charts** for this week, month, year or all time, filtered by 24 genres
- **Live search** for songs, artists and playlists as you type
- **Artist pages** with their most-played tracks, and full **playlist and album** pages

**Listen**
- Songs start playing right away while the rest downloads, and you can jump anywhere in them
- Queue with **Play next** and **Add to queue**, plus shuffle and repeat (all or one)
- Picks up where you left off: your last song, volume and settings are remembered

**Your library**
- **Liked Songs**, **Recently Played** and your **own playlists**
- Everything is saved on your PC. No account or sign-in needed

**Made for everyone**
- Dark and light themes
- Full keyboard control and screen-reader support, with text colors that meet WCAG AA contrast
- A true desktop app with no web browser inside. It runs on any 64-bit Windows 10 or 11 PC, even one without a graphics driver

## Download

1. Grab **`pulse.exe`** from the [latest release](https://github.com/mercy123xz/Pulse/releases/latest).
2. Double-click it. That's it.

The first time you open it, Windows may show **"Windows protected your PC"**. It does that for any
new app that isn't signed with a paid certificate. Click **More info → Run anyway**.

Your likes, playlists and settings are stored in `%APPDATA%\pulse-player`. To uninstall, delete
`pulse.exe` (and that folder if you want your data gone too).

## Keyboard shortcuts

| Action | Keys |
|---|---|
| Play / pause | <kbd>Space</kbd> |
| Next / previous song | <kbd>Ctrl</kbd> + <kbd>→</kbd> / <kbd>Ctrl</kbd> + <kbd>←</kbd> |
| Volume up / down | <kbd>Ctrl</kbd> + <kbd>↑</kbd> / <kbd>Ctrl</kbd> + <kbd>↓</kbd> |
| Mute | <kbd>M</kbd> |
| Shuffle / repeat | <kbd>S</kbd> / <kbd>R</kbd> |
| Search | <kbd>Ctrl</kbd> + <kbd>K</kbd> |
| Back / forward between pages | <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>Alt</kbd> + <kbd>→</kbd> (or your mouse's side buttons) |
| Move between buttons and songs | <kbd>Tab</kbd> / <kbd>Shift</kbd> + <kbd>Tab</kbd> |
| On a selected song: play / add to queue / play next | <kbd>Enter</kbd> / <kbd>Q</kbd> / <kbd>N</kbd> |
| Quit | <kbd>Ctrl</kbd> + <kbd>Q</kbd> |

Right-click any song for more options, like adding it to one of your playlists.

## Building from source

You need [Rust](https://rustup.rs). On Windows, the installer offers to set up the Visual Studio C++
Build Tools too; say yes.

```powershell
git clone https://github.com/mercy123xz/Pulse.git
cd Pulse
cargo run --release
```

The first build takes a few minutes. The finished app is at `target\release\pulse.exe`.

Every push to this repository also builds `pulse.exe` automatically: open the **Actions** tab,
pick the latest **Build Windows app** run, and download the `pulse-windows` artifact.

On Linux you'll also need the ALSA development headers (`sudo apt install libasound2-dev`).

### How the code is organised

| File | What it does |
|---|---|
| `src/main.rs` | Opens the window and picks the graphics backend (DirectX 12 on Windows) |
| `src/api.rs` | Talks to the Audius API: trending, search, playlists, artists, audio streams |
| `src/audio.rs` | Audio engine: plays MP3s while they download, seeking, volume, resuming dropped downloads |
| `src/app.rs` | App logic: pages and history, the queue, shuffle and repeat, likes, playlists, saving |
| `src/ui.rs` | Everything you see: sidebar, top bar, pages, player bar, dialogs |
| `src/widgets.rs` | Custom-drawn pieces: song rows, cards, sliders, buttons, accessibility helpers |
| `src/theme.rs` | Colors, fonts and styling for both themes, plus a contrast test |
| `build.rs` | Bakes the Visual C++ runtime into the Windows `.exe` so it runs on any PC |

Run the tests with `cargo test`.

## Built with

- [egui / eframe](https://github.com/emilk/egui): the user interface
- [wgpu](https://wgpu.rs): graphics through DirectX 12 on Windows
- [rodio](https://github.com/RustAudio/rodio) and [Symphonia](https://github.com/pdeljanov/Symphonia): audio playback and MP3 decoding
- [ureq](https://github.com/algesten/ureq) with [rustls](https://github.com/rustls/rustls): networking
- [Phosphor Icons](https://phosphoricons.com) and the [Inter](https://rsms.me/inter/) typeface (SIL Open Font License)

## About the music

All music is streamed from [Audius](https://audius.co) through its free public API. Artists keep all
rights to their work; please credit them when you share it. Tracks that artists have locked
(for example, pay-to-unlock releases) are hidden.

Pulse is an independent project and isn't affiliated with or endorsed by Audius.
