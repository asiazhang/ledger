import { mount, flushPromises, type VueWrapper } from "@vue/test-utils";
import { defineComponent, h, type Component } from "vue";
import { NDialogProvider } from "naive-ui";

export async function mountFlushed(
  component: Component,
  options?: Parameters<typeof mount>[1],
): Promise<VueWrapper> {
  const wrapper = mount(component, options);
  await flushPromises();
  return wrapper as VueWrapper;
}

export function mountWithDialog(component: Component): VueWrapper {
  return mount(NDialogProvider, {
    slots: { default: () => h(component) },
  }) as VueWrapper;
}

export function withSetup<T>(composable: () => T): T {
  let shell!: T;
  mount(
    defineComponent({
      setup() {
        shell = composable();
        return () => h("div");
      },
    }),
  );
  return shell as T;
}
