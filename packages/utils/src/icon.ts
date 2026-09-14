import type { Component } from 'vue'
import {
  BriefcaseOutline,
  BusOutline,
  CardOutline,
  CarOutline,
  CartOutline,
  EllipsisHorizontalOutline,
  FlashOutline,
  GiftOutline,
  GameControllerOutline,
  HomeOutline,
  MedkitOutline,
  PhonePortraitOutline,
  RestaurantOutline,
  SchoolOutline,
  TrendingUpOutline,
  TrophyOutline,
  WalletOutline,
} from '@vicons/ionicons5'

/**
 * 分类图标注册表：运行时按名查找（分类 icon 字段存的是图标名字符串）。
 *
 * 显式白名单，而非 `import * as` 全量命名空间——全量引入会让整包约 1300 个
 * 图标组件（每个都是带内联 SVG path 的 render 函数）全部落入产物：运行时
 * 字符串索引使 tree-shaking 失效，SettingsView chunk 因此膨胀到约 1.1MB
 * 并触发 Vite 500kB 体积告警。
 *
 * 白名单 = 已知使用面闭集：迁移种子数据的全部顶级分类图标（V004）。
 * AI 导入提示词不携带 icon，perf 生成器 icon 为 NULL；白名单外的名字不渲染
 * 图标，与无效名行为一致。新增分类图标时在 import 与映射表各补一行——
 * 按需扩展的显式成本，换取产物体积可控。
 */
const iconRegistry: Record<string, Component> = {
  BriefcaseOutline,
  BusOutline,
  CardOutline,
  CarOutline,
  CartOutline,
  EllipsisHorizontalOutline,
  FlashOutline,
  GiftOutline,
  GameControllerOutline,
  HomeOutline,
  MedkitOutline,
  PhonePortraitOutline,
  RestaurantOutline,
  SchoolOutline,
  TrendingUpOutline,
  TrophyOutline,
  WalletOutline,
}

export function getIconComponent(name: string | null): Component | null {
  if (!name) return null
  return iconRegistry[name] ?? null
}
