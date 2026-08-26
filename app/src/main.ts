import { createApp } from "vue";
import App from "./App.vue";
import "./styles/tokens.css";

// 主题：默认深色（产品主推方向），用户可切换并记忆
const saved = localStorage.getItem("theme");
document.documentElement.dataset.theme = saved ?? "dark";

createApp(App).mount("#app");

