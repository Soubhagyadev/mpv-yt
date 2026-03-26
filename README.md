# mpv-yt

<img width="1458" height="787" alt="Screenshot 2026-03-27 000853" src="https://github.com/user-attachments/assets/904ca6b7-4bef-4c0d-8bf2-edda57b16ebe" />

<img width="1452" height="782" alt="Screenshot 2026-03-27 001000" src="https://github.com/user-attachments/assets/d11de8f0-f8ee-406b-86c6-964eccb388a0" />

<img width="1470" height="831" alt="Screenshot 2026-03-27 001108" src="https://github.com/user-attachments/assets/89053825-817e-4a3f-8245-39cff6a819b1" />

A terminal-based YouTube browser and player built with Rust. Search for videos, browse results with metadata, and play them directly with mpv.

## Prerequisites

- [yt-dlp](https://github.com/yt-dlp/yt-dlp) installed and on PATH
- [mpv](https://mpv.io/) installed and on PATH

## Installation

```sh
git clone https://github.com/yourusername/mpv-yt.git
cd mpv-yt
cargo build --release
```

The binary will be at `target/release/mpv-yt.exe`.

## Usage

```sh
cargo run
```

The app opens in search mode. Type a query and press Enter to search YouTube.

## Keybindings

| Key          | Action                   |
|--------------|--------------------------|
| /            | Start new search         |
| j / Down     | Move selection down      |
| k / Up       | Move selection up        |
| Enter        | Play selected video      |
| n            | Next page of results     |
| p            | Previous page of results |
| q / Esc      | Quit                     |
| :?           | for filters              |

## How It Works

- Uses `yt-dlp` to search YouTube and fetch video metadata (title, channel, duration, views, upload date)
- Displays results in an interactive TUI built with `ratatui` and `crossterm`
- Plays selected videos by launching `mpv` with the video URL
- Supports pagination with 10 results per page

## Dependencies

- [ratatui](https://github.com/ratatui/ratatui) - Terminal UI framework
- [crossterm](https://github.com/crossterm-rs/crossterm) - Cross-platform terminal manipulation
- [serde](https://serde.rs/) / [serde_json](https://github.com/serde-rs/json) - JSON parsing
- [anyhow](https://github.com/dtolnay/anyhow) - Error handling
