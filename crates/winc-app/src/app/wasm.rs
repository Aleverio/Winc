//! Runtime plugin WebAssembly: aktivasi, command, dan jembatan host capability.
//!
//! Modul ini menyimpan instance yang sudah aktif dan kontribusi deklaratifnya. `App` membuat
//! snapshot editor/config sebelum memanggil guest; guest tidak pernah meminjam `App` langsung.
//! Semua perubahan masuk [`PluginEffect`] dan diterapkan di sini setelah panggilan selesai.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use winc_config::{Config, PluginManifest, PluginRegistry, PluginSettingKind};
use winc_i18n::t;
use winc_plugin::api::{
    ABI_VERSION, ActivateRequest, ActivateResponse, Capability, Context, DocumentInfo, DrawCommand,
    EventKind, HostReply, HostRequest, InvokeRequest, PluginInfo, ProcessResponse, RenderRequest,
    RenderSurface, SettingKind, StatusAlign,
};
use winc_plugin::{HostBridge, HostEffect, PluginEffect, PluginInstance, PluginRuntime};

use super::*;

fn app_notice(kind: winc_plugin::api::NoticeKind) -> NoticeKind {
    match kind {
        winc_plugin::api::NoticeKind::Info => NoticeKind::Info,
        winc_plugin::api::NoticeKind::Done => NoticeKind::Done,
        winc_plugin::api::NoticeKind::Warning => NoticeKind::Warning,
        winc_plugin::api::NoticeKind::Error => NoticeKind::Error,
    }
}

/// Terjemahkan konfigurasi yang aman dibaca guest menjadi pasangan kunci/nilai.
fn config_values(config: &Config) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let mut insert = |key: &str, value: String| {
        values.insert(key.to_owned(), value);
    };
    if let Some(value) = &config.ui_accent {
        insert("ui.accent", value.clone());
    }
    if let Some(value) = config.ui_panel_opacity {
        insert("ui.panel_opacity", value.to_string());
    }
    if let Some(value) = config.ui_corner_scale {
        insert("ui.corner_scale", value.to_string());
    }
    if let Some(value) = config.ui_animation_speed {
        insert("ui.animation_speed", value.to_string());
    }
    if let Some(value) = config.editor_size {
        insert("editor.font_size", value.to_string());
    }
    values
}

/// `#RRGGBB` 或 `RRGGBB` 是否是一个十六进制颜色。
fn is_hex_color(value: &str) -> bool {
    let digits = value.strip_prefix('#').unwrap_or(value);
    digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Terapkan perubahan konfigurasi yang diizinkan dari plugin.
fn set_config_value(config: &mut Config, key: &str, value: &str) -> bool {
    let parsed = value.parse::<u16>().ok();
    match key {
        "ui.accent" if is_hex_color(value) => config.ui_accent = Some(value.to_owned()),
        "ui.panel_opacity" => match value.parse::<u8>() {
            Ok(v) if Config::PANEL_OPACITY_RANGE.contains(&f32::from(v)) => {
                config.ui_panel_opacity = Some(v)
            }
            _ => return false,
        },
        "ui.corner_scale" => match parsed {
            Some(v) if Config::CORNER_SCALE_RANGE.contains(&f32::from(v)) => {
                config.ui_corner_scale = Some(v)
            }
            _ => return false,
        },
        "ui.animation_speed" => match parsed {
            Some(v) if Config::ANIMATION_SPEED_RANGE.contains(&f32::from(v)) => {
                config.ui_animation_speed = Some(v)
            }
            _ => return false,
        },
        "editor.font_size" => match value.parse::<f32>() {
            Ok(v) if Config::EDITOR_SIZE_RANGE.contains(&v) => config.editor_size = Some(v),
            _ => return false,
        },
        _ => return false,
    }
    true
}

/// Normalisasi path relatif terhadap direktori kerja saat ini; tidak mengikuti symlink.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_within(path: &Path, root: &Path) -> bool {
    normalize_path(path).starts_with(normalize_path(root))
}

/// Pastikan path masih berada di salah satu root setelah symlink diresolusi.
#[allow(
    clippy::disallowed_methods,
    reason = "只访问本机：路径限制在本机 workspace 或插件目录"
)]
fn allowed_path(path: &str, roots: &[PathBuf], create_parent: bool) -> Result<PathBuf, String> {
    let target = normalize_path(Path::new(path));
    if !roots.iter().any(|root| path_within(&target, root)) {
        return Err(format!("path “{path}” is outside the allowed directories"));
    }
    let parent = target
        .parent()
        .ok_or_else(|| format!("path “{path}” has no parent directory"))?;
    if create_parent {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create “{}”: {error}", parent.display()))?;
    }
    let canonical_parent = std::fs::canonicalize(parent)
        .map_err(|error| format!("could not resolve “{}”: {error}", parent.display()))?;
    let canonical_roots = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect::<Vec<_>>();
    if !canonical_roots
        .iter()
        .any(|root| canonical_parent.starts_with(root))
    {
        return Err(format!("path “{path}” escapes through a symbolic link"));
    }
    if target.exists() {
        let canonical_target = std::fs::canonicalize(&target)
            .map_err(|error| format!("could not resolve “{}”: {error}", target.display()))?;
        if !canonical_roots
            .iter()
            .any(|root| canonical_target.starts_with(root))
        {
            return Err(format!("path “{path}” escapes through a symbolic link"));
        }
    }
    Ok(target)
}

/// Command external yang boleh dipanggil plugin lewat capability `process`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessSpec {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
}

/// Command yang ditambahkan plugin WASM ke palette dan halaman setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WasmCommand {
    pub plugin: String,
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}

/// Setting yang ditambahkan plugin WASM ke halaman setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WasmSetting {
    pub plugin: String,
    pub key: String,
    pub title: String,
    pub kind: PluginSettingKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusText {
    text: String,
    align: StatusAlign,
    tooltip: Option<String>,
}

/// 状态栏向视图层公开的一项插件状态，包含悬停提示文字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatusEntryInfo {
    pub id: String,
    pub text: String,
    pub align: StatusAlign,
    pub tooltip: Option<String>,
}

/// Snapshot data yang boleh dibaca guest tanpa meminjam `App`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Snapshot {
    context: Context,
    documents: Vec<DocumentInfo>,
    texts: BTreeMap<u64, String>,
    plugin_values: BTreeMap<String, BTreeMap<String, String>>,
    config_values: BTreeMap<String, String>,
    plugin_dirs: BTreeMap<String, PathBuf>,
    processes: BTreeMap<String, ProcessSpec>,
    shell: winc_editor::shell::Shell,
}

/// Jembatan host untuk satu panggilan guest.
struct Bridge {
    plugin: PluginInfo,
    context: Context,
    snapshot: Snapshot,
    values: BTreeMap<String, String>,
    effects: Vec<PluginEffect>,
}

impl Bridge {
    fn new(plugin: PluginInfo, snapshot: Snapshot) -> Self {
        let context = snapshot.context.clone();
        let values = snapshot
            .plugin_values
            .get(&plugin.id)
            .cloned()
            .unwrap_or_default();
        Self {
            plugin,
            context,
            snapshot,
            values,
            effects: Vec::new(),
        }
    }

    fn effect(&mut self, effect: HostEffect) {
        self.effects.push(PluginEffect {
            plugin: self.plugin.id.clone(),
            effect,
        });
    }

    fn allowed_path(&self, path: &str, create_parent: bool) -> Result<PathBuf, String> {
        let mut roots = Vec::new();
        if let Some(dir) = self.snapshot.plugin_dirs.get(&self.plugin.id) {
            roots.push(dir.clone());
        }
        if let Some(workspace) = &self.context.workspace {
            roots.push(PathBuf::from(workspace));
        }
        if Path::new(path).is_relative() {
            let mut first_error = None;
            for root in &roots {
                let candidate = root.join(path).to_string_lossy().into_owned();
                match allowed_path(&candidate, &roots, create_parent) {
                    Ok(resolved) => return Ok(resolved),
                    Err(error) => first_error.get_or_insert(error),
                };
            }
            return Err(first_error.unwrap_or_else(|| "no allowed directory".to_owned()));
        }
        allowed_path(path, &roots, create_parent)
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "只访问本机：路径已经经过 workspace/插件目录和 symlink 检查"
)]
fn read_wasm_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("could not read “{}”: {error}", path.display()))
}

impl HostBridge for Bridge {
    fn context(&self) -> Context {
        self.context.clone()
    }

    fn handle(&mut self, request: HostRequest) -> HostReply {
        match request {
            HostRequest::Log { .. } => HostReply::ok(()),
            HostRequest::Notify { kind, message } => {
                self.effect(HostEffect::Notify { kind, message });
                HostReply::ok(())
            }
            HostRequest::Alert {
                title,
                message,
                confirm_label,
            } => {
                self.effect(HostEffect::Alert {
                    title,
                    message,
                    confirm_label,
                });
                HostReply::ok(())
            }
            HostRequest::EditorDocuments => HostReply::ok(self.snapshot.documents.clone()),
            HostRequest::EditorActive => HostReply::ok(self.context.active_document.clone()),
            HostRequest::EditorText { id } => HostReply::ok(self.snapshot.texts.get(&id).cloned()),
            HostRequest::EditorSlice { id, range } => {
                let text = self.snapshot.texts.get(&id).map(|text| {
                    text.chars()
                        .skip(range.start as usize)
                        .take((range.end.saturating_sub(range.start)) as usize)
                        .collect::<String>()
                });
                HostReply::ok(text)
            }
            HostRequest::EditorReplace { id, range, text } => {
                self.effect(HostEffect::EditorReplace { id, range, text });
                HostReply::ok(())
            }
            HostRequest::EditorSetSelection { id, range } => {
                self.effect(HostEffect::EditorSetSelection { id, range });
                HostReply::ok(())
            }
            HostRequest::EditorSave { id } => {
                self.effect(HostEffect::EditorSave { id });
                HostReply::ok(())
            }
            HostRequest::EditorOpen { path } => {
                self.effect(HostEffect::EditorOpen { path });
                HostReply::ok(())
            }
            HostRequest::ConfigGet { key } => {
                HostReply::ok(self.snapshot.config_values.get(&key).cloned())
            }
            HostRequest::ConfigSet { key, value } => {
                self.effect(HostEffect::ConfigSet { key, value });
                HostReply::ok(())
            }
            HostRequest::SettingsGet { key } => HostReply::ok(self.values.get(&key).cloned()),
            HostRequest::SettingsSet { key, value } => {
                self.effect(HostEffect::SettingsSet { key, value });
                HostReply::ok(())
            }
            HostRequest::CommandRegister { id, title } => {
                self.effect(HostEffect::CommandRegister { id, title });
                HostReply::ok(())
            }
            HostRequest::CommandExecute { id } => {
                self.effect(HostEffect::CommandExecute { id });
                HostReply::ok(())
            }
            HostRequest::PaletteRefresh => {
                self.effect(HostEffect::PaletteRefresh);
                HostReply::ok(())
            }
            HostRequest::EventSubscribe { kinds } => {
                self.effect(HostEffect::EventSubscribe { kinds });
                HostReply::ok(())
            }
            HostRequest::LspServers
            | HostRequest::LspRegister { .. }
            | HostRequest::LspRequest { .. } => HostReply::unsupported("lsp"),
            HostRequest::DiagnosticsPublish { path, diagnostics } => {
                self.effect(HostEffect::DiagnosticsPublish { path, diagnostics });
                HostReply::ok(())
            }
            HostRequest::StatusSet {
                id,
                text,
                align,
                tooltip,
            } => {
                self.effect(HostEffect::StatusSet {
                    id,
                    text,
                    align,
                    tooltip,
                });
                HostReply::ok(())
            }
            HostRequest::StatusRemove { id } => {
                self.effect(HostEffect::StatusRemove { id });
                HostReply::ok(())
            }
            HostRequest::ThemeTokens { tokens } => {
                self.effect(HostEffect::ThemeTokens { tokens });
                HostReply::ok(())
            }
            HostRequest::ThemeReset => {
                self.effect(HostEffect::ThemeReset);
                HostReply::ok(())
            }
            HostRequest::OverlaySet { id, commands } => {
                self.effect(HostEffect::OverlaySet { id, commands });
                HostReply::ok(())
            }
            HostRequest::UiRefresh => {
                self.effect(HostEffect::UiRefresh);
                HostReply::ok(())
            }
            HostRequest::FsRead { path } => match self.allowed_path(&path, false) {
                Ok(path) => read_wasm_file(&path)
                    .map(HostReply::ok)
                    .unwrap_or_else(HostReply::error),
                Err(error) => HostReply::error(error),
            },
            HostRequest::FsWrite { path, text } => match self.allowed_path(&path, true) {
                Ok(_) => {
                    self.effect(HostEffect::FsWrite { path, text });
                    HostReply::ok(())
                }
                Err(error) => HostReply::error(error),
            },
            HostRequest::HttpRequest { .. } => HostReply::unsupported("network"),
            HostRequest::ProcessRun { request } => {
                let Some(spec) = self.snapshot.processes.get(&request.command) else {
                    return HostReply::error(format!(
                        "command “{}” is not declared and trusted",
                        request.command
                    ));
                };
                let process = winc_editor::plugin::Request {
                    program: spec.program.clone(),
                    args: if request.args.is_empty() {
                        spec.args.clone()
                    } else {
                        request.args
                    },
                    cwd: spec.cwd.clone(),
                    env: spec.env.clone(),
                };
                let outcome = winc_editor::plugin::run(&process, &self.snapshot.shell);
                HostReply::ok(ProcessResponse {
                    ok: outcome.ok,
                    output: outcome.output,
                })
            }
        }
    }

    fn take_effects(&mut self) -> Vec<PluginEffect> {
        std::mem::take(&mut self.effects)
    }
}

/// Kedalaman panggilan host yang masih aktif; mencegah plugin memicu dirinya sendiri tanpa batas.
const MAX_CALL_DEPTH: usize = 4;

/// Kumpulan instance dan kontribusi plugin WASM yang sedang aktif.
#[derive(Default)]
pub(crate) struct Manager {
    busy_depth: usize,
    runtime: Option<PluginRuntime>,
    instances: BTreeMap<String, PluginInstance>,
    commands: Vec<WasmCommand>,
    settings: Vec<WasmSetting>,
    status: BTreeMap<(String, String), StatusText>,
    overlays: BTreeMap<(String, String), Vec<DrawCommand>>,
    theme_tokens: BTreeMap<String, String>,
    subscriptions: BTreeMap<String, Vec<EventKind>>,
    /// 上次观察到的文档状态 `(revision, anchor, head)`；用来在事件循环里发现编辑和光标变化。
    observed: BTreeMap<u64, (u64, u64, u64)>,
    pending: Vec<PluginEffect>,
    refresh_requested: bool,
    errors: Vec<String>,
}

impl Manager {
    /// Mulai satu panggilan plugin; menolak bila terlalu banyak panggilan bersarang.
    pub(in crate::app) fn enter_call(&mut self) -> bool {
        if self.busy_depth >= MAX_CALL_DEPTH {
            return false;
        }
        self.busy_depth += 1;
        true
    }

    /// Akhiri satu panggilan plugin.
    pub(in crate::app) fn leave_call(&mut self) {
        self.busy_depth = self.busy_depth.saturating_sub(1);
    }

    /// Muat ulang semua plugin WASM yang sedang diaktifkan pengguna.
    #[allow(
        clippy::disallowed_methods,
        reason = "只访问本机：WASM 模块来自本机清单路径"
    )]
    pub(in crate::app) fn reload(
        &mut self,
        config: &Config,
        plugins: &PluginRegistry,
        snapshot: Snapshot,
    ) {
        for instance in self.instances.values_mut() {
            let _ = instance.shutdown();
        }
        self.instances.clear();
        self.commands.clear();
        self.settings.clear();
        self.status.clear();
        self.overlays.clear();
        self.theme_tokens.clear();
        self.subscriptions.clear();
        self.observed.clear();
        self.pending.clear();
        self.errors.clear();

        let mut runtime = self.runtime.take();
        for (_, manifest) in plugins.enabled(config) {
            let Some(spec) = &manifest.wasm else {
                continue;
            };
            // Modul tidak dijalankan sampai user menyetujui salah satu command dan grant.
            let trusted = manifest.commands.iter().any(|command| {
                let fingerprint = format!("{:x}", manifest.command_fingerprint(command));
                config.plugin_trusted.iter().any(|t| t == &fingerprint)
            });
            if !trusted {
                continue;
            }
            let info = Self::info(manifest, config);
            let bytes = match std::fs::read(&spec.path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.errors
                        .push(format!("{}: {error}", spec.path.to_string_lossy()));
                    continue;
                }
            };
            let engine = runtime.get_or_insert_with(|| {
                PluginRuntime::new().expect("wasmtime engine should initialize")
            });
            let bridge = Box::new(Bridge::new(info.clone(), snapshot.clone()));
            let mut instance = match engine.load(&bytes, info.clone(), bridge) {
                Ok(instance) => instance,
                Err(error) => {
                    self.errors.push(format!("{}: {error}", manifest.name));
                    continue;
                }
            };
            let request = ActivateRequest {
                abi: ABI_VERSION,
                plugin: info,
                context: snapshot.context.clone(),
            };
            match instance.activate(&request) {
                Ok(response) => self.contribute(&manifest.id, response),
                Err(error) => self.errors.push(format!("{}: {error}", manifest.name)),
            }
            self.absorb(instance.take_effects());
            self.instances.insert(manifest.id.clone(), instance);
        }
        self.runtime = runtime;
    }

    /// Ambil dan kosongkan effect yang perlu diterapkan `App`.
    pub(in crate::app) fn take_effects(&mut self) -> Vec<PluginEffect> {
        std::mem::take(&mut self.pending)
    }

    /// Command WASM yang tersedia untuk palette dan halaman setting.
    pub(in crate::app) fn commands(&self) -> &[WasmCommand] {
        &self.commands
    }

    /// Setting WASM yang perlu ditampilkan di halaman setting plugin.
    pub(in crate::app) fn settings(&self) -> &[WasmSetting] {
        &self.settings
    }

    /// Masalah saat memuat modul atau mengaktifkan plugin.
    pub(in crate::app) fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Apakah plugin meminta penyegaran UI pada panggilan terakhir.
    pub(in crate::app) fn take_refresh(&mut self) -> bool {
        std::mem::take(&mut self.refresh_requested)
    }

    /// Item status bar yang saat ini disumbangkan plugin.
    pub(crate) fn status_entries(&self) -> Vec<StatusEntryInfo> {
        self.status
            .iter()
            .map(|((_, id), item)| StatusEntryInfo {
                id: id.clone(),
                text: item.text.clone(),
                align: item.align,
                tooltip: item.tooltip.clone(),
            })
            .collect()
    }

    /// Semua draw command statis yang tersimpan dari `OverlaySet`.
    pub(crate) fn all_overlay_commands(&self) -> Vec<DrawCommand> {
        self.overlays.values().flatten().cloned().collect()
    }

    /// Minta draw command untuk satu surface; dipanggil sekali per frame.
    pub(in crate::app) fn render(
        &mut self,
        surface: RenderSurface,
        target: Option<&str>,
        area: (f32, f32, f32, f32),
        snapshot: Snapshot,
    ) -> Vec<DrawCommand> {
        let ids: Vec<String> = self.instances.keys().cloned().collect();
        let mut all = Vec::new();
        for id in ids {
            if target.is_some_and(|target| target != id) {
                continue;
            }
            let Some(mut instance) = self.instances.remove(&id) else {
                continue;
            };
            let info = instance.info().clone();
            instance.set_bridge(Box::new(Bridge::new(info, snapshot.clone())));
            let request = RenderRequest {
                surface,
                target: target.map(str::to_owned),
                x: area.0,
                y: area.1,
                width: area.2,
                height: area.3,
                context: snapshot.context.clone(),
            };
            match instance.render(&request) {
                Ok(response) => all.extend(response.commands),
                Err(error) => self.push_error(format!("{id}: {error}")),
            }
            self.absorb(instance.take_effects());
            self.instances.insert(id, instance);
        }
        all
    }

    /// Kirim event lifecycle ke plugin yang berlangganan.
    pub(in crate::app) fn dispatch_event(&mut self, event: EventKind, snapshot: Snapshot) {
        let targets: Vec<String> = self
            .subscriptions
            .iter()
            .filter(|(_, kinds)| kinds.contains(&event))
            .map(|(id, _)| id.clone())
            .collect();
        for id in targets {
            let Some(mut instance) = self.instances.remove(&id) else {
                continue;
            };
            let info = instance.info().clone();
            instance.set_bridge(Box::new(Bridge::new(info, snapshot.clone())));
            let request = winc_plugin::api::EventRequest {
                event: event.clone(),
                context: snapshot.context.clone(),
            };
            if let Err(error) = instance.event(&request) {
                self.push_error(format!("{id}: {error}"));
            }
            self.absorb(instance.take_effects());
            self.instances.insert(id, instance);
        }
    }

    /// Bandingkan status dokumen dengan observasi terakhir dan kembalikan event yang berubah.
    ///
    /// Dokumen yang baru pertama kali terlihat tidak menghasilkan event, sehingga aktivasi atau
    /// membuka taut tidak dilaporkan sebagai perubahan.
    pub(in crate::app) fn observe_documents(
        &mut self,
        states: &[(u64, u64, u64, u64)],
    ) -> Vec<EventKind> {
        let present: BTreeSet<u64> = states.iter().map(|state| state.0).collect();
        self.observed.retain(|id, _| present.contains(id));

        let mut events = Vec::new();
        for &(id, revision, anchor, head) in states {
            let current = (revision, anchor, head);
            match self.observed.get_mut(&id) {
                Some(previous) => {
                    if previous.0 != revision {
                        events.push(EventKind::DocumentChanged { id });
                    }
                    if (previous.1, previous.2) != (anchor, head) {
                        events.push(EventKind::SelectionChanged { id });
                    }
                    *previous = current;
                }
                None => {
                    self.observed.insert(id, current);
                }
            }
        }
        events
    }

    /// Beri tahu semua plugin bahwa aplikasi sedang ditutup.
    pub(in crate::app) fn shutdown(&mut self) {
        let mut errors = Vec::new();
        for instance in self.instances.values_mut() {
            let id = instance.info().id.clone();
            if let Err(error) = instance.shutdown() {
                errors.push(format!("{id}: {error}"));
            }
        }
        for error in errors {
            self.push_error(error);
        }
    }

    fn push_error(&mut self, error: String) {
        if !self.errors.contains(&error) {
            self.errors.push(error);
        }
    }

    /// Jalankan satu command plugin WASM; mengembalikan pesan untuk notifikasi.
    pub(in crate::app) fn invoke(
        &mut self,
        plugin: &str,
        command: &str,
        snapshot: Snapshot,
    ) -> Result<Option<String>, String> {
        let Some(mut instance) = self.instances.remove(plugin) else {
            return Err(format!("plugin “{plugin}” is not active"));
        };
        let info = instance.info().clone();
        instance.set_bridge(Box::new(Bridge::new(info, snapshot.clone())));
        let result = instance.invoke(&InvokeRequest {
            command: command.to_owned(),
            context: snapshot.context,
        });
        self.absorb(instance.take_effects());
        self.instances.insert(plugin.to_owned(), instance);
        result
            .map(|response| response.message)
            .map_err(|e| e.to_string())
    }

    fn info(manifest: &PluginManifest, config: &Config) -> PluginInfo {
        let requested = manifest
            .wasm
            .as_ref()
            .map(|wasm| {
                wasm.capabilities
                    .iter()
                    .filter_map(|name| Capability::parse(name))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let allowed = config
            .plugin_grants
            .get(&manifest.id)
            .map(|grants| {
                grants
                    .iter()
                    .filter_map(|name| Capability::parse(name))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        PluginInfo {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest
                .description
                .clone()
                .unwrap_or_else(|| "1".to_owned()),
            dir: manifest.dir.to_string_lossy().into_owned(),
            requested: requested.clone(),
            granted: requested
                .into_iter()
                .filter(|cap| allowed.contains(cap))
                .collect(),
        }
    }

    fn contribute(&mut self, plugin: &str, response: ActivateResponse) {
        for command in response.commands {
            self.commands.push(WasmCommand {
                plugin: plugin.to_owned(),
                id: command.id,
                title: command.title,
                description: command.description,
            });
        }
        for setting in response.settings {
            let kind = match setting.kind {
                SettingKind::Bool { .. } => PluginSettingKind::Bool,
                SettingKind::String { .. }
                | SettingKind::Number { .. }
                | SettingKind::Choice { .. } => PluginSettingKind::String,
            };
            self.settings.push(WasmSetting {
                plugin: plugin.to_owned(),
                key: setting.key,
                title: setting.title,
                kind,
            });
        }
        for item in response.status_items {
            self.status.insert(
                (plugin.to_owned(), item.id),
                StatusText {
                    text: String::new(),
                    align: item.align,
                    tooltip: item.tooltip,
                },
            );
        }
        for panel in response.panels {
            self.overlays
                .entry((plugin.to_owned(), panel.id))
                .or_default();
            let _ = panel.side;
        }
        self.subscriptions
            .insert(plugin.to_owned(), response.subscriptions);
    }

    fn absorb(&mut self, effects: Vec<PluginEffect>) {
        for effect in effects {
            match effect.effect {
                HostEffect::CommandRegister { id, title } => {
                    self.commands.push(WasmCommand {
                        plugin: effect.plugin,
                        id,
                        title,
                        description: None,
                    });
                }
                HostEffect::EventSubscribe { kinds } => {
                    self.subscriptions
                        .entry(effect.plugin)
                        .or_default()
                        .extend(kinds);
                }
                HostEffect::StatusSet {
                    id,
                    text,
                    align,
                    tooltip,
                } => {
                    self.status.insert(
                        (effect.plugin, id),
                        StatusText {
                            text,
                            align,
                            tooltip,
                        },
                    );
                }
                HostEffect::StatusRemove { id } => {
                    self.status.remove(&(effect.plugin, id));
                }
                HostEffect::ThemeTokens { tokens } => self.theme_tokens.extend(tokens),
                HostEffect::ThemeReset => self.theme_tokens.clear(),
                HostEffect::OverlaySet { id, commands } => {
                    self.overlays.insert((effect.plugin, id), commands);
                }
                HostEffect::UiRefresh => self.refresh_requested = true,
                _ => self.pending.push(effect),
            }
        }
    }
}

impl App {
    /// Buat snapshot yang aman dibaca plugin saat ini.
    pub(in crate::app) fn wasm_snapshot(&self) -> Snapshot {
        let mut snapshot = Snapshot {
            context: Context {
                workspace: self
                    .editor
                    .root()
                    .map(|path| path.to_string_lossy().into_owned()),
                active_document: self.editor.active().map(|doc| DocumentInfo {
                    id: doc.id.0,
                    path: doc
                        .path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    language: doc.highlighter.language().id.to_owned(),
                    len: doc
                        .text()
                        .rope()
                        .map(|rope| rope.len_chars() as u32)
                        .unwrap_or(0),
                    dirty: doc.is_dirty(),
                }),
                locale: winc_i18n::locale().tag().to_owned(),
            },
            ..Default::default()
        };
        for doc in &self.editor.documents {
            let id = doc.id.0;
            let text = doc.text_range(0, usize::MAX);
            if let Some(text) = &text {
                snapshot.texts.insert(id, text.clone());
            }
            snapshot.documents.push(DocumentInfo {
                id,
                path: doc
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                language: doc.highlighter.language().id.to_owned(),
                len: text.map_or(0, |text| text.chars().count() as u32),
                dirty: doc.is_dirty(),
            });
        }
        snapshot.plugin_values = self.config.plugin_values.clone();
        snapshot.config_values = config_values(&self.config);
        snapshot.plugin_dirs = self
            .plugins
            .plugins
            .iter()
            .map(|manifest| (manifest.id.clone(), manifest.dir.clone()))
            .collect();
        snapshot.shell = self.format.formatter.shell().clone();
        for (_, manifest) in self.plugins.enabled(&self.config) {
            for command in &manifest.commands {
                if command.wasm {
                    continue;
                }
                let fingerprint = format!("{:x}", manifest.command_fingerprint(command));
                if !self.config.plugin_trusted.iter().any(|t| t == &fingerprint) {
                    continue;
                }
                let cwd = command
                    .cwd
                    .as_ref()
                    .map(|cwd| {
                        let path = PathBuf::from(cwd);
                        if path.is_absolute() {
                            path
                        } else {
                            manifest.dir.join(path)
                        }
                    })
                    .unwrap_or_else(|| manifest.dir.clone());
                snapshot.processes.insert(
                    winc_config::PluginRegistry::command_id(manifest, command),
                    ProcessSpec {
                        program: command.program.clone(),
                        args: command.args.clone(),
                        cwd,
                        env: command.env.clone(),
                    },
                );
            }
        }
        snapshot
    }

    /// Muat ulang plugin WASM setelah plugin/config berubah.
    pub(in crate::app) fn reload_wasm_plugins(&mut self) {
        if !self.wasm.enter_call() {
            return;
        }
        self.reload_wasm_plugins_inner();
        self.wasm.leave_call();
    }

    fn reload_wasm_plugins_inner(&mut self) {
        let snapshot = self.wasm_snapshot();
        self.wasm.reload(&self.config, &self.plugins, snapshot);
        self.apply_wasm_effects();
    }

    /// Terapkan effect yang dikumpulkan plugin ke state aplikasi.
    pub(in crate::app) fn apply_wasm_effects(&mut self) {
        let effects = self.wasm.take_effects();
        for PluginEffect { plugin, effect } in effects {
            match effect {
                HostEffect::Notify { kind, message } => self.notify(app_notice(kind), message),
                HostEffect::Alert {
                    title,
                    message,
                    confirm_label,
                } => {
                    self.show_alert(
                        &title,
                        &message,
                        vec![
                            AlertButton::new(&confirm_label),
                            AlertButton::cancel(t!("common-cancel")),
                        ],
                        None,
                        |_, _| None,
                    );
                }
                HostEffect::EditorReplace { id, range, text } => {
                    if self.wasm_activate(id) {
                        self.select(range.start as usize, range.end as usize);
                        self.run(Command::Paste(text));
                    }
                }
                HostEffect::EditorSetSelection { id, range } => {
                    if self.wasm_activate(id) {
                        self.select(range.start as usize, range.end as usize);
                    }
                }
                HostEffect::EditorSave { id } => self.save_by_id(DocumentId(id)),
                HostEffect::EditorOpen { path } => self.open_path(&PathBuf::from(path)),
                HostEffect::ConfigSet { key, value } => {
                    if set_config_value(&mut self.config, &key, &value) {
                        self.wasm_config_changed(&key);
                    }
                }
                HostEffect::SettingsSet { key, value } => {
                    self.config
                        .plugin_values
                        .entry(plugin)
                        .or_default()
                        .insert(key, value);
                    self.save_config();
                }
                HostEffect::CommandExecute { id } => self.run_plugin_command_id(&id),
                HostEffect::PaletteRefresh => self.palette_refresh(),
                HostEffect::ThemeTokens { tokens } => {
                    if let Some(accent) =
                        tokens.get("accent").or_else(|| tokens.get("accent_color"))
                        && set_config_value(&mut self.config, "ui.accent", accent)
                    {
                        self.wasm_config_changed("ui.accent");
                    }
                }
                HostEffect::ThemeReset => {
                    self.config.ui_accent = None;
                    self.wasm_config_changed("ui.accent");
                }
                HostEffect::DiagnosticsPublish { path, diagnostics } => {
                    self.publish_wasm_diagnostics(path, diagnostics);
                }
                HostEffect::FsWrite { path, text }
                    if !self.write_wasm_file(&plugin, &path, &text) =>
                {
                    self.notify(super::NoticeKind::Error, t!("plugin-unsupported-effect"));
                }
                _ => {}
            }
        }
        if self.wasm.take_refresh() {
            self.request_redraw();
        }
    }

    /// Publikasikan diagnostics plugin ke panel Problems tanpa bergantung pada status LSP.
    fn publish_wasm_diagnostics(
        &mut self,
        path: String,
        diagnostics: Vec<winc_plugin::api::Diagnostic>,
    ) {
        use winc_plugin::api::DiagnosticSeverity as ApiSeverity;

        let mut published = Vec::new();
        for diagnostic in diagnostics {
            let severity = match diagnostic.severity {
                ApiSeverity::Error => lsp_types::DiagnosticSeverity::ERROR,
                ApiSeverity::Warning => lsp_types::DiagnosticSeverity::WARNING,
                ApiSeverity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
                ApiSeverity::Hint => lsp_types::DiagnosticSeverity::HINT,
            };
            published.push(lsp_types::Diagnostic {
                range: lsp_types::Range {
                    start: self.wasm_position(&path, diagnostic.range.start),
                    end: self.wasm_position(&path, diagnostic.range.end),
                },
                severity: Some(severity),
                message: diagnostic.message,
                source: diagnostic.source,
                ..Default::default()
            });
        }
        self.lsp_diagnostics.insert(PathBuf::from(path), published);
        self.request_redraw();
    }

    /// Konversi offset karakter plugin menjadi posisi baris/kolom LSP.
    fn wasm_position(&self, path: &str, offset: u32) -> lsp_types::Position {
        let doc = self.editor.documents.iter().find(|doc| {
            doc.path.as_ref().is_some_and(|doc_path| {
                doc_path.to_string_lossy() == path || doc_path.ends_with(path)
            })
        });
        let Some(doc) = doc else {
            return lsp_types::Position::new(0, offset);
        };
        let Some(rope) = doc.text().rope() else {
            return lsp_types::Position::new(0, offset);
        };
        let offset = (offset as usize).min(rope.len_chars());
        let line = rope.char_to_line(offset);
        lsp_types::Position::new(line as u32, (offset - rope.line_to_char(line)) as u32)
    }

    /// Tulis file hanya saat path masih berada di workspace atau direktori plugin.
    #[allow(
        clippy::disallowed_methods,
        reason = "只访问本机：路径已经限制在本机 workspace 或插件目录"
    )]
    fn write_wasm_file(&self, plugin: &str, path: &str, text: &str) -> bool {
        let roots = [
            self.editor.root().map(Path::to_path_buf),
            self.plugins
                .plugins
                .iter()
                .find(|manifest| manifest.id == plugin)
                .map(|manifest| manifest.dir.clone()),
        ];
        let Ok(target) = allowed_path(path, &roots.into_iter().flatten().collect::<Vec<_>>(), true)
        else {
            return false;
        };
        std::fs::write(target, text).is_ok()
    }
    /// Jalankan command plugin WASM melalui instance yang sudah aktif.
    pub(in crate::app) fn run_wasm_command(&mut self, plugin: &str, command: &str) {
        if !self.wasm.enter_call() {
            return;
        }
        self.run_wasm_command_inner(plugin, command);
        self.wasm.leave_call();
    }

    fn run_wasm_command_inner(&mut self, plugin: &str, command: &str) {
        if let Some(manifest) = self.plugins.plugins.iter().find(|m| m.id == plugin)
            && let Some(entry) = manifest.commands.iter().find(|c| c.id == command)
        {
            let fingerprint = format!("{:x}", manifest.command_fingerprint(entry));
            if !self.config.plugin_trusted.iter().any(|t| t == &fingerprint) {
                self.ask_plugin_trust(
                    &PluginTarget {
                        plugin: plugin.to_owned(),
                        command: command.to_owned(),
                    },
                    fingerprint,
                );
                return;
            }
        }
        let snapshot = self.wasm_snapshot();
        match self.wasm.invoke(plugin, command, snapshot) {
            Ok(Some(message)) => self.notify(super::NoticeKind::Done, message),
            Ok(None) => {}
            Err(error) => self.notify(super::NoticeKind::Error, error),
        }
        self.apply_wasm_effects();
    }

    /// Minta draw command plugin untuk satu area dan terapkan effect yang dihasilkan.
    pub(crate) fn wasm_render(
        &mut self,
        surface: RenderSurface,
        target: Option<&str>,
        area: winc_render::Rect,
    ) -> Vec<DrawCommand> {
        if !self.wasm.enter_call() {
            return Vec::new();
        }
        let commands = self.wasm_render_inner(surface, target, area);
        self.wasm.leave_call();
        commands
    }

    fn wasm_render_inner(
        &mut self,
        surface: RenderSurface,
        target: Option<&str>,
        area: winc_render::Rect,
    ) -> Vec<DrawCommand> {
        let snapshot = self.wasm_snapshot();
        let commands =
            self.wasm
                .render(surface, target, (area.x, area.y, area.w, area.h), snapshot);
        self.apply_wasm_effects();
        commands
    }

    /// Kirim event lifecycle ke plugin yang berlangganan.
    pub(in crate::app) fn wasm_event(&mut self, event: EventKind) {
        if !self.wasm.enter_call() {
            return;
        }
        self.wasm_event_inner(event);
        self.wasm.leave_call();
    }

    fn wasm_event_inner(&mut self, event: EventKind) {
        let snapshot = self.wasm_snapshot();
        self.wasm.dispatch_event(event, snapshot);
        self.apply_wasm_effects();
    }

    /// Beri tahu plugin bahwa dokumen berhasil ditulis ke disk.
    pub(in crate::app) fn wasm_document_saved(&mut self, id: u64) {
        self.wasm_event(EventKind::DocumentSaved { id });
    }

    /// Beri tahu plugin bahwa workspace dibuka atau ditutup.
    pub(in crate::app) fn wasm_workspace_changed(&mut self, path: Option<String>) {
        self.wasm_event(EventKind::WorkspaceChanged { path });
    }

    /// Beri tahu plugin bahwa konfigurasi ditulis dengan revisi baru.
    pub(in crate::app) fn wasm_config_revision(&mut self, revision: u64) {
        self.wasm_event(EventKind::ConfigChanged { revision });
    }

    /// Amati revisi dan seleksi dokumen, lalu kirim event perubahan ke plugin.
    pub(in crate::app) fn wasm_observe_documents(&mut self) {
        let states = self
            .editor
            .documents
            .iter()
            .map(|doc| {
                (
                    doc.id.0,
                    doc.revision,
                    doc.selection.anchor as u64,
                    doc.selection.head as u64,
                )
            })
            .collect::<Vec<_>>();
        let events = self.wasm.observe_documents(&states);
        for event in events {
            self.wasm_event(event);
        }
    }

    /// Beri tahu semua plugin bahwa aplikasi sedang ditutup.
    pub(in crate::app) fn wasm_shutdown(&mut self) {
        if !self.wasm.enter_call() {
            return;
        }
        self.wasm.shutdown();
        self.wasm.leave_call();
    }

    /// Aktifkan dokumen yang dituju effect plugin; mengembalikan false bila sudah ditutup.
    fn wasm_activate(&mut self, id: u64) -> bool {
        let Some(index) = self.editor.index_of(DocumentId(id)) else {
            return false;
        };
        self.activate_tab(index);
        true
    }

    /// Simpan konfigurasi setelah perubahan dari plugin.
    fn wasm_config_changed(&mut self, key: &str) {
        match key {
            "ui.accent" => self.apply_accent(),
            "ui.corner_scale" => self.apply_corner_scale(),
            "editor.font_size" => self.apply_font_sizes(),
            _ => self.request_redraw(),
        }
        self.save_config();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_updates_allowed_config_keys() {
        let mut config = Config {
            ui_accent: Some("#123456".to_owned()),
            ..Default::default()
        };
        let values = config_values(&config);
        assert_eq!(values["ui.accent"], "#123456");

        assert!(set_config_value(&mut config, "ui.corner_scale", "120"));
        assert_eq!(config.ui_corner_scale, Some(120));
        assert!(!set_config_value(&mut config, "ui.corner_scale", "nope"));
        assert!(!set_config_value(&mut config, "ui.corner_scale", "500"));
        assert!(!set_config_value(&mut config, "ui.accent", "not-a-color"));
        assert!(!set_config_value(&mut config, "editor.font_size", "100"));
        assert!(!set_config_value(&mut config, "unknown.key", "1"));
    }

    #[test]
    fn observes_only_changed_documents() {
        let mut manager = Manager::default();
        let states = |revision, head| vec![(7, revision, head, head)];

        assert!(manager.observe_documents(&states(1, 0)).is_empty());
        let events = manager.observe_documents(&states(2, 0));
        assert_eq!(events, vec![EventKind::DocumentChanged { id: 7 }]);

        let events = manager.observe_documents(&states(2, 3));
        assert_eq!(events, vec![EventKind::SelectionChanged { id: 7 }]);

        assert!(manager.observe_documents(&[]).is_empty());
        assert!(manager.observe_documents(&states(1, 0)).is_empty());
    }

    #[test]
    fn status_entries_keep_the_tooltip() {
        let mut manager = Manager::default();
        manager.status.insert(
            ("demo".to_owned(), "build".to_owned()),
            StatusText {
                text: "ready".to_owned(),
                align: StatusAlign::Right,
                tooltip: Some("all checks passed".to_owned()),
            },
        );
        let entries = manager.status_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "build");
        assert_eq!(entries[0].tooltip.as_deref(), Some("all checks passed"));
    }

    #[test]
    fn limits_nested_plugin_calls() {
        let mut manager = Manager::default();
        assert!(manager.enter_call());
        assert!(manager.enter_call());
        assert!(manager.enter_call());
        assert!(manager.enter_call());
        assert!(!manager.enter_call());
        manager.leave_call();
        assert!(manager.enter_call());
    }

    #[test]
    fn rejects_paths_outside_the_allowed_roots() {
        let root = PathBuf::from("/tmp/opencode/winc-plugin-root");
        assert!(path_within(&root.join("notes.txt"), &root));
        assert!(!path_within(&root.join("../outside.txt"), &root));
    }

    #[test]
    fn plugin_info_only_exposes_granted_capabilities() {
        use winc_config::PluginWasm;

        let manifest = PluginManifest {
            id: "demo".to_owned(),
            name: "Demo".to_owned(),
            description: None,
            dir: PathBuf::from("/plugins/demo"),
            commands: Vec::new(),
            settings: Vec::new(),
            wasm: Some(PluginWasm {
                path: PathBuf::from("/plugins/demo/demo.wasm"),
                hash: 1,
                capabilities: vec!["editor_read".to_owned(), "network".to_owned()],
            }),
        };
        let config = Config {
            plugin_grants: BTreeMap::from([(
                "demo".to_owned(),
                vec!["editor_read".to_owned(), "unknown".to_owned()],
            )]),
            ..Default::default()
        };
        let info = Manager::info(&manifest, &config);
        assert_eq!(info.granted, [Capability::EditorRead]);
    }

    #[test]
    fn reads_files_only_from_allowed_roots() {
        let root = std::env::temp_dir().join(format!("winc-plugin-fs-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp root should be created");
        let allowed = root.join("allowed.txt");
        std::fs::write(&allowed, "hello").expect("fixture should be written");
        let snapshot = Snapshot {
            context: Context {
                workspace: Some(root.to_string_lossy().into_owned()),
                ..Default::default()
            },
            plugin_dirs: BTreeMap::from([("demo".to_owned(), root.clone())]),
            ..Default::default()
        };
        let plugin = PluginInfo {
            id: "demo".to_owned(),
            name: "Demo".to_owned(),
            version: "1".to_owned(),
            dir: root.to_string_lossy().into_owned(),
            requested: vec![Capability::FsRead],
            granted: vec![Capability::FsRead],
        };
        let mut bridge = Bridge::new(plugin, snapshot);

        let reply = bridge.handle(HostRequest::FsRead {
            path: allowed.to_string_lossy().into_owned(),
        });
        assert_eq!(
            reply.into_result::<String>().expect("read should work"),
            "hello"
        );

        let reply = bridge.handle(HostRequest::FsRead {
            path: "allowed.txt".to_owned(),
        });
        assert_eq!(
            reply
                .into_result::<String>()
                .expect("relative read should work"),
            "hello"
        );

        let outside = root.join("../outside-winc-plugin.txt");
        let reply = bridge.handle(HostRequest::FsRead {
            path: outside.to_string_lossy().into_owned(),
        });
        assert!(reply.into_result::<String>().is_err());

        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_paths_outside_allowed_roots() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("winc-plugin-link-{}", std::process::id()));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).expect("root should be created");
        std::fs::create_dir_all(&outside).expect("outside should be created");
        symlink(&outside, root.join("escape")).expect("symlink should be created");

        let path = root
            .join("escape/secret.txt")
            .to_string_lossy()
            .into_owned();
        assert!(allowed_path(&path, std::slice::from_ref(&root), false).is_err());

        std::fs::remove_dir_all(&base).ok();
    }
}
