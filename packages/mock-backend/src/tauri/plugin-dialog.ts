import type { OpenDialogOptions, SaveDialogOptions } from "@tauri-apps/plugin-dialog";

const FAKE_DIR = "/mock/selected";

function fakeFileName(options: (OpenDialogOptions | SaveDialogOptions) | undefined): string {
  const defaultPath = options?.defaultPath;
  if (defaultPath) return defaultPath.split("/").pop() ?? "file";
  const ext = options?.filters?.[0]?.extensions?.[0];
  return ext ? `file.${ext}` : "file";
}

export function open<T extends OpenDialogOptions>(options?: T): Promise<string | string[] | null> {
  const path = `${FAKE_DIR}/${fakeFileName(options)}`;
  return Promise.resolve(options?.multiple ? [path] : path);
}

export function save(options?: SaveDialogOptions): Promise<string | null> {
  return Promise.resolve(`${FAKE_DIR}/${fakeFileName(options)}`);
}
