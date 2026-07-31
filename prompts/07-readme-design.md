# CrabGrab README 设计

## 目标

为 CrabGrab 编写一份以实际使用为优先的中文 `README.md`。读者打开文档后应能快速完成安装、初始化配置并执行单项或组合任务；构建原理、开发工具准备和错误排查放在后半部分。

README 必须严格匹配当前 CLI、配置模板和发布包结构，不向 Release 用户描述不必要的 MediaInfo 或 FFmpeg 安装步骤。

## 目标读者与写作原则

- 主要读者是下载 GitHub Release 便携包的普通用户。
- 次要读者是从源码构建和测试 CrabGrab 的开发者。
- 开头简洁，优先回答“如何安装、如何配置、如何运行”。
- 命令提供可复制示例，并明确位置参数的顺序。
- 配置字段逐项说明用途、默认值、约束和示例。
- 开发者内容与普通用户内容明确分隔，避免用户误以为必须手动安装工具。
- 文档使用中文，命令、文件名、配置键和程序原始错误保留英文。

## README 结构

### 1. 项目简介

用短段落说明 CrabGrab 可在一次命令中完成 TMDB 图片刮削、MediaInfo 报告、视频截图和文件树报告。列出当前支持的发布平台：macOS Apple Silicon 与 Windows x86_64。

### 2. 快速安装

普通用户流程只包含：

1. 从 GitHub Releases 下载与系统匹配的便携包。
2. 完整解压发布包。
3. 在终端或 PowerShell 中运行 `crabgrab --help` 验证。

必须说明发布包已内置 MediaInfo 和 FFmpeg，用户无需额外下载、安装或配置 `PATH`。同时提醒必须保留 `crabgrab`、`tools/` 和许可证文件的相对目录结构，不能只复制主程序。

README 不虚构尚未给出的 Release URL；使用仓库 Releases 页面这一通用说法。

### 3. 三步快速上手

展示以下顺序：

```bash
crabgrab config init
# 编辑命令输出的 config.toml
crabgrab -psmt tmdb-movie-550 movie.mkv ./result
```

说明 Windows PowerShell 使用 `crabgrab.exe`，其余参数相同。

### 4. 初始化与查找配置

详细说明 `crabgrab config init`：

- 优先在 CrabGrab 可执行文件同级创建 `config.toml`。
- 只有可执行文件目录明确不可写时，才回退到系统配置目录下的 `crabgrab/config.toml`。
- 命令打印实际创建路径。
- 任一候选位置已有配置时拒绝创建第二份，也不会覆盖旧文件。
- 运行时优先读取便携配置；便携配置存在但无效时不会回退。

系统配置路径只描述为操作系统标准配置目录，避免写死可能因系统版本或账户环境变化的绝对路径；以命令实际输出为准。

### 5. 配置模板与字段

完整展示程序当前生成的 TOML 模板：

```toml
[tmdb]
api_token = ""
language = "zh-CN"

[screenshot]
count = 3
timestamps = []
subtitles = true
subtitle_languages = ["zh-CN", "zh", "chs", "chi"]
```

逐项说明：

- `tmdb.api_token`：填写用户自己的 TMDB API Read Access Token；使用 `-p` 时不能为空；不得填写 API key 查询参数形式。
- `tmdb.language`：TMDB 元数据和海报语言，默认 `zh-CN`。
- `screenshot.count`：目标截图数量，允许 `1..=100`。
- `screenshot.timestamps`：显式截图时间点；支持当前实现接受的绝对时间、毫秒时间和百分比格式，并给出配置示例。
- `screenshot.subtitles`：是否尝试烧录字幕。
- `screenshot.subtitle_languages`：字幕语言优先级，至少一个非空元素；从左到右匹配。

说明 `-m` 和 `-t` 自身不需要 TMDB Token；`-s` 读取截图配置但允许 TMDB Token 留空；只有 `-p` 强制要求有效 Token。

### 6. 组合参数使用

首先给出位置参数规则：

```text
仅 -p：             crabgrab -p    <RESOURCE_ID> <OUTPUT>
任意 -s/-m/-t：     crabgrab -smt  <VIDEO>       <OUTPUT>
-p 加媒体类功能：   crabgrab -psmt <RESOURCE_ID> <VIDEO> <OUTPUT>
```

分别解释：

- `-p` 下载 `background/background.jpg` 和 `cover/cover.jpg`。
- `-s` 创建 `screenshots/`、PNG 图片及清单。
- `-m` 创建 `mediainfo.txt`。
- `-t` 创建 `<输出目录名>.tree.txt`，并将外部视频作为虚拟条目列入报告。

动作开关书写顺序任意，例如 `-tmsp` 与 `-psmt` 选择相同功能；实际执行顺序始终固定为 `p → m → s → t`。tree 最后运行，因此会包含本次已生成的产物。任一步失败后停止，不执行后续步骤；已成功且完整提交的结果保留。

提供单项、两项、三项和四项组合示例，特别包含独立 `-m`：

```bash
crabgrab -m movie.mkv ./result
```

### 7. 常用场景与输出

提供电影、电视剧、只截图、MediaInfo 加 tree、全功能以及带空格/中文路径的示例。展示完整输出目录树，并采用 `<目录名>.tree.txt` 文件名。

### 8. 兼容命令

列出现有旧入口：

```bash
crabgrab -i <RESOURCE_ID> -o <OUTPUT>
crabgrab mediainfo -i <VIDEO> -o <OUTPUT>
crabgrab sc -i <VIDEO> -o <OUTPUT>
crabgrab --tree <VIDEO> --output <OUTPUT>
```

说明新脚本推荐使用组合短参数；旧 tree 短形式已被新的布尔 `-t` 替代。

### 9. 覆盖、失败与排查

简述各模块的事务式写入、截图目录所有权保护、配置不覆盖、工具完整性校验和常见错误。排查内容包括缺少配置、Token 为空、资源 ID 格式错误、视频不存在、输出不是目录、工具文件缺失或校验失败，以及截图目录不是 CrabGrab 所有目录。

### 10. 源码开发

放在 README 后部，并明确标注“仅源码开发者需要，GitHub Release 用户请忽略”。内容包括：

```bash
./scripts/fetch-mediainfo.sh
./scripts/fetch-ffmpeg.sh
cargo build
cargo test
```

说明脚本按照固定清单下载并校验开发用 sidecar，安装到 `.crabgrab-tools/`；普通 Release 包已经包含运行时 `tools/`，不执行这些脚本。

## 验证

- 将 README 中的命令与 `cargo run -- --help` 对照。
- 将配置模板逐字与 `CONFIG_TEMPLATE` 对照。
- 检查所有 tree 文件名使用 `.tree.txt`。
- 检查普通用户安装章节没有工具下载或系统 `PATH` 步骤。
- 检查源码工具脚本只出现在后部开发者章节。
- 扫描占位文本、失效命令和与当前平台支持范围矛盾的描述。
