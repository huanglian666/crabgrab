# CrabGrab 项目初始化实施计划

> **供智能代理执行：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 技能，逐项执行本计划。所有步骤使用复选框跟踪状态。

**目标：** 完成 CrabGrab 本地 Git 初始化，保留 GitHub 创建 MIT License 的提交历史，并将验证通过的 Rust 初始工程发布到 `origin/main`。

**实现思路：** GitHub 已经在远端创建 MIT License，因此以 `origin/main` 作为权威历史起点。本地文档提交应以普通 Git 操作衔接到该历史之上，再加入 Rust 初始工程和适度的跨平台忽略规则，全程不使用强制推送。

**技术栈：** Git、GitHub SSH 远端、Rust 2024 Edition、Cargo

## 全局约束

- CrabGrab 是可执行程序，必须跟踪 `Cargo.lock`。
- 不添加应用依赖、CLI 功能、README 内容、CI 配置或业务模块。
- 不覆盖远端出现的非预期文件，不使用强制推送。
- 必须验证格式、编译、测试、仓库内容以及本地和远端的同步状态。

---

### 任务一：检查并整合 GitHub 历史

**涉及文件：**

- 保留：`prompts/00-project-initialization-design.md`
- 从 `origin/main` 获取：`LICENSE`

**输入与产出：**

- 输入：已配置的 `origin` 远端和本地 `main` 分支。
- 产出：以 `origin/main` 为历史基础，同时包含 MIT License 和初始化规格的本地 `main`。

- [ ] **步骤 1：仅获取并检查远端，不修改工作区**

执行：

```bash
git fetch origin main
git log --oneline --decorate --all --graph
git ls-tree -r --name-only origin/main
```

预期：`origin/main` 存在并包含 GitHub 生成的 `LICENSE`。如果发现其他文件，先检查其内容再继续。

- [ ] **步骤 2：确认本地未跟踪文件完整**

执行 `git status --short`，确认 `.gitignore`、`Cargo.toml`、`Cargo.lock` 和 `src/main.rs` 在调整历史前均存在。

- [ ] **步骤 3：将本地文档提交衔接到远端历史**

使用不会强制改写远端的方式，使 `origin/main` 成为本地 `main` 的祖先。如果根提交无法直接变基，则在 `origin/main` 之上重新创建本地文档提交；不得覆盖远端已有提交。

- [ ] **步骤 4：验证历史和 License**

执行：

```bash
git merge-base --is-ancestor origin/main main
test -f LICENSE
git status --short
```

预期：祖先检查成功，`LICENSE` 存在，所有本地项目文件保持完整。

### 任务二：加入跨平台项目基线并发布

**涉及文件：**

- 修改：`.gitignore`
- 添加：`Cargo.toml`
- 添加：`Cargo.lock`
- 添加：`src/main.rs`
- 保留：`LICENSE`
- 保留：`prompts/00-project-initialization-design.md`
- 添加：`prompts/00-project-initialization-plan.md`

**输入与产出：**

- 输入：任务一整合后的 Git 历史。
- 产出：通过验证并已推送到 `origin/main` 的 Rust 初始仓库。

- [ ] **步骤 1：完善 `.gitignore` 的跨平台规则**

加入 `/target/`、Rust 备份文件、macOS 和 Windows 系统元数据、JetBrains/RustRover 用户状态、VS Code 私有状态、`.env` 变体、日志、交换文件、备份文件和临时目录规则。明确允许 `.env.example`，不得忽略 `Cargo.lock`。

- [ ] **步骤 2：格式化 Rust 源码**

执行：

```bash
cargo fmt
```

预期：Rust 源码完成格式化且命令无错误。

- [ ] **步骤 3：验证项目基线**

执行：

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
git status --short --ignored
```

预期：所有 Cargo 命令成功，`git diff --check` 没有空白错误，`/target/` 已被忽略，并且没有敏感文件或生成文件进入待提交范围。

- [ ] **步骤 4：提交初始化文件**

执行：

```bash
git add .gitignore Cargo.toml Cargo.lock src/main.rs prompts/00-project-initialization-plan.md
git commit -m "chore: initialize CrabGrab project"
```

预期：提交只包含预定的项目基线文件和实施计划。

- [ ] **步骤 5：以普通推送方式更新远端**

执行：

```bash
git push -u origin main
```

预期：远端接受一次快进更新，不需要强制推送。

- [ ] **步骤 6：验证本地与远端同步**

执行：

```bash
git fetch origin main
test "$(git rev-parse main)" = "$(git rev-parse origin/main)"
git status --short --branch
```

预期：本地和远端 `main` 指向同一提交，工作区干净。
