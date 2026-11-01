// 弹窗意图编排入口（barrel）：useModalIntent 工厂的唯一出口。
// 消费面无子路径需求，exports 仅暴露 . 单入口（issue #1316 / ADR-0118）。
// 返回类型随导出面收窄不再外露（issue #1997：零消费）。
export { useModalIntent } from "./useModalIntent";
