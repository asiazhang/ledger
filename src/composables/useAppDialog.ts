import { useDialog } from 'naive-ui'
import type { DialogOptions } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'

/**
 * useAppDialog（ADR-0035）：useDialog 的接线封装，删除确认等命令式对话框
 * 一律经本组合式函数使用——打开即上报弹层注册表，onAfterLeave（离场动画完成、
 * 对话框确定关闭）时撤销，驱动快捷键抑制。选项对象原样透传，调用方自带的
 * onAfterLeave 会在关闭上报后照常回调。
 */
export function useAppDialog() {
  const dialog = useDialog()

  function track(options: DialogOptions): DialogOptions {
    const overlay = createOverlayToken('dialog')
    overlay.set(true)
    const { onAfterLeave } = options
    return {
      ...options,
      onAfterLeave: () => {
        overlay.set(false)
        onAfterLeave?.()
      },
    }
  }

  return {
    info: (options: DialogOptions) => dialog.info(track(options)),
    success: (options: DialogOptions) => dialog.success(track(options)),
    warning: (options: DialogOptions) => dialog.warning(track(options)),
    error: (options: DialogOptions) => dialog.error(track(options)),
    destroyAll: () => dialog.destroyAll(),
  }
}
