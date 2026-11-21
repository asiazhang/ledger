import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { NDataTable } from "naive-ui";
import { setFakeMedia } from "./media-mock";

export async function assertMobileTierScrollX(mountView: () => VueWrapper): Promise<void> {
  setFakeMedia({ width: 400, hover: "hover", pointer: "fine" });
  const mobile = mountView();
  await flushPromises();
  const mobileTable = mobile.findComponent(NDataTable);
  const columns = mobileTable.props("columns") as unknown as Array<{ width?: number }>;
  const fixedSum = columns.reduce((sum, c) => sum + (typeof c.width === "number" ? c.width : 0), 0);
  expect(fixedSum).toBeGreaterThan(0);
  expect(mobileTable.props("scrollX")).toBe(fixedSum);

  setFakeMedia({ width: 1280, hover: "hover", pointer: "fine" });
  const desktop = mountView();
  await flushPromises();
  expect(desktop.findComponent(NDataTable).props("scrollX")).toBeUndefined();
}
