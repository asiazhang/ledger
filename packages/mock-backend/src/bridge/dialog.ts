/**
 * 文件对话框的浏览器替身（原 `@tauri-apps/plugin-dialog` 构建期替身，
 * ADR-0143 决策 5；#2083 切轨后为唯一实现）：系统文件对话框在浏览器下不可达
 * ——替身按调用语境返回确定性假路径（非 null，不触发「用户取消」分支，写入
 * 闭环得以走通）；`message` / `ask` / `confirm` 无生产消费面（模态确认已应用
 * 内化，ADR-0078），不提供。
 */

/** 文件类型过滤器（与原插件契约同形）。 */
export interface DialogFilter {
  name?: string;
  extensions?: string[];
}

export interface OpenDialogOptions {
  title?: string;
  defaultPath?: string;
  filters?: DialogFilter[];
  multiple?: boolean;
  directory?: boolean;
}

export interface SaveDialogOptions {
  title?: string;
  defaultPath?: string;
  filters?: DialogFilter[];
}

/** 假路径根：一眼可辨来自替身，避免误当真实文件路径排错。 */
const FAKE_DIR = "/mock/selected";

/** 从对话框选项推导假文件名：有 defaultPath 取其名，有过滤器取首个扩展名。 */
function fakeFileName(options: (OpenDialogOptions | SaveDialogOptions) | undefined): string {
  const defaultPath = options?.defaultPath;
  if (defaultPath) return defaultPath.split("/").pop() ?? "file";
  const ext = options?.filters?.[0]?.extensions?.[0];
  return ext ? `file.${ext}` : "file";
}

export function open<T extends OpenDialogOptions>(options?: T): Promise<string | string[] | null> {
  // multiple 场景返回单元素数组，形态与原插件契约同构
  const path = `${FAKE_DIR}/${fakeFileName(options)}`;
  return Promise.resolve(options?.multiple ? [path] : path);
}

export function save(options?: SaveDialogOptions): Promise<string | null> {
  return Promise.resolve(`${FAKE_DIR}/${fakeFileName(options)}`);
}
