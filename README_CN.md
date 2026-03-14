# OpenCode Stats

一个面向 OpenCode 使用数据的终端统计面板。

![screenshot](images/overview.png)

`oc-stats` 会读取 OpenCode 本地 SQLite 数据库或 JSON 导出文件，在终端中展示 token 使用量、成本估算、模型与提供商分布，以及最近 365 天的活跃热力图。它参考了 Claude Code `/stats` 的使用体验，但以本地可运行、可导出、可分享为目标做了独立实现。

> 如果你已经在使用 OpenCode，并希望快速看清自己的调用量、成本和活跃趋势，这个工具可以直接上手。

## 功能特性

- 基于 `ratatui` 构建的终端仪表盘界面
- 自动读取 OpenCode 本地数据库，也支持通过 `--json` 加载导出文件
- 展示总 token、成本、会话数、消息数、提示数等概览指标
- 按模型、按提供商查看使用占比与明细
- 提供全部时间、最近 7 天、最近 30 天三种统计范围
- 内置最近 365 天活跃热力图，便于观察长期使用趋势
- 支持亮色 / 暗色主题启动参数
- 支持将当前页面复制到剪贴板：优先导出图片分享卡片，失败时自动降级为文本摘要
- 支持本地缓存模型价格数据，并提供缓存更新 / 清理命令
- 输出数据统计方式和 `opencode stats` 和 `opencode stats --models` 的输出对齐，保持数据一致

## 预览

OpenCode Stats 提供了三种数据预览视角，分别是

- 年视图下的热力图和数据总览
  ![screenshot](images/overview.png)
- 不同时间视图下的分模型使用情况
  ![model uses](images/models.png)
- 不同时间视图下的分供应商使用情况
  ![provider uses](images/providers.png)

同时，每个页面都支持以透明底的卡片风格直接渲染到剪贴板，比如

![card](images/card.png)

## 安装

### 通过 crates.io 安装

```bash
cargo install opencode-stats
```

安装后直接运行：

```bash
oc-stats
```

### 从 GitHub Release 下载预编译二进制

在 Releases 页面下载对应平台的压缩包，解压后直接运行 `oc-stats`。

当前 Release 工作流会构建以下平台：

- Windows `x86_64-pc-windows-msvc`
- macOS `x86_64-apple-darwin`
- macOS `aarch64-apple-darwin`
- Linux `x86_64-unknown-linux-gnu`
- Linux `x86_64-unknown-linux-musl`

### 从源码构建

```bash
git clone https://github.com/Cateds/opencode-stats.git
cd opencode-stats
cargo build --release
```

生成的可执行文件位于：

```bash
target/release/oc-stats
```

或者直接使用 git 路径构建

```bash
cargo install --git https://github.com/Cateds/opencode-stats.git
```

## 使用方法

### 默认启动

默认情况下，程序会自动寻找 OpenCode 本地数据库并加载数据：

```bash
oc-stats
```

### 指定数据库路径

```bash
oc-stats --db /path/to/opencode.db
```

### 指定 JSON 导出文件

```bash
oc-stats --json /path/to/export.json
```

### 指定主题

```bash
oc-stats --theme dark
oc-stats --theme light
```

### 缓存管理命令

查看本地价格缓存路径：

```bash
oc-stats cache path
```

更新本地价格缓存：

```bash
oc-stats cache update
```

清理本地价格缓存：

```bash
oc-stats cache clean
```

## 交互说明

程序启动后可通过键盘快速切换页面和统计范围：

- `Tab` / `Left` / `Right` / `h` / `l`：切换页面
- `Up` / `Down` / `j` / `k`：在 `Models` / `Providers` 页面中移动焦点
- `r`：循环切换统计范围
- `1` / `2` / `3`：快速切换时间范围
- `Ctrl+S`：复制当前页面到剪贴板
- `q` / `Esc`：退出程序

页面包含：

- `Overview`：总体使用概览
- `Models`：模型维度统计
- `Providers`：提供商维度统计

时间范围包含：

- `All time`
- `Last 7 days`
- `Last 30 days`

## 数据来源与价格计算

### 数据输入

`oc-stats` 支持两种输入来源：

- OpenCode 本地 SQLite 数据库
- OpenCode 导出的 JSON 文件

默认数据库位置通常为：

- Windows: `%APPDATA%/opencode/opencode.db`
- Linux: `~/.local/share/opencode/opencode.db`
- macOS: `~/Library/Application Support/opencode/opencode.db`

### 价格数据

模型价格信息会优先从本地缓存读取，并在需要时从远程源刷新：

- 本地缓存路径：`~/.config/oc-stats/models.json`
- 远程来源：`https://models.dev/api.json`
- 缓存有效期：1 小时

如果 OpenCode 配置中存在本地覆盖项，则会优先使用覆盖配置。

当完整价格信息不可用时，程序会使用回退策略估算缓存读写成本；如果数据库中已经记录了实际成本，也会优先采用数据库中的值。

## 适用场景

- 想快速查看自己在 OpenCode 中的 token 消耗情况
- 想按模型或提供商分析使用偏好
- 想了解最近一周、一个月或长期使用趋势
- 想将统计结果导出为图片或文本，便于分享

## License

MIT

## 致谢

- 字体文件使用 [Cascadia Code](https://github.com/microsoft/cascadia-code)，遵循 SIL Open Font License
- 项目体验灵感参考了 Claude Code 的 `/stats`
- 参考项目：[ocmonitor-share](https://github.com/Shlomob/ocmonitor-share)
