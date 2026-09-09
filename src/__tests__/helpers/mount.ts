import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { defineComponent, h, type Component } from 'vue'
import { NDialogProvider } from 'naive-ui'

/**
 * 测试侧挂载助手的单一出口（issue #748，ADR-0085 决策 7「通用能力上收」）。
 *
 * 收编目录级 common 与测试文件中反复出现的两种挂载编排：
 * - 「mount + flushPromises 一体」——视图/组件挂载后立即冲刷 self-init 异步链；
 * - 「NDialogProvider 包裹」——组件顶层调用 useDialog（删除二次确认等）时，
 *   与 App.vue 同构需要 Provider 上下文（TransactionsView/InstrumentBrowser 先例）。
 *
 * `flushPromises` 刻意不在此另行包装（ADR-0085 决策 6）：需要更细粒度冲刷的
 * 用例直连官方导入，本出口只在「挂载后首刷」这一固定编排处收口。
 */

/** 挂载并冲刷：`mount` 后立即 `flushPromises`，返回就绪 wrapper。 */
export async function mountFlushed(
  component: Component,
  options?: Parameters<typeof mount>[1],
): Promise<VueWrapper> {
  const wrapper = mount(component, options)
  await flushPromises()
  return wrapper as VueWrapper
}

/** 以 NDialogProvider 包裹挂载（组件顶层 useDialog 所需的 Provider 上下文）。 */
export function mountWithDialog(component: Component): VueWrapper {
  return mount(NDialogProvider, {
    slots: { default: () => h(component) },
  }) as VueWrapper
}

/**
 * 在真实组件 setup 上下文内调用组合式函数（薄壳数据层测试的直调收口）。
 *
 * 薄壳 composable 在 setup 内注册 onMounted 自动首刷（部分另有 onUnmounted 清理），
 * 测试裸调会触发 Vue「no active component instance」警告且自动首刷不生效；经本助手
 * 在宿主组件 setup 内调用后生命周期如实生效。返回 shell（组合式函数的返回值）；
 * 宿主 wrapper 由全局 enableAutoUnmount（setup.ts）在每测 afterEach 统一卸载，
 * onUnmounted 清理随之自动执行，无需调用方手工卸载。
 */
export function withSetup<T>(composable: () => T): T {
  let shell!: T
  mount(
    defineComponent({
      setup() {
        shell = composable()
        return () => h('div')
      },
    }),
  )
  return shell as T
}
