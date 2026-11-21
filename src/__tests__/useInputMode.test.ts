import { describe, expect, it } from "vitest";
import { effectScope } from "vue";
import {
  fakeMediaListenerCount,
  resetFakeMedia,
  setFakeMedia,
} from "@ledger/test-support/media-mock";
import {
  FINE_POINTER_QUERY,
  HOVER_QUERY,
  useInputMode,
  type InputMode,
} from "@/composables/useInputMode";

/**
 * 输入轴（Input Mode）模块测试（issue #841，ADR-0088 决策 6 / 词汇表「输入轴」）：
 * composable 只锁「hover / pointer 信号 → 触控轴 / 指针轴」纯映射——hover 与
 * pointer 的组合矩阵与实时换档；交互形态断言（快捷键退役渲染等）不上浮到本层。
 * 换档一律经媒体查询测试接缝（@ledger/test-support/media-mock）。
 * 共享订阅单例（issue #2187）经接缝的监听计数观察口锁定：多消费者共享一组监听、
 * 作用域全销毁后解绑、宿主替换重绑——删除共享订阅接线（退回每消费者独立监听）
 * 共享计数断言即红。
 */

/** 在指定 hover / pointer 信号下取输入轴（effectScope 内实例化，读毕即销毁不留泄漏）。 */
function modeAt(hover: "hover" | "none", pointer: "fine" | "coarse"): InputMode {
  setFakeMedia({ hover, pointer });
  const scope = effectScope();
  const value = scope.run(() => useInputMode())!.value;
  scope.stop();
  return value;
}

describe("useInputMode 信号 → 输入轴纯映射（hover / pointer 组合矩阵）", () => {
  it.each([
    ["可悬停 + 精细主指针（桌面鼠标 / 触控板）→ 指针轴", "hover", "fine", "pointer"],
    ["不可悬停 + 粗糙主指针（Android 手机 / 平板）→ 触控轴", "none", "coarse", "touch"],
    ["可悬停 + 粗糙主指针 → 触控轴（主指针粗糙即触控信号）", "hover", "coarse", "touch"],
    ["不可悬停 + 精细主指针 → 触控轴（不可悬停即触控信号）", "none", "fine", "touch"],
  ])("%s", (_label, hover, pointer, expected) => {
    expect(modeAt(hover as "hover" | "none", pointer as "fine" | "coarse")).toBe(expected);
  });
});

describe("useInputMode 实时换档（媒体查询 change 驱动）", () => {
  it("同一实例随 hover 信号重编程在两轴间往返翻转", () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const scope = effectScope();
    const mode = scope.run(() => useInputMode())!;
    expect(mode.value).toBe("pointer");

    setFakeMedia({ hover: "none" });
    expect(mode.value).toBe("touch");

    setFakeMedia({ hover: "hover" });
    expect(mode.value).toBe("pointer");
    scope.stop();
  });

  it("pointer 信号翻转同样驱动换档（两信号任一变化即重判）", () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const scope = effectScope();
    const mode = scope.run(() => useInputMode())!;
    expect(mode.value).toBe("pointer");

    setFakeMedia({ pointer: "coarse" });
    expect(mode.value).toBe("touch");
    scope.stop();
  });
});

describe("useInputMode 共享订阅单例（issue #2187，监听计数经接缝观察口）", () => {
  it("多消费者并发消费时两路查询各仅一组监听（退回每消费者独立监听即红）", () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const scopes = [effectScope(), effectScope(), effectScope(), effectScope()];
    const modes = scopes.map((s) => s.run(() => useInputMode())!);
    expect(fakeMediaListenerCount(HOVER_QUERY)).toBe(1);
    expect(fakeMediaListenerCount(FINE_POINTER_QUERY)).toBe(1);
    // 全体消费者同轴
    for (const mode of modes) expect(mode.value).toBe("pointer");

    setFakeMedia({ hover: "none" });
    for (const mode of modes) expect(mode.value).toBe("touch");

    for (const s of scopes) s.stop();
  });

  it("全部消费作用域销毁后监听全部解绑（零泄漏），再消费重新登记", () => {
    setFakeMedia({ hover: "hover", pointer: "fine" });
    const scope = effectScope();
    scope.run(() => useInputMode());
    expect(fakeMediaListenerCount(HOVER_QUERY)).toBe(1);
    scope.stop();
    expect(fakeMediaListenerCount(HOVER_QUERY)).toBe(0);
    expect(fakeMediaListenerCount(FINE_POINTER_QUERY)).toBe(0);

    // 再消费：重新登记且信号按当前宿主应答校准
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const scope2 = effectScope();
    const mode = scope2.run(() => useInputMode())!;
    expect(mode.value).toBe("touch");
    expect(fakeMediaListenerCount(HOVER_QUERY)).toBe(1);
    scope2.stop();
  });

  it("媒体查询宿主替换后新消费登记重绑，既有消费者同步恢复换档（用例间复位隔离）", () => {
    setFakeMedia({ hover: "none", pointer: "coarse" });
    const scope = effectScope();
    const first = scope.run(() => useInputMode())!;
    expect(first.value).toBe("touch");

    // 宿主替换（同 resetFakeMedia 每测语义）：旧查询对象失联，信号状态回默认
    resetFakeMedia();
    setFakeMedia({ hover: "hover", pointer: "fine" });

    // 新消费登记重绑到当前宿主：既有消费者一并恢复换档
    const scope2 = effectScope();
    const second = scope2.run(() => useInputMode())!;
    expect(second.value).toBe("pointer");
    expect(first.value).toBe("pointer");
    expect(fakeMediaListenerCount(HOVER_QUERY)).toBe(1);
    expect(fakeMediaListenerCount(FINE_POINTER_QUERY)).toBe(1);

    setFakeMedia({ hover: "none" });
    expect(first.value).toBe("touch");
    expect(second.value).toBe("touch");

    scope.stop();
    scope2.stop();
    expect(fakeMediaListenerCount(HOVER_QUERY)).toBe(0);
  });
});
