// viewLabel 契约测试：侧栏/内容区标题的 key 构造收口（common.nav.<name>）。
// 回归背景：App.vue 调用点曾漏写 common. 域前缀（写成 nav.<name>），vue-i18n 对
// 缺失 key 原样渲染 key 代号，侧栏整排显示 nav.dashboard；而两个 locale 结构
// 全等（check-i18n-keys 只查互全等），一起缺 key 时门槛拦不住——只能在消费侧
// 断言解析结果，故 key 构造收口在 i18n/view-label 并在此测试。
import { describe, expect, it } from 'vitest'
import { viewShortcuts } from '@/composables/useViewShortcuts'
import { viewLabel } from '@/i18n/view-label'
import zhCN from '@/i18n/locales/zh-CN'

describe('viewLabel（视图标题文案）', () => {
  it('全部视图名解析为文案而非 key 代号（漏域名前缀回归）', () => {
    for (const { name } of viewShortcuts.value) {
      const label = viewLabel(name)
      expect(label, name).toBeTruthy()
      expect(label, name).not.toContain('nav.')
    }
  })

  it('key 契约：文案挂在 common.nav 下且覆盖全部视图名', () => {
    for (const { name } of viewShortcuts.value) {
      expect(zhCN.common.nav, name).toHaveProperty(name)
    }
  })

  it('zh-CN 解析示例：dashboard → 概览', () => {
    expect(viewLabel('dashboard')).toBe('概览')
  })
})
