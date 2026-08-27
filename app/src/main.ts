import { createApp } from "vue";
import App from "./App.vue";
import FloatingOverlay from "./windows/FloatingOverlay.vue";
import "./styles/tokens.css";

// 主题存储 key（带版本，避免旧 localStorage 干扰）
const THEME_KEY = "asplayer-theme-v2";

// 首次运行默认深色（产品主推方向）；之后记住用户切换
let saved: string | null = null;
try {
  saved = localStorage.getItem(THEME_KEY);
} catch {}
// 首次运行跟随系统；之后记住用户选择（light / dark / system）
const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");
const resolved =
  saved === "light" || saved === "dark" ? saved : prefersDark.matches ? "dark" : "light";
document.documentElement.dataset.theme = resolved;

// M3：同一前端入口按 URL 参数区分窗口 —— 悬浮字幕窗挂载轻量组件，
// 不加载播放器/媒体库逻辑
const params = new URLSearchParams(window.location.search);
if (params.get("window") === "overlay") {
  // 悬浮窗必须真正透明：覆盖 tokens.css 给 html/body 刷的全局底色
  // （inline 样式优先级最高，防止被任何主题规则盖回不透明）
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
  createApp(FloatingOverlay).mount("#app");
} else {
  createApp(App).mount("#app");
}

