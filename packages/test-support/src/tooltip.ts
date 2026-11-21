import { flushPromises, type DOMWrapper } from "@vue/test-utils";

function tipText(): string {
  return (document.body.querySelector(".n-popover")?.textContent ?? "").trim();
}

export async function hoverTipText(trigger: DOMWrapper<Element>): Promise<string> {
  await trigger.trigger("mouseenter");
  await new Promise((resolve) => setTimeout(resolve, 200));
  await flushPromises();
  return tipText();
}

export async function clickTipText(trigger: DOMWrapper<Element>): Promise<string> {
  await trigger.trigger("click");
  await flushPromises();
  return tipText();
}

export function tipOpen(): boolean {
  return document.body.querySelector(".n-popover") !== null;
}
