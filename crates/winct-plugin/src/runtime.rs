//! Wasmtime instance, host import, dan antrean effect untuk satu plugin.

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;
use wasmtime::{Caller, Config, Engine, Linker, Memory, Module, Store, TypedFunc};
use winct_plugin_api::*;

/// Fuel yang diberikan untuk satu panggilan guest. Guest yang loop tanpa henti akan trap.
pub const FUEL: u64 = 50_000_000;

/// Jembatan dari runtime ke aplikasi.
///
/// `handle` boleh menjawab request baca dari snapshot yang sudah disiapkan app. Semua perubahan
/// state harus masuk [`HostEffect`] dan diterapkan app setelah guest selesai dipanggil.
pub trait HostBridge {
    /// Konteks Winct saat plugin dipanggil.
    fn context(&self) -> Context;

    /// Tangani satu host call yang sudah lolos pemeriksaan capability.
    fn handle(&mut self, request: HostRequest) -> HostReply;

    /// Ambil effect yang dikumpulkan selama satu panggilan guest.
    fn take_effects(&mut self) -> Vec<PluginEffect>;
}

/// Effect beserta id plugin yang memintanya.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginEffect {
    pub plugin: String,
    pub effect: HostEffect,
}

/// Perubahan yang diminta plugin; diterapkan oleh `winct-app` setelah call selesai.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEffect {
    Notify {
        kind: NoticeKind,
        message: String,
    },
    Alert {
        title: String,
        message: String,
        confirm_label: String,
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
    ConfigSet {
        key: String,
        value: String,
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
    LspRegister {
        language: String,
        program: String,
        args: Vec<String>,
        env: std::collections::BTreeMap<String, String>,
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
        tokens: std::collections::BTreeMap<String, String>,
    },
    ThemeReset,
    OverlaySet {
        id: String,
        commands: Vec<DrawCommand>,
    },
    UiRefresh,
    FsWrite {
        path: String,
        text: String,
    },
    ProcessRun {
        request: ProcessRequest,
    },
}

/// State yang melekat pada satu `Store<HostState>`.
pub struct HostState {
    /// Identitas dan grant plugin ini.
    pub plugin: PluginInfo,
    bridge: Box<dyn HostBridge>,
}

impl HostState {
    fn new(plugin: PluginInfo, bridge: Box<dyn HostBridge>) -> Self {
        Self { plugin, bridge }
    }
}

/// Mesin Wasmtime bersama semua instance plugin.
#[derive(Clone)]
pub struct PluginRuntime {
    engine: Engine,
}

impl PluginRuntime {
    /// Buat engine dengan fuel dan batas memori yang aman untuk plugin pihak ketiga.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila Wasmtime gagal menginisialisasi engine.
    pub fn new() -> Result<Self, PluginError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.max_wasm_stack(1024 * 1024);
        let engine = Engine::new(&config)?;
        Ok(Self { engine })
    }

    /// Muat dan instantiate satu modul plugin.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila modul tidak valid, export wajib tidak ada, atau linker gagal.
    pub fn load(
        &self,
        wasm: &[u8],
        plugin: PluginInfo,
        bridge: Box<dyn HostBridge>,
    ) -> Result<PluginInstance, PluginError> {
        let module = Module::new(&self.engine, wasm)?;
        let mut linker = Linker::new(&self.engine);
        linker.func_wrap(
            HOST_MODULE,
            HOST_CALL,
            |mut caller: Caller<'_, HostState>,
             req_ptr: i32,
             req_len: i32,
             out_ptr: i32,
             out_cap: i32| {
                let reply = match read_bytes(&mut caller, req_ptr, req_len) {
                    Err(error) => HostReply::error(error),
                    Ok(bytes) => match serde_json::from_slice::<HostRequest>(&bytes) {
                        Err(error) => HostReply::error(format!("invalid host request: {error}")),
                        Ok(request) => {
                            let state = caller.data_mut();
                            match required_capability(&request) {
                                None => state.bridge.handle(request),
                                Some(cap) if state.plugin.granted.contains(&cap) => {
                                    state.bridge.handle(request)
                                }
                                Some(cap) => HostReply::error(format!(
                                    "plugin is missing capability {}",
                                    cap.as_str()
                                )),
                            }
                        }
                    },
                };
                let bytes = serde_json::to_vec(&reply).unwrap_or_else(|_| {
                    br#"{"status":"err","error":"could not encode host reply"}"#.to_vec()
                });
                if bytes.len() > MESSAGE_LIMIT {
                    return -(MESSAGE_LIMIT as i32);
                }
                if out_ptr <= 0 || out_cap <= 0 || (bytes.len() as u32) > out_cap as u32 {
                    return -(bytes.len() as i32);
                }
                match write_bytes(&mut caller, out_ptr, &bytes) {
                    Ok(()) => bytes.len() as i32,
                    Err(error) => -(error.len() as i32),
                }
            },
        )?;

        let mut store = Store::new(&self.engine, HostState::new(plugin, bridge));
        store.set_fuel(FUEL)?;
        let instance = linker.instantiate(&mut store, &module)?;
        let memory = instance
            .get_export(&mut store, "memory")
            .and_then(|value| value.into_memory())
            .ok_or_else(|| PluginError::Protocol("plugin has no exported memory".to_owned()))?;

        let alloc = instance.get_typed_func::<i32, i32>(&mut store, ALLOC)?;
        let activate =
            instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, ACTIVATE)?;
        let invoke = instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, INVOKE)?;
        let event = instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, EVENT)?;
        let render = instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, RENDER)?;
        let shutdown = instance.get_typed_func::<(), ()>(&mut store, SHUTDOWN).ok();

        Ok(PluginInstance {
            store,
            memory,
            alloc,
            activate,
            invoke,
            event,
            render,
            shutdown,
        })
    }
}

impl Default for PluginRuntime {
    fn default() -> Self {
        Self::new().expect("wasmtime engine should initialize")
    }
}

/// Satu plugin yang sudah aktif di dalam Winct.
pub struct PluginInstance {
    store: Store<HostState>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    activate: TypedFunc<(i32, i32, i32, i32), i32>,
    invoke: TypedFunc<(i32, i32, i32, i32), i32>,
    event: TypedFunc<(i32, i32, i32, i32), i32>,
    render: TypedFunc<(i32, i32, i32, i32), i32>,
    shutdown: Option<TypedFunc<(), ()>>,
}

impl PluginInstance {
    /// Identitas plugin yang dijalankan instance ini.
    pub fn info(&self) -> &PluginInfo {
        &self.store.data().plugin
    }

    /// Aktivasi awal; mengembalikan kontribusi command/setting/UI dari plugin.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila guest trap, kehabisan fuel, atau menulis JSON yang tidak valid.
    pub fn activate(&mut self, request: &ActivateRequest) -> Result<ActivateResponse, PluginError> {
        self.call_json(Export::Activate, request)
    }

    /// Jalankan command yang dipilih user.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila guest trap atau protokol balasan tidak sesuai ABI.
    pub fn invoke(&mut self, request: &InvokeRequest) -> Result<InvokeResponse, PluginError> {
        self.call_json(Export::Invoke, request)
    }

    /// Kirim event lifecycle.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila guest trap saat menangani event.
    pub fn event(&mut self, request: &EventRequest) -> Result<(), PluginError> {
        self.call_json(Export::Event, request)
    }

    /// Minta draw command untuk sebuah surface.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila guest trap atau draw command tidak bisa dibaca.
    pub fn render(&mut self, request: &RenderRequest) -> Result<RenderResponse, PluginError> {
        self.call_json(Export::Render, request)
    }

    /// Ganti jembatan host untuk satu panggilan berikutnya.
    ///
    /// Ini dipakai host setiap kali `PluginInstance` dipanggil ulang: snapshot editor dan config
    /// berubah antar-panggilan, tetapi instance WASM-nya tetap dijalankan.
    pub fn set_bridge(&mut self, bridge: Box<dyn HostBridge>) {
        self.store.data_mut().bridge = bridge;
    }

    /// Ambil effect yang dikumpulkan selama satu panggilan guest terakhir.
    pub fn take_effects(&mut self) -> Vec<PluginEffect> {
        self.store.data_mut().bridge.take_effects()
    }

    /// Beri tahu plugin bahwa Winct sedang menutup.
    ///
    /// # Errors
    ///
    /// Mengembalikan error bila export `winct_shutdown` ada tetapi trap.
    pub fn shutdown(&mut self) -> Result<(), PluginError> {
        if let Some(func) = self.shutdown.clone() {
            self.store.set_fuel(FUEL)?;
            func.call(&mut self.store, ())?;
        }
        Ok(())
    }

    fn call_json<I: Serialize, O: DeserializeOwned>(
        &mut self,
        which: Export,
        request: &I,
    ) -> Result<O, PluginError> {
        // Setiap panggilan diberi fuel baru; plugin yang sudah aktif tidak boleh kehabisan hanya
        // karena sering dipanggil.
        self.store.set_fuel(FUEL)?;
        let bytes = serde_json::to_vec(request)?;
        if bytes.len() > MESSAGE_LIMIT {
            return Err(PluginError::Protocol(
                "request exceeds message limit".to_owned(),
            ));
        }
        let req_ptr = self
            .alloc
            .clone()
            .call(&mut self.store, bytes.len() as i32)?;
        self.write_memory(req_ptr, &bytes)?;
        let out_ptr = self
            .alloc
            .clone()
            .call(&mut self.store, MESSAGE_LIMIT as i32)?;
        let args = (req_ptr, bytes.len() as i32, out_ptr, MESSAGE_LIMIT as i32);
        let written = match which {
            Export::Activate => self.activate.clone().call(&mut self.store, args)?,
            Export::Invoke => self.invoke.clone().call(&mut self.store, args)?,
            Export::Event => self.event.clone().call(&mut self.store, args)?,
            Export::Render => self.render.clone().call(&mut self.store, args)?,
        };
        if written < 0 {
            return Err(PluginError::Protocol(format!(
                "guest rejected {which:?} with {written}"
            )));
        }
        if written as usize > MESSAGE_LIMIT {
            return Err(PluginError::Protocol(
                "guest reply exceeds message limit".to_owned(),
            ));
        }
        let output = self.read_memory(out_ptr, written as usize)?;
        // `winct_event` tidak wajib menulis respons; JSON kosong dibaca sebagai `()`.
        if output.is_empty() {
            return serde_json::from_value(serde_json::Value::Null).map_err(Into::into);
        }
        Ok(serde_json::from_slice(&output)?)
    }

    fn read_memory(&self, ptr: i32, len: usize) -> Result<Vec<u8>, PluginError> {
        if ptr < 0 {
            return Err(PluginError::Protocol("negative guest pointer".to_owned()));
        }
        let start = ptr as usize;
        let end = start
            .checked_add(len)
            .ok_or_else(|| PluginError::Protocol("guest range overflow".to_owned()))?;
        if end > self.memory.data(&self.store).len() {
            return Err(PluginError::Protocol(
                "guest range out of bounds".to_owned(),
            ));
        }
        Ok(self.memory.data(&self.store)[start..end].to_vec())
    }

    fn write_memory(&mut self, ptr: i32, bytes: &[u8]) -> Result<(), PluginError> {
        if ptr < 0 {
            return Err(PluginError::Protocol("negative guest pointer".to_owned()));
        }
        let start = ptr as usize;
        let end = start
            .checked_add(bytes.len())
            .ok_or_else(|| PluginError::Protocol("guest range overflow".to_owned()))?;
        if end > self.memory.data(&self.store).len() {
            return Err(PluginError::Protocol(
                "guest range out of bounds".to_owned(),
            ));
        }
        self.memory.data_mut(&mut self.store)[start..end].copy_from_slice(bytes);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum Export {
    Activate,
    Invoke,
    Event,
    Render,
}

impl fmt::Display for Export {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Activate => ACTIVATE,
            Self::Invoke => INVOKE,
            Self::Event => EVENT,
            Self::Render => RENDER,
        };
        f.write_str(name)
    }
}

/// Capability minimum untuk sebuah host request.
fn required_capability(request: &HostRequest) -> Option<Capability> {
    Some(match request {
        HostRequest::Log { .. } | HostRequest::Notify { .. } | HostRequest::Alert { .. } => {
            Capability::Notifications
        }
        HostRequest::EditorDocuments
        | HostRequest::EditorActive
        | HostRequest::EditorText { .. }
        | HostRequest::EditorSlice { .. } => Capability::EditorRead,
        HostRequest::EditorReplace { .. }
        | HostRequest::EditorSetSelection { .. }
        | HostRequest::EditorOpen { .. } => Capability::EditorWrite,
        HostRequest::EditorSave { .. } => Capability::EditorSave,
        HostRequest::ConfigGet { .. } | HostRequest::SettingsGet { .. } => Capability::ConfigRead,
        HostRequest::ConfigSet { .. } | HostRequest::SettingsSet { .. } => Capability::ConfigWrite,
        HostRequest::CommandRegister { .. }
        | HostRequest::CommandExecute { .. }
        | HostRequest::PaletteRefresh => Capability::Commands,
        HostRequest::EventSubscribe { .. } => Capability::Events,
        HostRequest::LspServers
        | HostRequest::LspRegister { .. }
        | HostRequest::LspRequest { .. }
        | HostRequest::DiagnosticsPublish { .. } => Capability::Lsp,
        HostRequest::StatusSet { .. } | HostRequest::StatusRemove { .. } => Capability::UiStatus,
        HostRequest::ThemeTokens { .. } | HostRequest::ThemeReset => Capability::UiTheme,
        HostRequest::OverlaySet { .. } => Capability::UiOverlay,
        HostRequest::UiRefresh => Capability::UiOverlay,
        HostRequest::FsRead { .. } => Capability::FsRead,
        HostRequest::FsWrite { .. } => Capability::FsWrite,
        HostRequest::HttpRequest { .. } => Capability::Network,
        HostRequest::ProcessRun { .. } => Capability::Process,
    })
}

fn read_bytes(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) -> Result<Vec<u8>, String> {
    if ptr < 0 || len < 0 {
        return Err("invalid host buffer".to_owned());
    }
    let memory = caller
        .get_export("memory")
        .and_then(|value| value.into_memory())
        .ok_or_else(|| "plugin has no exported memory".to_owned())?;
    let start = ptr as usize;
    let end = start
        .checked_add(len as usize)
        .ok_or_else(|| "host buffer overflow".to_owned())?;
    if end > memory.data(&caller).len() {
        return Err("host buffer out of bounds".to_owned());
    }
    Ok(memory.data(&caller)[start..end].to_vec())
}

fn write_bytes(caller: &mut Caller<'_, HostState>, ptr: i32, bytes: &[u8]) -> Result<(), String> {
    if ptr < 0 {
        return Err("invalid host output pointer".to_owned());
    }
    let memory = caller
        .get_export("memory")
        .and_then(|value| value.into_memory())
        .ok_or_else(|| "plugin has no exported memory".to_owned())?;
    let start = ptr as usize;
    let end = start
        .checked_add(bytes.len())
        .ok_or_else(|| "host output overflow".to_owned())?;
    if end > memory.data(&caller).len() {
        return Err("host output out of bounds".to_owned());
    }
    memory.data_mut(caller)[start..end].copy_from_slice(bytes);
    Ok(())
}

/// Error yang bisa terjadi saat memuat atau memanggil plugin.
#[derive(Debug)]
pub enum PluginError {
    /// Engine, linker, atau trap dari Wasmtime.
    Engine(anyhow::Error),
    /// Protokol JSON atau buffer guest tidak sesuai ABI.
    Protocol(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(error) => write!(f, "wasm plugin error: {error:#}"),
            Self::Protocol(error) => write!(f, "wasm plugin protocol error: {error}"),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<anyhow::Error> for PluginError {
    fn from(error: anyhow::Error) -> Self {
        Self::Engine(error)
    }
}

impl From<wasmtime::Error> for PluginError {
    fn from(error: wasmtime::Error) -> Self {
        Self::Protocol(format!("wasmtime: {error}"))
    }
}

impl From<serde_json::Error> for PluginError {
    fn from(error: serde_json::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}
#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct TestBridge {
        requests: Arc<Mutex<Vec<HostRequest>>>,
        effects: Arc<Mutex<Vec<PluginEffect>>>,
    }

    impl HostBridge for TestBridge {
        fn context(&self) -> Context {
            Context {
                locale: "id-ID".to_owned(),
                ..Context::default()
            }
        }

        fn handle(&mut self, request: HostRequest) -> HostReply {
            self.requests.lock().unwrap().push(request.clone());
            if let HostRequest::Notify { kind, message } = request {
                self.effects.lock().unwrap().push(PluginEffect {
                    plugin: "demo".to_owned(),
                    effect: HostEffect::Notify { kind, message },
                });
            }
            HostReply::ok(())
        }

        fn take_effects(&mut self) -> Vec<PluginEffect> {
            std::mem::take(&mut self.effects.lock().unwrap())
        }
    }

    fn wat_bytes(bytes: &[u8]) -> String {
        let mut out = String::new();
        for byte in bytes {
            out.push_str(&format!("\\{byte:02x}"));
        }
        out
    }

    fn module(log: &HostRequest, response: &ActivateResponse) -> Vec<u8> {
        let log_bytes = serde_json::to_vec(log).unwrap();
        let response_bytes = serde_json::to_vec(response).unwrap();
        let wat = format!(
            r#"(module
                (import "winct" "host_call" (func $host_call (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 258)
                (global $next (mut i32) (i32.const 4096))
                (func (export "winct_alloc") (param $size i32) (result i32)
                    (local $ptr i32)
                    (local.set $ptr (global.get $next))
                    (global.set $next (i32.add (local.get $ptr) (local.get $size)))
                    (local.get $ptr))
                (data (i32.const 100) "{resp}")
                (data (i32.const 1000) "{log}")
                (func (export "winct_activate") (param $req i32) (param $len i32) (param $out i32) (param $cap i32) (result i32)
                    (drop (call $host_call (i32.const 1000) (i32.const {log_len}) (i32.const 2000) (i32.const 128)))
                    (memory.copy (local.get $out) (i32.const 100) (i32.const {resp_len}))
                    (i32.const {resp_len}))
                (func $empty (param i32 i32 i32 i32) (result i32) (i32.const 0))
                (export "winct_invoke" (func $empty))
                (export "winct_event" (func $empty))
                (export "winct_render" (func $empty))
            )"#,
            resp = wat_bytes(&response_bytes),
            resp_len = response_bytes.len(),
            log = wat_bytes(&log_bytes),
            log_len = log_bytes.len(),
        );
        wat::parse_str(wat).unwrap()
    }

    fn info(granted: Vec<Capability>) -> PluginInfo {
        PluginInfo {
            id: "demo".to_owned(),
            name: "Demo".to_owned(),
            version: "1".to_owned(),
            dir: "/plugins/demo".to_owned(),
            requested: vec![Capability::Notifications],
            granted,
        }
    }

    #[test]
    fn host_call_is_allowed_when_capability_is_granted() {
        let bridge = TestBridge::default();
        let requests = bridge.requests.clone();
        let mut instance = PluginRuntime::new()
            .unwrap()
            .load(
                &module(
                    &HostRequest::Notify {
                        kind: NoticeKind::Info,
                        message: "halo".to_owned(),
                    },
                    &ActivateResponse::default(),
                ),
                info(vec![Capability::Notifications]),
                Box::new(bridge.clone()),
            )
            .unwrap();

        let response = instance
            .activate(&ActivateRequest {
                abi: ABI_VERSION,
                plugin: info(vec![Capability::Notifications]),
                context: Context::default(),
            })
            .unwrap();

        assert_eq!(response, ActivateResponse::default());
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(
            instance.take_effects(),
            vec![PluginEffect {
                plugin: "demo".to_owned(),
                effect: HostEffect::Notify {
                    kind: NoticeKind::Info,
                    message: "halo".to_owned(),
                },
            }]
        );
    }

    #[test]
    fn host_call_is_denied_without_capability() {
        let bridge = TestBridge::default();
        let requests = bridge.requests.clone();
        let mut instance = PluginRuntime::new()
            .unwrap()
            .load(
                &module(
                    &HostRequest::Notify {
                        kind: NoticeKind::Info,
                        message: "halo".to_owned(),
                    },
                    &ActivateResponse::default(),
                ),
                info(Vec::new()),
                Box::new(bridge.clone()),
            )
            .unwrap();

        instance
            .activate(&ActivateRequest {
                abi: ABI_VERSION,
                plugin: info(Vec::new()),
                context: Context::default(),
            })
            .unwrap();

        assert!(requests.lock().unwrap().is_empty());
        assert!(instance.take_effects().is_empty());
    }
}
