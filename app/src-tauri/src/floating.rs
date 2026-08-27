//! M3 迷你悬浮字幕窗（设计 §9.2）：置顶透明、不可聚焦、跳过任务栏。
//!
//! 通信一律走"后端中继"：主窗 → 后端命令 → 转发到悬浮窗；悬浮窗点击 → 后端
//! 命令 → 转发回主窗。这样 Rust 侧是唯一事实来源（可见/锁定状态），且无需为
//! 主窗增加跨窗口事件的 ACL 权限。

use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use crate::db::MediaDb;
use crate::AppState;

pub const OVERLAY_LABEL: &str = "overlay";

/// 悬浮窗运行时状态（进程内事实来源）
#[derive(Default)]
pub struct OverlayState {
    pub visible: AtomicBool,
    pub locked: AtomicBool,
}

// ---------- 偏好持久化（设计 §3）：settings KV 单键 JSON ----------

pub const OVERLAY_PREFS_KEY: &str = "asplayer.overlay.prefs.v1";

/// 悬浮窗偏好。容器级 #[serde(default)]：JSON 缺字段自动补 Default。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayPrefs {
    /// original | bilingual | translation
    pub display_mode: String,
    /// soft-white | amber | rose | mist-blue | mint | lavender
    pub trans_color: String,
    /// keep-last | fade-5s
    pub gap_behavior: String,
    /// 0.8 ~ 2.0
    pub font_scale: f64,
}

impl Default for OverlayPrefs {
    fn default() -> Self {
        Self {
            display_mode: "bilingual".into(),
            trans_color: "soft-white".into(),
            gap_behavior: "keep-last".into(),
            font_scale: 1.0,
        }
    }
}

/// 从 DB 读偏好；键不存在返回默认，坏 JSON 显式报错
fn load_prefs(db: &MediaDb) -> Result<OverlayPrefs, String> {
    match db.get_setting(OVERLAY_PREFS_KEY) {
        Ok(Some(raw)) => serde_json::from_str(&raw).map_err(|e| format!("解析悬浮窗偏好失败: {e}")),
        Ok(None) => Ok(OverlayPrefs::default()),
        Err(e) => Err(format!("{e}")),
    }
}

#[tauri::command]
pub fn get_overlay_prefs(state: State<'_, AppState>) -> Result<OverlayPrefs, String> {
    let db = state.db.lock().map_err(err_s)?;
    load_prefs(&db)
}

/// 写库成功后向悬浮窗与主窗双向广播新值（两窗 reactive 共享同一事实）
#[tauri::command]
pub fn set_overlay_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    prefs: OverlayPrefs,
) -> Result<(), String> {
    {
        let db = state.db.lock().map_err(err_s)?;
        let raw = serde_json::to_string(&prefs).map_err(err_s)?;
        db.save_setting(OVERLAY_PREFS_KEY, &raw).map_err(err_s)?;
    }
    let _ = app.emit_to(OVERLAY_LABEL, "overlay://prefs-changed", prefs.clone());
    if let Some(main) = primary_label(&app) {
        let _ = app.emit_to(main, "overlay://prefs-changed", prefs);
    }
    Ok(())
}

/// 后端 → 悬浮窗的当前句字幕载荷
#[derive(Clone, serde::Serialize)]
pub struct OverlaySubtitle {
    /// 原文
    pub text: String,
    /// 译文（可为空）
    pub translation: String,
    /// 该句开始时间（毫秒），点击原文跳转用
    pub start_ms: i64,
}

/// 找到主窗标签（除悬浮窗外唯一的业务窗口），避免硬编码 label
fn primary_label(app: &AppHandle) -> Option<String> {
    app.webview_windows()
        .keys()
        .find(|l| l.as_str() != OVERLAY_LABEL)
        .cloned()
}

/// 应用启动时创建隐藏的悬浮窗（内容预加载，首次显示无白屏）
pub fn create_overlay_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        WebviewUrl::App("index.html?window=overlay".into()),
    )
    .title("ASPlayer 悬浮字幕")
    .inner_size(560.0, 148.0)
    .position(60.0, 60.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .resizable(false)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()?;
    // 默认非锁定态：可拖动、不穿透；锁定由全局快捷键/设置面板切换
    let _ = win.set_ignore_cursor_events(false);
    Ok(())
}

// ---------- 后端中继（供其他模块/命令调用的推送函数） ----------

/// 推送当前句字幕到悬浮窗
pub fn push_subtitle(app: &AppHandle, sub: &OverlaySubtitle) {
    let _ = app.emit_to(OVERLAY_LABEL, "overlay://subtitle", sub);
}

/// 通知主窗悬浮窗可见性变化（工具栏图标同步）
fn notify_visibility(app: &AppHandle, visible: bool) {
    if let Some(main) = primary_label(app) {
        let _ = app.emit_to(main, "overlay://visibility", visible);
    }
}

// ---------- 前端命令 ----------

/// 显示/隐藏悬浮窗。显示时若处于锁定态则保持鼠标穿透。
#[tauri::command]
pub fn set_overlay_visible(
    app: AppHandle,
    state: State<'_, OverlayState>,
    visible: bool,
) -> Result<(), String> {
    let win = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "悬浮窗未初始化".to_string())?;
    if visible {
        win.show().map_err(err_s)?;
        // 保证置顶状态在系统休眠/全屏切换后仍生效
        let _ = win.set_always_on_top(true);
    } else {
        win.hide().map_err(err_s)?;
    }
    state.visible.store(visible, Ordering::SeqCst);
    notify_visibility(&app, visible);
    Ok(())
}

/// 工具栏一键切换显隐
#[tauri::command]
pub fn toggle_overlay_visible(app: AppHandle, state: State<'_, OverlayState>) -> Result<(), String> {
    let cur = state.visible.load(Ordering::SeqCst);
    set_overlay_visible(app, state, !cur)
}

#[tauri::command]
pub fn is_overlay_visible(state: State<'_, OverlayState>) -> bool {
    state.visible.load(Ordering::SeqCst)
}

/// 切换鼠标穿透锁定：锁定 = 穿透开；解锁 = 穿透关、悬浮窗最前可点。
#[tauri::command]
pub fn set_overlay_locked(
    app: AppHandle,
    state: State<'_, OverlayState>,
    locked: bool,
) -> Result<(), String> {
    let win = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or_else(|| "悬浮窗未初始化".to_string())?;
    win.set_ignore_cursor_events(locked).map_err(err_s)?;
    state.locked.store(locked, Ordering::SeqCst);
    let _ = app.emit_to(OVERLAY_LABEL, "overlay://lock-changed", locked);
    if let Some(main) = primary_label(&app) {
        let _ = app.emit_to(main, "overlay://lock-changed", locked);
    }
    Ok(())
}

#[tauri::command]
pub fn is_overlay_locked(state: State<'_, OverlayState>) -> bool {
    state.locked.load(Ordering::SeqCst)
}

/// 悬浮窗点击原文 → 请求主窗跳转到该句（毫秒）。暂停与否由主窗决定（保持播放）。
#[tauri::command]
pub fn overlay_request_seek(app: AppHandle, ms: i64) {
    if let Some(main) = primary_label(&app) {
        let _ = app.emit_to(main, "overlay://do-seek", ms);
    }
}

/// 主窗 → 后端中继 → 悬浮窗：推送当前句（歌词式更新）
#[tauri::command]
pub fn push_overlay_subtitle(app: AppHandle, text: String, translation: String, start_ms: i64) {
    push_subtitle(
        &app,
        &OverlaySubtitle {
            text,
            translation,
            start_ms,
        },
    );
}

fn err_s<E: std::fmt::Display>(e: E) -> String {
    format!("{e}")
}

#[cfg(test)]
mod prefs_tests {
    use super::*;

    #[test]
    fn prefs_partial_json_fills_defaults() {
        let p: OverlayPrefs = serde_json::from_str(r#"{"display_mode":"original"}"#).unwrap();
        assert_eq!(p.display_mode, "original");
        assert_eq!(p.trans_color, "soft-white");
        assert_eq!(p.gap_behavior, "keep-last");
        assert_eq!(p.font_scale, 1.0);
    }

    #[test]
    fn prefs_roundtrip_keeps_fields() {
        let p = OverlayPrefs {
            trans_color: "amber".into(),
            font_scale: 1.4,
            ..Default::default()
        };
        let back: OverlayPrefs =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        assert_eq!(back.trans_color, "amber");
        assert_eq!(back.font_scale, 1.4);
    }

    #[test]
    fn prefs_invalid_json_rejected_not_default() {
        // 坏数据必须显式报错而非静默回落（上层决定如何兜底）
        assert!(serde_json::from_str::<OverlayPrefs>("not-json").is_err());
    }
}
