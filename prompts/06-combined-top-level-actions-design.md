# CrabGrab 顶层组合参数设计

## 目标

将海报图片刮削、截图生成、MediaInfo 报告和 tree 报告聚合到一次 CrabGrab 调用中。用户通过可组合的短参数选择功能，通过位置参数提供资源 ID、视频和输出目录，不再为聚合调用重复书写 `--id`、`--input` 和 `--output`。

本次只聚合 CLI 和任务编排。各功能现有的输出格式、事务写入规则及底层工具保持不变。

## CLI 语法

四个顶层短参数都是不带值的布尔开关，并允许按 clap 的短参数规则组合：

- `-p`：下载 TMDB 背景图和海报。
- `-s`：生成截图。
- `-m`：生成 `mediainfo.txt`。
- `-t`：生成 tree 报告。

位置参数根据所选功能确定：

```text
仅 -p：             crabgrab -p    <RESOURCE_ID> <OUTPUT>
仅媒体类功能：      crabgrab -smt  <VIDEO>       <OUTPUT>
-p 与媒体类功能：   crabgrab -psmt <RESOURCE_ID> <VIDEO> <OUTPUT>
```

其中“媒体类功能”指 `-s`、`-m`、`-t` 中任意一个或多个。示例：

```bash
crabgrab -psmt tmdb-movie-550 movie.mkv ./result
crabgrab -smt movie.mkv ./result
crabgrab -p tmdb-movie-550 ./result
crabgrab -s movie.mkv ./result
```

至少选择一个功能。参数数量与所选功能不匹配时，在读取配置、访问网络、启动辅助程序或写入文件之前报错。错误必须显示当前组合对应的正确用法。

`-p` 继续使用现有资源 ID 格式，例如 `tmdb-movie-550` 和 `tmdb-tv-1399`。最后一个位置参数始终是输出目录，因此路径包含空格时由 shell 引号处理，不在程序内部拆分。

## 兼容性

保留以下现有入口：

- `crabgrab -i <RESOURCE_ID> -o <OUTPUT>`
- `crabgrab mediainfo -i <VIDEO> -o <OUTPUT>`
- `crabgrab sc -i <VIDEO> -o <OUTPUT>`
- `crabgrab config init`
- `-h/--help` 和 `-v/--version`

现有顶层 `-t <VIDEO> -o <OUTPUT>` 无法与新的无值组合开关 `-t` 同时保持相同语法，因此它属于本次有意的短参数破坏性变更。旧 tree 功能保留一个明确的兼容入口 `crabgrab --tree <VIDEO> --output <OUTPUT>`；帮助文本标注旧入口已弃用并推荐 `crabgrab -t <VIDEO> <OUTPUT>`。后续大版本可删除旧长参数入口。

聚合位置参数和旧式 `-i/-o/--tree/--output` 不允许混用，避免同一个值出现两个来源。

## 参数模型与校验

CLI 解析层先得到四个动作开关和位置参数列表，再将其转换为一个经过验证的 `CombinedRequest`：

```text
CombinedRequest
├── actions: poster / screenshots / mediainfo / tree
├── resource_id: ResourceId?  （仅 -p 时存在）
├── video: PathBuf?           （任一媒体类功能时存在）
└── output: PathBuf           （始终存在）
```

转换过程按以下顺序完成预检：

1. 验证至少有一个动作。
2. 根据动作组合验证位置参数数量。
3. 如果包含 `-p`，解析并验证资源 ID。
4. 如果包含媒体类功能，验证视频存在且是普通文件。
5. 输出路径存在时必须是目录；不存在时创建目录。
6. 只加载所需配置：`-p` 需要 TMDB 配置，`-s` 需要截图配置；`-m` 和 `-t` 不因自身读取配置。

所有可以在本地预先发现的错误都在任务启动前返回，减少执行一半后才发现参数错误的情况。

## 执行顺序与数据流

动作使用固定顺序执行：

```text
参数预检 → p 海报刮削 → m MediaInfo → s 截图 → t tree
```

tree 必须最后执行，确保报告包含本次生成的 `background`、`cover`、`mediainfo.txt` 和 `screenshots`。未选择的步骤直接跳过。

这是一条同步、失败即停的流水线。任一步失败：

- 立即返回非零退出状态，不再启动后续步骤。
- 已成功步骤的完整结果予以保留。
- 失败步骤继续依赖其现有事务写入机制，不暴露半写入文件。
- 错误指出失败动作，例如 `poster`、`mediainfo`、`screenshots` 或 `tree`。

聚合命令代表“一次用户调用”，不承诺所有媒体任务只启动一次辅助进程。当前 MediaInfo 文本报告和截图所需 JSON 探测协议不同，首版继续分别调用既有接口，避免为性能优化改变稳定输出。以后可以在不改变 CLI 的前提下增加媒体元数据缓存。

## 输出

每个成功步骤继续打印现有摘要，顺序与执行顺序一致。全部成功后追加一行：

```text
completed: p,m,s,t
```

该列表只包含实际请求且成功的动作。标准输出适合人工阅读；警告继续写入标准错误。首版不新增 JSON 输出模式。

输出目录结构沿用现有实现：

```text
<OUTPUT>/
├── background/background.jpg
├── cover/cover.jpg
├── mediainfo.txt
├── screenshots/
└── <输出目录名>.tree.txt
```

## 代码边界

- `cli.rs` 定义动作开关、位置参数、兼容入口和调度。
- 新增小型请求解析单元，将 clap 原始值转换为 `CombinedRequest`，集中维护条件参数规则。
- 海报、MediaInfo、截图和 tree 模块保持独立，通过现有公共函数调用。
- 公共步骤（配置解析、服务构造和成功摘要）由编排层复用，不把四个业务模块耦合成一个大函数。

本次不重构各功能内部事务、不改变输出文件名、不增加并行执行。固定串行顺序让失败语义和 tree 快照具有确定性。

## 测试策略

解析测试覆盖：

- `-psmt` 被识别为四个动作。
- `-p`、媒体类组合以及两者组合的正确位置参数数量。
- 缺少动作、资源 ID、视频或输出目录。
- 多余位置参数。
- 非法资源 ID 和不存在的视频在副作用前失败。
- 聚合参数与旧式参数混用时失败。
- `--tree <VIDEO> --output <OUTPUT>` 兼容入口仍可用。

编排测试使用可注入的伪服务记录调用，验证：

- 调用顺序固定为 `p → m → s → t`。
- 未选择的功能不会执行。
- 中间步骤失败后不执行后续步骤。
- tree 在其他产物生成后才扫描输出目录。
- 成功摘要只列出实际完成的动作。

现有各模块单元测试继续负责文件事务、下载、媒体分析、字幕和 tree 内容等细节；聚合测试不重复这些内部测试。

## 不在本次范围内

- 并行执行四项任务。
- 一次 MediaInfo 进程同时生成文本报告和截图探测数据。
- 自动从文件名推断 TMDB ID。
- 自动推导或省略输出目录。
- 批量处理多个视频。
- 删除现有独立子命令。
