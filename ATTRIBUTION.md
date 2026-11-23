# Attribution

All code, metadata schemas, demo company records, and generated PDF layouts in this repository are original clean-room work by ERPCraft contributors, dual-licensed under `MIT OR Apache-2.0`.

## Shared libraries and patched dependencies

| Component | Author / Source | Version / Commit | Licence | Notes |
| --- | --- | --- | --- | --- |
| `craft-atomic`, `craft-dialogs`, `craft-platform`, `craft-settings`, `craft-window`, `craft-web-api`, `craft-web-host` | ArtCraft Team & contributors ([storytold/craft-libs](https://github.com/storytold/craft-libs)) | `eeb5b26198865182463ceba31c8b64394793d22b` | `MIT OR Apache-2.0` | Shared Craft suite libraries |
| `egui`, `eframe`, `egui_extras` | Emil Ernerfeldt & Storytold fork ([storytold/egui](https://github.com/storytold/egui)) | `6d518fc799681571963842f95b55a6b68431b15a` | `MIT OR Apache-2.0` | Patched egui used across Craft apps |
| `winit` | rust-windowing & Storytold fork ([storytold/winit](https://github.com/storytold/winit)) | `a517b3846ee8457d84ee4975110bfda6797c44dc` | `Apache-2.0` | Patched winit used across Craft apps |

## Fonts and assets

No third-party font files or proprietary UI assets are committed or bundled in this repository. Runtime typography uses `egui`'s default open fonts and optional OS system fonts loaded at runtime (never embedded or redistributed).
