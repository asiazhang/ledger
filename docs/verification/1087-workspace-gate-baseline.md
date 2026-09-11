# workspace 骨架与门禁继承基线（issue #1087 / 父 spec #1086）

> 本票把 Rust 根升级为 workspace（根包仍是 tauri 应用包），成员 crate 集中放
> `src-tauri/crates/` 下，并以基础设施 crate `ledger-infra` 的首位成员（IPC
> 载荷脱敏，自根包 `lib.rs` 迁出）走通「建 crate → 门禁生效 → 检查覆盖全
> workspace」整条链路。本文件记录门禁接线的负向验收与编译耗时/体积基线，
> 作为后续拆分票（#1088–#1108）的观测起点。

## 交付物

- `src-tauri/Cargo.toml`：`[workspace] members = ["crates/*"]`（glob 纳入成员）+
  `[workspace.lints.clippy]` 六件套 deny 的唯一声明处；根包与成员均经
  `[lints] workspace = true` 继承。
- `src-tauri/crates/infra/`：新成员 crate `ledger-infra`（首位模块
  `redact::redact_passphrase_payload`）。
- `scripts/check.sh` / `scripts/test.sh` / `scripts/lint-fix.sh` /
  `.github/workflows/build.yml`：cargo clippy / test / fmt 命令显式声明
  `--workspace` / 词尾 `--all`。
- `scripts/check-structure.ts`：新增 crate 边界核对（成员登记、门禁继承、
  依赖方向、命令覆盖）；`CRATES` 是 crate 边界的唯一事实源。
- `scripts/check-test-support.ts`：扫描范围扩到 `crates/*/src` 与 `crates/*/tests`。

## 负向验收（删除即变红）

1. **成员漏继承门禁**：删除 `src-tauri/crates/infra/Cargo.toml` 的
   `[lints] workspace = true` 后 `bun scripts/check-structure.ts` 红：

   ```
   ✗ 门禁继承：成员 crate ledger-infra 缺 [lints] workspace = true（crates/infra/Cargo.toml）
   ❌ 结构守门失败：1 处问题
   ```

   同时实测（独立 scratch workspace）：成员不继承时 `cargo clippy --workspace
   --all-targets -- -D warnings` 对生产路径 `.unwrap()` **照常绿**——门禁静默
   消失，正是必须机器守门、不能依赖 clippy 自身的原因。

2. **新成员未登记**：`crates/` 下新增一个未在 `CRATES` 登记的 crate 目录 →
   红（`成员 crate 未登记 CRATES`）。
3. **命令未覆盖全成员**：任一门槛命令缺 `--workspace` / 词尾 `--all` → 红
   （`workspace 命令覆盖`）；`--all-targets` 这类 `--all*` 旗标不算范围（用 `\b`
   匹配 `--all` 会在这里假绿，已用夹具用例锁死）。
4. **依赖方向**：基础设施 crate 反向依赖壳层 crate → 红（`crate 依赖方向`）。

以上 1–4 均有 `src/__tests__/check-structure.test.ts` 的夹具用例覆盖，断言对准
检查失败这一可观察结果（退出码与输出），不对准实现形状。

## 编译耗时 / 体积基线

### CI（本票分支的 Build run，后端测试 job）

commit `de097771` 触发的 `Build` workflow（run `34618048780`，后端测试 job
`103324784451`，Linux x64 runner）：

- **依赖缓存体积**：恢复 `Cache Size: ~2009 MB (2107050681 B)`（restore-key 命中
  主干缓存），保存 `2107047336 B`——首位成员带来的缓存体积增量 ≈ −3.4 KB，在
  rust-cache 清理噪声内，可视作零增量。
- **编译阶段**：`Finished \`test\` profile [unoptimized + debuginfo] target(s) in
  1m 23s`（83s），本仓只编 `tauri-app` + `ledger-infra` 两个 crate。
- **后端测试 job** wall clock ≈ 362s（含缓存解包 ~62s、apt ~5s、测试执行）。

对照基线：父 spec #1086 记录的「本票前」单 crate 编译为 113s（run 34599136819）、
依赖缓存 2009 MB。缓存体积前后一致；编译耗时低于旧基线（单次运行的波动，本票
不主张因果）。CI 阶段实测随 #1110「编译与缓存调优 + 基线刷新」继续跟踪。

### 本机（控制变量：touch 源文件后重编，依赖全热）

测量机为本仓库开发机（macOS / arm64，kache rustc-wrapper 命中，`CARGO_PROFILE_DEV_DEBUG=1`）：

| 项 | 本票前（main，无 workspace） | 本票后（workspace + 首位成员） |
| --- | --- | --- |
| `cargo test --no-run` 重编耗时 | 51.03s（根包 + 全部测试 target） | 50.94s（根包 + 成员 + 全部测试 target） |
| 成员 crate 清缓存后单独编译 | — | 2.57s |
| 成员 crate 产物（rlib + rmeta + rcgu.o） | — | ≈ 1.1 MB |

结论：首位成员在依赖全热时**不增加可测得的编译耗时**（差值在噪声内）；其增量
产物约 1.1 MB，相对既有 CI 依赖缓存 2009 MB 约 0.05%。

## 未验证项

- 桌面三平台与 Android 打包（DMG / deb / AppImage / nsis / APK）未在本机跑通——
  发布构建仅 tag / 手动 dispatch 触发。已用 `cargo check --workspace --all-targets`
  与三层测试证明 workspace 改动不破坏编译；打包链路的实测按仓库约定在 PR 正文
  声明未验证。
