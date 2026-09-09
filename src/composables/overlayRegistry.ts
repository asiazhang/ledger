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
 *
 * 栈与关闭出口（issue #845 / ADR-0088 决策 7）：openOverlays 是**有序栈**（元素 =
 * 打开序，与视觉 z 序同构——后开者在顶层）。token 可携带 requestClose 关闭请求
 * 回调（由封装组件提供：受控用法中继调用方监听器、非受控用法落影子态、对话框
 * 走 destroy），系统返回桥接据此实现「关最上层弹层」——与 ESC 同构（ESC 由
 * naive-ui 内置 closeOnEsc 承担，系统返回没有内置机构，此出口是它的等价物）。
 * 无通道的 token 只参与「有弹层」判定，closeTopOverlay 对其返回 false。
 */

interface OverlayToken {
  /** 弹层族名（modal/select/date-picker/dropdown/popconfirm/dialog），调试用 */
  readonly name: string
  open: boolean
  /** 关闭请求回调：尝试以与显式关闭通道等价的方式关掉本弹层；返回是否成功受理 */
  readonly requestClose: (() => boolean) | null
}

/** 打开中的弹层栈（打开序 = z 序，后开者在栈顶） */
const openOverlays = reactive<OverlayToken[]>([])

/** 关闭请求回调的签名：封装组件提供，注册表只调用不解释 */
export type OverlayCloseRequest = () => boolean

export interface OverlayTokenHandle {
  readonly name: string
  /** 上报打开/关闭（幂等：与当前状态相同的重复上报不重复计数） */
  set(open: boolean): void
}

/** 创建弹层 token：必须在组件实例作用域调用（每个弹层实例一个，禁止模块级共享） */
export function createOverlayToken(name: string, requestClose?: OverlayCloseRequest): OverlayTokenHandle {
  const token: OverlayToken = { name, open: false, requestClose: requestClose ?? null }
  return {
    name,
    set(next: boolean) {
      if (token.open === next) return
      token.open = next
      if (next) openOverlays.push(token)
      else openOverlays.splice(openOverlays.indexOf(token), 1)
    },
  }
}

/** 任一弹层打开时为 true——两套快捷键（裸键记一笔、Cmd+数字切视图）的公共闸门 */
export function hasOpenOverlay(): boolean {
  return openOverlays.length > 0
}

/** 当前打开的弹层名，按打开序（栈底 → 栈顶；调试/测试辅助） */
export function openOverlayNames(): string[] {
  return openOverlays.map((t) => t.name)
}

/**
 * 关闭最上层弹层（系统返回桥接专用，issue #845）：只调用栈顶的 requestClose，
 * 返回其受理结果；空栈或栈顶无通道返回 false、不动栈。注意：消费方
 * （useSystemBack）以 hasOpenOverlay() 先行判定本次返回键归属弹层层——
 * 栈顶不可关时本次返回被吞掉（不回退路由：弹层开着时路由回退会把状态埋在
 * 弹层下，属数据丢失面），本函数返回值只表达「关闭是否受理」。
 */
export function closeTopOverlay(): boolean {
  const top = openOverlays[openOverlays.length - 1]
  if (!top || !top.requestClose) return false
  return top.requestClose()
}

/** 测试专用：清空注册表（模拟组件整体卸载后的干净状态） */
export function resetOverlays(): void {
  openOverlays.length = 0
}
