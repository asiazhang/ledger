import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const { fakeMessageApi } = vi.hoisted(() => ({
  fakeMessageApi: { error: vi.fn() },
}))

// 只替换 useMessage：模拟「消息提供器上下文」取到的消息 API
vi.mock('naive-ui', async (importOriginal) => {
  const actual = await importOriginal<typeof import('naive-ui')>()
  return { ...actual, useMessage: () => fakeMessageApi }
})

import { useLoadable, registerToastSink } from '@/composables/useLoadable'
import MessageSinkBridge from '@/components/MessageSinkBridge.vue'

beforeEach(() => {
  fakeMessageApi.error.mockClear()
  // 复位为 no-op，确保 sink 生效只能来自桥接注册
  registerToastSink({ error: () => {} })
})

describe('MessageSinkBridge：Loadable toast sink 接线（ADR-0040）', () => {
  it('桥接挂载即把消息提供器的 message API 注册为模块 sink，失败经其弹 toast', async () => {
    mount(MessageSinkBridge)

    const { error, run } = useLoadable(async () => Promise.reject(new Error('桥接失败')))
    await run()

    expect(fakeMessageApi.error).toHaveBeenCalledTimes(1)
    expect(fakeMessageApi.error).toHaveBeenCalledWith('桥接失败')
    expect(error.value).toBe('桥接失败')
  })
})
