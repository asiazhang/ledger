import "./assets/global.css";
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { restoreLastView, router } from "./router";
import { initAppLocale } from "@ledger/i18n";
import { installGlobalErrorHandler } from "@/composables/global-error-handler";

async function bootstrap() {
  if (import.meta.env.MODE === "mock") {
    const { installMockBackend } = await import("@ledger/mock-backend");
    installMockBackend();
  }

  const app = createApp(App);
  const pinia = createPinia();
  app.use(pinia);
  installGlobalErrorHandler(app, pinia);
  app.use(router);

  await initAppLocale();

  await restoreLastView(router);

  app.mount("#app");
}

bootstrap();
