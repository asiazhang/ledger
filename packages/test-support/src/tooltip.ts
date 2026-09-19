import { flushPromises, type DOMWrapper } from "@vue/test-utils";

/**
 * 口径说明 tooltip 的开启仪式（issue #1369）：NTooltip 有默认 100ms 防误触延迟，
 * jsdom 下必须等真实时钟走完再 flush，否则拿到空气泡——这段仪式曾被复制到 7 个
 * 测试文件里，收在此处作单点（同 invoke-mock / media-mock 的接缝先例）。
 *
 * 两轴各一个入口：指针轴 `hoverTipText`（悬停即现）、触控轴 `clickTipText`
 * （点按弹出，ADR-0088 决策 6）。两者都返回气泡正文（去模板空白），
 * 未开启气泡时返回空串。
 */

/** 气泡正文（去首尾模板空白）；未开启返回空串 */
function tipText(): string {
  return (document.body.querySelector(".n-popover")?.textContent ?? "").trim();
}

/** 指针轴：悬停触发器并返回 tooltip 文案 */
export async function hoverTipText(trigger: DOMWrapper<Element>): Promise<string> {
  await trigger.trigger("mouseenter");
  await new Promise((resolve) => setTimeout(resolve, 200));
  await flushPromises();
  return tipText();
}

/** 触控轴：点按触发器并返回气泡文案 */
export async function clickTipText(trigger: DOMWrapper<Element>): Promise<string> {
  await trigger.trigger("click");
  await flushPromises();
  return tipText();
}

/** 当前是否已开出气泡（断言「未点按前不出气泡」用） */
export function tipOpen(): boolean {
  return document.body.querySelector(".n-popover") !== null;
}
