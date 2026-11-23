//! 系统的深浅色外观（macOS 以外的 Unix 平台）。
//!
//! `winit` 在 X11、Wayland 上取不到系统外观：`Window::theme` 在 X11 上总是 `None`，在 Wayland
//! 上只返回应用固定的主题，两者都不发出 `ThemeChanged`。本模块经 XDG 桌面门户
//! （xdg-desktop-portal）的 `org.freedesktop.portal.Settings` 接口读取
//! `org.freedesktop.appearance` 的 `color-scheme`，并在后台线程上等待其 `SettingChanged` 信号。
//!
//! 门户的取值为 0（没有偏好）、1（偏好深色）、2（偏好浅色）。GNOME 的默认外观（浅色）报告的是 0，
//! 因此只有 1 算深色，与 libadwaita 的处理一致。

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use winit::window::Theme;
use zbus::blocking::proxy::SignalIterator;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

/// 外观设置所在的命名空间。
const NAMESPACE: &str = "org.freedesktop.appearance";
/// 外观的设置项。
const KEY: &str = "color-scheme";

/// [`current`] 等待首次读取的最长时间。超时后按取不到处理，之后读到时经 [`watch`] 的回调切换。
const FIRST_READ: Duration = Duration::from_millis(100);

/// 门户的回答：还没读到时为 `None`；读到后为 `Some(外观)`，门户不可用、没有该设置或
/// [`current`] 等不到时外观为 `None`。
static ANSWER: Mutex<Option<Option<Theme>>> = Mutex::new(None);
/// 首次读到时唤醒等待中的 [`current`]。
static ANSWERED: Condvar = Condvar::new();

/// 在后台读取系统的外观并等待它变化；每次读到外观都以该外观调用 `changed`。
///
/// 首次读到时同样调用，建窗口时没等到（见 [`FIRST_READ`]）的窗口据此切换。进程中只调用一次，
/// 且在建窗口之前调用。
pub fn watch(changed: impl Fn(Theme) + Send + 'static) {
    std::thread::spawn(move || {
        let Ok((settings, changes)) = subscribe() else {
            publish(None);
            return;
        };
        let theme = read(&settings);
        publish(theme);
        if let Some(theme) = theme {
            changed(theme);
        }
        for message in changes {
            let body = message.body();
            let Ok((_, _, value)) = body.deserialize::<(&str, &str, Value<'_>)>() else {
                continue;
            };
            if let Some(theme) = theme_of(&value) {
                publish(Some(theme));
                changed(theme);
            }
        }
    });
}

/// 系统当前的外观；门户不可用、没有该设置或等不到时返回 `None`。
///
/// 还没读到时最多等待 [`FIRST_READ`]，首个窗口在显示之前即可按系统的外观绘制。等不到时记为取
/// 不到，之后的调用不再等待。
pub fn current() -> Option<Theme> {
    let answer = ANSWER.lock().ok()?;
    let (mut answer, _) = ANSWERED
        .wait_timeout_while(answer, FIRST_READ, |answer| answer.is_none())
        .ok()?;
    *answer.get_or_insert(None)
}

/// 记下门户的回答，唤醒等待中的 [`current`]。
fn publish(theme: Option<Theme>) {
    if let Ok(mut answer) = ANSWER.lock() {
        *answer = Some(theme);
    }
    ANSWERED.notify_all();
}

/// 连接会话总线，订阅外观的变化。
///
/// 先订阅再读取，两者之间的变化不会漏掉；门户尚未启动时同样可以订阅，读取时由总线启动门户。
fn subscribe() -> zbus::Result<(Proxy<'static>, SignalIterator<'static>)> {
    let connection = Connection::session()?;
    let settings = Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )?;
    let changes =
        settings.receive_signal_with_args("SettingChanged", &[(0, NAMESPACE), (1, KEY)])?;
    Ok((settings, changes))
}

/// 读取门户当前的外观；门户不可用或没有该设置时返回 `None`。
fn read(settings: &Proxy<'_>) -> Option<Theme> {
    // 使用 `Read` 而不是第 2 版接口加入的 `ReadOne`：Debian 12 等发行版的门户还没有 `ReadOne`。
    // `Read` 的结果比 `ReadOne` 多套一层变体，由 `theme_of` 拆开。
    let value: OwnedValue = settings.call("Read", &(NAMESPACE, KEY)).ok()?;
    theme_of(&value)
}

/// `color-scheme` 的取值 `value`（可以套着变体）对应的外观；不是 `u32` 时返回 `None`。
fn theme_of(value: &Value<'_>) -> Option<Theme> {
    match value {
        Value::Value(inner) => theme_of(inner),
        Value::U32(1) => Some(Theme::Dark),
        Value::U32(_) => Some(Theme::Light),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
