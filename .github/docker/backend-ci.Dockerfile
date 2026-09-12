# 后端 CI 预构建镜像（issue #1109 / spec #1086）：tauri 系统依赖、mold 链接器
# 与 Rust stable 工具链全部烘进镜像，build.yml 的 backend / backend-lint job
# 以 container: 挂载本镜像，job 内不再执行任何系统依赖安装（改前 apt 阶段
# 20–98s 波动，实测缓存命中时也有 22–28s）。
#
# 重建路径（ci-image.yml 三个触发器）：
# 1. 改本文件（或依赖清单变更）→ merge 进 main 后自动重建；
# 2. 每周一 cron 自动重建，跟随 Rust stable 安全更新与基础镜像补丁；
# 3. 紧急手动：Actions → CI Image → Run workflow（可在任意分支上 dispatch）。
#
# 依赖清单纪律：本文件的 apt 清单与 build.yml 各 job 的系统依赖保持同一来源
# 语义——改 tauri 依赖时先改这里，CI job 侧只通过镜像获得依赖，不再各自维护
# 安装步骤。基础镜像锁 ubuntu:24.04（与 ubuntu-latest runner 同大版本，本地
# 复现一致）；刻意不跟随 ubuntu-latest 漂移，基础镜像升级是显式的文件改动。

FROM ubuntu:24.04

# RUSTUP_HOME/CARGO_HOME 放 /usr/local 供后续 docker build 层与容器内 root
# 直用；LANG 显式置 UTF-8（rustc / cucumber 输出含非 ASCII）。
ENV LANG=C.UTF-8 \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# 分组安装、单层提交（镜像体积与重建速度）：
# - 构建基础设施：git（actions/checkout 必需，裸 ubuntu 无）、ca-certificates、
#   curl（rustup 引导）、pkg-config（glib/webkit -sys crate 发现系统库）；
# - vendored C 编译：build-essential + perl——rusqlite 的
#   bundled-sqlcipher-vendored-openssl 需 make/gcc/perl（openssl Configure 是
#   perl 脚本），runner 镜像预装而裸 ubuntu 没有，必须显式带上；
# - tauri 系统依赖 + mold：与改前 backend job 的 apt 清单一致。
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    pkg-config \
    build-essential \
    perl \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    mold \
  && rm -rf /var/lib/apt/lists/*

# Rust stable + fmt/clippy 组件（backend-lint job 的 fmt/clippy/rustdoc 门禁
# 同用这套工具链）。不锁 patch 版本：镜像重建时取最新 stable，工具链升级随
# 每周重建自动进入 CI；锁版本改为显式改本文件。
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal --component rustfmt,clippy \
  && rustc --version \
  && cargo --version \
  && rustfmt --version \
  && cargo clippy --version \
  && mold --version
