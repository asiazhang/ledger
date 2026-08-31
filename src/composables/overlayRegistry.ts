import { reactive } from 'vue'

/**
 * 弹层注册表（ADR-0035）：「弹层是否打开」是显式声明的应用状态，不做 DOM 推断。
 *
 * 取代旧的 OVERLAY_SELECTORS class 名嗅探——嗅探对 naive-ui 的渲染策略做了
 * 「仅打开时存在」的错误假设，已两次因「关闭后残留隐藏空壳」（.n-modal-container、
 * .n-base-select-menu）导致快捷键永久静默失效（ADR-0021 修过前者，后者即本次）。
 *
 * 机制：每类弹层封装组件（AppModal / AppSelect / AppDatePicker / AppDropdown /
 * AppPopconfirm / useAppDialog）在实例作用域持有一个 token，随自身 show 状态
 * 上报开/关；快捷键闸门只读注册表。失效模式反转：未来漏接线（直接用裸组件）
 * 的症状是「弹层开着时快捷键误触发」——当场可见可修，不再静默永久失效。
 */

interface OverlayToken {
  /** 弹层族名（modal/select/date-picker/dropdown/popconfirm/dialog），调试用 */
  readonly name: string
  open: boolean
}

const openOverlays = reactive(new Set<OverlayToken>())

export interface OverlayTokenHandle {
  readonly name: string
  /** 上报打开/关闭（幂等：与当前状态相同的重复上报不重复计数） */
  set(open: boolean): void
}

/** 创建弹层 token：必须在组件实例作用域调用（每个弹层实例一个，禁止模块级共享） */
export function createOverlayToken(name: string): OverlayTokenHandle {
  const token: OverlayToken = { name, open: false }
  return {
    name,
    set(next: boolean) {
      if (token.open === next) return
      token.open = next
      if (next) openOverlays.add(token)
      else openOverlays.delete(token)
    },
  }
}

/** 任一弹层打开时为 true——两套快捷键（裸键记一笔、Cmd+数字切视图）的公共闸门 */
export function hasOpenOverlay(): boolean {
  return openOverlays.size > 0
}

/** 当前打开的弹层名（调试/测试辅助） */
export function openOverlayNames(): string[] {
  return [...openOverlays].map((t) => t.name)
}

/** 测试专用：清空注册表（模拟组件整体卸载后的干净状态） */
export function resetOverlays(): void {
  openOverlays.clear()
}
