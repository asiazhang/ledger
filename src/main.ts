import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { restoreLastView, router } from "./router";
import { initAppLocale } from "@ledger/i18n";
import { installGlobalErrorHandler } from "@/composables/global-error-handler";

async function bootstrap() {
  // 浏览器开发模式（issue #1962 / ADR-0143 决策 4）：mock mode 下安装替身后端——
  // getBootStatus 答「已解锁」、tauri API 面由构建期模块替身接手（vite alias，
  // 见 vite.config.ts mockModeAliases）。生产构建 MODE 恒非 mock，本分支恒假、
  // 动态 import 被静态消除，替身不进生产包（接线守门：src/__tests__/mock-mode-wiring.test.ts）。
  if (import.meta.env.MODE === "mock") {
    const { installMockBackend } = await import("@ledger/mock-backend");
    installMockBackend();
  }

  const app = createApp(App);
  const pinia = createPinia();
  app.use(pinia);
  // 全局渲染错误兜底（issue #926）：渲染层异常 → 非阻断提示条 + 后端日志落盘；
  // 需在 pinia 安装后、挂载前安装（handler 经 pinia 实例解析错误 store）。
  installGlobalErrorHandler(app, pinia);
  app.use(router);

  // 界面语言：按判定链（手动覆盖 > 系统语言 > zh-CN）在首帧前解析完成，
  // 英文系统用户不闪中文；详见 @ledger/i18n（ADR-0049）。
  await initAppLocale();

  // ViewState：启动时恢复到上次所在视图；非法 / 缺失 / 已关闭功能回退默认路由（dashboard）。
  await restoreLastView(router);

  app.mount("#app");
}

bootstrap();
