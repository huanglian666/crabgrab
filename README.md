# CrabGrab

CrabGrab 是一个面向影视资源整理的命令行工具。它可以在一次命令中完成：

- 从 TMDB 下载背景图和海报；
- 生成带可选字幕的视频截图；
- 导出 MediaInfo 文本报告；
- 生成包含文件大小的目录树报告。

当前发布目标为 macOS Apple Silicon 和 Windows x86_64。

## 快速安装

1. 在本项目的 GitHub Releases 页面下载与你的系统匹配的发布包。
2. 将发布包完整解压到一个固定目录。
3. 打开终端或 PowerShell，进入解压目录并检查程序是否可以运行。

macOS：

```bash
./crabgrab --help
```

Windows PowerShell：

```powershell
.\crabgrab.exe --help
```

发布包已经内置 CrabGrab 所需的 MediaInfo 和 FFmpeg。普通用户不需要单独下载它们，也不需要配置系统 `PATH`。

请保留发布包的完整目录结构，不要只复制 `crabgrab` 或 `crabgrab.exe`：

```text
crabgrab-release/
├── crabgrab                  # Windows 中为 crabgrab.exe
├── tools/
│   ├── mediainfo            # Windows 中为 mediainfo.exe
│   └── ffmpeg               # Windows 中为 ffmpeg.exe
└── licenses/
```

`tools/` 中的程序缺失、被修改或移动后，MediaInfo 和截图功能会拒绝运行。

## 三步快速上手

```bash
# 1. 生成配置文件
crabgrab config init

# 2. 按命令输出的路径打开 config.toml，填写 TMDB Token

# 3. 一次完成海报、MediaInfo、截图和 tree 报告
crabgrab -psmt tmdb-movie-550 movie.mkv ./result
```

在 macOS 解压目录中运行时，命令可能需要写成 `./crabgrab`；Windows PowerShell 中写成 `.\crabgrab.exe`。本文后续统一简写为 `crabgrab`。

## 初始化配置

运行：

```bash
crabgrab config init
```

程序会创建配置模板并打印实际文件路径。创建规则如下：

1. 优先在 `crabgrab` 可执行文件同级创建 `config.toml`，便于随便携包一起使用。
2. 只有可执行文件所在目录明确不可写时，才回退到操作系统的标准用户配置目录，并使用其中的 `crabgrab/config.toml`。
3. 两个候选位置中只要已有配置，程序就会拒绝创建第二份配置。
4. `config init` 永远不会覆盖已有文件。

不同系统和账户的标准配置目录可能不同，请以 `config init` 成功后打印的路径为准。

运行其他命令时，CrabGrab 同样优先读取可执行文件同级的 `config.toml`。如果这份便携配置存在但内容无效，程序会直接报错，不会静默改用系统配置目录中的另一份文件。

## 配置文件模板

`config init` 生成以下模板：

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

### `[tmdb]`

#### `api_token`

填写你自己的 TMDB API Read Access Token：

```toml
api_token = "你的 TMDB API Read Access Token"
```

- 使用 `-p` 下载图片时，此字段必须填写且不能为空。
- 应填写 Read Access Token，而不是传统的 API Key 查询参数。
- Token 只保存在本地配置文件中，不应提交到 Git 或发送给他人。
- 单独使用 `-s`、`-m` 或 `-t` 时，不要求填写 TMDB Token。

#### `language`

控制 TMDB 元数据请求和海报语言：

```toml
language = "zh-CN"
```

默认值是 `zh-CN`。需要其他语言时可以换成相应的 TMDB 语言代码，例如 `en-US`。

### `[screenshot]`

#### `count`

目标截图数量：

```toml
count = 5
```

- 默认值为 `3`。
- 允许范围为 `1` 到 `100`。
- 当 `timestamps` 数量少于 `count` 时，程序会在视频的安全时间范围内补充随机时间点。
- 当唯一的显式时间点多于 `count` 时，程序会保留全部显式时间点，因此实际截图数可以超过 `count`。

#### `timestamps`

指定必须截图的时间点。空数组表示全部由程序生成：

```toml
timestamps = []
```

可以同时使用三种格式：

```toml
timestamps = [
  "00:10:30",      # HH:MM:SS
  "01:02:03.500",  # HH:MM:SS.mmm
  "65.5%",         # 视频时长百分比
]
```

时间格式规则：

- 小时可以超过 `24`，分钟和秒必须小于 `60`。
- 毫秒部分允许 `1` 到 `3` 位数字。
- 百分比必须大于 `0%` 且小于 `100%`。
- 时间点必须位于视频有效时长内，不能等于视频结尾。
- 重复时间点会被去重并产生警告。

#### `subtitles`

控制截图时是否尝试烧录字幕：

```toml
subtitles = true
```

- `true`：尝试选择外挂或内封字幕并渲染到截图中。
- `false`：生成无字幕截图。

#### `subtitle_languages`

字幕语言优先级，从左到右匹配：

```toml
subtitle_languages = ["zh-CN", "zh", "chs", "chi"]
```

数组至少要包含一个非空值。CrabGrab 会优先查找与视频同目录、同文件名并带语言后缀的外挂字幕，例如：

```text
Movie.mkv
Movie.zh-CN.ass
Movie.zh.srt
```

支持选择的外挂字幕格式包括 ASS、SSA、SRT 和 VTT。没有合适的外挂字幕时，程序会继续尝试可渲染的内封字幕；字幕渲染失败时会输出警告并按可用回退方案继续。

### 一份可直接修改的配置示例

```toml
[tmdb]
api_token = "在这里填写 Read Access Token"
language = "zh-CN"

[screenshot]
count = 6
timestamps = ["00:05:00", "25%", "50%", "75%"]
subtitles = true
subtitle_languages = ["zh-CN", "zh", "chs", "chi", "en"]
```

## `-p/-s/-m/-t` 组合参数

四个短参数都是不带值的功能开关：

| 参数 | 功能 | 主要产物 |
| --- | --- | --- |
| `-p` | 从 TMDB 下载背景图和海报 | `background/background.jpg`、`cover/cover.jpg` |
| `-s` | 生成视频截图 | `screenshots/*.png` 和 `.crabgrab-screenshots` 清单 |
| `-m` | 导出 MediaInfo 信息 | `mediainfo.txt` |
| `-t` | 生成目录树报告 | `<输出目录名>.tree.txt` |

### 位置参数规则

组合命令不使用 `--id`、`--input` 和 `--output`，而是按固定位置传值。

仅使用 `-p` 时，需要资源 ID 和输出目录：

```text
crabgrab -p <RESOURCE_ID> <OUTPUT>
```

使用 `-s`、`-m`、`-t` 中任意一个或多个时，需要视频和输出目录：

```text
crabgrab -smt <VIDEO> <OUTPUT>
```

同时包含 `-p` 和任意媒体类功能时，需要资源 ID、视频和输出目录：

```text
crabgrab -psmt <RESOURCE_ID> <VIDEO> <OUTPUT>
```

参数含义：

- `RESOURCE_ID`：当前支持 `tmdb-movie-<数字 ID>` 和 `tmdb-tv-<数字 ID>`，例如 `tmdb-movie-550`、`tmdb-tv-1399`。
- `VIDEO`：本地视频文件路径。程序要求该路径存在并且是普通文件。
- `OUTPUT`：所有结果写入的父目录；不存在时由组合命令创建，已存在时必须是目录。

### 单项使用

只下载电影图片：

```bash
crabgrab -p tmdb-movie-550 ./result
```

只下载电视剧图片：

```bash
crabgrab -p tmdb-tv-1399 ./result
```

只生成截图：

```bash
crabgrab -s movie.mkv ./result
```

只生成 MediaInfo 报告：

```bash
crabgrab -m movie.mkv ./result
```

只生成 tree 报告：

```bash
crabgrab -t movie.mkv ./result
```

不包含 `-p` 时，资源 ID 可以并且必须省略。

### 组合使用

生成 MediaInfo 和 tree：

```bash
crabgrab -mt movie.mkv ./result
```

生成截图、MediaInfo 和 tree：

```bash
crabgrab -smt movie.mkv ./result
```

下载图片并生成截图：

```bash
crabgrab -ps tmdb-movie-550 movie.mkv ./result
```

下载图片、生成 MediaInfo 和 tree：

```bash
crabgrab -pmt tmdb-movie-550 movie.mkv ./result
```

执行全部功能：

```bash
crabgrab -psmt tmdb-movie-550 movie.mkv ./result
```

### 参数书写顺序与执行顺序

短参数的书写顺序可以任意。以下命令选择的是同一组功能：

```bash
crabgrab -psmt tmdb-movie-550 movie.mkv ./result
crabgrab -tmsp tmdb-movie-550 movie.mkv ./result
crabgrab -mpst tmdb-movie-550 movie.mkv ./result
```

无论短参数如何排列，内部始终按照以下顺序执行：

```text
p → m → s → t
```

因此 tree 总是在最后运行，会把本次已经生成的图片、MediaInfo 和截图列入目录树。任一步失败后，程序立即停止，不再执行后续步骤；前面已经完整生成的结果会保留。

为了便于阅读和交流，建议统一写成 `-psmt`。

### 中文或带空格的路径

路径中包含空格时，请使用引号：

```bash
crabgrab -psmt tmdb-movie-550 "/Media/火遮眼 (2025).mkv" "/Media/火遮眼 (2025)"
```

Windows PowerShell 示例：

```powershell
.\crabgrab.exe -psmt tmdb-movie-550 "D:\影视\火遮眼 (2025).mkv" "D:\影视\火遮眼 (2025)"
```

## 输出目录

执行全部功能后，典型输出如下：

```text
result/
├── background/
│   └── background.jpg
├── cover/
│   └── cover.jpg
├── screenshots/
│   ├── .crabgrab-screenshots
│   ├── 01.png
│   ├── 02.png
│   └── 03.png
├── mediainfo.txt
└── result.tree.txt
```

- tree 报告名由输出目录名生成。例如输出目录是 `火遮眼 (2025)`，报告名就是 `火遮眼 (2025).tree.txt`。
- 如果视频文件不在输出目录中，tree 报告会以虚拟条目的形式列出该视频及其大小，但不会移动或复制视频。
- `.crabgrab-screenshots` 是截图目录的所有权和结果清单，请不要随意删除。

## 旧命令兼容

现有独立命令仍然可用：

```bash
# TMDB 图片
crabgrab -i tmdb-movie-550 -o ./result

# MediaInfo
crabgrab mediainfo -i movie.mkv -o ./result

# 截图
crabgrab sc -i movie.mkv -o ./result

# tree 的旧长参数入口
crabgrab --tree movie.mkv --output ./result
```

新的 `-t` 是不带值的布尔开关，因此旧短形式 `-t movie.mkv -o ./result` 不再使用。新脚本建议统一采用：

```bash
crabgrab -t movie.mkv ./result
```

聚合位置参数不能与旧式 `-i/-o/--tree/--output` 混用。

## 文件覆盖与失败行为

- `config init` 不覆盖已有配置，也不会同时维护两份配置。
- 背景图和海报下载完成后才替换正式文件；其中一张下载失败时不会提交一半结果。
- `mediainfo.txt` 先完整生成，再替换已有报告。
- `screenshots/` 使用临时目录完整生成后再提交。非空且没有有效 `.crabgrab-screenshots` 清单的目录不会被覆盖。
- tree 报告通过临时文件写入并替换，不暴露半写入内容。
- 组合任务失败后停止后续步骤；此前已经成功提交的步骤不会自动回滚。

## 常见问题

### 找不到配置文件

如果错误提示包含 `no configuration file found`，运行：

```bash
crabgrab config init
```

然后编辑命令打印出的配置文件路径。

### 使用 `-p` 时提示 Token 为空

在配置文件中填写：

```toml
[tmdb]
api_token = "你的 TMDB API Read Access Token"
```

确认编辑的是 CrabGrab 实际读取的配置路径，并检查 TOML 引号是否完整。

### 资源 ID 无效

当前只接受以下格式：

```text
tmdb-movie-550
tmdb-tv-1399
```

数字 ID 必须大于零，不能省略 `movie` 或 `tv`。

### 视频不存在或不是文件

检查 `VIDEO` 是否指向真实文件，而不是目录。路径包含空格或括号时应使用引号。

### 输出路径不是目录

如果 `OUTPUT` 已存在，它必须是目录。不要把现有普通文件作为输出路径。

### MediaInfo 或 FFmpeg 缺失、校验失败

Release 中的 `tools/` 文件可能被移动、遗漏或修改。重新下载官方发布包并完整解压，不要从其他来源替换其中的程序。

### 截图目录拒绝覆盖

CrabGrab 不会覆盖无法确认归属的非空 `screenshots/`。请先检查并手动备份该目录；不要为了绕过保护而伪造 `.crabgrab-screenshots` 文件。

### 字幕没有出现在截图中

检查：

- `subtitles = true`；
- 外挂字幕与视频位于同一目录；
- 字幕文件名与视频主体一致，例如 `Movie.zh-CN.ass`；
- 字幕语言存在于 `subtitle_languages`；
- 字幕格式受支持且内容有效。

程序无法渲染首选字幕时会输出 `warning`，并尝试内封字幕或无字幕回退。

## 查看帮助和版本

```bash
crabgrab --help
crabgrab --version
```

短形式：

```bash
crabgrab -h
crabgrab -v
```

## 从源码开发

> 本节仅供源码开发者使用。GitHub Release 用户不需要执行这里的工具下载脚本。

安装 Rust 工具链后，在项目根目录运行：

```bash
./scripts/fetch-mediainfo.sh
./scripts/fetch-ffmpeg.sh
cargo build
cargo test
```

两个脚本根据 `tools/` 中的固定清单下载并校验开发用 MediaInfo 和 FFmpeg，然后安装到：

```text
.crabgrab-tools/
├── mediainfo/<target>/
└── ffmpeg/<target>/
```

这些脚本不会修改系统 `PATH`，也不会把工具安装到系统目录。当前脚本可在 macOS Apple Silicon 上自动识别目标；为其他目标准备开发工具时需要显式传入受支持的 target，并满足脚本所需的系统解压工具。

常用开发检查：

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

正式 Release 包的 `tools/` 在发布流程中组装，普通用户无需执行上述源码开发步骤。

## 许可证

CrabGrab 的许可证见 [LICENSE](LICENSE)。发布包同时包含所捆绑 MediaInfo 和 FFmpeg 的许可证文件。
