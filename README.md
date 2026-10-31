# notes-rs

An Apple Notes-style desktop app written in Rust with [egui](https://github.com/emilk/egui).

The structure and wording come from reverse engineering the real Notes.app with
[REA](https://github.com/morluto/rea) and Ghidra: its UI nibs, localized strings
and binary symbols.

## Features

- Folders, All Notes, Recently Deleted (notes purged after 30 days, Recover)
- Pin / Unpin, Duplicate, Move to, `#tags`, search
- Sort per folder (Date Edited / Date Created / Title)
- List and Gallery views
- Heading markers (`# `, `## `) and a Checklist button
- One-time "moved to Recently Deleted" notice
- Right-click a custom folder to Delete Folder

Not implemented: checklist toggling, locked notes, sharing, smart folders, attachments.

## Run

```
cargo run --release
cargo test
```

Notes are stored in `~/Library/Application Support/notes-app/notes.json` on macOS.
