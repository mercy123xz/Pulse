<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo/logo-for-dark-backgrounds.svg">
    <img src="assets/logo/logo-for-light-backgrounds.svg" alt="Pulse" height="72">
  </picture>
</p>

<p align="center">
  <b>A fast, native music player for Windows, written in Rust.</b><br>
  Search any artist and play full songs, with autoplay when your queue runs out.<br>
  One <code>.exe</code>: no installer, no account.
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
- **Home:** today's top songs, popular albums, artists and playlists, and quick genre picks
- **Charts:** the top 100 songs overall or in 27 genres
- **Live search** for songs, artists, albums and playlists as you type
- **Artist pages** with their latest release, most popular songs, full discography and similar artists
- **Album and playlist pages** with cover art

**Listen**
- Full-length songs, found and streamed from YouTube automatically
- **Autoplay:** when your queue runs out, Pulse keeps playing similar songs (turn it off in the Queue)
- The next song is found while the current one plays, so tracks change without a wait
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

The first time you play a song, Pulse downloads two free tools it needs for YouTube playback,
[yt-dlp](https://github.com/yt-dlp/yt-dlp) and [Deno](https://deno.com) (about 60 MB, once). It gets
them from their official GitHub releases and checks them against the published checksums. After
that, songs start in a few seconds, and Pulse updates yt-dlp by itself once a day so playback keeps
working when YouTube changes.

Your likes, playlists, settings and those tools are stored in `%APPDATA%\pulse-player`. To
uninstall, delete `pulse.exe` (and that folder if you want everything gone).

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
| `src/deezer.rs` | Song info, artwork and charts from the Deezer API: search, artists, albums, playlists, similar songs |
| `src/youtube.rs` | YouTube playback: installs and updates yt-dlp and Deno, finds the matching video, gets the audio link |
| `src/audio.rs` | Audio engine: streams MP3/M4A in chunks, seeking, volume, resuming dropped downloads |
| `src/net.rs` | Shared HTTP helpers |
| `src/app.rs` | App logic: pages and history, the queue, shuffle and repeat, likes, playlists, saving |
| `src/ui.rs` | Everything you see: sidebar, top bar, pages, player bar, dialogs |
| `src/widgets.rs` | Custom-drawn pieces: song rows, cards, sliders, buttons, accessibility helpers |
| `src/theme.rs` | Colors, fonts and styling for both themes, plus a contrast test |
| `build.rs` | Bakes the Visual C++ runtime into the Windows `.exe` so it runs on any PC |

Run the tests with `cargo test`.

## Built with

- [egui / eframe](https://github.com/emilk/egui): the user interface
- [wgpu](https://wgpu.rs): graphics through DirectX 12 on Windows
- [rodio](https://github.com/RustAudio/rodio) and [Symphonia](https://github.com/pdeljanov/Symphonia): audio playback and MP3/AAC decoding
- [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [Deno](https://deno.com): finding and streaming songs from YouTube (downloaded on first use)
- [Deezer API](https://developers.deezer.com/api): song info, artwork and charts
- [ureq](https://github.com/algesten/ureq) with [rustls](https://github.com/rustls/rustls): networking
- [Phosphor Icons](https://phosphoricons.com) and the [Inter](https://rsms.me/inter/) typeface (SIL Open Font License)

## About the music

Song info, artwork and charts come from the free [Deezer API](https://developers.deezer.com/api).
The audio is streamed from YouTube using yt-dlp. Streaming YouTube outside its own apps may go
against YouTube's Terms of Service; use Pulse for personal listening and support the artists you
enjoy. Pulse only plays full songs: if YouTube can't play one, Pulse says so and you can skip to the next.

Pulse is an independent project and isn't affiliated with or endorsed by Deezer, YouTube or Google.
