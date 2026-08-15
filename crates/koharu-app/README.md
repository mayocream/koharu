# koharu-app

`koharu-app` owns Koharu's Tauri-managed application state, command API,
project lifecycle, processing jobs, typed channels, and agent host. It uses
`koharu-desktop` to prepare and publish page frames for the browser canvas.

Rust command signatures are the authoritative frontend contract:

```powershell
cargo run -p koharu-app --bin generate
```
