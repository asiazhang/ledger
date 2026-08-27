import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import { getSavedRouteName } from "@/utils/view-state";

async function bootstrap() {
  const app = createApp(App);
  app.use(createPinia());
  app.use(router);

  // ViewState：启动时恢复到上次所在视图；非法/缺失回退默认路由（dashboard）。
  const saved = getSavedRouteName();
  if (saved && router.hasRoute(saved)) {
    await router.replace({ name: saved });
  }

  app.mount("#app");
}

bootstrap();
