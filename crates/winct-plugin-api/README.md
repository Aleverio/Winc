# WASM plugin ABI

Winct plugin adalah modul WebAssembly yang dijalankan oleh `wasmtime` di dalam proses aplikasi.
Plugin **tidak** menerima pointer `App` dan tidak menulis framebuffer langsung. Semua interaksi lewat
host call yang divalidasi terhadap capability yang disetujui user.

## Prinsip

1. **Sandbox dulu, izin kemudian.** Modul tidak bisa mengakses FS, network, editor, atau UI sampai
   manifest meminta capability dan user menyetujuinya.
2. **Satu pintu host call.** Guest hanya mengimpor `winc.host_call(ptr, len, out_ptr, out_cap)`.
   Body request adalah JSON `HostRequest`; balasan adalah JSON `HostReply`. Ini membuat ABI mudah
   diveresikan tanpa menambah import baru tiap fitur.
3. **Kontribusi deklaratif.** Saat aktivasi plugin mengembalikan command, setting, status item,
   panel, dan event yang diminati. Host yang membuat row UI, palette, dan hit region.
4. **Activasi bertahap.** Enum `HostRequest` sudah mencakup editor, command, setting, LSP, UI,
   FS, network, dan process. Implementasi host bisa mengembalikan `Unsupported` untuk endpoint yang
   belum aktif tanpa merusak plugin.

## Export yang diharapkan host

| Export | Tujuan |
| --- | --- |
| `winct_alloc(size) -> ptr` | mengalokasikan buffer guest untuk request host |
| `winct_activate(req, len, out, cap) -> i32` | menerima `ActivateRequest`, menulis `ActivateResponse` |
| `winct_invoke(req, len, out, cap) -> i32` | menjalankan command yang dipilih user |
| `winct_event(req, len, out, cap) -> i32` | menerima event lifecycle (open/save/change/cursor/dll) |
| `winct_render(req, len, out, cap) -> i32` | mengembalikan `Vec<DrawCommand>` untuk surface tertentu |
| `winct_shutdown()` | membersihkan resource plugin |

Nilai balasan bertipe `i32`:

- `> 0` — jumlah byte JSON yang ditulis ke `out_ptr`.
- `0` — respons kosong.
- `-N` — buffer terlalu kecil, butuh `N` byte. Guest mengulang dengan buffer `N`.

Ukuran dibatasi 16 MiB per pesan.

## Capability

`editor_read`, `editor_write`, `editor_save`, `config_read`, `config_write`, `commands`,
`notifications`, `events`, `ui_status`, `ui_overlay`, `ui_theme`, `lsp`, `fs_read`, `fs_write`,
`network`, `process`.

Grant disimpan per plugin di `plugins.grants` dan dipisah dari fingerprint trust. Mengubah command
atau module membuat fingerprint berubah, sehingga user diminta menyetujui ulang.

## Manifest

Plugin lama berupa external command tetap berjalan apa adanya. Untuk WASM, tambahkan `runtime` dan
biarkan command tanpa `program`:

```jsonc
{
  "id": "demo",
  "name": "Demo",
  "runtime": {
    "wasm": "demo.wasm",
    "capabilities": ["notifications", "editor_read", "ui_status"]
  },
  "commands": [{ "id": "hello", "title": "Say Hello" }]
}
```

`runtime.wasm` relatif terhadap direktori manifest. Host membaca file tersebut sekali saat memuat
manifest, menyimpan hash FNV-1a, dan memasukkan hash serta daftar capability ke fingerprint trust.
Grant tersimpan di konfigurasi Winct:

```jsonc
{
  "plugins": {
    "enabled": ["demo"],
    "trusted": ["…"],
    "grants": { "demo": ["notifications", "editor_read", "ui_status"] }
  }
}
```

## Host request (ringkasan)

- Editor: `editor_documents`, `editor_active`, `editor_text`, `editor_slice`, `editor_replace`,
  `editor_set_selection`, `editor_save`, `editor_open`.
- Command: `command_register`, `command_execute`, `palette_refresh`.
- Setting/konfigurasi: `config_get`, `config_set`, `settings_get`, `settings_set`, `notify`, `alert`.
- Event: `event_subscribe`.
- LSP: `lsp_servers`, `lsp_register`, `lsp_request`, `diagnostics_publish`.
- UI: `status_set`, `status_remove`, `theme_tokens`, `theme_reset`, `ui_refresh`, `overlay_set`.
- Sistem: `fs_read`, `fs_write`, `http_request`, `process_run`, `log`.

`winct_render` menerima `x`/`y`/`width`/`height` surface dalam piksel logis relatif terhadap jendela.
`DrawCommand` memakai sistem koordinat yang sama; host memotong gambar ke area surface dan menolak
nilai yang tidak finite.

Detail type ada di crate `winct-plugin-api`.

## Status implementasi

Aktif: runtime Wasmtime, aktivasi, command/palette, settings, notify, editor read/write/save/open,
config read/write (kunci UI yang diizinkan), status bar beserta tooltip, theme token, overlay draw
command, seluruh event lifecycle (`DocumentOpened`/`Closed`/`Saved`/`Changed`, `SelectionChanged`,
`WorkspaceChanged`, `ThemeChanged`, `ConfigChanged`, `Shutdown`), diagnostics publish,
`fs_read`/`fs_write` dalam workspace atau direktori plugin (path relatif diresolusi terhadap
direktori plugin lalu workspace; symlink ke luar root ditolak), dan process runner untuk command
external yang sudah dipercaya. Capability bisa dicabut per plugin dari halaman setting.

Masih mengembalikan `Unsupported`: `http_request` dan request/register/server LSP. Panel kontribusi
saat ini hanya menjadi area render `RenderSurface::Panel`; host belum membuat tab panel baru.

Rencana berikutnya: keymap binding dari plugin, panel UI interaktif, network capability, dan
component-model/WIT untuk SDK yang lebih ergonomis.
