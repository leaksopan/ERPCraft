# ERPCraft on the web

`apps/erpcraft-web` compiles the desktop egui client to WebAssembly with no handwritten JavaScript: `wasm-bindgen` generates the loader in `www/pkg`, and `www/index.html` hosts the canvas.

## Craft web contracts

- **`craft-web-api`**: exposes `window.erpcraft` (`request`, `execute`, `commands`) over `Session::run`.
- **`craft-web-host`**: speaks the Craft host protocol (`?host=parent` or `data-host="page"`) for `ready`, `open`, `save`, `dirty`, and `failed` messages.
- **`craft-platform::browser`**: downloads generated PDFs (`document.print`) directly in the browser with automatic object-URL cleanup.

## Build and run

One-time setup:

```powershell
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130   # must match wasm-bindgen in Cargo.lock
```

Build and serve:

```powershell
./scripts/build-web.ps1
python -m http.server 8765 --directory apps/erpcraft-web/www
```

Then open http://localhost:8765.

## What runs in the browser

- The engine runs inside the page on an in-memory store, starting with the demo company.
- The UI, commands, posting, reports, and PDF generation use the same Rust code as the desktop app.
- The engine is built without its `postgres` and `bc` features (native sockets are not available in WASM).
