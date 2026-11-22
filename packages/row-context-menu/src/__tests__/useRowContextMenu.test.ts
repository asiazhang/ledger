import { describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { useRowContextMenu } from "../useRowContextMenu";

interface TestRow {
  id: string;
  label: string;
}

function mouseAt(x: number, y: number): MouseEvent {
  return { clientX: x, clientY: y } as MouseEvent;
}

const rowA: TestRow = { id: "row-a", label: "A" };
const rowB: TestRow = { id: "row-b", label: "B" };

describe("useRowContextMenu 初始状态", () => {
  it("状态为 null（关闭终态，可见性由非空派生）", () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    expect(menu.state.value).toBeNull();
  });
});

describe("useRowContextMenu open（统一「收起 → 下一帧重开」单路径）", () => {
  it("从关闭态打开：同样延迟一帧，下一帧落位事件坐标与目标行（无快路径）", async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    menu.open(mouseAt(100, 200), rowA);
    expect(menu.state.value).toBeNull();
    await nextTick();
    expect(menu.state.value).toEqual({ x: 100, y: 200, row: rowA });
  });

  it("已开时 open 重定位：状态过 null 一跳，下一帧新坐标新行重开", async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    menu.open(mouseAt(100, 200), rowA);
    await nextTick();
    expect(menu.state.value).toEqual({ x: 100, y: 200, row: rowA });

    menu.open(mouseAt(300, 400), rowB);
    expect(menu.state.value).toBeNull();
    await nextTick();
    expect(menu.state.value).toEqual({ x: 300, y: 400, row: rowB });
  });

  it("同一同步批次内连续 open：最后一次开启胜出", async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    menu.open(mouseAt(100, 200), rowA);
    menu.open(mouseAt(300, 400), rowB);
    await nextTick();
    expect(menu.state.value).toEqual({ x: 300, y: 400, row: rowB });
  });
});

describe("useRowContextMenu select（收起并交付收起瞬间的 (key, row)）", () => {
  it("收起菜单，回调收到收起瞬间的目标行（捕获先于清空）", async () => {
    const onSelect = vi.fn();
    const menu = useRowContextMenu<TestRow>(onSelect);
    menu.open(mouseAt(100, 200), rowA);
    await nextTick();

    menu.select("edit");
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith("edit", rowA);
    expect(menu.state.value).toBeNull();
    await nextTick();
    expect(menu.state.value).toBeNull();
  });

  it("菜单未开时 select：只收起（无状态可交付），回调不触发", () => {
    const onSelect = vi.fn();
    const menu = useRowContextMenu<TestRow>(onSelect);
    menu.select("delete");
    expect(onSelect).not.toHaveBeenCalled();
    expect(menu.state.value).toBeNull();
  });
});

describe("useRowContextMenu close", () => {
  it("close 清回全空终态，无滞留目标行", async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    menu.open(mouseAt(100, 200), rowA);
    await nextTick();
    menu.close();
    expect(menu.state.value).toBeNull();
  });
});

describe("useRowContextMenu 定位坐标（open 同步更新、close 保留）", () => {
  it("open 同步更新定位坐标（不下一帧，与手写拷贝舞步一致）", () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    expect(menu.position.value).toEqual({ x: 0, y: 0 });
    menu.open(mouseAt(100, 200), rowA);
    expect(menu.position.value).toEqual({ x: 100, y: 200 });
  });

  it("close 后定位坐标保留（离场动画仍消费 x/y，清零会左上角闪现，issue #798）", async () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    menu.open(mouseAt(100, 200), rowA);
    await nextTick();
    menu.close();
    expect(menu.state.value).toBeNull();
    expect(menu.position.value).toEqual({ x: 100, y: 200 });
  });

  it("同一同步批次内连续 open：定位坐标最后一次开启胜出", () => {
    const menu = useRowContextMenu<TestRow>(vi.fn());
    menu.open(mouseAt(100, 200), rowA);
    menu.open(mouseAt(300, 400), rowB);
    expect(menu.position.value).toEqual({ x: 300, y: 400 });
  });
});

describe("useRowContextMenu 工厂形态（每次调用独立实例）", () => {
  it("两个实例互不串扰", async () => {
    const menu1 = useRowContextMenu<TestRow>(vi.fn());
    const menu2 = useRowContextMenu<TestRow>(vi.fn());
    menu1.open(mouseAt(100, 200), rowA);
    await nextTick();
    expect(menu1.state.value).toEqual({ x: 100, y: 200, row: rowA });
    expect(menu2.state.value).toBeNull();
    menu2.open(mouseAt(300, 400), rowB);
    await nextTick();
    expect(menu1.state.value).toEqual({ x: 100, y: 200, row: rowA });
    expect(menu2.state.value).toEqual({ x: 300, y: 400, row: rowB });
    menu1.close();
    expect(menu1.state.value).toBeNull();
    expect(menu2.state.value).toEqual({ x: 300, y: 400, row: rowB });
  });
});
