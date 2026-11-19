# ERPCraft on the web

`apps/erpcraft-web` is the desktop egui client compiled to WebAssembly, the way VectorCraft does its web build. There is no handwritten JS: wasm-bindgen generates the small loader in `www/pkg`, and `www/index.html` is just a canvas.

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

Then open http://localhost:8765. The build goes to `target-web/`, and `www/pkg` is generated (git-ignored).

## What runs in the browser

- The engine runs inside the page on an in-memory store, starting with the Auto Craft demo company. Reloading the page starts over.
- The UI, commands, posting and reports are the same code as the desktop app.
- The engine is built without its `postgres` and `bc` features. Those are on by default only for the native apps (`apps/erpcraft`, `apps/erpcraft-cli`, `crates/mcp`). So the web build has no PostgreSQL, no Business Central connector and no file access.
- **Printing:** `document.print` needs a file path, so it is not available on the web yet. Without a path the command returns the PDF as `pdf_base64`; a browser download is a later step.
- **Fonts:** egui's built-in fonts are used, because Segoe UI is read from Windows at run time and never shipped.

## Next steps

- Keep the company in the browser (IndexedDB), or connect to a server that runs the engine on PostgreSQL.
- Download printed PDFs in the browser.
