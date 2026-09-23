import { ref, type Ref } from "vue";
import { api } from "@ledger/api";
import { createLatestWins } from "@ledger/latest-wins";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import type { Instrument } from "@ledger/types";

/**
 * 标的远程搜索（issue #1308）：「查询进、候选出」的域件——防抖 + 空查询清空不发
 * 请求 + `listInstruments({ search, page_size: 50 })` + 失败刻意吞错静默清空 +
 * 纪元守卫作废在途（issue #1401：迟到旧纪元结果不落位）全部内化，搜索语义（时长、
 * 竞态、吞错）改动一处生效。失败吞错是「刻意静默不收编」的合法形态（词汇表
 * Loadable 边界），不迁入 Loadable。
 *
 * 接口最窄（#1308 grilling 决策 4）：原始标的不做 options 投影——label 拼法、
 * 编辑回填候选合并、基金判定等领域形态归消费方；命令式 `search` 与 naive-ui
 * `@search` 回传同形，组件接线零改动；不暴露 cancel/invalidate。防抖时长单源
 * SEARCH_DEBOUNCE_MS（跨域「搜索输入防抖」不变量）。
 *
 * 服务端分页列表读路径（InstrumentBrowser）不属本接缝：search 只是它四个过滤
 * 维度之一，还牵动 total、分页归零与空态判定，只共享防抖常量（#1308 决策 1）。
 */
export interface UseInstrumentSearchReturn {
  /** 最近一次查询的候选（原始标的）；初始、清空输入与失败后为空集 */
  items: Ref<Instrument[]>;
  /** 查询在途标志：请求发出前置位，终态（结果落位 / 失败 / 空查询）置收 */
  searching: Ref<boolean>;
  /** 搜索意图入口：与 naive-ui `@search` 回传同形 */
  search(query: string): void;
}

export function useInstrumentSearch(): UseInstrumentSearchReturn {
  const items = ref<Instrument[]>([]);
  const searching = ref(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** 搜索在途竞态纪元（issue #1401 首例，#1678 起消费共享 module）：每次输入开启
   *  新纪元，迟到旧纪元结果不落位 */
  const wins = createLatestWins();

  function search(query: string) {
    const myToken = wins.begin();
    clearTimeout(timer);
    timer = setTimeout(async () => {
      if (myToken.isStale()) return;
      if (!query.trim()) {
        items.value = [];
        searching.value = false;
        return;
      }
      searching.value = true;
      try {
        const res = await api.listInstruments({ search: query.trim(), page_size: 50 });
        if (myToken.isStale()) return;
        items.value = res.items;
      } catch {
        if (myToken.isStale()) return;
        items.value = [];
      } finally {
        if (!myToken.isStale()) searching.value = false;
      }
    }, SEARCH_DEBOUNCE_MS);
  }

  return { items, searching, search };
}
