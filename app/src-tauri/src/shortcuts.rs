//! M3 全局快捷键（设计 §8）：游戏内也可操作的系统级热键。
//!
//! MVP 固定默认组合（后续版本再开放自定义重映射）：
//!   Ctrl+Alt+Space   播放/暂停
//!   Ctrl+Alt+O       悬浮字幕窗显示/隐藏
//!   Ctrl+Alt+L       悬浮窗鼠标穿透锁定切换（打游戏常开）
//!   Ctrl+Alt+Left    上一句句首
//!   Ctrl+Alt+Right   下一句句首
//!
//! 实现方式：Rust 侧注册 + handler 直接转发事件给对应窗口，
//! 业务逻辑仍在窗口 JS 中（复用现有播放器/字幕 store）。
//!
//! 注意：组合键一律用 `Shortcut::new(Some(mods), Code)` 显式构造，
//! 不走字符串解析 —— 各平台解析器对大小写/别名支持不一致，
//! 注册失败又只能静默忽略，宁可编译期就把组合写死。

use crate::floating::{self, OVERLAY_LABEL};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcut, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

fn primary_label(app: &AppHandle) -> Option<String> {
    app.webview_windows()
        .keys()
        .find(|l| l.as_str() != OVERLAY_LABEL)
        .cloned()
}

fn ctrl_alt(key: Code) -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), key)
}

fn pressed(e: &ShortcutEvent) -> bool {
    e.state == ShortcutState::Pressed
}

fn reg(
    gs: &GlobalShortcut<tauri::Wry>,
    name: &str,
    scs: &[(String, Shortcut)],
    f: impl Fn(&AppHandle, &ShortcutEvent) + Send + Sync + Clone + 'static,
) {
    // 注册失败绝不能静默：热键被 Intel 核显旋转/网易云/输入法等占用时，
    // 用户会以为功能坏了。按候选列表依次降级（Ctrl+Alt+X → Ctrl+Alt+Shift+X）。
    for (label, sc) in scs {
        let f = f.clone();
        match gs.on_shortcut(*sc, move |app, _s, e| f(app, &e)) {
            Ok(_) => {
                eprintln!("[ASPlayer][shortcuts] 已注册 {label}（{name}）");
                return;
            }
            Err(e) => eprintln!("[ASPlayer][shortcuts] 注册失败 {label}: {e}"),
        }
    }
    eprintln!("[ASPlayer][shortcuts] {name} 全部候选组合均不可用！");
}

fn ctrl_alt_shift(key: Code) -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT), key)
}

/// 注册全部全局快捷键（setup 中调用一次）
pub fn register_all(app: &AppHandle) {
    let gs = app.global_shortcut();

    // 播放/暂停：直接转发到主窗
    reg(
        gs,
        "播放/暂停",
        &[("Ctrl+Alt+Space".to_string(), ctrl_alt(Code::Space)), ("Ctrl+Alt+Shift+Space".to_string(), ctrl_alt_shift(Code::Space))],
        |app, e| {
            if pressed(e) {
                if let Some(main) = primary_label(app) {
                    let _ = app.emit_to(main, "overlay://global-action", "togglePlay");
                }
            }
        },
    );

    // 悬浮窗显隐
    reg(
        gs,
        "悬浮窗显隐",
        &[("Ctrl+Alt+O".to_string(), ctrl_alt(Code::KeyO)), ("Ctrl+Alt+Shift+O".to_string(), ctrl_alt_shift(Code::KeyO))],
        |app, e| {
            if pressed(e) {
                let _ = floating::toggle_overlay_visible(app.clone(), app.state());
            }
        },
    );

    // 穿透锁定切换
    reg(
        gs,
        "悬浮窗锁定",
        &[("Ctrl+Alt+L".to_string(), ctrl_alt(Code::KeyL)), ("Ctrl+Alt+Shift+L".to_string(), ctrl_alt_shift(Code::KeyL))],
        |app, e| {
            if pressed(e) {
                let st = app.state::<floating::OverlayState>();
                let cur = st.locked.load(std::sync::atomic::Ordering::SeqCst);
                match floating::set_overlay_locked(app.clone(), st, !cur) {
                    Ok(_) => eprintln!("[ASPlayer][shortcuts] lock -> {}", !cur),
                    Err(err) => eprintln!("[ASPlayer][shortcuts] set_overlay_locked 出错: {err}"),
                }
            }
        },
    );

    // 上一句 / 下一句：交由主窗依据当前时间计算目标句并 seek
    for (key, delta, dir) in [
        (Code::ArrowLeft, -1i32, "Left"),
        (Code::ArrowRight, 1i32, "Right"),
    ] {
        reg(
            gs,
            dir,
            &[
                (format!("Ctrl+Alt+{dir}"), ctrl_alt(key)),
                (format!("Ctrl+Alt+Shift+{dir}"), ctrl_alt_shift(key)),
            ],
            move |app, e| {
                if pressed(e) {
                    if let Some(main) = primary_label(app) {
                        let _ = app.emit_to(main, "overlay://step-subtitle", delta);
                    }
                }
            },
        );
    }
}

/// 退出前解除全部全局热键占用（on_window_event CloseRequested 时调用更佳；
/// 进程退出时系统也会自动回收，这里只是显式清理）
pub fn unregister_all(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}
