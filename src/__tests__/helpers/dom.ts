import { expect } from 'vitest'
import { DOMWrapper, flushPromises, type VueWrapper } from '@vue/test-utils'

/**
 * 测试侧 DOM 查找助手的单一出口（issue #746，ADR-0085 决策 6）。
 *
 * 收编全仓测试文件中按钮查找（文本匹配 / body-teleport / data-testid 三变体）
 * 与输入查找家族的同构副本：既有形态为「`findAll('button')` + 文本过滤 +
 * 调用方自行断言」与「弹窗 teleport 到 body 后 `document.body.querySelector`」
 * 两类，本文件在单点收窄为带类型的查找函数。
 *
 * 约定（两种未命中形态，均不抛错，命中与否由调用方按用例语义断言）：
 * - 文本/body 两变体返回 `DOMWrapper | undefined`，未命中为 undefined；
 * - testid 变体与 findInput 返回 test-utils 的不存在 wrapper（`exists()` 为
 *   false），保持与 `wrapper.find` 直用形态一致——「找不到即失败」的期望
 *   文案属于用例，不属于查找；
 * - 文本匹配默认包含匹配（`includes`），`{ exact: true }` 切换精确匹配
 *   （trim 后全等）；wrapper 侧用 test-utils 的 `text()`，body 侧用
 *   `textContent?.trim()`，两者口径一致；
 * - `flushPromises` 刻意不在此包装（ADR-0085 决策 6）：直连官方导入不是布线
 *   噪音，包装反增间接层。
 */

/** 文本匹配选项：`exact` 缺省为包含匹配，置 true 为 trim 后全等。 */
export interface FindByTextOptions {
  exact?: boolean
}

/** 输入查找选项：placeholder / type 任选其一或组合。 */
export interface FindInputOptions {
  placeholder?: string
  type?: string
}

/** wrapper 范围内按文本找按钮（既有 `findAll('button')` + 过滤形态的收口）。 */
export function findButton(
  wrapper: VueWrapper,
  text: string,
  options: FindByTextOptions = {},
): DOMWrapper<HTMLButtonElement> | undefined {
  const hit = wrapper.findAll('button').find((b) =>
    options.exact ? b.text() === text : b.text().includes(text),
  )
  return hit as DOMWrapper<HTMLButtonElement> | undefined
}

/** wrapper 范围内按 data-testid 找按钮（既有 `wrapper.find('[data-testid="…"]')` 形态的收口）。 */
export function findButtonByTestId(wrapper: VueWrapper, testid: string): DOMWrapper<HTMLButtonElement> {
  return wrapper.find(`[data-testid="${testid}"]`) as DOMWrapper<HTMLButtonElement>
}

/** body 范围内按文本找按钮（弹窗经 NModal teleport 到 body 后的查找形态）。 */
export function findBodyButton(
  text: string,
  options: FindByTextOptions = {},
): DOMWrapper<HTMLButtonElement> | undefined {
  const hit = Array.from(document.body.querySelectorAll('button')).find((b) => {
    const t = b.textContent?.trim() ?? ''
    return options.exact ? t === text : t.includes(text)
  })
  return hit ? new DOMWrapper(hit) : undefined
}

/** body 范围内按 data-testid 找按钮（弹窗内按钮的既有 testid 查找形态）。 */
export function findBodyButtonByTestId(testid: string): DOMWrapper<HTMLButtonElement> | undefined {
  const el = document.body.querySelector(`[data-testid="${testid}"]`)
  return el ? new DOMWrapper(el as HTMLButtonElement) : undefined
}

/** 输入查找家族：裸 `input` / `input[placeholder="…"]` / `input[type="…"]` 的公共形态。 */
export function findInput(
  wrapper: VueWrapper,
  options: FindInputOptions = {},
): DOMWrapper<HTMLInputElement> {
  let selector = 'input'
  if (options.placeholder !== undefined) selector += `[placeholder="${options.placeholder}"]`
  if (options.type !== undefined) selector += `[type="${options.type}"]`
  return wrapper.find(selector) as DOMWrapper<HTMLInputElement>
}

/**
 * 弹窗表单内输入框：testid 载体组件经 `findComponent` 锚定后取内部 input。
 *
 * NModal 内容 teleport 到 body 且表单控件的 testid 落在包裹组件根元素上，
 * 直用 CSS 选择器不总可达，需经组件树锚定（SubscriptionsPane/InstallmentsPane
 * 先例形态的收口，issue #748）。
 */
export function findInputByTestId(
  wrapper: VueWrapper,
  testid: string,
): DOMWrapper<HTMLInputElement> {
  const carrier = wrapper.findComponent(`[data-testid="${testid}"]`)
  // 载体未命中时直接返回不存在 wrapper（exists() 为 false），与 findInput 家族
  // 「找不到即失败、期望文案归用例」的约定一致，不在查找层拖错。
  return (carrier.exists() ? carrier.find('input') : carrier) as DOMWrapper<HTMLInputElement>
}

// —— 弹窗可见性助手（issue #748 上收：原 TransactionsView/common 目录级实现） ——
// useDialog 确认框与 NModal 卡片均 teleport 到 body，需从 document 查询；
// 同一 modal 容器在 jsdom 中会残留（过渡不结束），只取可见节点。

/** 过滤 v-show 隐藏容器（jsdom 中 leave 过渡不会结束会残留旧内容），只取可见节点。 */
function hasHiddenAncestor(el: Element): boolean {
  let node: Element | null = el
  while (node && node !== document.body) {
    if ((node as HTMLElement).style.display === 'none') return true
    node = node.parentElement
  }
  return false
}

function visibleNodes(selector: string): Element[] {
  return [...document.querySelectorAll(selector)].filter((el) => !hasHiddenAncestor(el))
}

function visibleDialogButtons() {
  return visibleNodes('.n-dialog button')
}

/** useDialog 确认框的可见文本（未打开为空串）。 */
export function dialogText(): string {
  return visibleNodes('.n-dialog')
    .map((el) => el.textContent ?? '')
    .join('')
}

/** NModal 卡片内容文本：NModal teleport 到 body 且组件根为占位符，需从 document 查卡片。 */
export function visibleModalText(): string {
  return visibleNodes('.n-card')
    .map((el) => el.textContent ?? '')
    .join('')
}

/** 点击确认/取消删除对话框中指定文案的按钮。 */
export async function clickDialogButton(text: string) {
  const btn = visibleDialogButtons().find((el) => el.textContent?.trim() === text)!
  await new DOMWrapper(btn).trigger('click')
  await flushPromises()
}

/**
 * 确认框遮罩「按下-抬起」完整事件序列：真实浏览器中按下-抬起在遮罩上合成 click
 * 触发关闭判定，jsdom 不自动合成，手动派发三段事件等价模拟
 * （AppModal 契约测试同款先例）。
 */
export async function pressReleaseOnDialogMask() {
  const mask = document.body.querySelector('.n-modal-mask')
  expect(mask, '.n-modal-mask 应存在').not.toBeNull()
  mask!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  mask!.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
  mask!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  await flushPromises()
}
