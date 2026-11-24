//! 插件清单：从本机 JSONC 文件读入命令、设置和可选的 WebAssembly 模块，并计算信任指纹。
//!
//! 插件分两种：外部命令插件只描述「程序 + 参数 + 环境变量」；WASM 插件额外声明一个 `.wasm`
//! 文件和所需 capability，由 `winct-plugin` 在沙箱里加载。两者都必须由用户显式开启并信任；
//! 清单只从用户配置的路径读取，不自动执行项目仓库里的内容。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::json::Json;

/// 一个插件：一组相关命令和可在设置页填写的值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    /// 稳定标识，用来在配置中启用、信任和保存设置值。
    pub id: String,
    /// 显示在设置页和命令面板中的名称。
    pub name: String,
    /// 简短说明。
    pub description: Option<String>,
    /// 清单文件所在目录：命令的相对工作目录相对它解析。
    pub dir: PathBuf,
    /// 插件提供的命令。
    pub commands: Vec<PluginCommand>,
    /// 插件的设置项。
    pub settings: Vec<PluginSetting>,
    /// WebAssembly 模块；有值时命令由沙箱运行时执行，而不是启动外部程序。
    pub wasm: Option<PluginWasm>,
}

/// 清单中声明的 WebAssembly 模块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginWasm {
    /// 相对清单目录的模块路径。
    pub path: PathBuf,
    /// 模块内容的 FNV-1a 哈希；改动模块会让信任指纹变化。
    pub hash: u64,
    /// 插件请求的 capability 名称。
    pub capabilities: Vec<String>,
}

/// 一条插件命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    /// 插件内唯一标识。
    pub id: String,
    /// 命令面板里显示的标题。
    pub title: String,
    /// 要启动的程序。
    pub program: String,
    /// 传给程序的参数；支持 `{file}`、`{root}`、`{selection}` 占位符。
    pub args: Vec<String>,
    /// 工作目录；相对路径相对插件目录解析。
    pub cwd: Option<String>,
    /// 额外环境变量。
    pub env: BTreeMap<String, String>,
    /// 是否把当前文件、目录或选区作为额外参数追加。
    pub input: PluginInput,
    /// 运行结果的呈现方式。
    pub output: PluginOutput,
    /// 为真时由 WASM 运行时执行 `id` 指定的导出命令，而不是启动 `program`。
    pub wasm: bool,
}

/// 插件命令追加的上下文输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginInput {
    /// 不追加。
    #[default]
    None,
    /// 当前文件的绝对路径。
    File,
    /// 当前工作区根目录的绝对路径。
    Root,
    /// 当前选中的文字。
    Selection,
}

/// 插件命令结果的呈现方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginOutput {
    /// 在界面右上角显示（默认）。
    #[default]
    Notification,
    /// 显示完整输出；当前版本同样以通知显示，但会保留多行文本。
    Terminal,
}

/// 插件在设置页中显示的一项配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSetting {
    /// 配置键，用来在 `plugins.values` 中保存值。
    pub key: String,
    /// 显示的标题。
    pub title: String,
    /// 说明。
    pub description: Option<String>,
    /// 值的类型。
    pub kind: PluginSettingKind,
    /// 默认值。
    pub default: String,
}

/// 插件设置项的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginSettingKind {
    /// 一行文本。
    #[default]
    String,
    /// 布尔开关。
    Bool,
}

/// 已发现的插件以及清单里的错误。
#[derive(Debug, Clone, Default)]
pub struct PluginRegistry {
    /// 成功读入的插件。
    pub plugins: Vec<PluginManifest>,
    /// 无法读入的清单，供设置页显示。
    pub problems: Vec<String>,
}

impl PluginRegistry {
    /// 用户配置中启用的插件，按清单顺序返回。
    pub fn enabled<'a>(
        &'a self,
        config: &'a crate::Config,
    ) -> impl Iterator<Item = (usize, &'a PluginManifest)> + 'a {
        self.plugins
            .iter()
            .enumerate()
            .filter(move |(_, plugin)| config.plugin_enabled.iter().any(|id| id == &plugin.id))
    }

    /// 命令的完整标识：`插件 id.命令 id`。
    pub fn command_id(plugin: &PluginManifest, command: &PluginCommand) -> String {
        format!("{}.{}", plugin.id, command.id)
    }
}

/// 把 `s` 缓存成 `'static` 字符串：设置行的名称和命令面板的标题跟随插件注册表存活。
pub fn intern(s: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<BTreeMap<String, &'static str>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    if let Some(found) = cache.get(s) {
        return found;
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    cache.insert(s.to_owned(), leaked);
    leaked
}

impl PluginManifest {
    /// 从 JSONC 对象读入清单；缺少 `id`、`name` 或 `commands` 时返回错误说明。
    ///
    /// # Errors
    ///
    /// 对象结构不符、必填字段缺失或类型不对时返回可读的原因。
    pub fn parse(value: &Json, dir: PathBuf) -> Result<PluginManifest, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "the manifest must be an object".to_owned())?;
        let required = |key: &str| -> Result<String, String> {
            object
                .get(key)
                .and_then(Json::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("missing or empty “{key}”"))
        };
        let id = required("id")?;
        let name = required("name")?;
        let description = object
            .get("description")
            .and_then(Json::as_str)
            .map(str::to_owned);
        let wasm = match object.get("runtime") {
            None => None,
            Some(Json::Object(fields)) => {
                let path = fields
                    .get("wasm")
                    .and_then(Json::as_str)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| "runtime.wasm must be a non-empty path".to_owned())?;
                let capabilities = fields
                    .get("capabilities")
                    .and_then(Json::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect();
                Some(PluginWasm {
                    path: dir.join(expand(path)),
                    hash: 0,
                    capabilities,
                })
            }
            Some(_) => return Err("runtime must be an object".to_owned()),
        };
        let commands = object
            .get("commands")
            .and_then(Json::as_array)
            .ok_or_else(|| "commands must be an array".to_owned())?
            .iter()
            .enumerate()
            .map(|(i, value)| {
                PluginCommand::parse(value, wasm.is_some())
                    .map_err(|e| format!("commands[{i}]: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let settings = object
            .get("settings")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(i, value)| {
                PluginSetting::parse(value).map_err(|e| format!("settings[{i}]: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PluginManifest {
            id,
            name,
            description,
            dir,
            commands,
            settings,
            wasm,
        })
    }

    /// 命令的信任指纹：带上插件 id 和 WASM 模块哈希，模块或 capability 改动后需要重新确认。
    pub fn command_fingerprint(&self, command: &PluginCommand) -> u64 {
        let mut fingerprinted = command.clone();
        fingerprinted.id = format!("{}.{}", self.id, command.id);
        let mut text = format!("{:x}\0", Self::fingerprint(&fingerprinted));
        if let Some(wasm) = &self.wasm {
            text.push_str(&format!("{:x}\0", wasm.hash));
            for capability in &wasm.capabilities {
                text.push_str(capability);
                text.push('\0');
            }
        }
        fnv1a(text.bytes())
    }

    /// 这条命令的信任指纹：清单改变后需要重新确认。
    pub fn fingerprint(command: &PluginCommand) -> u64 {
        let mut text = format!(
            "{}\0{}\0{:?}\0{:?}\0{}\0",
            command.id,
            command.program,
            command.args,
            command.input,
            command.cwd.as_deref().unwrap_or_default()
        );
        for (key, value) in &command.env {
            text.push_str(key);
            text.push('=');
            text.push_str(value);
            text.push('\0');
        }
        fnv1a(text.bytes())
    }
}

/// FNV-1a 64 位哈希，用于模块内容和信任指纹。
fn fnv1a(bytes: impl Iterator<Item = u8>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

impl PluginCommand {
    fn parse(value: &Json, wasm: bool) -> Result<PluginCommand, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "the command must be an object".to_owned())?;
        let required = |key: &str| -> Result<String, String> {
            object
                .get(key)
                .and_then(Json::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("missing or empty “{key}”"))
        };
        let args = object
            .get("args")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect();
        let env = match object.get("env") {
            None => BTreeMap::new(),
            Some(Json::Object(values)) => values
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_owned())))
                .collect(),
            Some(_) => return Err("env must be an object of strings".to_owned()),
        };
        let cwd = object
            .get("cwd")
            .and_then(Json::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let input = match object.get("input").and_then(Json::as_str) {
            None | Some("none") => PluginInput::None,
            Some("file") => PluginInput::File,
            Some("root") => PluginInput::Root,
            Some("selection") => PluginInput::Selection,
            Some(other) => return Err(format!("unknown input “{other}”")),
        };
        let output = match object.get("output").and_then(Json::as_str) {
            None | Some("notification") => PluginOutput::Notification,
            Some("terminal") => PluginOutput::Terminal,
            Some(other) => return Err(format!("unknown output “{other}”")),
        };
        Ok(PluginCommand {
            id: required("id")?,
            title: required("title")?,
            program: if wasm {
                object
                    .get("program")
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned()
            } else {
                required("program")?
            },
            args,
            cwd,
            env,
            input,
            output,
            wasm,
        })
    }
}

impl PluginSetting {
    fn parse(value: &Json) -> Result<PluginSetting, String> {
        let object = value
            .as_object()
            .ok_or_else(|| "the setting must be an object".to_owned())?;
        let required = |key: &str| -> Result<String, String> {
            object
                .get(key)
                .and_then(Json::as_str)
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("missing or empty “{key}”"))
        };
        let kind = match object.get("type").and_then(Json::as_str) {
            None | Some("string") => PluginSettingKind::String,
            Some("bool") => PluginSettingKind::Bool,
            Some(other) => return Err(format!("unknown type “{other}”")),
        };
        let default = object
            .get("default")
            .and_then(|value| match value {
                Json::String(s) => Some(s.clone()),
                Json::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| {
                if kind == PluginSettingKind::Bool {
                    "false"
                } else {
                    ""
                }
                .to_owned()
            });
        Ok(PluginSetting {
            key: required("key")?,
            title: required("title")?,
            description: object
                .get("description")
                .and_then(Json::as_str)
                .map(str::to_owned),
            kind,
            default,
        })
    }
}

/// 展开路径开头的 `~`。
fn expand(path: &str) -> PathBuf {
    if let (Some(rest), Some(home)) = (
        path.strip_prefix("~/"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    ) {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// 从用户配置的路径读入所有清单；目录中的 `*.json` 文件按文件名排序。
///
/// 只读取本机文件；错误逐条记入 [`PluginRegistry::problems`]，一个坏清单不影响其他插件。
pub fn load_plugins(paths: &[String]) -> PluginRegistry {
    let mut registry = PluginRegistry::default();
    for path in paths {
        let path = expand(path.trim());
        let files: Vec<PathBuf> = if path.is_dir() {
            let mut files = std::fs::read_dir(&path)
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .filter(|file| file.extension().is_some_and(|ext| ext == "json"))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            files.sort();
            files
        } else {
            vec![path]
        };
        for file in files {
            let text = match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(err) => {
                    registry.problems.push(format!("{}: {err}", file.display()));
                    continue;
                }
            };
            let value = match Json::parse_jsonc(&text) {
                Some((value, _)) => value,
                None => {
                    registry
                        .problems
                        .push(format!("{}: invalid JSONC", file.display()));
                    continue;
                }
            };
            let dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
            match PluginManifest::parse(&value, dir) {
                Ok(mut manifest) => {
                    if let Some(wasm) = &mut manifest.wasm {
                        match std::fs::read(&wasm.path) {
                            Ok(bytes) => wasm.hash = fnv1a(bytes.iter().copied()),
                            Err(err) => {
                                registry.problems.push(format!(
                                    "{}: could not read WASM module: {err}",
                                    wasm.path.display()
                                ));
                                continue;
                            }
                        }
                    }
                    if registry.plugins.iter().any(|p| p.id == manifest.id) {
                        registry.problems.push(format!(
                            "{}: duplicate plugin id “{}”",
                            file.display(),
                            manifest.id
                        ));
                    } else {
                        registry.plugins.push(manifest);
                    }
                }
                Err(err) => registry.problems.push(format!("{}: {err}", file.display())),
            }
        }
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_and_computes_stable_fingerprint() {
        let json = Json::parse(
            r#"{"id":"demo","name":"Demo","commands":[{"id":"hi","title":"Demo: Hi","program":"echo","args":["{file}"],"env":{"A":"B"}}],"settings":[{"key":"name","title":"Name","type":"string","default":"x"}]}"#,
        )
        .unwrap();
        let manifest = PluginManifest::parse(&json, PathBuf::from("/plugins/demo")).unwrap();
        assert_eq!(manifest.id, "demo");
        assert_eq!(manifest.commands.len(), 1);
        assert_eq!(manifest.settings[0].default, "x");
        let a = PluginManifest::fingerprint(&manifest.commands[0]);
        let b = PluginManifest::fingerprint(&manifest.commands[0]);
        assert_eq!(a, b);
        let mut changed = manifest.commands[0].clone();
        changed.program = "other".to_owned();
        assert_ne!(a, PluginManifest::fingerprint(&changed));
    }

    #[test]
    fn rejects_incomplete_manifest() {
        let json = Json::parse(r#"{"id":"demo"}"#).unwrap();
        assert!(PluginManifest::parse(&json, PathBuf::new()).is_err());
    }

    #[test]
    fn parses_wasm_manifest_and_fingerprints_module() {
        let json = Json::parse(
            r#"{"id":"w","name":"W","runtime":{"wasm":"p.wasm","capabilities":["editor_read"]},"commands":[{"id":"hi","title":"Hi"}]}"#,
        )
        .unwrap();
        let manifest = PluginManifest::parse(&json, PathBuf::from("/plugins/w")).unwrap();
        let wasm = manifest.wasm.as_ref().unwrap();
        assert_eq!(wasm.path, PathBuf::from("/plugins/w/p.wasm"));
        assert_eq!(wasm.capabilities, ["editor_read"]);
        assert!(manifest.commands[0].wasm);
        let a = manifest.command_fingerprint(&manifest.commands[0]);
        let mut changed = manifest.clone();
        changed.wasm.as_mut().unwrap().hash = 1;
        assert_ne!(a, changed.command_fingerprint(&changed.commands[0]));
        let mut changed = manifest.clone();
        changed
            .wasm
            .as_mut()
            .unwrap()
            .capabilities
            .push("editor_write".to_owned());
        assert_ne!(a, changed.command_fingerprint(&changed.commands[0]));
    }
}
