import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import { getSavedRouteName } from "@/utils/view-state";
import { initAppLocale } from "@/i18n";
import { installGlobalErrorHandler } from "@/utils/global-error-handler";

async function bootstrap() {
  const app = createApp(App);
  const pinia = createPinia();
  app.use(pinia);
  // 全局渲染错误兜底（issue #926）：渲染层异常 → 非阻断提示条 + 后端日志落盘；
  // 需在 pinia 安装后、挂载前安装（handler 经 pinia 实例解析错误 store）。
  installGlobalErrorHandler(app, pinia);
  app.use(router);

  // 界面语言：按判定链（手动覆盖 > 系统语言 > zh-CN）在首帧前解析完成，
  // 英文系统用户不闪中文；详见 @/i18n（ADR-0049）。
  await initAppLocale();

  // ViewState：启动时恢复到上次所在视图；非法/缺失回退默认路由（dashboard）。
  const saved = getSavedRouteName();
  if (saved && router.hasRoute(saved)) {
    await router.replace({ name: saved });
  }

  app.mount("#app");
}

bootstrap();
