/**
 * 媒体查询测试接缝：可编程假 matchMedia（issue #841，ADR-0088 决策 11 票①）。
 *
 * jsdom 无 matchMedia，既有全局桩对所有查询一律应答 false（静态）；本接缝在
 * 既有测试支持工厂体系（helpers/*-mock.ts + setup.ts 全局壳层）下提供**可编程**
 * 假 matchMedia：测试以 setFakeMedia 设定 hover / pointer / 视口宽度应答从而
 * 换档，重编程时按 MediaQueryList 语义向 matches 翻转的查询派发 change 事件。
 * 全部组件 / composable 测试经它换档，不允许第二套换档机制。
 *
 * 求值语义：接缝只认识 min-width / max-width / hover / pointer 四类特征（含
 * not 前缀与 and 组合），不认识的查询一律应答 false——与既有全局桩行为一致，
 * 外来消费者（组件库内部等）不受影响；断言不落在这类查询上。
 */

/** 假 matchMedia 的可编程状态。 */
export interface FakeMediaState {
  /** 视口宽度（px）：窗口分级断点判定（宽度轴信号）。 */
  width: number
  /** hover 能力：'hover' 可悬停 / 'none' 不可（输入轴信号一）。 */
  hover: 'hover' | 'none'
  /** 主指针精度：'fine' 精细 / 'coarse' 粗糙（输入轴信号二）。 */
  pointer: 'fine' | 'coarse'
}

/** 默认状态：桌面指针环境（宽视口 + 可悬停 + 精细主指针），既有桌面档测试语义零迁移。 */
export const DEFAULT_MEDIA_STATE: FakeMediaState = {
  width: 1280,
  hover: 'hover',
  pointer: 'fine',
}

type MediaListener = (event: MediaQueryListEvent) => void

/** 注册表项：发放时记录查询与可变命中态，复位时随注册表整体清空。 */
interface RegisteredMql {
  media: string
  matches: boolean
  onchange: MediaListener | null
  listeners: Set<MediaListener>
}

let state: FakeMediaState = { ...DEFAULT_MEDIA_STATE }
let registry: RegisteredMql[] = []

/** 单特征求值：true / false / undefined（本接缝不认识的特征）。 */
function evaluateFeature(feature: string, s: FakeMediaState): boolean | undefined {
  const m = /^\(\s*([\w-]+)\s*:\s*([^)]+?)\s*\)$/.exec(feature)
  if (!m) return undefined
  const name = m[1]
  const value = m[2]
  switch (name) {
    case 'min-width':
      return s.width >= Number.parseFloat(value)
    case 'max-width':
      return s.width <= Number.parseFloat(value)
    case 'hover':
      return value === s.hover
    case 'pointer':
      return value === s.pointer
    default:
      return undefined
  }
}

/** 查询求值：not 前缀取反（内层未知仍为未知），and 组合一假即假、全真才真。 */
function evaluateQuery(query: string, s: FakeMediaState): boolean | undefined {
  const q = query.trim()
  if (q.startsWith('not ')) {
    const inner = evaluateQuery(q.slice(4), s)
    return inner === undefined ? undefined : !inner
  }
  let known = false
  for (const part of q.split(/\s+and\s+/)) {
    const result = evaluateFeature(part.trim(), s)
    if (result === false) return false
    if (result === true) known = true
  }
  return known
}

/** DOM 回调形态（函数或 { handleEvent }）归一为函数监听。 */
function toListener(listener: EventListenerOrEventListenerObject | null): MediaListener | null {
  if (!listener) return null
  if (typeof listener === 'function') return listener
  return (e: MediaQueryListEvent) => listener.handleEvent(e)
}

/**
 * 假 matchMedia 本体：setup.ts 以 window.matchMedia 挂载，产品代码经公开面消费。
 * matches / onchange 经 getter 读写注册表可变状态（DOM 类型只读，翻转经注册表写入）；
 * 此后由 setFakeMedia 重编程并派发 change。
 */
export function fakeMatchMedia(query: string): MediaQueryList {
  const listeners = new Set<MediaListener>()
  const entry: RegisteredMql = {
    media: query,
    matches: evaluateQuery(query, state) === true,
    onchange: null,
    listeners,
  }
  const mql = {
    get matches() {
      return entry.matches
    },
    media: query,
    get onchange() {
      return entry.onchange
    },
    set onchange(fn: MediaListener | null) {
      entry.onchange = fn
    },
    addEventListener: (_type: string, listener: EventListenerOrEventListenerObject | null) => {
      const fn = toListener(listener)
      if (fn) listeners.add(fn)
    },
    removeEventListener: (_type: string, listener: EventListenerOrEventListenerObject | null) => {
      const fn = toListener(listener)
      if (fn) listeners.delete(fn)
    },
    addListener: (listener: MediaListener) => {
      listeners.add(listener)
    },
    removeListener: (listener: MediaListener) => {
      listeners.delete(listener)
    },
    dispatchEvent: () => false,
  }
  registry.push(entry)
  return mql as unknown as MediaQueryList
}

/**
 * 可编程换档唯一入口：合并状态补丁，重求值全部已发放查询；matches 翻转者
 * 同步翻转并派发 change（未翻转不派发，浏览器同款语义）。
 */
export function setFakeMedia(patch: Partial<FakeMediaState>): void {
  state = { ...state, ...patch }
  for (const entry of registry) {
    const next = evaluateQuery(entry.media, state) === true
    if (next === entry.matches) continue
    entry.matches = next
    const event = { matches: next, media: entry.media } as MediaQueryListEvent
    if (entry.onchange) entry.onchange(event)
    for (const listener of entry.listeners) listener(event)
  }
}

/** 复位：状态回默认、注册表与监听清空（全局壳层每测调用，语义同清理四件套）。 */
export function resetFakeMedia(): void {
  state = { ...DEFAULT_MEDIA_STATE }
  registry = []
}
