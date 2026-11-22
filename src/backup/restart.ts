import { api } from "@ledger/api";

export function restartAppShortly(): void {
  setTimeout(() => {
    api
      .restartApp()
      .then(() => {
        window.location.reload();
      })
      .catch((e) => {
        console.warn("应用重启（原位重引导）失败，已保留当前界面", e);
      });
  }, 800);
}
