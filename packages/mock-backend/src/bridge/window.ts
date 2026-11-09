/**
 * 窗口句柄的浏览器替身（原 `@tauri-apps/api/window` 构建期替身，issue #1962 /
 * ADR-0143 决策 5；#2083 切轨后为唯一实现）：浏览器下没有窗口句柄——生产消费
 * 面只有 `getCurrentWindow().setTitle`（窗口标题随界面语言，App.vue）与
 * `.destroy`（系统返回栈底交还，useSystemBack，桌面档不注册）。两个消费点都
 * 以 no-op promise 应答，静默容忍语义（try/catch 或 .catch）由调用方既有代码承担。
 */

interface MockWindow {
  setTitle(title: string): Promise<void>;
  destroy(): Promise<void>;
}

export function getCurrentWindow(): MockWindow {
  return {
    setTitle: (_title: string) => Promise.resolve(),
    destroy: () => Promise.resolve(),
  };
}
