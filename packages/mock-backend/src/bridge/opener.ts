/**
 * 外链打开的浏览器替身（原 `@tauri-apps/plugin-opener` 构建期替身，issue #1962 /
 * ADR-0143 决策 5；#2083 切轨后为唯一实现）：外链打开在浏览器下交给系统本就
 * 可达——但生产消费面（关于页仓库链接、更新发布页）替身为空操作并留 console
 * 痕迹，避免开发时真拉起浏览器打断心流。
 */

export function openUrl(url: string | URL, _openWith?: "inAppBrowser" | string): Promise<void> {
  console.info(`[mock-backend] openUrl 空操作: ${String(url)}`);
  return Promise.resolve();
}
