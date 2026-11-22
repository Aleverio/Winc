//! ABI type yang dipakai bersama host Winc dan plugin WebAssembly.
//!
//! Plugin tidak pernah diberi pointer ke state aplikasi. Semua interaksi memakai [`HostRequest`]
//! yang diserialisasi sebagai JSON, dipanggil lewat satu import `winc.host_call`, lalu divalidasi
//! host terhadap capability yang disetujui user.
//!
//! Modul guest juga wajib mengekspor `winc_activate`, `winc_invoke`, `winc_event`, dan
//! `winc_render`. Lihat README crate ini untuk protokol panjang buffer.

// Guest-helper yang memanggil `winc.host_call` memakai FFI WebAssembly; ini satu-satunya unsafe
// di crate ini dan dibatasi pada modul `guest` di bawah.
#![allow(unsafe_code)]
// Struct ABI adalah wire format JSON; nama field dan `docs/wasm-plugins.md` sudah menjadi
// dokumentasi protokolnya, jadi beri komentar pada tipe yang tidak jelas saja.
#![allow(missing_docs)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Versi ABI yang didukung crate ini.
pub const ABI_VERSION: u32 = 1;

/// Modul import yang dipakai guest untuk memanggil host.
pub const HOST_MODULE: &str = "winc";

/// Nama fungsi host yang dipanggil guest.
pub const HOST_CALL: &str = "host_call";

/// Export guest: mengalokasikan buffer untuk request dari host.
pub const ALLOC: &str = "winc_alloc";

/// Export guest: aktivasi awal.
pub const ACTIVATE: &str = "winc_activate";

/// Export guest: menjalankan sebuah command.
pub const INVOKE: &str = "winc_invoke";

/// Export guest: menerima event lifecycle.
pub const EVENT: &str = "winc_event";

/// Export guest: menghasilkan draw command untuk sebuah surface.
pub const RENDER: &str = "winc_render";

/// Export guest: pemberitahuan sebelum instance dibuang.
pub const SHUTDOWN: &str = "winc_shutdown";

/// Ukuran maksimum satu pesan JSON, dua arah.
pub const MESSAGE_LIMIT: usize = 16 * 1024 * 1024;

/// Izin yang bisa diminta plugin. Host menolak request di luar grant ini.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Membaca daftar dokumen, path, bahasa, dan teks.
    EditorRead,
    /// Mengubah teks, seleksi, dan membuka dokumen.
    EditorWrite,
    /// Menyimpan dokumen ke disk.
    EditorSave,
    /// Membaca konfigurasi Winc.
    ConfigRead,
    /// Menulis konfigurasi Winc.
    ConfigWrite,
    /// Mendaftar dan menjalankan command, serta menyegarkan palette.
    Commands,
    /// Menampilkan notifikasi dan alert.
    Notifications,
    /// Menerima event lifecycle.
    Events,
    /// Menambah atau mengubah item status bar.
    UiStatus,
    /// Menggambar overlay di atas UI.
    UiOverlay,
    /// Mengubah theme token yang diizinkan.
    UiTheme,
    /// Mendaftar server LSP dan mempublikasikan diagnostics.
    Lsp,
    /// Membaca file dalam workspace atau direktori plugin.
    FsRead,
    /// Menulis file dalam workspace atau direktori plugin.
    FsWrite,
    /// Melakukan HTTP request lewat proxy Winc.
    Network,
    /// Menjalankan external command yang sudah dideklarasikan di manifest.
    Process,
}

impl Capability {
    /// Semua capability yang dikenal, dipakai saat membaca grant dari config.
    pub const ALL: &'static [Self] = &[
        Self::EditorRead,
        Self::EditorWrite,
        Self::EditorSave,
        Self::ConfigRead,
        Self::ConfigWrite,
        Self::Commands,
        Self::Notifications,
        Self::Events,
        Self::UiStatus,
        Self::UiOverlay,
        Self::UiTheme,
        Self::Lsp,
        Self::FsRead,
        Self::FsWrite,
        Self::Network,
        Self::Process,
    ];

    /// Nama `snake_case` yang dipakai di manifest dan config.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EditorRead => "editor_read",
            Self::EditorWrite => "editor_write",
            Self::EditorSave => "editor_save",
            Self::ConfigRead => "config_read",
            Self::ConfigWrite => "config_write",
            Self::Commands => "commands",
            Self::Notifications => "notifications",
            Self::Events => "events",
            Self::UiStatus => "ui_status",
            Self::UiOverlay => "ui_overlay",
            Self::UiTheme => "ui_theme",
            Self::Lsp => "lsp",
            Self::FsRead => "fs_read",
            Self::FsWrite => "fs_write",
            Self::Network => "network",
            Self::Process => "process",
        }
    }

    /// Parse nama capability; nama yang tidak dikenal ditolak.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == value)
    }
}

/// Informasi plugin yang diberikan host saat aktivasi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub dir: String,
    /// Capability yang diminta manifest, sebelum grant user diterapkan.
    pub requested: Vec<Capability>,
    /// Capability yang benar-benar disetujui user.
    pub granted: Vec<Capability>,
}

/// Konteks singkat tentang keadaan Winc saat sebuah fungsi guest dipanggil.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub workspace: Option<String>,
    pub active_document: Option<DocumentInfo>,
    pub locale: String,
}

/// Ringkasan sebuah dokumen yang sedang dibuka.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInfo {
    /// Id internal yang dipakai host untuk request berikutnya.
    pub id: u64,
    pub path: Option<String>,
    pub language: String,
    /// Panjang teks dalam offset karakter (satuan yang dipakai `ropey` di Winc).
    pub len: u32,
    pub dirty: bool,
}

/// Rentang offset karakter di dalam dokumen, sesuai satuan `ropey` yang dipakai editor Winc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

/// Permintaan aktivasi, dikirim ke `winc_activate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivateRequest {
    pub abi: u32,
    pub plugin: PluginInfo,
    pub context: Context,
}

/// Command yang ditambahkan plugin ke palette dan halaman setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandContribution {
    /// Id unik di dalam plugin.
    pub id: String,
    /// Judul yang ditampilkan.
    pub title: String,
    pub description: Option<String>,
}

/// Jenis setting yang bisa ditampilkan host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SettingKind {
    String {
        default: String,
    },
    Bool {
        default: bool,
    },
    Number {
        default: f64,
        min: f64,
        max: f64,
    },
    Choice {
        default: String,
        options: Vec<String>,
    },
}

/// Setting yang ditambahkan plugin ke halaman setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingContribution {
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    pub kind: SettingKind,
}

/// Item status bar yang ditambahkan plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusContribution {
    pub id: String,
    pub align: StatusAlign,
    pub tooltip: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusAlign {
    Left,
    Right,
}

/// Panel yang ditambahkan plugin ke samping editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelContribution {
    pub id: String,
    pub title: String,
    pub side: PanelSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelSide {
    Left,
    Right,
    Bottom,
}

/// Event lifecycle yang bisa dilanggan plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    DocumentOpened { id: u64 },
    DocumentClosed { id: u64 },
    DocumentSaved { id: u64 },
    DocumentChanged { id: u64 },
    SelectionChanged { id: u64 },
    WorkspaceChanged { path: Option<String> },
    ThemeChanged { dark: bool },
    ConfigChanged { revision: u64 },
    Shutdown,
}

/// Hasil aktivasi yang dikembalikan `winc_activate`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ActivateResponse {
    pub commands: Vec<CommandContribution>,
    pub settings: Vec<SettingContribution>,
    pub status_items: Vec<StatusContribution>,
    pub panels: Vec<PanelContribution>,
    /// Event yang ingin diterima plugin lewat `winc_event`.
    pub subscriptions: Vec<EventKind>,
}

/// Permintaan menjalankan command, dikirim ke `winc_invoke`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvokeRequest {
    pub command: String,
    pub context: Context,
}

/// Hasil `winc_invoke`; plugin boleh mengembalikan teks untuk notifikasi.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvokeResponse {
    pub message: Option<String>,
}

/// Permintaan event, dikirim ke `winc_event`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRequest {
    pub event: EventKind,
    pub context: Context,
}

/// Surface yang sedang digambar plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderSurface {
    Overlay,
    StatusBar,
    Panel,
}

/// Permintaan draw, dikirim ke `winc_render`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderRequest {
    pub surface: RenderSurface,
    pub target: Option<String>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub context: Context,
}

/// Perintah menggambar yang aman: host yang menerjemahkan ke renderer software.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DrawCommand {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: u32,
        radius: f32,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: u32,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: u32,
        width: f32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderResponse {
    pub commands: Vec<DrawCommand>,
}

/// Level log yang diteruskan ke log Winc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Info,
    Warning,
    Error,
}

/// Jenis notifikasi yang didukung host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    Info,
    Done,
    Warning,
    Error,
}

/// Permintaan HTTP sederhana; dijalankan lewat proxy Winc bila capability `network` diberikan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Permintaan menjalankan external command dari manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRequest {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessResponse {
    pub ok: bool,
    pub output: String,
}

/// Diagnostics yang dipublikasikan plugin ke panel Problems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: TextRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

/// Satu permintaan dari guest ke host.
///
/// Enum ini sengaja menjadi satu titik ekspansi: menambah fitur baru menambah varian di sini dan
/// cabang di host, tanpa mengubah export modul.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    Log {
        level: LogLevel,
        message: String,
    },
    Notify {
        kind: NoticeKind,
        message: String,
    },
    Alert {
        title: String,
        message: String,
        confirm_label: String,
    },
    EditorDocuments,
    EditorActive,
    EditorText {
        id: u64,
    },
    EditorSlice {
        id: u64,
        range: TextRange,
    },
    EditorReplace {
        id: u64,
        range: TextRange,
        text: String,
    },
    EditorSetSelection {
        id: u64,
        range: TextRange,
    },
    EditorSave {
        id: u64,
    },
    EditorOpen {
        path: String,
    },
    ConfigGet {
        key: String,
    },
    ConfigSet {
        key: String,
        value: String,
    },
    SettingsGet {
        key: String,
    },
    SettingsSet {
        key: String,
        value: String,
    },
    CommandRegister {
        id: String,
        title: String,
    },
    CommandExecute {
        id: String,
    },
    PaletteRefresh,
    EventSubscribe {
        kinds: Vec<EventKind>,
    },
    LspServers,
    LspRegister {
        language: String,
        program: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    LspRequest {
        language: String,
        method: String,
        params: serde_json::Value,
    },
    DiagnosticsPublish {
        path: String,
        diagnostics: Vec<Diagnostic>,
    },
    StatusSet {
        id: String,
        text: String,
        align: StatusAlign,
        tooltip: Option<String>,
    },
    StatusRemove {
        id: String,
    },
    ThemeTokens {
        tokens: BTreeMap<String, String>,
    },
    ThemeReset,
    OverlaySet {
        id: String,
        commands: Vec<DrawCommand>,
    },
    UiRefresh,
    FsRead {
        path: String,
    },
    FsWrite {
        path: String,
        text: String,
    },
    HttpRequest {
        request: HttpRequest,
    },
    ProcessRun {
        request: ProcessRequest,
    },
}

/// Balasan host. `Unsupported` dipakai untuk endpoint yang sudah didefinisikan ABI-nya tetapi belum
/// diaktifkan di fase saat ini.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HostReply {
    Ok { value: serde_json::Value },
    Err { error: String },
    Unsupported { feature: String },
}

impl HostReply {
    pub fn ok<T: Serialize>(value: T) -> Self {
        Self::Ok {
            value: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self::Err {
            error: error.into(),
        }
    }

    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
        }
    }

    /// Ubah balasan menjadi `Result`; `Unsupported` dibaca sebagai error juga.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila host menolak request atau nilai balasan tidak bisa di-decode.
    pub fn into_result<T: serde::de::DeserializeOwned>(self) -> Result<T, String> {
        match self {
            Self::Ok { value } => serde_json::from_value(value).map_err(|e| e.to_string()),
            Self::Err { error } => Err(error),
            Self::Unsupported { feature } => Err(format!("unsupported plugin feature: {feature}")),
        }
    }
}

/// Helper guest: serialisasi request, panggil `winc.host_call`, lalu decode balasannya.
pub mod guest {
    use super::*;

    unsafe extern "C" {
        fn host_call(req_ptr: *const u8, req_len: i32, out_ptr: *mut u8, out_cap: i32) -> i32;
    }

    /// Panggil host dengan satu `HostRequest`.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila JSON tidak bisa disusun, host menolak, atau buffer melebihi
    /// [`MESSAGE_LIMIT`].
    pub fn call<T: serde::de::DeserializeOwned>(request: &HostRequest) -> Result<T, String> {
        let request = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        if request.len() > MESSAGE_LIMIT {
            return Err("host request is too large".to_owned());
        }
        let mut output = Vec::new();
        loop {
            let written = unsafe {
                // SAFETY: `request` dan `output` hidup selama panggilan, dan host melakukan
                // bounds check sebelum membaca atau menulis rentang ini.
                host_call(
                    request.as_ptr(),
                    request.len() as i32,
                    output.as_mut_ptr(),
                    output.len() as i32,
                )
            };
            if written >= 0 {
                output.truncate(written as usize);
                let reply: HostReply =
                    serde_json::from_slice(&output).map_err(|e| e.to_string())?;
                return reply.into_result();
            }
            let needed = (-written) as usize;
            if needed > MESSAGE_LIMIT {
                return Err("host reply is too large".to_owned());
            }
            output.resize(needed, 0);
        }
    }

    /// Kirim log ke host; error diabaikan supaya logging tidak mengganggu plugin.
    pub fn log(level: LogLevel, message: impl Into<String>) {
        let _ = call::<()>(&HostRequest::Log {
            level,
            message: message.into(),
        });
    }
}
