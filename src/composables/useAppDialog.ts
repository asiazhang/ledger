import { useDialog } from 'naive-ui'
import type { DialogOptions, DialogReactive } from 'naive-ui'
import { createOverlayToken } from '@/composables/overlayRegistry'
import type { OverlayCloseRequest } from '@/composables/overlayRegistry'

/**
 * useAppDialog（ADR-0035）：useDialog 的接线封装，删除确认等命令式对话框
 * 一律经本组合式函数使用——打开即上报弹层注册表，onAfterLeave（离场动画完成、
 * 对话框确定关闭）时撤销，驱动快捷键抑制。选项对象原样透传，调用方自带的
 * onAfterLeave 会在关闭上报后照常回调。
 *
 * 关闭请求通道（issue #845）：token 携带 requestClose，经 DialogReactive 的
 * destroy() 走正常离场路径（onAfterLeave 照常触发）；destroy 前先撤销上报
 * （幂等双保险：离场动画期间注册表已归零，系统返回判定不被动画拖住）。
 */
export function useAppDialog() {
  const dialog = useDialog()

  function track(options: DialogOptions): { options: DialogOptions; setInstance: (instance: DialogReactive) => void } {
    let instance: DialogReactive | null = null
    const requestClose: OverlayCloseRequest = () => {
      instance?.destroy()
      overlay.set(false)
      return true
    }
    const overlay = createOverlayToken('dialog', requestClose)
    overlay.set(true)
    const { onAfterLeave } = options
    return {
      options: {
        ...options,
        onAfterLeave: () => {
          overlay.set(false)
          instance = null
          onAfterLeave?.()
        },
      },
      setInstance: (value: DialogReactive) => {
        instance = value
      },
    }
  }

  const open = (method: 'info' | 'success' | 'warning' | 'error', options: DialogOptions): DialogReactive => {
    const tracked = track(options)
    const instance = dialog[method](tracked.options)
    tracked.setInstance(instance)
    return instance
  }

  return {
    info: (options: DialogOptions) => open('info', options),
    success: (options: DialogOptions) => open('success', options),
    warning: (options: DialogOptions) => open('warning', options),
    error: (options: DialogOptions) => open('error', options),
    destroyAll: () => dialog.destroyAll(),
  }
}
