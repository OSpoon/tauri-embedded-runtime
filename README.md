# Tauri Embedded Runtime

一个基于 Tauri 2 的桌面应用内置运行时示例。

应用会在自己的数据目录中准备 Python、Node.js 以及业务服务依赖，并启动本地服务供
桌面端调用。运行时与项目依赖不会写入宿主机的 PATH、Shell 配置或全局包目录。

## 适合什么场景

- 桌面应用需要内置 Python 或 Node.js 服务
- 不希望用户预先安装开发环境
- 不希望污染用户已有的 Python、Node.js 和全局依赖
- 需要在首次打开应用时展示可理解的安装进度
- 需要为不同业务服务维护独立的项目依赖

## 使用方式

启动应用后，点击“运行检测”。应用会按顺序检查并准备环境：

```text
检查安装环境
  → 准备 Python
  → 安装 FastAPI 环境
  → 准备 Node.js
  → 安装 Express 环境
  → 检查项目工具
  → 验证运行时
```

每个步骤都可以展开查看日志。首次运行会下载运行时和项目依赖，后续运行会复用已
通过校验的内容；只有版本变化、文件缺失或依赖损坏时才会重新准备。

环境准备完成后，点击“打开业务首页”即可调用内置的 FastAPI 和 Express 示例服务。
两个服务只监听本机地址，并由应用负责分配端口、健康检查和关闭回收。

## 主要特性

- 应用私有的 Python、Node.js 运行时
- Python 项目使用独立 virtualenv
- Node.js 项目使用独立 `node_modules`
- 固定版本、SHA-256 校验和断点续传
- 安装过程可取消、重试，并保留上一份可用环境
- 项目依赖缺失、损坏或基础运行时更新后可重新准备
- 服务启动失败时自动停止已启动的服务
- 应用退出时回收由应用启动的服务进程
- 基于 shadcn-vue 的统一界面组件和滚动区域

## 开始使用

### 环境要求

- Node.js `24.15.0`
- pnpm `10.15.1`
- Rust `1.95.0`
- Tauri 2 对应的平台 WebView、编译器和系统依赖

Node.js 和 Rust 版本分别记录在 [`.nvmrc`](./.nvmrc) 和
[`rust-toolchain.toml`](./rust-toolchain.toml) 中，pnpm 版本记录在
[`package.json`](./package.json) 中。

Linux 用户请先按照 [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
安装系统依赖。

### 安装依赖

```bash
pnpm install --frozen-lockfile
```

### 启动应用

```bash
pnpm tauri dev
```

首次运行需要网络访问运行时制品源和 Python/Node.js 包源。运行时文件会写入 Tauri
应用数据目录，不会安装到用户的系统环境中。

### 构建前端

```bash
pnpm build
```

### 发布版本

项目使用 `bumpp` 同步更新 `package.json`、`src-tauri/Cargo.toml` 和
`src-tauri/tauri.conf.json`，并创建带 `v` 前缀的 Git tag。版本更新后，bumpp 会调用 Cargo
自动维护 `src-tauri/Cargo.lock`，并将该自动生成的变化一并提交。确认工作区干净且已配置
GitHub 推送权限后执行：

```bash
pnpm release
```

`bumpp` 会创建 `release: v<version>` 提交、`v<version>` tag 并推送到 GitHub。
提交格式和发布约束见 [`AGENTS.md`](./AGENTS.md)。

发布 tag 会触发 [Release workflow](./.github/workflows/release.yml)，默认构建 macOS ARM 和
Windows 安装包并创建 GitHub Draft Release。macOS Intel 和 Linux 构建项已在 workflow 中保留，
需要时可以取消注释。稳定版本构建全部启用的安装包；预发布版本的 Windows 构建使用 NSIS，
以满足 MSI 对数值版本号的限制。构建完成后，workflow 会生成带签名的 updater artifact 和
`latest.json`，并将构建结果保存到 GitHub Draft Release。根目录 [`CHANGELOG.md`](./CHANGELOG.md) 由 `changelogen` 在 `pnpm release` 时
根据 Conventional Commits 自动生成，不要手动维护其中的发布条目。发布后可在 GitHub
Draft Release 中使用对应的生成条目，检查构建产物和发布说明后再正式发布。

### 应用内更新

应用更新不增加前端更新页面，而是通过原生菜单完成。打开应用菜单中的“检查更新…”，
应用会检查 GitHub Release 的 `latest.json`，确认后下载并验证签名，安装完成后自动重启。
Tauri updater 的签名私钥不能提交到仓库；本地生成或保存私钥时建议使用：

```bash
pnpm tauri signer generate -w ./.tauri/updater.key
```

将 `.tauri/updater.key` 的内容配置到 GitHub Actions secret
`TAURI_SIGNING_PRIVATE_KEY`，无密码私钥将 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空；如使用
密码保护的私钥，则同时配置对应密码。`.tauri/` 已被 `.gitignore` 忽略，私钥不可提交。
公钥已写入 [`src-tauri/tauri.conf.json`](./src-tauri/tauri.conf.json)，更换密钥时必须同步更新
公钥并在更换前规划已安装版本的升级兼容性。

本地执行完整 bundle 构建时也需要提供签名私钥；开发运行或使用 `--no-bundle` 时不需要：

```bash
TAURI_SIGNING_PRIVATE_KEY=./.tauri/updater.key \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
pnpm tauri build
```

## 数据与安全边界

运行时、下载缓存、项目环境和服务日志均保存在应用私有数据目录中。服务默认只绑定
`127.0.0.1`，不会自动暴露到局域网。

这套机制解决的是依赖隔离、版本管理和进程生命周期问题，不是用于执行不可信代码的
安全沙箱。应用仍然需要遵循操作系统权限和网络安全边界。

## 故障处理

如果某个步骤失败：

1. 展开失败步骤查看具体日志。
2. 确认网络可用、磁盘空间充足。
3. 点击“修复并启动”重新准备缺失或损坏的内容。
4. 如果只是项目依赖变化，可重新执行检测，应用会保留可复用的基础运行时。

服务启动失败时，应用会停止本次启动的服务，并保留上一份可用运行时（如果存在）。

如果 macOS 提示“应用已损坏，无法打开”，并且应用来自可信的发布来源，可以在终端
移除下载隔离标记后重新打开：

```bash
xattr -d com.apple.quarantine "/Applications/tauri-embedded-runtime.app"
open "/Applications/tauri-embedded-runtime.app"
```

如果应用不在“应用程序”目录，请替换为实际路径。该命令只是临时绕过 macOS 下载隔离，
不等同于正式的 Apple 签名与公证。

## 项目扩展

项目资源按服务分别存放在 `src-tauri/resources/projects/<project-id>/` 下。新增业务
项目时，为它创建独立目录和 `project.json`，再在
`src-tauri/src/runtime/projects/` 注册对应模块。

服务注册表会生成对应的运行时准备阶段和项目依赖阶段。同一服务下可以注册多个项目，
它们会在同一个服务阶段内依次准备，不需要复制整套安装流程。

运行时制品的版本、平台、架构、下载地址、归档格式和 SHA-256 摘要统一维护在
[`src-tauri/resources/runtime-artifacts.json`](./src-tauri/resources/runtime-artifacts.json)。

## 开发检查

提交代码前可以执行：

```bash
pnpm lint
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml --locked
```

GitHub Actions 会在 Pull Request 和 push 时执行相同的核心检查，配置见
[`.github/workflows/ci.yml`](./.github/workflows/ci.yml)。

提交前 hook 会通过 `lint-staged` 自动修复暂存的 TypeScript/Vue 文件，并格式化
暂存的 Rust 文件。首次安装依赖后如需手动重新安装 hook，可执行 `pnpm prepare`。

开发者接入 Python 或 Node.js 项目的步骤见
[`docs/development/project-adapter.md`](./docs/development/project-adapter.md)。

## 许可证

本项目使用 [MIT License](./LICENSE)。项目依赖、字体和运行时制品仍受各自上游许可
证约束，具体版本以 `pnpm-lock.yaml` 和 `src-tauri/Cargo.lock` 为准。
