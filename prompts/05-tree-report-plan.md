# 文件树报告实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `crabgrab -t <VIDEO> -o <DIRECTORY>`，递归生成带准确文件大小和外部视频虚拟条目的树形报告。

**Architecture:** 新建独立 `tree_report` 模块完成验证、目录快照、渲染、自身大小求稳和临时文件原子安装；CLI 只负责解析互斥参数、调用模块并输出最终路径。目录节点使用内存树结构统一排序，目标报告作为特殊文件节点参与大小固定点计算。

**Tech Stack:** Rust 2024、clap 4、thiserror 2、tempfile 3、标准库文件系统 API。

## Global Constraints

- 命令形式固定为 `crabgrab -t <VIDEO> -o <DIRECTORY>` 和对应长选项。
- 不复制、移动或修改视频源，不读取 TMDB 配置，不访问网络。
- 使用 UTF-8、LF 和二进制大小单位 `B/KiB/MiB/GiB/TiB`。
- 同层目录优先，再按 UTF-8 文件名排序；不递归符号链接。
- `tree.txt` 的显示大小必须等于最终文件的实际 UTF-8 字节数。
- 失败时不得破坏已有的有效报告。
- 直接在当前工作区执行；用户验收前不暂存或提交代码。

---

### Task 1: 树快照、排序与大小渲染

**Files:**
- Create: `src/tree_report.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces: `pub fn generate_tree_report(video: &Path, output: &Path) -> Result<PathBuf, TreeReportError>`。
- Produces: `TreeReportError`，包含输入、遍历、UTF-8 名称、同名冲突、大小不收敛和安装失败错误。

- [ ] **Step 1: 写入失败单元测试**

在 `src/tree_report.rs` 的测试模块先定义期望行为：临时输出目录中创建 `资料/01.jpg`、根级文本和外部 `Movie.mp4`，断言报告字面量包含正确连接符、目录优先排序以及 `3 B`、`1.00 KiB` 等手工推导值；再分别覆盖 `1024^2`、`1024^3`、`1024^4` 的纯格式函数边界。

- [ ] **Step 2: 验证测试因功能缺失而失败**

Run: `cargo test tree_report::tests --lib`

Expected: FAIL，原因是 `generate_tree_report`、快照或大小渲染尚未实现，而不是测试夹具错误。

- [ ] **Step 3: 实现最小快照与渲染器**

实现：

```rust
pub fn generate_tree_report(video: &Path, output: &Path) -> Result<PathBuf, TreeReportError>;

#[derive(Debug)]
enum EntryKind {
    Directory(Vec<Entry>),
    File(u64),
    Report,
}

#[derive(Debug)]
struct Entry {
    name: String,
    kind: EntryKind,
}
```

用 `symlink_metadata` 跳过符号链接，递归收集普通目录和文件；目标报告始终转换为唯一 `Report` 节点。以 `(是否文件, name)` 排序，递归渲染 `├──`、`└──`、`│   ` 和 `    `。`format_size(u64)` 在字节时返回整数，其余单位使用 `value / 1024_f64.powi(n)` 和两位小数。

- [ ] **Step 4: 运行单元测试并确认通过**

Run: `cargo test tree_report::tests --lib`

Expected: PASS。

### Task 2: 自引用大小、去重与安全安装

**Files:**
- Modify: `src/tree_report.rs`

**Interfaces:**
- Consumes: Task 1 的 `Entry` 树和渲染函数。
- Produces: 完整可用的 `generate_tree_report`，成功返回最终绝对或调用方语义一致的报告路径。

- [ ] **Step 1: 写入失败测试**

增加真实文件系统测试并断言：报告中解析出的自身大小等于 `fs::metadata(report).len()`；第二次生成可替换旧内容；外部视频内容与路径不变；视频位于输出树时只出现一次；外部视频与根级同名文件冲突时报错且保留旧报告；无效视频和非目录输出报错。

- [ ] **Step 2: 验证新增测试正确失败**

Run: `cargo test tree_report::tests --lib`

Expected: FAIL 于尚未实现的固定点、去重、冲突或安全替换行为。

- [ ] **Step 3: 实现固定点和临时文件安装**

以报告大小 `0` 开始渲染，取 `contents.as_bytes().len() as u64` 作为下一轮大小；最多迭代 32 次，连续两轮一致即收敛。使用 `tempfile::NamedTempFile::new_in(output)`、`write_all`、`sync_all` 和 `persist(report_path)` 安装；任何失败都让临时文件自动清理，并在错误中保留路径和底层原因。通过规范化的输出目录和视频路径判断视频是否已在树内；外部视频只追加到根节点。

- [ ] **Step 4: 运行模块测试并确认通过**

Run: `cargo test tree_report::tests --lib`

Expected: PASS，且报告元数据大小与报告内显示值一致。

### Task 3: CLI 集成与回归验证

**Files:**
- Modify: `src/cli.rs`
- Test: `tests/cli.rs`

**Interfaces:**
- Consumes: `tree_report::generate_tree_report(video, output)`。
- Produces: 顶层 `tree: Option<PathBuf>` 参数和 `AppError::TreeReport` 错误映射。

- [ ] **Step 1: 写入失败的解析与端到端测试**

在 CLI 单元测试中覆盖 `-t movie.mp4 -o result`、`--tree movie.mp4 --output result` 和 `-i tmdb:movie:1 -t movie.mp4 -o result` 冲突。增加不需要 TMDB 配置的调度测试：创建真实视频及输出夹具，执行 CLI 后断言 `<目录名>.tree.txt` 存在、包含视频条目且视频仍在原路径。

- [ ] **Step 2: 验证 CLI 测试正确失败**

Run: `cargo test cli --lib`

Expected: FAIL，原因是 clap 尚无 `-t/--tree` 参数或尚未调度树报告。

- [ ] **Step 3: 实现 CLI 最小集成**

在 `Cli` 增加：

```rust
#[arg(short = 't', long, requires = "output", conflicts_with = "id", value_name = "VIDEO")]
tree: Option<PathBuf>,
```

调整 `output` 的 clap 约束，使其可被 `id` 或 `tree` 使用，并保持孤立 `-o` 报错。`run_with_services` 在子命令冲突检查后优先匹配 `tree`，调用 `generate_tree_report` 并输出 `tree: <path>`；只有下载分支读取 TMDB 配置。

- [ ] **Step 4: 运行 CLI 与完整测试套件**

Run: `cargo test --all-targets`

Expected: 所有测试 PASS，无 warning。

- [ ] **Step 5: 格式与静态检查**

Run: `cargo fmt --check`

Expected: PASS；若失败，运行 `cargo fmt` 后重新执行 `cargo fmt --check` 和 `cargo test --all-targets`。

Run: `cargo clippy --all-targets -- -D warnings`

Expected: PASS。

- [ ] **Step 6: 人工冒烟检查**

在临时目录创建中文名称、嵌套图片和外部稀疏视频，运行构建出的 CLI；检查树形视觉、排序、每个文件大小和视频未移动。不得把冒烟夹具写入仓库。
