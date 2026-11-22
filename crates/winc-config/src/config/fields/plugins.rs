//! `plugins` 节：清单路径、启用的插件、信任指纹和设置值的读写。

use std::collections::BTreeMap;

use winc_i18n::t;

use super::*;

/// 插件设置节。
pub const PLUGINS: &[Field] = &[
    Field {
        key: "paths",
        read: |config, value, name, problems| {
            config.plugin_paths.clear();
            read_string_list(value, name, &mut config.plugin_paths, problems);
        },
        write: |config| string_list_json(&config.plugin_paths),
    },
    Field {
        key: "enabled",
        read: |config, value, name, problems| {
            config.plugin_enabled.clear();
            read_string_list(value, name, &mut config.plugin_enabled, problems);
        },
        write: |config| string_list_json(&config.plugin_enabled),
    },
    Field {
        key: "trusted",
        read: |config, value, name, problems| {
            config.plugin_trusted.clear();
            read_string_list(value, name, &mut config.plugin_trusted, problems);
        },
        write: |config| string_list_json(&config.plugin_trusted),
    },
    Field {
        key: "values",
        read: |config, value, name, problems| {
            config.plugin_values.clear();
            read_string_maps(value, name, &mut config.plugin_values, problems);
        },
        write: |config| string_maps_json(&config.plugin_values),
    },
    Field {
        key: "grants",
        read: |config, value, name, problems| {
            config.plugin_grants.clear();
            read_string_lists_map(value, name, &mut config.plugin_grants, problems);
        },
        write: |config| string_lists_map_json(&config.plugin_grants),
    },
];

/// 读入一个字符串数组，空字符串和空白项忽略。
fn read_string_list(value: &Json, name: &str, out: &mut Vec<String>, problems: &mut Vec<String>) {
    let Some(items) = value.as_array() else {
        problems.push(t!("config-not-array", name = name));
        return;
    };
    for item in items {
        match item.as_str().map(str::trim) {
            Some("") | None => problems.push(t!("config-item-not-non-empty-string", name = name)),
            Some(text) => out.push(text.to_owned()),
        }
    }
}

/// 写出一个字符串数组；空数组不写。
fn string_list_json(values: &[String]) -> Option<Json> {
    (!values.is_empty()).then(|| Json::Array(values.iter().cloned().map(Json::String).collect()))
}

/// 读入「插件 id → 字符串数组」的表。
fn read_string_lists_map(
    value: &Json,
    name: &str,
    out: &mut BTreeMap<String, Vec<String>>,
    problems: &mut Vec<String>,
) {
    let Some(map) = value.as_object() else {
        problems.push(t!("config-not-object", name = name));
        return;
    };
    for (id, values) in map {
        let mut list = Vec::new();
        read_string_list(values, &format!("{name}.{id}"), &mut list, problems);
        if !list.is_empty() {
            out.insert(id.clone(), list);
        }
    }
}

/// 写出「插件 id → 字符串数组」的表。
fn string_lists_map_json(values: &BTreeMap<String, Vec<String>>) -> Option<Json> {
    if values.is_empty() {
        return None;
    }
    let mut map = BTreeMap::new();
    for (id, list) in values {
        map.insert(
            id.clone(),
            Json::Array(list.iter().cloned().map(Json::String).collect()),
        );
    }
    Some(Json::Object(map))
}

/// 读入「插件 id → 键 → 值」的字符串表。
fn read_string_maps(
    value: &Json,
    name: &str,
    out: &mut BTreeMap<String, BTreeMap<String, String>>,
    problems: &mut Vec<String>,
) {
    let Some(map) = value.as_object() else {
        problems.push(t!("config-not-object", name = name));
        return;
    };
    for (id, values) in map {
        let Some(values) = values.as_object() else {
            problems.push(t!("config-not-object", name = format!("{name}.{id}")));
            continue;
        };
        let mut entry = BTreeMap::new();
        for (key, value) in values {
            match value.as_str() {
                Some(value) => {
                    entry.insert(key.clone(), value.to_owned());
                }
                None => problems.push(t!("config-not-string", name = format!("{name}.{id}.{key}"))),
            }
        }
        if !entry.is_empty() {
            out.insert(id.clone(), entry);
        }
    }
}

/// 写出「插件 id → 键 → 值」的字符串表；空表不写。
fn string_maps_json(map: &BTreeMap<String, BTreeMap<String, String>>) -> Option<Json> {
    (!map.is_empty()).then(|| {
        Json::object(map.iter().map(|(id, values)| {
            (
                id.as_str(),
                Json::object(
                    values
                        .iter()
                        .map(|(key, value)| (key.as_str(), Json::String(value.clone()))),
                ),
            )
        }))
    })
}
