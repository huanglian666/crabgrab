# CrabGrab 第三阶段：便携式 MediaInfo 报告设计

## 阶段目标

为 CrabGrab 增加独立的 MediaInfo 报告能力。用户提供一个本地媒体文件和输出目录，程序驱动随 CrabGrab 发布的预编译 MediaInfo CLI，并在输出目录中生成 `mediainfo.txt`。

CrabGrab 采用便携版发布，不安装 MediaInfo、不修改系统 `PATH`、不要求用户额外配置软件。项目不再集成 MediaInfoLib 源码，不使用 C++、CMake、FFI、静态链接或 `build.rs` 编译原生库。

本阶段正式支持：

- Windows x86_64：`x86_64-pc-windows-msvc`。
- macOS Apple Silicon：`aarch64-apple-darwin`。

Linux 是后续目标，本阶段不发布或承诺 Linux 产物。

## 核心决策

- 采用 MediaArea 官方预编译的 MediaInfo CLI。
- Windows 从官方 x64 ZIP 提取 `mediainfo.exe`。
- macOS 从官方 CLI DMG 提取已经编译好的通用 `mediainfo`，不编译源码。
- MediaInfo 固定到明确版本；下载地址和 SHA-256 固定在工具清单中。
- MediaInfo 二进制不提交进 Git。
- 开发者通过显式脚本下载一次，`cargo build`、`cargo test` 不自动下载或编译 MediaInfo。
- GitHub Actions 按清单下载、校验、提取并组装便携发布包。
- 运行时只使用随包固定版本；缺失、不可执行或校验失败时直接报错，不搜索系统 `PATH`，也不联网补装。

## CLI 设计

```bash
crabgrab mediainfo -i <媒体文件> -o <输出目录>
crabgrab mediainfo --input <媒体文件> --output <输出目录>
```

- `mediainfo` 是独立子命令。
- `-i/--input` 表示单个本地媒体文件。
- `-o/--output` 表示输出目录。
- 两个参数都必须提供，每次只分析一个文件。
- 子命令不读取 TMDB 配置，不访问 TMDB，不修改 `background` 和 `cover`。
- 现有顶层 TMDB 下载、版本、帮助和 `config` 行为保持不变。

成功后输出：

```text
mediainfo: <输出目录>/mediainfo.txt
```

错误写入标准错误，进程以非零状态退出。

## 开发与发布目录

本地开发工具目录：

```text
.crabgrab-tools/
└── mediainfo/
    └── aarch64-apple-darwin/
        └── mediainfo
```

`.crabgrab-tools/` 必须加入 `.gitignore`。显式下载脚本根据当前或指定目标平台获取官方产物，校验 SHA-256 后通过临时目录提取，并原子安装到该目录。失败时不留下被误认为有效的半成品。

Windows 便携包：

```text
crabgrab-windows-x86_64/
├── crabgrab.exe
├── tools/
│   └── mediainfo.exe
└── licenses/
    └── MediaInfo.txt
```

macOS 便携包：

```text
crabgrab-macos-arm64/
├── crabgrab
├── tools/
│   └── mediainfo
└── licenses/
    └── MediaInfo.txt
```

正式运行时，以 `current_exe()` 所在目录为基准，只定位 `tools/mediainfo.exe` 或 `tools/mediainfo`。开发测试可显式注入 `.crabgrab-tools` 中的路径，但生产逻辑不接受任意 `PATH` 回退。

## 输入与输出

- 输入必须存在、可读取并且是普通文件。
- 不根据扩展名维护视频格式白名单。
- 需要时自动创建输出目录；已存在时必须是目录。
- 子命令只生成 `<输出目录>/mediainfo.txt`。
- `mediainfo.txt` 已存在时默认安全覆盖。

最终目录可以与图片输出共存：

```text
<输出目录>/
├── background/background.jpg
├── cover/cover.jpg
└── mediainfo.txt
```

## MediaInfo 调用协议

Rust 使用 `std::process::Command` 直接启动辅助程序，不通过 Shell。

- 每次只传入一个输入路径。
- 使用 MediaInfo 默认英文标准文本视图。
- 不启用 `--Full`，避免输出大量重复的原始值和内部字段。
- 不使用 `--Language=raw`，因为它会输出 `CompleteName`、`Format/String` 等内部字段名。
- 清除可能影响语言选择的环境变量，并以英文标准输出为验收结果。
- 标准输出和标准错误分开捕获。
- 不把媒体路径拼接成命令字符串，中文、空格和特殊字符由操作系统参数接口原样传递。
- 可设置合理的输出大小上限，防止异常辅助程序耗尽内存；本阶段不强制中断正常的长时间媒体分析。

只有以下条件全部满足才接受报告：

- 辅助程序成功启动。
- 退出码为零。
- 标准输出是有效 UTF-8 且非空。
- 报告包含 `General`，并至少包含 `Video`、`Audio`、`Text`、`Image`、`Menu` 或 `Other` 中一个实际媒体分区。

失败时错误包含阶段、输入路径、退出码以及经过长度限制的标准错误，但不输出媒体内容。

## 工具完整性与版本固定

项目维护单一工具清单，记录：

- MediaInfo 版本。
- 两个平台的官方来源 URL。
- 原始下载文件 SHA-256。
- 提取后辅助程序 SHA-256。
- 目标平台和预期文件名。

下载脚本和 GitHub Actions 共用该清单，禁止使用浮动的“latest”地址。运行前验证辅助程序存在、是普通文件、平台名称正确、可执行权限正确，并与清单中的提取后 SHA-256 一致。校验失败立即拒绝运行。

版本升级必须显式修改清单、重新计算校验值、运行两平台测试并审核 MediaInfo 输出变化。

## 报告格式与安全覆盖

- 报告采用 MediaInfo 英文标准文本格式，与执行 `mediainfo <文件>` 的内容一致。
- 保留 `General`、`Video`、`Audio`、`Text` 等实际存在的分区和厂商自定义字段。
- 保留 `Complete name`。
- 输出编码固定为 UTF-8。
- 换行统一为 LF，文件末尾恰好一个换行。
- 同一 CrabGrab 版本固定 MediaInfo 版本；升级后不保证逐字一致。

覆盖流程：

1. 分析完成前不触碰正式报告。
2. 在输出目录中创建本次操作专用临时文件。
3. 完整写入、刷新并同步临时文件。
4. 将旧报告移动到本次操作专用备份路径。
5. 将临时文件重命名为 `mediainfo.txt`。
6. 成功后删除备份；失败时尽最大可能恢复旧文件。

并发写入同一目录不保证顺序，但不同进程必须使用独立临时文件，不能暴露半写入报告，也不能删除不属于本次操作的文件。

## 组件职责

```text
src/
├── cli.rs
├── media_info.rs
└── media_info/
    ├── process.rs
    ├── install.rs
    └── tool.rs

scripts/
└── fetch-mediainfo.*

tools/
└── mediainfo-manifest.*
```

- `cli.rs`：参数解析和命令分派。
- `media_info.rs`：输入验证、分析编排、报告验证与统一错误。
- `process.rs`：安全启动 MediaInfo、捕获退出状态和输出。
- `install.rs`：换行规范化和事务式写入。
- `tool.rs`：辅助程序定位、版本及 SHA-256 校验。
- 下载脚本：显式下载和提取开发工具，不参与 Cargo 构建。
- 工具清单：固定版本、URL、平台和校验值。
- GitHub Actions：使用同一清单组装便携发布包。

Rust 代码通过可注入的 `MediaAnalyzer` 与进程实现解耦。绝大多数测试使用假的分析器；只有少量 sidecar 集成测试启动真实 MediaInfo。

## 数据流

1. clap 解析 `mediainfo` 子命令。
2. Rust 验证输入文件和输出目录。
3. 定位并校验固定版本 MediaInfo 辅助程序。
4. 通过操作系统参数接口启动辅助程序并传入输入路径。
5. 捕获退出码、标准输出和标准错误。
6. 验证英文标准报告结构并规范化换行。
7. 事务式替换 `mediainfo.txt`。
8. 输出最终报告路径。

整个运行流程不访问网络。

## 错误处理

- 输入不存在、不是普通文件或不可读：报告相关路径。
- 输出目录无法创建或不是目录：拒绝执行。
- 辅助程序缺失：提示便携包不完整，要求重新解压官方 CrabGrab 包。
- 权限不正确：提示辅助程序不可执行。
- SHA-256 不匹配：提示完整性校验失败，禁止执行。
- 进程无法启动：报告系统错误。
- 非零退出：报告退出码及经过截断的标准错误。
- 输出为空、非 UTF-8 或没有媒体分区：拒绝生成新报告。
- 写入、替换或回滚失败：报告具体阶段；如果回滚也失败，同时保留两个错误。

任何分析失败都不得覆盖已有的有效 `mediainfo.txt`。

## 下载、许可证与供应链

- 仅从 MediaArea 官方 HTTPS 地址下载。
- 下载到临时文件后先校验原始包 SHA-256，再提取。
- Windows 只提取官方 ZIP 中所需的 CLI 和许可证材料。
- macOS 只从官方 CLI DMG 提取已编译程序和许可证材料，不执行安装器脚本。
- 发布包必须包含 MediaInfo 二进制再分发所要求的版权和许可证文本。
- 工具清单和下载脚本提交到 Git；下载到的二进制、DMG、ZIP 和提取目录不提交。

## 测试策略

### Rust 单元测试

- 输入和输出路径验证。
- 辅助程序缺失、类型错误、权限错误和哈希错误。
- 子进程成功、启动失败、非零退出、空输出、非 UTF-8 和过长错误输出。
- 报告分区验证、CRLF 规范化和末尾换行。
- 旧报告安全替换以及失败回滚。

### CLI 与回归测试

- 短参数和长参数均可解析，缺少参数时失败。
- MediaInfo 子命令不读取 TMDB 配置或访问网络。
- 现有版本、帮助、配置和 TMDB 下载测试继续通过。

### 真实 sidecar 集成测试

- 使用体积小、来源与许可证明确的媒体样本。
- 报告包含 `General`、`Video` 和标准英文字段。
- 中文与空格路径可以分析。
- 非媒体文件产生受控错误且不破坏旧报告。
- 真实测试只在已经显式获取工具或发布 CI 中运行；普通 Rust 单元测试不隐式下载工具。

### 发布验证

- Windows x86_64 与 macOS arm64 分别下载并校验官方产物。
- 在未安装系统 MediaInfo 的环境中运行便携包冒烟测试。
- 临时移除 `tools/mediainfo[.exe]` 后必须报告包不完整，而不是回退到系统命令。
- 发布 ZIP/TAR.GZ 附带 SHA-256 校验文件。

## 明确不做

- 不集成或编译 MediaInfoLib、ZenLib、zlib 源码。
- 不引入 C++、CMake、FFI、静态链接和 Git submodule。
- 不调用系统 `PATH` 中的 MediaInfo。
- 不在运行时自动下载或修复辅助程序。
- 不把 MediaInfo 二进制提交进 Git。
- 不制作系统安装器；本阶段只发布便携压缩包。
- 不实现多文件批处理、自定义模板、JSON/XML/HTML/CSV、本地化参数和 `--Full` 模式。
- 不修改、转码、修复或上传媒体文件。
- 不发布 Linux 产物。

## 完成标准

- 两个正式平台的便携包都包含固定版本、校验通过的 MediaInfo CLI。
- 用户解压后无需安装其他软件即可生成 `mediainfo.txt`。
- Cargo 构建不下载、不编译、不链接任何 MediaInfo 原生源码。
- 报告格式符合 MediaInfo 英文标准文本。
- 中文、空格和特殊字符路径工作正常。
- 辅助程序缺失或被修改时明确失败，不使用系统版本。
- 安全覆盖不会因分析失败破坏旧报告。
- 发布包包含所需许可证和校验文件。
- 现有 TMDB 与 CLI 功能无回归。
