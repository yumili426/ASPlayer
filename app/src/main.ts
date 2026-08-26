import { createApp } from "vue";
import App from "./App.vue";
import "./styles/tokens.css";

// 主题：跟随系统，可被 localStorage 覆盖
const saved = localStorage.getItem("theme");
const prefersLight = window.matchMedia("(prefers-color-scheme: light)").matches;
document.documentElement.dataset.theme =
  saved ?? (prefersLight ? "light" : "dark");

createApp(App).mount("#app");

