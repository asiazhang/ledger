export function openUrl(url: string | URL, _openWith?: "inAppBrowser" | string): Promise<void> {
  console.info(`[mock-backend] openUrl 空操作: ${String(url)}`);
  return Promise.resolve();
}
