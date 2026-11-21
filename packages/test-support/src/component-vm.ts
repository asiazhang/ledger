import type { ComponentPublicInstance } from "vue";
import type { VueWrapper } from "@vue/test-utils";

export function fireProp(w: unknown, prop: string, ...args: unknown[]): void {
  const fn = (w as { props(key: string): unknown }).props(prop) as
    | ((...a: unknown[]) => void)
    | undefined;
  fn?.(...args);
}

export function componentVm(w: unknown): ComponentPublicInstance {
  return (w as VueWrapper).vm;
}
