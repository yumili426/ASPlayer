import { createApp } from "vue";
import App from "./App.vue";
import "./styles/tokens.css";

// 主题存储 key（带版本，避免旧 localStorage 干扰）
const THEME_KEY = "asplayer-theme-v2";

// 首次运行默认深色（产品主推方向）；之后记住用户切换
let saved: string | null = null;
try {
  saved = localStorage.getItem(THEME_KEY);
} catch {}
document.documentElement.dataset.theme = saved ?? "dark";

createApp(App).mount("#app");

