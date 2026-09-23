# rusqlite 类型化持久化 feature（chrono / uuid / serde_json）

本项目不开启 rusqlite 的 `chrono` / `uuid` / `serde_json` feature——不把域模型的 `String` 字段（时间、id）改成数据库强类型，也不为 JSON 载荷列引入类型化读写辅助。

## 为什么不做

**类型收口的前提已被单点工厂否定。** 时间与 id 的格式定义分别收口在 `src-tauri/crates/infra/src/ids.rs` 的 `now_iso()` / `iso_at()`（`%Y-%m-%dT%H:%M:%SZ` UTC）和 `new_uuid()`（v7 进程内严格单调，#1489——「id 升序 = 插入序」是各域破平口径前提）。不存在「解析散落」问题，feature 提供的格式校验是重复保险。

**剩余收益付不起改动面。** 类型化的实际收益只有 row.get 编译期类型检查 + 读出即格式校验，但代价是所有域模型字段 `String` → 强类型，而 tauri command / HTTP / 前端契约仍然是字符串——每个边界都要双向转换，覆盖全部域模型 + 前端类型。单点工厂已是收口形态，类型化是形式整齐而非行为更正确。

**serde_json 没有目标列。** 全仓零 `json_extract`、零 JSON 列；`serde_json::from_str` 的现有用法都是对字符串载荷（settings、sync op）的解析，与该 feature 无关。

**chrono 的散落解析管不到。** budget / scheduled / policy 等处的 `NaiveDate::parse_from_str` 是业务日期运算，不是 row 读写，开 feature 不改变它们。

## 重新触发条件

- 出现引入 JSON 载荷列的 schema 计划（如同步元数据、备注结构化）→ 走迁移纪律 + 发布边界判断后另行开票，届时一并评估 `serde_json` feature。
- 若未来放弃字符串 IPC / 前端契约（模型字段改为强类型直达边界）→ 重新评估 `chrono` / `uuid` feature。

## Prior requests

- #1723: 拷问单：rusqlite `chrono`/`uuid`/`serde_json` — 类型化读写是否值得动契约
