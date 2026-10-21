/**
 * `@tauri-apps/api/window` 构建期模块替身（issue #1962 / ADR-0143 决策 5）：
 * 浏览器下没有窗口句柄——生产消费面只有 `getCurrentWindow().setTitle`（窗口
 * 标题随界面语言，App.vue）与 `.destroy`（系统返回栈底交还，useSystemBack，
 * 桌面档不注册）。两个消费点都以 no-op promise 应答，非 Tauri 环境的静默
 * 容忍语义（try/catch 或 .catch）由调用方既有代码承担。
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
