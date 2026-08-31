import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import { getSavedRouteName } from "@/utils/view-state";
import { initAppLocale } from "@/i18n";

async function bootstrap() {
  const app = createApp(App);
  app.use(createPinia());
  app.use(router);

  // 界面语言：按判定链（手动覆盖 > 系统语言 > zh-CN）在首帧前解析完成，
  // 英文系统用户不闪中文；详见 @/i18n（ADR-0048）。
  await initAppLocale();

  // ViewState：启动时恢复到上次所在视图；非法/缺失回退默认路由（dashboard）。
  const saved = getSavedRouteName();
  if (saved && router.hasRoute(saved)) {
    await router.replace({ name: saved });
  }

  app.mount("#app");
}

bootstrap();
