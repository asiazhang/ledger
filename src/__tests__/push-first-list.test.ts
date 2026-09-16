import { describe, it, expect, beforeEach } from 'vitest'
import {
  captureListenHandlers,
  mockListen,
  type CapturedListener,
} from '@ledger/test-support/listen-mock'
import { flushPromises } from '@vue/test-utils'
import { ref } from 'vue'
import { createPushFirstList } from '@/composables/push-first-list'

/** 手动完结的 load 替身：测试按用例节奏 resolve / reject。 */
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

/** 捕获 ledger:changed 监听处理器（工厂创建时注册） */
let handlers: CapturedListener[]

beforeEach(() => {
  handlers = captureListenHandlers()
})

describe('createPushFirstList（push-first 清单生命周期工厂，ADR-0123）', () => {
  it('self-init：创建即同步置 loading 并发起一次加载，成功后 apply 落位、version=1、status=ready', async () => {
    const first = deferred<string[]>()
    const sink = ref<string[]>([])
    let loadCalls = 0
    const list = createPushFirstList(
      () => {
        loadCalls++
        return first.promise
      },
      (snapshot) => {
        sink.value = snapshot
      },
    )

    // self-init 同步发起：恰一次加载、status 已置 loading（调用方尚无数据）
    expect(loadCalls).toBe(1)
    expect(list.status.value).toBe('loading')
    expect(list.version.value).toBe(0)
    expect(sink.value).toEqual([])

    first.resolve(['a'])
    await flushPromises()
    expect(sink.value).toEqual(['a'])
    expect(list.version.value).toBe(1)
    expect(list.status.value).toBe('ready')
  })

  it('SWR 整体替换：重拉在途期间旧快照保留（不二次 apply），成功后整体替换并 version 自增', async () => {
    const first = deferred<string[]>()
    const second = deferred<string[]>()
    const sink = ref<string[]>(['seed'])
    const applied: string[][] = []
    let loadCalls = 0
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++
        return loadCalls === 1 ? first.promise : second.promise
      },
      (snapshot) => {
        applied.push(snapshot)
        sink.value = snapshot
      },
    )
    first.resolve(['v1'])
    await flushPromises()
    expect(sink.value).toEqual(['v1'])

    const reloading = list.refresh()
    expect(list.status.value).toBe('loading')
    second.resolve(['v2'])
    await reloading
    // 整体替换：apply 恰两次（初载 + 重拉），不存在部分落位
    expect(applied).toEqual([['v1'], ['v2']])
    expect(sink.value).toEqual(['v2'])
    expect(list.version.value).toBe(2)
    expect(list.status.value).toBe('ready')
  })

  it('失败语义：load 拒绝 → status=error、不落位、version 不变，refresh 向调用方上抛', async () => {
    const first = deferred<string[]>()
    const sink = ref<string[]>([])
    const list = createPushFirstList(
      () => first.promise,
      (snapshot) => {
        sink.value = snapshot
      },
    )
    first.reject(new Error('boom'))
    await flushPromises()
    expect(list.status.value).toBe('error')
    expect(sink.value).toEqual([])
    expect(list.version.value).toBe(0)

    // 显式 refresh 同语义上抛（self-init 侧由工厂静默，调用方显式刷新自行处置）
    const failing = deferred<string[]>()
    let loadCalls = 0
    const again = createPushFirstList<string[]>(
      () => {
        loadCalls++
        return loadCalls === 1 ? Promise.resolve(['x']) : failing.promise
      },
      () => {},
    )
    await flushPromises() // 初载成功
    failing.reject(new Error('boom'))
    await expect(again.refresh()).rejects.toThrow('boom')
    expect(again.status.value).toBe('error')
  })

  it('失败后 refresh 可恢复：error → loading → ready，version 续增', async () => {
    let loadCalls = 0
    const sink = ref<string[]>([])
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++
        return loadCalls === 1 ? Promise.reject(new Error('boom')) : Promise.resolve(['ok'])
      },
      (snapshot) => {
        sink.value = snapshot
      },
    )
    await flushPromises()
    expect(list.status.value).toBe('error')

    await list.refresh()
    expect(list.status.value).toBe('ready')
    expect(sink.value).toEqual(['ok'])
    expect(list.version.value).toBe(1)
  })

  it('部分失败不落位：快照内任一子加载失败 → 整体失败、apply 未被调用', async () => {
    const ok = deferred<string[]>()
    const bad = deferred<number>()
    const sink = ref<{ a: string[]; b: number } | null>(null)
    const list = createPushFirstList(
      () => Promise.all([ok.promise, bad.promise]).then(([a, b]) => ({ a, b })),
      (snapshot) => {
        sink.value = snapshot
      },
    )
    ok.resolve(['a'])
    bad.reject(new Error('stats boom'))
    await flushPromises()
    expect(list.status.value).toBe('error')
    expect(sink.value).toBeNull()
    expect(list.version.value).toBe(0)
  })

  it('在途合并：并发 refresh 共享同一次加载（load 恰一次），全部同解', async () => {
    const pending = deferred<string[]>()
    let loadCalls = 0
    const sink = ref<string[]>([])
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++
        return loadCalls === 1 ? Promise.resolve(['init']) : pending.promise
      },
      (snapshot) => {
        sink.value = snapshot
      },
    )
    await flushPromises()

    const p1 = list.refresh()
    const p2 = list.refresh()
    const p3 = list.refresh()
    expect(loadCalls).toBe(2)
    pending.resolve(['fresh'])
    await Promise.all([p1, p2, p3])
    expect(sink.value).toEqual(['fresh'])
    expect(list.version.value).toBe(2)
  })

  it('ledger:changed 订阅与事件重拉：每实例注册一次，事件到达即静默重拉（在途期间事件被合并）', async () => {
    const sink = ref<string[]>([])
    let loadCalls = 0
    // holder 绕开 TS 对闭包内赋值不跟踪导致的 never 收窄
    const holder: { pending: ReturnType<typeof deferred<string[]>> | null } = { pending: null }
    const list = createPushFirstList<string[]>(
      () => {
        loadCalls++
        if (loadCalls === 1) return Promise.resolve(['init'])
        holder.pending = deferred<string[]>()
        return holder.pending.promise
      },
      (snapshot) => {
        sink.value = snapshot
      },
    )
    await flushPromises()
    expect(sink.value).toEqual(['init'])

    // 订阅接线（删除即变红的负向守卫：工厂卸掉 listen 注册本断言即红）
    expect(mockListen).toHaveBeenCalledWith('ledger:changed', expect.any(Function))
    expect(handlers.length).toBe(1)

    // 事件到达：发起第二次加载（在途，旧数据保留）
    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    await flushPromises()
    expect(loadCalls).toBe(2)
    expect(sink.value).toEqual(['init'])
    expect(list.status.value).toBe('loading')

    // 在途期间再到达的事件：合并进同一次加载，不新增 IPC
    handlers.forEach((h) => h({ event: 'ledger:changed', payload: null }))
    expect(loadCalls).toBe(2)

    holder.pending?.resolve(['fresh'])
    await flushPromises()
    expect(sink.value).toEqual(['fresh'])
    expect(list.version.value).toBe(2)

    // 第二个实例各自注册（store 各一份订阅的既有形态不变）
    createPushFirstList<string[]>(() => Promise.resolve([]), () => {})
    expect(handlers.length).toBe(2)
  })
})
