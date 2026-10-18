/**
 * 「关于」Tab 的读型动作落点（issue #1957 / ADR-0022 修订注记）：项目主页与反馈
 * 入口。发布页**不在此**——「通用 → 更新」卡片已持有它（release-page 形态的
 * 打开发布页引导），同址再开一条就是第二处入口。
 *
 * 常量单点：地址口径只在这里写一次，卡片与测试同源消费。
 */

/** 项目主页（GitHub 仓库）。 */
export const PROJECT_HOME_URL = "https://github.com/asiazhang/ledger";

/** 反馈入口（GitHub Issues）：报障时版本信息可一并粘贴。 */
export const ISSUES_URL = "https://github.com/asiazhang/ledger/issues";
