import { describe, it, expect } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, h, onUnmounted } from 'vue'
import { getActivePinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { refCurrencies, stubReferenceInvoke } from './helpers/reference-stubs'
import { invokeHandler } from './factories'
import { messageApi } from './helpers/message-mock'
import { useMessage } from 'naive-ui'

// —— 跨测状态捕获（清理四件套的跨测断言用） ——
let probePinia: object | null = null
let probeUnmounted = false
let probeMessageCalls = -1

const Probe = defineComponent({
  name: 'SeamProbe',
  setup() {
    onUnmounted(() => {
      probeUnmounted = true
    })
    return () => h('div', { class: 'seam-probe' }, 'probe')
  },
})

describe('invoke 测试接缝（issue #746，ADR-0085）', () => {
  describe('未命中报错基座（全局壳层）', () => {
    it('未接线命令按命令名拒绝，报「unexpected invoke: <命令名>」', async () => {
      await expect(mockInvoke('no_such_cmd')).rejects.toThrow('unexpected invoke: no_such_cmd')
      await expect(invoke('another_cmd')).rejects.toThrow('unexpected invoke: another_cmd')
    })
  })

  describe('wireInvokeSeam：组装接线一体', () => {
    it('求值序：overrides 表 → defaults 表 → 参考数据兜底 → 未命中 reject', async () => {
      wireInvokeSeam({
        // 求值序钉定用例：defaults 表故意枚举参考命令以钉住「defaults 压过参考
        // 兜底」的优先级语义——ADR-0085 决策 5 的「defaults 不得重复枚举参考命令」
        // 禁的是常规布线回潮，本用例是接缝自身的符合性测试（豁免先例）。
        defaults: { get_static: 'from-defaults', list_currencies: 'defaults-win' },
        overrides: {
          get_static: 'from-overrides',
          get_dynamic: (args) => ({ echo: args }),
          get_reject: () => Promise.reject(new Error('一次失败')),
        },
      })
      await expect(mockInvoke('get_static')).resolves.toBe('from-overrides')
      await expect(mockInvoke('get_dynamic', { k: 1 })).resolves.toEqual({ echo: { k: 1 } })
      // 函数型 override 的非 thenable 返回值包装为 resolved promise
      await expect(mockInvoke('get_dynamic', undefined)).resolves.toEqual({ echo: undefined })
      await expect(mockInvoke('get_reject')).rejects.toThrow('一次失败')
      // defaults 表命中（静态值）
      await expect(mockInvoke('list_currencies')).resolves.toBe('defaults-win')
      // 未命中走参考数据兜底
      await expect(mockInvoke('list_merchants')).resolves.toEqual(expect.any(Array))
      // 全层未命中 → 报错基座
      await expect(mockInvoke('nope_cmd')).rejects.toThrow('unexpected invoke: nope_cmd')
    })

    it('仅 defaults 表：未命中命令由参考数据兜底应答', async () => {
      wireInvokeSeam({ defaults: { domain_cmd: { rows: [] } } })
      await expect(mockInvoke('domain_cmd')).resolves.toEqual({ rows: [] })
      await expect(mockInvoke('list_currencies')).resolves.toEqual(refCurrencies)
    })

    it('无参调用：defaults/overrides 全缺省时仍接线，未命中即报错', async () => {
      wireInvokeSeam()
      await expect(mockInvoke('list_accounts')).resolves.toEqual(expect.any(Array))
      await expect(mockInvoke('other_cmd')).rejects.toThrow('unexpected invoke: other_cmd')
    })

    it('返回分发器：mockImplementationOnce 处理完一次性命令后委托回接缝', async () => {
      const base = wireInvokeSeam({ defaults: { list_things: ['a', 'b'] } })
      expect(typeof base).toBe('function')
      mockInvoke.mockImplementationOnce(
        (cmd, args) => (cmd === 'create_thing' ? Promise.resolve('new-id') : base(cmd, args)) as Promise<unknown>,
      )
      await expect(mockInvoke('create_thing')).resolves.toBe('new-id')
      await expect(mockInvoke('list_things')).resolves.toEqual(['a', 'b'])
      // Once 消耗完后自动回到接缝布线
      await expect(mockInvoke('create_thing')).rejects.toThrow('unexpected invoke: create_thing')
    })

    it('调用事实断言：接线后 mock.calls 可观察命令与 args', async () => {
      wireInvokeSeam({ defaults: { get_static: 'v' } })
      await mockInvoke('get_static', { q: 7 })
      expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'get_static')).toBe(true)
      expect(mockInvoke.mock.calls[0]?.[1]).toEqual({ q: 7 })
    })

    it('store 层预热默认关：不发放 ready 信号', () => {
      const base = wireInvokeSeam()
      expect(base.ready).toBeUndefined()
    })

    it('store 层预热 opt-in：接线后代做参考 store 预载刷新，ready 就绪', async () => {
      const base = wireInvokeSeam({ refreshReferenceStores: true })
      expect(base.ready).toBeInstanceOf(Promise)
      await base.ready
      const store = useReferenceStore()
      await flushPromises()
      expect(store.status).toBe('ready')
      expect(store.currencies).toEqual(refCurrencies)
    })
  })

  describe('迁移期薄别名（既有使用者不改一字）', () => {
    it('stubReferenceInvoke：接线 + 覆写 + 参考兜底 + 未命中拒绝，语义不变', async () => {
      const base = stubReferenceInvoke({ domain_cmd: 'stubbed' })
      await expect(mockInvoke('domain_cmd')).resolves.toBe('stubbed')
      await expect(mockInvoke('list_categories')).resolves.toEqual(expect.any(Array))
      await expect(mockInvoke('unknown_cmd')).rejects.toThrow('unexpected invoke: unknown_cmd')
      expect(typeof base).toBe('function')
    })

    it('invokeHandler：只组装不接线，函数型 handler 裸返回值原样透传', async () => {
      const handler = invokeHandler(
        { defaults_cmd: 'd' },
        { fn_cmd: () => 'raw', identity_cmd: (args?: Record<string, unknown>) => args },
      )
      // 只组装不接线：组装后 mock 仍处于全局未命中报错基座态
      await expect(mockInvoke('defaults_cmd')).rejects.toThrow('unexpected invoke: defaults_cmd')
      mockInvoke.mockImplementation(handler)
      await expect(mockInvoke('defaults_cmd')).resolves.toBe('d')
      // 裸值透传：返回值是裸字符串而非 Promise 包装
      expect(mockInvoke('fn_cmd')).toBe('raw')
      // 函数型 handler 以零参调用（既有语义）
      expect(mockInvoke('identity_cmd')).toBeUndefined()
      await expect(mockInvoke('list_insurers')).resolves.toEqual(expect.any(Array))
      await expect(mockInvoke('nope')).rejects.toThrow('unexpected invoke: nope')
    })
  })

  describe('清理四件套（全局壳层每测自动执行）', () => {
    it('第一测：布线接缝、写存储、挂组件、记消息调用、捕获 Pinia 实例', async () => {
      wireInvokeSeam({ defaults: { seam_probe_cmd: 'wired' } })
      localStorage.setItem('seam-probe', '1')
      probePinia = getActivePinia() ?? null
      probeUnmounted = false
      useMessage().error('boom')
      probeMessageCalls = messageApi.error.mock.calls.length
      mount(Probe, { attachTo: document.body })
      expect(probeMessageCalls).toBe(1)
      await expect(mockInvoke('seam_probe_cmd')).resolves.toBe('wired')
    })

    it('第二测：上一测的布线/存储/消息/DOM/Pinia 一概不入本测', async () => {
      // 接线不外泄：未命中报错基座已重挂
      await expect(mockInvoke('seam_probe_cmd')).rejects.toThrow('unexpected invoke: seam_probe_cmd')
      // 本地存储清空
      expect(localStorage.getItem('seam-probe')).toBeNull()
      // 消息接口清零（实例稳定，仅调用记录清零）
      expect(messageApi.error.mock.calls.length).toBe(0)
      expect(probeMessageCalls).toBe(1)
      // Pinia 容器重置（新实例）
      expect(probePinia).not.toBeNull()
      expect(getActivePinia()).not.toBe(probePinia)
      // 文档体清空 + 组件卸载
      expect(document.body.querySelector('.seam-probe')).toBeNull()
      expect(document.body.innerHTML).toBe('')
      expect(probeUnmounted).toBe(true)
    })

    it('消息替身稳定实例：同一实例跨 useMessage() 调用与跨测发放', () => {
      expect(useMessage()).toBe(messageApi)
      expect(useMessage()).toBe(useMessage())
      messageApi.success('stable')
      expect(messageApi.success).toHaveBeenCalledTimes(1)
    })
  })
})
