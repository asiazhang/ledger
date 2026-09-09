import { defineStore } from 'pinia'
import { ref } from 'vue'

/**
 * 渲染错误提示条状态（issue #926）：全局渲染层错误（app.config.errorHandler
 * 兜底）的非阻断可见态。仿 GlobalBusyBar 形态学——非模态环境指示，不接弹层
 * 注册表、不抑制快捷键（ADR-0035 豁免同口径）；持久界面状态单一归宿纪律
 * 不适用：这是会话内瞬态，冷启动清零、零写盘。
 *
 * message 是「当前提示条内容」唯一事实源：null = 不渲染；重复报错覆盖为
 * 最新文案；用户关闭（dismiss）即清除，后续同类错误重新出现（不跨关闭记忆）。
 */
export const useRenderErrorsStore = defineStore('render-errors', () => {
  /** 当前展示的渲染错误摘要文案；null = 无错误提示 */
  const message = ref<string | null>(null)

  function report(text: string): void {
    message.value = text
  }

  function dismiss(): void {
    message.value = null
  }

  return { message, report, dismiss }
})
