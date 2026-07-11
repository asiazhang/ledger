# 错误处理

所有命令统一使用 `AppError`，序列化为 `{ kind, message }` 格式：

```ts
// 序列化格式: { "kind": "Db" | "NotFound" | "Invalid" | "Parse" | "Io", "message": string }
```

| 错误类型 | 说明 |
|----------|------|
| `Db` | 数据库错误 |
| `NotFound` | 数据不存在（当前未使用） |
| `Invalid` | 参数校验错误（中文提示） |
| `Parse` | 导入解析错误 |
| `Io` | IO 错误 |

- **后端**：`src-tauri/src/error.rs`
- **前端消费**：`src/api/index.ts` 调用中，错误通过 `catch` 捕获
