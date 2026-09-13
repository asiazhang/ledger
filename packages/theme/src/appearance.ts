/**
 * Appearance 模式类型（issue #1154 下移）：应用外观的亮/暗两态，原定义在应用设置
 * store（`stores/app.ts`），随主题包抽取下移至此——theme 对 store 的四条类型边
 * （design-tokens / semantic-colors / chart-style / theme-contract）全部改为包内
 * 消费，主题层对 store 依赖归零；store 反向从包导入此类型，全仓无第二处定义。
 *
 * 归属：Appearance 是用户对视觉呈现的选择（参考数据与设置域词条），包内只定义
 * 模式闭集；选择状态的持有、持久化与切换仍归应用设置 store（单一状态源不变）。
 */
export type Theme = 'dark' | 'light'
