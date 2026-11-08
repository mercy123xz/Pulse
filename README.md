# Pulse

A fast, native music player written in Rust. It streams free, full-length music from
[Audius](https://audius.co), an open music platform for independent artists.

- Home, Trending (by genre and time range), live search for songs, artists and playlists
- Artist and playlist pages
- Liked Songs, Recently Played, your own playlists (saved on your computer)
- A queue with shuffle, repeat and "play next"
- Songs start playing while they download, and you can jump anywhere in them

---

## Running it on Windows (first time)

### 1. Install Rust

1. Go to **https://rustup.rs** and download `rustup-init.exe` (64-bit).
2. Run it. If it says you need the **Visual Studio C++ Build Tools**, choose the option that
   installs them for you (option 1) and wait. It's a big download, but you only do it once.
3. When it asks how to install, press **Enter** for the default.
4. **Close and reopen** any terminal windows afterwards.

To check that it worked, open **PowerShell** and type:

```powershell
cargo --version
```

If you see a version number, you're set.

### 2. Run Pulse

1. Unzip `pulse.zip` somewhere, for example `Documents\pulse`.
2. Open that folder in File Explorer, click the address bar, type `powershell` and press Enter.
   A terminal opens inside the folder.
3. Run:

```powershell
cargo run --release
```

The first build downloads the libraries and compiles everything, which takes about 2–5 minutes.
After that it starts in a few seconds.

### 3. Get a normal .exe (optional)

After step 2, the app is at:

```
target\release\pulse.exe
```

You can copy that one file anywhere (Desktop, another PC) and double-click it. Right-click it →
**Send to → Desktop (create shortcut)** for a desktop icon.

---

## Using it

| Do this | How |
|---|---|
| Play a song | Double-click it, or hover and click ▶ |
| Search | Click the search bar or press **Ctrl + K** and start typing |
| Play / pause | **Space** (or the big button) |
| Next / previous | **Ctrl + →** / **Ctrl + ←** |
| Back / forward between pages | **Alt + ←** / **Alt + →**, or your mouse's side buttons |
| Like a song | Click the ♡ on its row or in the player bar |
| Add to a playlist / queue | Right-click a song |
| Rename or delete a playlist | Right-click it in the sidebar |
| Shuffle / repeat / mute | **S** / **R** / **M** |
| Volume up / down | **Ctrl + ↑** / **Ctrl + ↓** |
| Dark / light mode | Sun/moon button, top right |
| Move between controls | **Tab** / **Shift + Tab**, then **Enter** to activate |
| On a focused song | **Enter** play · **Q** add to queue · **N** play next |
| On a focused slider (seek, volume) | **←** / **→** |
| Clear listening history | "Clear history" on the Recently Played page |

Your likes, playlists, volume and last song are saved automatically.

---

## How the code is organised

| File | What it does |
|---|---|
| `src/main.rs` | Starts the app and opens the window |
| `src/api.rs` | Talks to the Audius API (trending, search, playlists, artists, audio streams) |
| `src/audio.rs` | The audio engine: plays MP3s while they're still downloading, seeking, volume |
| `src/app.rs` | The app's brain: pages, queue, shuffle/repeat, likes, playlists, saving |
| `src/ui.rs` | Everything you see: sidebar, top bar, pages, player bar, pop-ups |
| `src/widgets.rs` | Custom-drawn pieces: track rows, cards, sliders, buttons |
| `src/theme.rs` | Colors, fonts and styling. Edit the `DARK` / `LIGHT` palettes to re-color the app |

Libraries used: [egui/eframe](https://github.com/emilk/egui) (UI),
[rodio](https://github.com/RustAudio/rodio) + symphonia (audio),
[ureq](https://github.com/algesten/ureq) (HTTP),
[Phosphor](https://phosphoricons.com) (icons),
[Inter](https://rsms.me/inter/) (font, SIL Open Font License, see `assets/fonts`).

## Notes

- All music comes from Audius. Artists keep their rights; please credit them if you share
  anything. Some tracks are locked by their artists (pay-to-unlock), and Pulse hides those.
- If Audius ever starts requiring an API key for reading, you can get one free at
  https://api.audius.co/plans. The client is in `src/api.rs`.
- No sound? Check that Windows has a default output device selected (Settings → System → Sound).

## Building from GitHub instead

If you put this project in a GitHub repository, the included workflow
(`.github/workflows/build.yml`) builds `pulse.exe` for you on every push. Open the **Actions**
tab, click the latest run, and download the `pulse-windows` artifact. You don't need Rust installed on your PC for this.
