/**
 * 应用桥的浏览器替身（原 `@tauri-apps/api/app` 构建期替身，issue #1962 /
 * ADR-0143 决策 5；#2083 切轨后为唯一实现）：生产消费面唯一是
 * `onBackButtonPress`（系统返回桥接，useSystemBack，仅移动档注册）。替身注册
 * 即成功、unregister 为 no-op——浏览器开发模式不派发返回键，无处理器状态
 * 可持（捕获 handler 属测试替身的职责，不在此重现）。
 */

interface BackPressPayload {
  canGoBack: boolean;
}

interface MockPluginListener {
  unregister(): Promise<void>;
}

export function onBackButtonPress(
  _handler: (payload: BackPressPayload) => void,
): Promise<MockPluginListener> {
  return Promise.resolve({ unregister: () => Promise.resolve() });
}
