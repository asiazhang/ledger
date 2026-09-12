import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { messageApi } from '@ledger/test-support/message-mock'
import { useLoadable, registerToastSink } from '@/composables/useLoadable'
import MessageSinkBridge from '@/components/MessageSinkBridge.vue'

beforeEach(() => {
  // 复位为 no-op，确保 sink 生效只能来自桥接注册
  registerToastSink({ error: () => {} })
})

describe('MessageSinkBridge：Loadable toast sink 接线（ADR-0040）', () => {
  it('桥接挂载即把消息提供器的 message API 注册为模块 sink，失败经其弹 toast', async () => {
    mount(MessageSinkBridge)

    const { error, run } = useLoadable(async () => Promise.reject(new Error('桥接失败')))
    await run()

    expect(messageApi.error).toHaveBeenCalledTimes(1)
    expect(messageApi.error).toHaveBeenCalledWith('桥接失败')
    expect(error.value).toBe('桥接失败')
  })
})
