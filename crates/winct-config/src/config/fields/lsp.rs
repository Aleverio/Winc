//! `lsp` 节：自动检测、各语言的服务器配置和额外可执行文件名。

use std::collections::BTreeMap;

use winct_i18n::t;

use super::*;

/// 读入「语言 id → 非空字符串数组」，语言和元素都做校验。
fn read_string_arrays(
    value: &Json,
    name: &str,
    out: &mut BTreeMap<String, Vec<String>>,
    problems: &mut Vec<String>,
) {
    let Some(map) = value.as_object() else {
        problems.push(t!("config-not-object", name = name));
        return;
    };
    for (language, values) in map {
        let Some(id) = language_id(language, name, problems) else {
            continue;
        };
        let Some(items) = values.as_array() else {
            problems.push(t!("config-not-array", name = format!("{name}.{language}")));
            continue;
        };
        if items
            .iter()
            .any(|item| item.as_str().is_none_or(str::is_empty))
        {
            problems.push(t!(
                "config-item-not-non-empty-string",
                name = format!("{name}.{language}")
            ));
            continue;
        }
        out.insert(
            id,
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
        );
    }
}

/// 把「语言 id → 字符串数组」写成 JSON 对象。
fn string_arrays_json(map: &BTreeMap<String, Vec<String>>) -> Option<Json> {
    (!map.is_empty()).then(|| {
        Json::object(map.iter().map(|(language, values)| {
            (
                language.as_str(),
                Json::Array(values.iter().cloned().map(Json::String).collect()),
            )
        }))
    })
}

/// 读入一个语言服务器的详细配置。
fn read_lsp_entry(
    value: &Json,
    language: &str,
    index: usize,
    name: &str,
    problems: &mut Vec<String>,
) -> Option<LspServerEntry> {
    let Some(object) = value.as_object() else {
        problems.push(t!(
            "config-lsp-entry-invalid",
            language = language,
            index = index,
            name = name
        ));
        return None;
    };
    let id = object
        .get("id")
        .and_then(Json::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let command: Option<Vec<String>> = object.get("command").and_then(Json::as_array).map(|args| {
        args.iter()
            .filter_map(|arg| {
                arg.as_str()
                    .filter(|arg| !arg.is_empty())
                    .map(str::to_owned)
            })
            .collect()
    });
    let (Some(id), Some(command)) = (id, command) else {
        problems.push(t!(
            "config-lsp-entry-invalid",
            language = language,
            index = index,
            name = name
        ));
        return None;
    };
    if command.is_empty() {
        problems.push(t!(
            "config-lsp-entry-invalid",
            language = language,
            index = index,
            name = name
        ));
        return None;
    }
    let mut env = BTreeMap::new();
    if let Some(value) = object.get("env") {
        let Some(values) = value.as_object() else {
            problems.push(t!(
                "config-not-object",
                name = format!("{name}.{language}.env")
            ));
            return None;
        };
        for (key, value) in values {
            match value.as_str() {
                Some(value) => {
                    env.insert(key.clone(), value.to_owned());
                }
                None => {
                    problems.push(t!(
                        "config-not-string",
                        name = format!("{name}.{language}.env.{key}")
                    ));
                    return None;
                }
            }
        }
    }
    let cwd = match object.get("cwd") {
        None | Some(Json::Null) => None,
        Some(Json::String(cwd)) if !cwd.is_empty() => Some(cwd.clone()),
        Some(_) => {
            problems.push(t!(
                "config-not-string",
                name = format!("{name}.{language}.cwd")
            ));
            None
        }
    };
    let enabled = object
        .get("enabled")
        .and_then(Json::as_bool)
        .unwrap_or(true);
    Some(LspServerEntry {
        id,
        command,
        env,
        cwd,
        enabled,
    })
}

/// 读入各语言的详细 LSP 服务器配置。
fn read_lsp_entries(
    value: &Json,
    name: &str,
    problems: &mut Vec<String>,
) -> BTreeMap<String, Vec<LspServerEntry>> {
    let Some(map) = value.as_object() else {
        problems.push(t!("config-not-object", name = name));
        return BTreeMap::new();
    };
    let mut result = BTreeMap::new();
    for (language, entries) in map {
        let Some(id) = language_id(language, name, problems) else {
            continue;
        };
        let Some(entries) = entries.as_array() else {
            problems.push(t!("config-not-array", name = format!("{name}.{language}")));
            continue;
        };
        let mut servers = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let Some(server) = read_lsp_entry(entry, language, index, name, problems) else {
                continue;
            };
            if servers
                .iter()
                .any(|other: &LspServerEntry| other.id == server.id)
            {
                problems.push(t!(
                    "config-lsp-entry-duplicate",
                    id = server.id,
                    name = name
                ));
                continue;
            }
            servers.push(server);
        }
        result.insert(id, servers);
    }
    result
}

/// 把各语言的详细 LSP 服务器配置写成 JSON 对象。
fn lsp_entries_json(entries: &BTreeMap<String, Vec<LspServerEntry>>) -> Option<Json> {
    (!entries.is_empty()).then(|| {
        Json::object(entries.iter().map(|(language, servers)| {
            let servers = servers.iter().map(|server| {
                Json::object([
                    ("id", Json::String(server.id.clone())),
                    (
                        "command",
                        Json::Array(server.command.iter().cloned().map(Json::String).collect()),
                    ),
                    (
                        "env",
                        Json::object(
                            server
                                .env
                                .iter()
                                .map(|(key, value)| (key.as_str(), Json::String(value.clone()))),
                        ),
                    ),
                    (
                        "cwd",
                        server
                            .cwd
                            .as_ref()
                            .map_or(Json::Null, |cwd| Json::String(cwd.clone())),
                    ),
                    ("enabled", Json::Bool(server.enabled)),
                ])
            });
            (language.as_str(), Json::Array(servers.collect()))
        }))
    })
}

/// `lsp` 一节。
pub const LSP: &[Field] = &[
    Field {
        key: "auto",
        read: |config, value, name, problems| {
            config.lsp_auto.clear();
            let Some(languages) = value.as_array() else {
                problems.push(t!("config-not-array", name = name));
                return;
            };
            for language in languages {
                let Some(language) = language.as_str() else {
                    problems.push(t!("config-item-not-string", name = name));
                    continue;
                };
                if let Some(id) = language_id(language, name, problems)
                    && !config.lsp_auto.contains(&id)
                {
                    config.lsp_auto.push(id);
                }
            }
        },
        write: |config| {
            (!config.lsp_auto.is_empty())
                .then(|| Json::Array(config.lsp_auto.iter().cloned().map(Json::String).collect()))
        },
    },
    Field {
        key: "servers",
        read: |config, value, name, problems| {
            config.lsp_servers.clear();
            let Some(servers) = value.as_object() else {
                problems.push(t!("config-not-object", name = name));
                return;
            };
            for (language, command) in servers {
                let Some(args) = command.as_array() else {
                    problems.push(t!("config-not-array", name = format!("{name}.{language}")));
                    continue;
                };
                let Some(args) = args
                    .iter()
                    .map(|arg| {
                        arg.as_str()
                            .filter(|arg| !arg.is_empty())
                            .map(str::to_owned)
                    })
                    .collect::<Option<Vec<_>>>()
                    .filter(|args| !args.is_empty())
                else {
                    problems.push(t!(
                        "config-item-not-string",
                        name = format!("{name}.{language}")
                    ));
                    continue;
                };
                let id = language.trim().to_lowercase();
                if config.lsp_servers.insert(id, args).is_some() {
                    problems.push(t!(
                        "config-duplicate-language",
                        name = name,
                        language = language
                    ));
                }
            }
        },
        write: |config| {
            (!config.lsp_servers.is_empty()).then(|| {
                Json::object(config.lsp_servers.iter().map(|(language, args)| {
                    (
                        language.clone(),
                        Json::Array(args.iter().cloned().map(Json::String).collect()),
                    )
                }))
            })
        },
    },
    Field {
        key: "serverEntries",
        read: |config, value, name, problems| {
            config.lsp_server_entries = read_lsp_entries(value, name, problems);
        },
        write: |config| lsp_entries_json(&config.lsp_server_entries),
    },
    bool_field!("detect", lsp_detect),
    Field {
        key: "detectDirs",
        read: |config, value, name, problems| {
            config.lsp_detect_dirs.clear();
            let Some(dirs) = value.as_array() else {
                problems.push(t!("config-not-array", name = name));
                return;
            };
            for dir in dirs {
                match dir.as_str().map(str::trim) {
                    Some("") => continue,
                    Some(dir) => config.lsp_detect_dirs.push(dir.to_owned()),
                    None => problems.push(t!("config-item-not-non-empty-string", name = name)),
                }
            }
        },
        write: |config| {
            (!config.lsp_detect_dirs.is_empty()).then(|| {
                Json::Array(
                    config
                        .lsp_detect_dirs
                        .iter()
                        .cloned()
                        .map(Json::String)
                        .collect(),
                )
            })
        },
    },
    Field {
        key: "detectNames",
        read: |config, value, name, problems| {
            config.lsp_detect_names.clear();
            read_string_arrays(value, name, &mut config.lsp_detect_names, problems);
        },
        write: |config| string_arrays_json(&config.lsp_detect_names),
    },
];
