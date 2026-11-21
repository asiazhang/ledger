import { flushPromises } from "@vue/test-utils";
import type { VueWrapper } from "@vue/test-utils";

export async function openMobileDrawer(wrapper: VueWrapper) {
  await wrapper.find(".mobile-hamburger").trigger("click");
  await flushPromises();
}

export function drawerMenuItemTexts(): string[] {
  return [...document.body.querySelectorAll(".n-drawer .n-menu-item-content")].map(
    (el) => el.textContent ?? "",
  );
}

export function drawerMoreLinkTexts(): string[] {
  return [...document.body.querySelectorAll(".n-drawer .group-more-link")].map(
    (el) => el.textContent ?? "",
  );
}

export function drawerGroupTitles(): string[] {
  return [...document.body.querySelectorAll(".n-drawer .sidebar-group-title")].map((el) =>
    (el.textContent ?? "").replace(/更多/g, "").trim(),
  );
}

export function findDrawerItem(label: string): HTMLElement {
  const item = drawerMenuItemTexts().length
    ? ([...document.body.querySelectorAll(".n-drawer .n-menu-item-content")].find((el) =>
        el.textContent?.includes(label),
      ) as HTMLElement | undefined)
    : undefined;
  return item!;
}
