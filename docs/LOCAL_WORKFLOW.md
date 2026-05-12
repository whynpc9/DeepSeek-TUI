# 本地开发与上游 PR 工作流

> **范围**：本文档只存在于 `local/otel` 私有分支，**不会**进入向上游提交的 PR。它记录的是这台 fork（`whynpc9/DeepSeek-TUI`）相对于上游（`Hmbown/DeepSeek-TUI`）的分支管理约定，方便在不同机器、不同时间点恢复同样的工作流。

---

## 1. 仓库拓扑

```
upstream/main (Hmbown)  ──────────────────────►
       │
       ├── feat/<新功能>    ← 干净的；从 upstream/main 拉，push 到 origin，PR 到 upstream
       │
       ├── local/otel       ← OTel + 本地文档（含本文）；永不上行到 upstream
       │
       └── local/dev        ← 可选；日常运行用：local/otel ⊕ 当前在开发的 feature
```

| 分支 | 跟踪 | 用途 | 谁能看到 |
|------|------|------|---------|
| `main` | `upstream/main` | 干净的上游 main，**不在这里改代码** | 公开 |
| `feat/<name>` | `origin/feat/<name>` | 一个 PR 一条分支，从 `upstream/main` 拉 | 公开（在 PR 中可见） |
| `local/otel` | `origin/local/otel` | OTel + 本地文档的私有补丁 | 仅本人 fork |
| `local/dev` | （可选）`origin/local/dev` | 日常跑 agent 时用的集成分支 | 仅本人 fork |

**核心原则**：每个要提 PR 的 feature 单独一条分支，从 `upstream/main` 拉；`local/otel` 永远不与 `feat/*` 合并。

---

## 2. Remote 配置

```bash
git remote -v
# origin    https://github.com/whynpc9/DeepSeek-TUI    (fetch/push)   ← 你的 fork
# upstream  https://github.com/Hmbown/DeepSeek-TUI.git (fetch/push)   ← 上游
```

如果在新机器上克隆：

```bash
git clone https://github.com/whynpc9/DeepSeek-TUI
cd DeepSeek-TUI
git remote add upstream https://github.com/Hmbown/DeepSeek-TUI.git
git fetch upstream
git branch --set-upstream-to=upstream/main main
git checkout -b local/otel origin/local/otel    # 拉本地私有分支
```

---

## 3. 开发新 feature 并提 PR

```bash
# 1) 同步上游
git fetch upstream

# 2) 从最新 upstream/main 开一条 feature 分支
git checkout -b feat/<short-name> upstream/main

# 3) 写代码 + 提交
#    保持 commit 干净（一个逻辑改动一条 commit；信息说明 "why" 而非 "what"）
git add ...
git commit -m "..."

# 4) push 到自己的 fork
git push -u origin feat/<short-name>

# 5) 用 gh 在上游开 PR
gh pr create --repo Hmbown/DeepSeek-TUI \
             --base main --head whynpc9:feat/<short-name> \
             --title "..." --body "..."
```

PR 的 diff 里只会有 feature 本身的改动；OTel 与本文档对上游不可见。

**提交前自查**：

```bash
git log upstream/main..HEAD --stat      # 列出本分支独有的 commit + 改动文件
gh pr diff                              # PR 提交后远程视角再看一遍
```

清单上若出现 `crates/tui/src/telemetry.rs`、`docs/OPENTELEMETRY.md`、`docs/LOCAL_WORKFLOW.md` 这种文件，说明你**切错分支了**或不小心 cherry-pick 了 OTel 那条 commit——立即停下来排查。

---

## 4. 日常本地跑（要带 OTel）

最简单：直接在 `local/otel` 上：

```bash
git checkout local/otel
cargo run --bin deepseek
```

要同时跑一个正在开发的 feature，建一条集成分支：

```bash
git checkout -b local/dev local/otel
git merge feat/<short-name>      # 或 rebase，看你偏好
cargo run --bin deepseek
```

`local/dev` 可以被任意时刻 `git reset --hard` 重建，**不要**在它上面写代码——所有改动应当落在 `feat/<name>` 或 `local/otel`。

---

## 5. 上游有更新时（同步流程）

```bash
git fetch upstream

# 5.1 main 跟上游（应该永远是 fast-forward）
git checkout main
git merge --ff-only upstream/main
git push origin main

# 5.2 把 local/otel rebase 到新的 upstream/main
git checkout local/otel
git rebase upstream/main
# 解决冲突（如有），然后
#   git add <files> && git rebase --continue
git push --force-with-lease origin local/otel

# 5.3 在 feature 分支上也 rebase（如果还没合 PR）
git checkout feat/<short-name>
git rebase upstream/main
git push --force-with-lease origin feat/<short-name>
```

**为什么对 local/otel 用 `--force-with-lease` 而不是普通 `--force`**：`--force-with-lease` 在远端被别人推过新东西时会拒绝，避免在多机协作时无意覆盖。即使这条分支只你一个人用，养成习惯也无害。

---

## 6. 切分支前的卫生检查

```bash
git status                       # 必须 clean 才能切；否则先 commit / stash
git branch --show-current        # 确认你以为的当前分支
git log --oneline -3             # 看最近 3 条提交头
```

特别提醒：

1. **写 feature 时第一眼看 `git branch --show-current`**。在 `feat/*` 上误改了 `telemetry.rs` 就会泄到 PR 里。
2. **不要 `cherry-pick`** 跨 `local/otel` ↔ `feat/*`。OTel 那条 commit 同时改了 `Cargo.lock`、`Cargo.toml`、`client/chat.rs` 等等，cherry-pick 到 feature 分支等于污染 PR。
3. **`Cargo.lock`** 在 feature 分支上由 feature 自己更新；在 `local/otel` 上由 OTel 自己更新。两者尽量不要互相覆盖。如果出现，相信 feature 分支上的版本，OTel 分支 rebase 时重新跑 `cargo build` 让 lock 自己再生成。

---

## 7. 同时开两个 worktree（推荐）

写 feature 时想边写边在带 OTel 的环境里 smoke test：

```bash
git worktree add ../DeepSeek-TUI-dev local/dev
```

之后：

- 主目录 `~/Projects/DeepSeek-TUI`：停在 `feat/<name>` 上写代码。
- 副目录 `~/Projects/DeepSeek-TUI-dev`：停在 `local/dev` 上跑 `deepseek`，带 OTel 上报。
- 两个目录各自的 `target/` 互不干扰；改完 feature 后到副目录里 `git pull` / `git merge feat/<name>` 即可同步。

清理：

```bash
git worktree remove ../DeepSeek-TUI-dev
```

---

## 8. 故障排查

### 8.1 PR diff 里出现了不该出现的文件

```bash
git log upstream/main..HEAD --stat | less    # 哪一条 commit 引入的？
git rebase -i upstream/main                  # 把脏 commit drop / edit 掉
git push --force-with-lease origin feat/<name>
```

### 8.2 `rebase upstream/main` 冲突在 OTel 改过的文件上

通常发生在 `client/chat.rs`、`turn_loop.rs`、`config.rs` 这几个文件，因为 OTel 本身就在改它们。

- 接受上游版本作为基线；
- 把 OTel 的 hook 重新缝回去（搜 `gen_ai.` / `telemetry::` / `Instrument` 这些标记定位插入点）；
- `cargo build -p deepseek-tui` 确认通过；
- 在 telemetry 测试 (`cargo test -p deepseek-tui --bin deepseek-tui telemetry`) 上跑一遍；
- `git add` + `git rebase --continue`。

### 8.3 不小心在 `main` 上 commit 了

```bash
git log --oneline main ^upstream/main        # 看 main 比 upstream 多了什么
# 如果还没 push：
git checkout -b feat/<name>                  # 把改动连同 commit 一起带到 feature 分支
git checkout main
git reset --hard upstream/main               # main 回到干净状态
# 如果已经 push 到 origin/main：
git push --force-with-lease origin main      # 谨慎；只在确认无人协作 main 时使用
```

### 8.4 想完全放弃 OTel（恢复到纯上游）

```bash
git checkout main
git merge --ff-only upstream/main            # 已经在 upstream tip
# local/otel 留着以备后用；要彻底删：
git branch -D local/otel
git push origin --delete local/otel
```

---

## 9. 与 `AGENTS.md` 的关系

仓库根目录的 `AGENTS.md` 是**上游约定**，会随 upstream 演进。它讲的是「这个项目里如何写代码」；本文档讲的是「在这个 fork 上如何**管理**代码」，两者不冲突，本文档不替代它。

向上游提 PR 前的本地校验仍然走 `AGENTS.md` 第 1 节的命令：

```bash
cargo build
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features
cargo fmt --all
```
