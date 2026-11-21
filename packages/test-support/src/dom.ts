import { expect } from "vitest";
import { DOMWrapper, flushPromises, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";

export interface FindByTextOptions {
  exact?: boolean;
}

export interface FindInputOptions {
  placeholder?: string;
  type?: string;
}

export function findButton(
  wrapper: VueWrapper | DOMWrapper<Element>,
  text: string,
  options: FindByTextOptions = {},
): DOMWrapper<HTMLButtonElement> | undefined {
  const hit = wrapper
    .findAll("button")
    .find((b) => (options.exact ? b.text() === text : b.text().includes(text)));
  return hit as DOMWrapper<HTMLButtonElement> | undefined;
}

export function findButtonByTestId(
  wrapper: VueWrapper,
  testid: string,
): DOMWrapper<HTMLButtonElement> {
  return wrapper.find(`[data-testid="${testid}"]`) as DOMWrapper<HTMLButtonElement>;
}

export function findTab(
  wrapper: VueWrapper,
  label: string,
  options: FindByTextOptions = {},
): DOMWrapper<Element> | undefined {
  const hit = wrapper
    .findAll(".n-tabs-tab")
    .find((tab) => (options.exact ? tab.text() === label : tab.text().includes(label)));
  return hit as DOMWrapper<Element> | undefined;
}

export async function clickTab(wrapper: VueWrapper, label: string): Promise<void> {
  const tab = findTab(wrapper, label);
  expect(tab, `页签「${label}」应存在`).toBeTruthy();
  await tab!.trigger("click");
  await flushPromises();
  await nextTick();
  await nextTick();
}

export function findBodyButton(
  text: string,
  options: FindByTextOptions = {},
): DOMWrapper<HTMLButtonElement> | undefined {
  const hit = Array.from(document.body.querySelectorAll("button")).find((b) => {
    const t = b.textContent?.trim() ?? "";
    return options.exact ? t === text : t.includes(text);
  });
  return hit ? new DOMWrapper(hit) : undefined;
}

export function findBodyButtonByTestId(testid: string): DOMWrapper<HTMLButtonElement> | undefined {
  const el = document.body.querySelector(`[data-testid="${testid}"]`);
  return el ? new DOMWrapper(el as HTMLButtonElement) : undefined;
}

export function findInput(
  wrapper: VueWrapper,
  options: FindInputOptions = {},
): DOMWrapper<HTMLInputElement> {
  let selector = "input";
  if (options.placeholder !== undefined) selector += `[placeholder="${options.placeholder}"]`;
  if (options.type !== undefined) selector += `[type="${options.type}"]`;
  return wrapper.find(selector) as DOMWrapper<HTMLInputElement>;
}

export function findInputByTestId(
  wrapper: VueWrapper,
  testid: string,
): DOMWrapper<HTMLInputElement> {
  const carrier = wrapper.findComponent(`[data-testid="${testid}"]`);
  return (carrier.exists() ? carrier.find("input") : carrier) as DOMWrapper<HTMLInputElement>;
}

function hasHiddenAncestor(el: Element): boolean {
  let node: Element | null = el;
  while (node && node !== document.body) {
    if ((node as HTMLElement).style.display === "none") return true;
    node = node.parentElement;
  }
  return false;
}

function visibleNodes(selector: string): Element[] {
  return [...document.querySelectorAll(selector)].filter((el) => !hasHiddenAncestor(el));
}

function visibleDialogButtons() {
  return visibleNodes(".n-dialog button");
}

export function dialogText(): string {
  return visibleNodes(".n-dialog")
    .map((el) => el.textContent ?? "")
    .join("");
}

export function visibleModalText(): string {
  return visibleNodes(".n-card")
    .map((el) => el.textContent ?? "")
    .join("");
}

export async function clickDialogButton(text: string) {
  const btn = visibleDialogButtons().find((el) => el.textContent?.trim() === text)!;
  await new DOMWrapper(btn).trigger("click");
  await flushPromises();
}

export async function pressReleaseOn(selector: string) {
  const mask = document.body.querySelector(selector);
  expect(mask, `${selector} 应存在`).not.toBeNull();
  mask!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
  mask!.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
  mask!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  await flushPromises();
}

export function probeColor(color: string): string {
  const probe = document.createElement("span");
  probe.style.color = color;
  return probe.style.color;
}

export async function pressReleaseOnDialogMask() {
  await pressReleaseOn(".n-modal-mask");
}
