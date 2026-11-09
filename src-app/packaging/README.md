# 发布形态：release 二进制 + .desktop 直跑（issue #2083 / ADR-0145 决策 7 修订）

切轨后壳是纯 Rust 产物（NSH 原生 GTK 壳，无 WebView 打包面），发布形态收敛为
**单个 release 二进制 + 手工 .desktop 桌面入口**；AppImage / deb 自动打包属范围
外，另票承接（CI 侧 `linux-build` job 只产出二进制 artifact）。

## 直跑安装（发布二进制）

1. 下载 release 的 `nsh-app` 二进制，放到 `~/.local/bin/`：`chmod +x nsh-app`。
2. 复制本目录两个文件到用户级安装位：

   ```bash
   cp packaging/ledger.png ~/.local/share/icons/ledger.png
   cp packaging/ledger.desktop ~/.local/share/applications/ledger.desktop
   ```

3. 编辑 `~/.local/share/applications/ledger.desktop`，把 `Exec=` 行的
   `/usr/local/bin/nsh-app` 改成你的实际二进制路径（图标路径若不同也一并改）。
4. 应用列表即出现「开源记账」；无需刷新缓存（`update-desktop-database` 仅在
   某些极简桌面环境需要，可按需执行）。

运行时依赖：GLib/GTK4（主流发行版默认自带；极简镜像装 `gtk4` 包）。

## 从源码

```bash
cd src-app
cargo build --release        # 产物 target/release/nsh-app
```

把 `target/release/nsh-app` 当作上面的二进制即可。

## .desktop 模板

`ledger.desktop` 是模板：`Exec` / `Icon` 使用绝对路径占位，由安装者按上节改写。
`StartupWMClass=nsh-app` 与二进制名一致（GTK 以 WM_CLASS 匹配运行实例）。
