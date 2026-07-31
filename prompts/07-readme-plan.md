# README 编写实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 创建一份使用优先的中文 `README.md`，准确说明 Release 安装、配置初始化与模板字段，以及 `-p/-s/-m/-t` 的单项和组合用法。

**Architecture:** README 前半部分服务普通 Release 用户，按“安装 → 初始化 → 配置 → 命令”组织；后半部分容纳兼容入口、失败行为、排查和源码开发。所有事实直接来自当前 Rust 实现、工具清单和实际 `--help`，不修改程序行为。

**Tech Stack:** GitHub Flavored Markdown、Rust/clap CLI、TOML 配置。

## Global Constraints

- 普通用户只需下载 GitHub Release 对应平台包、完整解压并运行。
- Release 包已内置 MediaInfo 和 FFmpeg；快速安装不得要求额外下载或配置系统 `PATH`。
- 源码工具脚本只出现在后部“源码开发”章节，并标注 Release 用户无需执行。
- `-p/-s/-m/-t` 的书写顺序任意，内部固定按 `p → m → s → t` 执行。
- tree 报告文件名固定为 `<输出目录名>.tree.txt`。
- 配置模板必须与 `src/config.rs` 的 `CONFIG_TEMPLATE` 一致。
- README 使用中文，命令、路径、配置键和错误关键字保留英文。

---

### Task 1: 普通用户安装、快速上手与配置

**Files:**
- Create: `README.md`

**Interfaces:**
- Consumes: `src/config.rs::CONFIG_TEMPLATE`、`resolve_config_paths()`、`init_config()` 和 GitHub Release 便携目录约束。
- Produces: README 的项目简介、快速安装、快速上手、配置初始化和字段参考。

- [ ] **Step 1: 编写项目简介与快速安装**

说明 CrabGrab 的四项能力和当前 macOS Apple Silicon、Windows x86_64 发布目标。安装只列“下载对应 Release → 完整解压 → 运行帮助”，明确 `tools/` 已内置且必须保持相对目录。

- [ ] **Step 2: 编写三步快速上手**

依次展示：

```bash
crabgrab config init
# 编辑命令输出的 config.toml
crabgrab -psmt tmdb-movie-550 movie.mkv ./result
```

补充 Windows 使用 `crabgrab.exe`，不改变参数语法。

- [ ] **Step 3: 编写配置初始化规则**

准确记录便携配置优先、明确不可写时回退系统配置目录、已有配置拒绝覆盖、实际路径以命令输出为准，以及便携配置无效时不静默回退。

- [ ] **Step 4: 写入完整模板并逐项解释**

逐字使用当前模板，解释 Token、语言、截图数量 `1..=100`、时间点 `HH:MM:SS`/`HH:MM:SS.mmm`/`0%..100%`、字幕开关和语言优先级。明确只有 `-p` 强制要求非空 TMDB Token。

### Task 2: 动作参数、组合规则与输出

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: `src/cli.rs` 的 `ActionSet`、`CombinedRequest` 和 `run_with_services()`。
- Produces: 可复制的单项、组合、场景示例与输出目录说明。

- [ ] **Step 1: 编写位置参数规则**

展示三种条件语法：

```text
crabgrab -p <RESOURCE_ID> <OUTPUT>
crabgrab -smt <VIDEO> <OUTPUT>
crabgrab -psmt <RESOURCE_ID> <VIDEO> <OUTPUT>
```

解释 `RESOURCE_ID`、`VIDEO`、`OUTPUT` 的格式和何时可以省略。

- [ ] **Step 2: 逐项解释四个动作**

写明 `-p` 的两张图片、`-s` 的截图目录和清单、`-m` 的 `mediainfo.txt`、`-t` 的 `<目录名>.tree.txt`，并明确独立 `-m` 用法：

```bash
crabgrab -m movie.mkv ./result
```

- [ ] **Step 3: 编写组合矩阵与固定执行顺序**

给出常用两项、三项和四项组合；说明 `-tmsp` 与 `-psmt` 选中同一组动作，但总按 `p → m → s → t` 执行，失败后停止后续动作。

- [ ] **Step 4: 编写实际场景和路径示例**

覆盖电影、电视剧、只截图、MediaInfo 加 tree、全功能及带中文/空格路径。路径含空格时使用 shell 引号。展示最终目录树，其中 tree 文件名带点号分隔。

### Task 3: 后置参考、开发流程和事实验证

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: 旧 CLI 入口、事务写入实现、`scripts/fetch-mediainfo.sh`、`scripts/fetch-ffmpeg.sh` 和工具清单。
- Produces: 兼容说明、错误排查、源码开发和验证完成的 README。

- [ ] **Step 1: 编写旧命令兼容说明**

列出旧图片、MediaInfo、截图和 `--tree` 长参数入口；说明旧 `-t VIDEO -o OUTPUT` 已由新的布尔 `-t` 取代。

- [ ] **Step 2: 编写覆盖与错误排查**

说明配置不覆盖、各产物完整提交、截图目录所有权保护、失败即停和工具 SHA-256 校验；给出缺配置、Token 空、ID 错、视频不存在、输出路径错误、sidecar 缺失/损坏和截图目录冲突的处理方向。

- [ ] **Step 3: 编写源码开发章节**

在章节标题和首句标注“仅源码开发者需要”。展示 macOS Apple Silicon 开发环境的两个下载脚本以及 `cargo build`、`cargo test`。说明脚本使用固定清单安装到 `.crabgrab-tools/`，Release 用户无需执行。

- [ ] **Step 4: 对照实现验证文档**

Run: `cargo run -- --help`

Expected: README 中四个短参数、兼容入口和三种组合语法与帮助一致。

Run: `git diff --check`

Expected: 无格式错误。

手工核对清单：配置模板与 `CONFIG_TEMPLATE` 逐字一致；普通安装章节不含 `fetch-mediainfo.sh`、`fetch-ffmpeg.sh` 或 `PATH` 配置要求；这些脚本只出现在源码开发章节；所有 tree 示例均使用 `.tree.txt`。
