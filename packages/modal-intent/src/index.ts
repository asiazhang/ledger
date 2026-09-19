// 弹窗意图编排入口（barrel）：useModalIntent 工厂与其返回类型契约的唯一出口。
// 消费面无子路径需求，exports 仅暴露 . 单入口（issue #1316 / ADR-0118）。
export { useModalIntent } from "./useModalIntent";
export type { UseModalIntentReturn } from "./useModalIntent";
