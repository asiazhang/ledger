import type { ComponentPublicInstance } from 'vue'
import type { VueWrapper } from '@vue/test-utils'

/**
 * 组件实例窄化：`findComponent('<CSS 选择器>')` 在 @vue/test-utils v2 类型上刻意
 * 返回无 `vm` 的 `WrapperLike`（字符串选择器无组件类型可依），需要访问组件实例
 * （`$emit`、实例字段断言）时经本助手单点窄化，替代测试内散布的 as 断言。
 *
 * 运行时行为与 `.vm` 完全一致（VueWrapper 实例确有 vm）；窄化只发生在类型层。
 */
export function componentVm(w: unknown): ComponentPublicInstance {
  return (w as VueWrapper).vm
}
