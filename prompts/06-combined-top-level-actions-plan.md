# 顶层组合参数实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `crabgrab -psmt [RESOURCE_ID] [VIDEO] OUTPUT`，其中 `-m` 独立生成 MediaInfo 报告，并允许按动作省略不需要的位置参数。

**Architecture:** 在 `src/cli.rs` 中把 clap 原始开关与位置参数转换为经过验证的 `CombinedRequest`，再由串行编排器按照 `p → m → s → t` 调用现有业务函数。旧式图片、MediaInfo、截图命令继续工作；旧 tree 的短参数让位给布尔 `-t`，保留 `--tree VIDEO --output OUTPUT`。

**Tech Stack:** Rust 2024、clap 4 derive、现有可注入 MediaInfo/截图服务、Cargo 测试。

## Global Constraints

- `-p`、`-s`、`-m`、`-t` 都是不带值并且可以聚合的短参数。
- `-m` 必须能够单独使用：`crabgrab -m VIDEO OUTPUT`。
- 仅 `-p` 使用 `RESOURCE_ID`；任一 `-s/-m/-t` 使用 `VIDEO`；最后一个位置参数始终是 `OUTPUT`。
- 固定串行顺序为 `p → m → s → t`，失败后停止，tree 最后扫描。
- 不改变各业务模块的文件名、事务写入和辅助工具协议。
- 不新增依赖，不增加并行执行，不删除现有独立子命令。

---

### Task 1: 组合参数解析与兼容语法

**Files:**
- Modify: `src/cli.rs`
- Modify: `tests/cli.rs`

**Interfaces:**
- Produces: `ActionSet { poster, screenshots, media_info, tree: bool }`
- Produces: `CombinedRequest::parse(actions: ActionSet, arguments: &[String]) -> Result<CombinedRequest, AppError>`
- Produces: `CombinedRequest { actions, resource_id: Option<ResourceId>, video: Option<PathBuf>, output: PathBuf }`

- [ ] **Step 1: 添加失败的 clap 解析测试**

在 `src/cli.rs` 测试模块加入测试，验证 `Cli::try_parse_from(["crabgrab", "-psmt", "tmdb-movie-550", "movie.mkv", "out"])` 同时设置四个开关；另验证 `-m movie.mkv out` 设置 MediaInfo 开关。更新旧 tree 解析测试，使兼容形式为 `--tree movie.mkv --output out`。

- [ ] **Step 2: 运行解析测试并确认失败**

Run: `cargo test cli::tests::parses_combined_short_actions --lib`

Expected: FAIL，因为 `-p/-s/-m` 尚不存在且 `-t` 仍要求取值。

- [ ] **Step 3: 实现 clap 字段**

在 `Cli` 中增加 `poster: bool`、`screenshots: bool`、`media_info: bool`、`tree: bool` 和 `arguments: Vec<String>`。四个动作分别使用短参数 `p/s/m/t` 和 `ArgAction::SetTrue`；旧 tree 字段改名为 `legacy_tree`，只保留 `long = "tree"`。调整顶层冲突检查，使组合位置参数不能与 `-i/-o/--tree` 或子命令混用。

- [ ] **Step 4: 添加请求转换失败测试**

覆盖以下精确案例：`-m movie.mkv out`、`-p tmdb-movie-550 out`、`-pm tmdb-movie-550 movie.mkv out` 成功；无动作、缺参、多参、非法资源 ID 失败；错误发生时不读取配置。

- [ ] **Step 5: 实现最小请求转换**

实现 `ActionSet::any()`、`ActionSet::needs_video()` 和 `CombinedRequest::parse()`。根据 `(poster, needs_video)` 只接受 2、2、3 个位置参数，将 ID 解析为 `ResourceId`，将视频与输出转换为 `PathBuf`，其余数量返回包含正确用法的 `AppError::CombinedArguments`。

- [ ] **Step 6: 运行 CLI 解析测试**

Run: `cargo test cli::tests --lib`

Expected: PASS。

### Task 2: 串行组合编排与 `-m` 执行

**Files:**
- Modify: `src/cli.rs`

**Interfaces:**
- Consumes: Task 1 的 `CombinedRequest`
- Produces: `run_combined(...) -> Result<(), AppError>`，使用现有 `generate_report`、`generate_screenshots`、`generate_tree_report` 和图片安装流程。

- [ ] **Step 1: 添加 `-m` 的失败执行测试**

使用 `FakeAnalyzer`、临时视频和不存在的输出目录，解析 `crabgrab -m VIDEO OUTPUT` 并调用 `run_with_media_analyzer`；断言生成 `OUTPUT/mediainfo.txt`，且不创建或读取 TMDB 配置。

- [ ] **Step 2: 运行单测并确认失败**

Run: `cargo test cli::tests::combined_mediainfo_generates_report_without_tmdb_config --lib`

Expected: FAIL，因为组合命令尚未调度。

- [ ] **Step 3: 实现组合预检和 MediaInfo 调度**

组合入口先验证视频是普通文件，输出存在时必须为目录，不存在时创建。选择 `-m` 时调用 `generate_report(analyzer, video, output)` 并打印 `mediainfo: PATH`。只选择 `-m` 时不得加载配置。

- [ ] **Step 4: 添加 `-mt` 顺序失败测试**

执行 `crabgrab -mt VIDEO OUTPUT`，断言 tree 文件存在且内容包含 `mediainfo.txt`，由此证明 `m` 先于 `t`；再验证 tree 报告包含外部视频的虚拟条目。

- [ ] **Step 5: 实现完整串行调度**

按 `p → m → s → t` 顺序调用现有功能。`p` 复用 TMDB 配置、provider 和图片安装流程；`s` 复用截图配置与可注入服务；`t` 最后调用 tree。每步沿用当前成功摘要，末尾打印按请求顺序构造的 `completed: ...`。

- [ ] **Step 6: 添加失败即停测试**

让 MediaInfo fake 返回错误并请求 `-mt`，断言命令失败且 tree 文件不存在。该测试证明中间失败不会启动后续动作。

- [ ] **Step 7: 运行库测试**

Run: `cargo test --lib`

Expected: PASS。

### Task 3: 二进制 CLI 行为、帮助和回归验证

**Files:**
- Modify: `tests/cli.rs`
- Modify: `src/cli.rs`

**Interfaces:**
- Consumes: Task 1 和 Task 2 的最终 CLI。
- Produces: 面向用户的帮助、参数错误与兼容行为。

- [ ] **Step 1: 添加二进制帮助与错误测试**

断言 `--help` 显示 `-p`、`-s`、`-m`、`-t` 及位置参数说明。调用 `-m` 缺少输出、`-p` 缺少 ID、组合参数混用 `-m ... -o ...` 时必须非零退出并显示对应正确用法。

- [ ] **Step 2: 运行集成测试并确认新增断言失败**

Run: `cargo test --test cli`

Expected: FAIL，直到帮助文本和错误信息补齐。

- [ ] **Step 3: 完善 clap 帮助与错误信息**

为四个动作和位置参数添加中文含义清晰的 help/value_name；确保 `AppError::CombinedArguments` 显示三种条件语法；把旧 `--tree` 标记为 deprecated 帮助说明，但保持可执行。

- [ ] **Step 4: 运行格式化和完整测试**

Run: `cargo fmt --check`

Expected: PASS。

Run: `cargo test`

Expected: PASS，现有图片、截图、MediaInfo 和 tree 测试无回归。

- [ ] **Step 5: 运行静态检查**

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: PASS，无 warning。

- [ ] **Step 6: 检查最终差异**

Run: `git diff --check`

Expected: 无输出。确认只修改 `src/cli.rs`、`tests/cli.rs`、本设计文档和本计划文档，且没有构建产物进入版本控制。
