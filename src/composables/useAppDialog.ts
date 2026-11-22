import { useDialog } from "naive-ui";
import type { DialogOptions, DialogReactive } from "naive-ui";
import { createOverlayToken } from "@ledger/ui-kit/overlayRegistry";
import type { OverlayCloseRequest } from "@ledger/ui-kit/overlayRegistry";

export function useAppDialog() {
  const dialog = useDialog();

  function open(
    method: "info" | "success" | "warning" | "error",
    options: DialogOptions,
  ): DialogReactive {
    let instance: DialogReactive | null = null;
    const requestClose: OverlayCloseRequest = () => {
      instance?.destroy();
      overlay.set(false);
      return true;
    };
    const overlay = createOverlayToken("dialog", requestClose);
    overlay.set(true);
    const { onAfterLeave } = options;
    instance = dialog[method]({
      ...options,
      onAfterLeave: () => {
        overlay.set(false);
        instance = null;
        onAfterLeave?.();
      },
    });
    return instance;
  }

  return {
    info: (options: DialogOptions) => open("info", options),
    success: (options: DialogOptions) => open("success", options),
    warning: (options: DialogOptions) => open("warning", options),
    error: (options: DialogOptions) => open("error", options),
    destroyAll: () => dialog.destroyAll(),
  };
}
