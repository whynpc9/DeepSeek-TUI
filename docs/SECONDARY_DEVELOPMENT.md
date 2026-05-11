# 二次开发指南

面向在本仓库内扩展功能、接入集成或维护分支的贡献者与二次开发者。架构全貌仍以英文 **[ARCHITECTURE.md](./ARCHITECTURE.md)** 为准；本文侧重「从哪里下手」与 crate 边界，便于快速定位代码。

---

## 1. 环境与约束

| 项目 | 说明 |
|------|------|
| **Rust** | **1.88+**（workspace `rust-version`）；代码大量使用稳定的 `let_chains`。 |
| **Edition** | 2024。 |
| **工具链** | **仅 stable**：禁止使用 `#![feature(...)]` 或依赖 nightly。参见仓库根目录 **AGENTS.md**。 |
| **推荐校验** | `cargo build`、`cargo test --workspace --all-features`、`cargo clippy --workspace --all-targets --all-features`、`cargo fmt --all`。 |
| **运行入口** | 对用户推荐使用 **`deepseek`**（dispatcher）；开发时可 `cargo run --bin deepseek` 或 `cargo run -p deepseek-tui-cli`。 |

---

## 2. 仓库拓扑：Workspace 成员

根 **`Cargo.toml`** 列出全部 crate。**职责简述**如下（按二次开发常见关注度排序）。

### 2.1 运行时与界面（改动最集中）

| Crate | 包名 | 作用 |
|--------|------|------|
| **`crates/tui`** | `deepseek-tui` | **当前终端 Agent 的主体实现**：`ratatui` UI、与模型交互的 **引擎与会话循环**（`src/core/`）、内置 **`tools/`**、**`runtime_api.rs`**（HTTP）、**`task_manager.rs`**、**`skills/`**、**`lsp/`**、`client`/`llm_client` 等。绝大部分交互式能力与工具逻辑在此。 |
| **`crates/cli`** | `deepseek-tui-cli` | **`deepseek` 二进制**：解析 CLI，多数子命令 **fork/exec 同目录下的 `deepseek-tui`**；另有 **`deepseek app-server`**、`login`、`config`、`thread` 等与 **deepseek-core / app-server** 直连的路径。 |
| **`crates/app-server`** | `deepseek-app-server` | 基于 Axum 的 **HTTP + JSON-RPC（stdio）** 服务端；组装 **`deepseek_core::Runtime`**，暴露 `/thread`、`/prompt`、`/tool`、`/jobs`、`/mcp/startup` 等。 |

### 2.2 共享逻辑 crate（可被 TUI / CLI / Server 复用）

| Crate | 作用 |
|--------|------|
| **`crates/core`** | **`deepseek-core`**：**无头 `Runtime`** —— 线程元数据、任务队列、`invoke_tool`、hook/MCP 启动等与 **`deepseek_protocol`** 对齐的 API。**不包含** TUI 里那套完整 streaming turn loop（那是在 **`deepseek-tui`** 里）。 |
| **`crates/protocol`** | 请求/响应、线程、事件帧、`ToolCall`/`ToolPayload` 等协议类型。 |
| **`crates/tools`** | **`ToolRegistry`、`ToolCall`**、能力位、`ApprovalRequirement` 等与工具分发相关的 **共享类型与分发骨架**；具体工具实现大量仍在 **`deepseek-tui`**。 |
| **`crates/config`** | `ConfigToml`、profile、环境变量与 CLI 覆盖解析。 |
| **`crates/agent`** | `ModelRegistry`：模型 ID → provider/base URL 等解析。 |
| **`crates/execpolicy`** | 执行策略：审批、sandbox 相关的 **`ExecPolicyEngine`**。 |
| **`crates/hooks`** | 生命周期 hook（stdout、jsonl、webhook 等）。 |
| **`crates/mcp`** | MCP 客户端与 stdio MCP server 集成。 |
| **`crates/state`** | SQLite 持久化：线程、消息、checkpoint、任务状态等。 |
| **`crates/secrets`** | API key 等与 OS/keyring 的交互。 |

### 2.3 脚手架与演进中组件

| Crate | 作用 |
|--------|------|
| **`crates/tui-core`** | `deepseek-tui-core`：**事件驱动 UI 状态机 scaffold**（`UiEvent`/`UiState` 等），与重量级 **`deepseek-tui`** 并存；二次开发若以 UI 架构演进为目标可关注此地。 |

---

## 3. 双二进制模型：`deepseek` 与 `deepseek-tui`

- **`deepseek`（`crates/cli`）**  
  - 入口：`crates/cli/src/main.rs` → `deepseek_tui_cli::run_cli()`（实现在 **`crates/cli/src/lib.rs`**）。  
  - **`doctor`、`exec`、`mcp`、`serve`** 等大量命令会把参数转发给 **`deepseek-tui`**（与之并排安装或通过 **`DEEPSEEK_TUI_BIN`** 指定绝对路径）。

- **`deepseek-tui`（`crates/tui`）**  
  - 承载完整 Agent 循环、工具执行与 TUI。  
  - **`cargo build` 的 default-members 包含 `cli` 与 `tui`**，本地调试时需二者齐备或通过环境变量指向已有 `deepseek-tui`。

**二次开发含义**：改交互 Agent 行为 → 优先进 **`crates/tui`**；改分发器/安装探测/`app-server` 子命令 → **`crates/cli`**；改 HTTP 无头 API → **`crates/app-server`** + **`crates/core`** + （如需完整推理链路仍需对齐 **`crates/tui`** 中的运行时语义）。

---

## 4. `deepseek-tui` 内部导图（最常改的目录）

以下路径均相对于 **`crates/tui/src/`**。

| 路径 | 内容 |
|------|------|
| **`main.rs`** | TUI 程序入口；模块声明；顶层 CLI/`clap` 与子命令路由入口之一。 |
| **`core/`** | **`engine.rs`、`engine/turn_loop.rs`**：会话与流式轮次、工具编排；**上下文/compaction/cycle/capacity** 钩子见 **§12**；**`session.rs`、`turn.rs`、`events.rs`**；**`capacity*.rs`、`engine/capacity_flow.rs`**；**`tool_parser.rs`**；**`engine/approval.rs`**；**`engine/lsp_hooks.rs`** 衔接编辑后 LSP。 |
| **`tools/`** | 内置工具实现与 **`mod.rs`/`registry.rs`**；新增模型可见工具时通常在此处扩展并与 registry 注册逻辑对齐。 |
| **`tui/`** | `app.rs`、`ui.rs`、按键与视图、`streaming/`、`approval.rs` 等纯前端交互层。 |
| **`commands/`** | 斜杠命令与用户命令分发（`/compact`、`/mcp` 等）。 |
| **`client.rs`、`client/`、`llm_client/`、`models.rs`** | HTTP、流式解析、`ContentBlock::Thinking` 等与 Chat Completions 对齐的实现。 |
| **`prompts.rs`、`prompts/`** | 系统提示与 Agent 文案模板（分层与运行时拼装见 **§10**）。 |
| **`skills/`** | Skills 加载与安装。 |
| **`mcp.rs`、`mcp_server.rs`** | MCP 池与内置 MCP server。 |
| **`runtime_api.rs`、`runtime_threads.rs`、`task_manager.rs`** | 运行时 HTTP API、线程事件时间与耐久任务队列。 |
| **`lsp/`** | 编辑后诊断注入管线。 |

---

## 5. 按场景的改动指引

### 5.1 新增或修改「模型可调用的工具」

1. 阅读 **[TOOL_SURFACE.md](./TOOL_SURFACE.md)**（设计理念与命名）。  
2. 整体机制（**`ToolSpec`**、registry、模式装配、触发与约束）见 **§11**。  
3. 在 **`crates/tui/src/tools/`** 实现逻辑；必要时在 **`crates/tools`** 补充共享类型或 trait。  
4. 将工具注册进 **`tools/registry.rs`**（**`ToolRegistryBuilder`**）或 **`core/engine/tool_setup.rs`**，并确保 **`prompts`** 中的 agent 说明与可见工具列表一致。  
5. 若涉及审批或 sandbox，核对 **`execpolicy`** 与工具 **`ApprovalRequirement`**。  

### 5.2 调整会话行为、流式轮次、工具调用前后钩子

- 核心：**`crates/tui/src/core/engine.rs`** 与 **`core/engine/turn_loop.rs`**（主循环分段说明见 **§9**）。  
- **上下文压缩 / 周期 / working set / seam**：**§12** 与 **`compaction.rs`**、**`cycle_manager.rs`**、**`working_set.rs`**、**`seam_manager.rs`**。  
- Hook：**`crates/hooks`**（crate）+ **`crates/tui/src/hooks.rs`**（集成方式）。  

### 5.3 TUI 展示、快捷键、国际化

- UI：**`crates/tui/src/tui/`**。  
- 本地化：**[LOCALIZATION.md](./LOCALIZATION.md)**，`localization.rs` 与翻译资源。

### 5.4 配置与 Provider

- 解析与合并：**`crates/config`**。  
- TUI 内 **`config.rs`、`settings.rs`、`commands/config.rs`**。  
- 文档：**[CONFIGURATION.md](./CONFIGURATION.md)**；与上下文/记忆相关的 **`config` vs `settings` vs env** 分层见 **§14**。  

### 5.5 无头 HTTP / 集成第三方宿主

- 路由与 handler：**`crates/app-server`**。  
- 线程与工具预览语义：**`crates/core`** 中的 **`Runtime`**。  
- 完整能力与 SSE 细节：**[RUNTIME_API.md](./RUNTIME_API.md)**，以及与 **`crates/tui/src/runtime_api.rs`** 的对照。

### 5.6 子 Agent / RLM

- **[SUBAGENTS.md](./SUBAGENTS.md)**。  
- 代码：**`tools/subagent/`**、`tools/rlm.rs`、`rlm/`。

### 5.7 MCP

- **[MCP.md](./MCP.md)**；Rust：**`crates/mcp`** + **`crates/tui/src/mcp*.rs`**。

### 5.8 运行模式（Plan / Agent / YOLO）

- **[MODES.md](./MODES.md)**。

### 5.9 用户记忆、Skills 与配置分层

- **记忆**：行为与代码触点 **§13**，用户手册 **[MEMORY.md](./MEMORY.md)**。  
- **Skills / AGENTS.md / `instructions`**：**§15**。  
- **`config.toml` vs `settings.toml` vs 环境变量**（与上下文/seam/capacity 对照）：**§14**。

---

## 6. 易混淆点（刻意单独列出）

1. **`deepseek-core` vs `crates/tui/src/core`**  
   - **`deepseek-core`**：`Runtime`、`ThreadManager`、面向 **`deepseek-app-server`** 的 **`invoke_tool`/`handle_thread`** 等 **API 层**。  
   - **`crates/tui/src/core`**：**交互式 Agent 引擎**，包含完整 **`turn_loop`** 与 streaming client。二者 complementary，不要假设「逻辑已全部搬进 core crate」。  

2. **`deepseek-tools` crate vs `crates/tui/src/tools`**  
   - **`deepseek-tools`**：通用 **`ToolRegistry`/`ToolCall`/capability**；具体 **`read_file`/`exec_shell`/…** 多数仍在 **`deepseek-tui`**。  

3. **默认文档里的路径**：ARCHITECTURE 中部分路径写的是 **`tools/`、`core/`** 这类片段——在本仓库里默认指 **`crates/tui/src/`** 下对应模块。

---

## 7. 相关文档索引

| 文档 | 用途 |
|------|------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 分层架构与数据流（主文档）。 |
| [TOOL_SURFACE.md](./TOOL_SURFACE.md) | 工具设计理念与清单。 |
| [CONFIGURATION.md](./CONFIGURATION.md) | 配置项与环境变量。 |
| [RUNTIME_API.md](./RUNTIME_API.md) | HTTP/SSE 运行时 API。 |
| [MODES.md](./MODES.md) | Plan / Agent / YOLO。 |
| [SUBAGENTS.md](./SUBAGENTS.md) | 子 Agent 契约。 |
| [MCP.md](./MCP.md) | MCP 接入说明。 |
| [MEMORY.md](./MEMORY.md) | 用户记忆文件注入行为。 |
| [KEYBINDINGS.md](./KEYBINDINGS.md) | 快捷键参考。 |
| [OPENTELEMETRY.md](./OPENTELEMETRY.md) | OpenTelemetry / SigNoz 上报与 LLM tracing 集成（v0.8.16+）。 |
| [capacity_controller.md](./capacity_controller.md) | 容量控制器（capacity flow）策略与默认关闭说明。 |
| [../PROMPT_ANALYSIS.md](../PROMPT_ANALYSIS.md) | 系统提示与协作策略演进说明（仓库根目录）。 |
| 仓库根目录 **AGENTS.md** | 面向 AI/自动化助手与本仓库贡献的规则摘要。 |
| **§14**（本文） | `config.toml` / `settings.toml` / 环境与 §12–§13 的对照。 |
| **§15**（本文） | Skills、`AGENTS.md`、`instructions` 与源码入口。 |

---

## 8. 版本与工作方式提示

- 当前 workspace 版本见根 **`Cargo.toml`** `[workspace.package] version`。  
- 提交 PR 前在本地跑通 **build + test + clippy**（见第 1 节）。  
- DeepSeek V4 **thinking + tool_calls** 时需在后续请求中携带 **`reasoning_content`**；详见 ARCHITECTURE 与 upstream API 说明，修改对话拼装路径时需一并 regression。

---

## 9. Agent 主循环（Engine ↔ Turn Loop）

本节对应 **`crates/tui/src/core/engine.rs`**（引擎生命周期、`EngineConfig`、`spawn_engine`）与 **`crates/tui/src/core/engine/turn_loop.rs`**（`handle_deepseek_turn`）：交互式 Agent 的 **主推理–工具循环** 都在 **`deepseek-tui`** 内完成，**不是** **`deepseek-core`** crate。

### 9.1 外层：`Engine::run` 与 `Op`

- **`spawn_engine`**（`engine.rs` 末尾）在后台 task 里运行 **`Engine::run`**。
- **`Engine::run`** 阻塞在 **`rx_op`**：每条 **`Op`** 驱动一类会话级动作；与用户一发 Prompt 对应的路径主要是 **`Op::SendMessage`**。
- 其它 **`Op`**（节选）：**`CancelRequest`**、`CompactContext`、`SyncSession`、`Rlm { … }`、`Shutdown` 等——分别在 **`match`** 各分支内处理；RLM 有单独的 **`run_rlm_turn`** 路径，与普通 **`handle_deepseek_turn`** 分离。

### 9.2 中层：`handle_send_message` → 单次「用户回合」

对用户提交的每条 **`SendMessage`**，引擎大致顺序为：

1. **`TurnContext::new`**：为本回合分配 **`turn.id`**、**`max_steps`**（默认来自 **`EngineConfig::max_steps`**，典型值 **100**），**`step`** 从 0 计数。
2. **可选 `pre_turn_snapshot`**（workspace side-git）。
3. **`TurnStarted`** 事件。
4. 校验 **`deepseek_client`**；把用户消息写入 **`session`**（含 **`working_set`** 观测）。
5. 同步会话字段：**模型**、**reasoning_effort**、**auto_model**、**allow_shell** / **`trust_mode`**、**审批模式**（YOLO 会强行 **`ApprovalMode::Auto`**）。
6. **`refresh_system_prompt(mode)`**（见 §10）：保证系统提示与当前模式 / 记忆 / handoff 等一致。
7. **`build_turn_tool_registry_builder`** → **`ToolRegistry`**；按需挂载 **子 Agent runtime + MCP 工具列表** → **`build_model_tool_catalog`** 得到发给模型的 **`tools`** schema。
8. 调用 **`handle_deepseek_turn(&mut turn, …)`** —— **主多步循环**。
9. 成功后 **`maybe_advance_cycle`**（checkpoint-restart cycle）；累计 **`turn.usage`**；**`TurnComplete`**；可选 **`post_turn_snapshot`**。

### 9.3 内层：`handle_deepseek_turn`（单回合内的多「步」）

`TurnContext::step` 表示 **在同一用户消息之下**，模型请求 ↔ 工具执行的迭代次数；上限 **`turn.at_max_steps()`**。

每一「外层迭代」（until break）的典型流水线：

| 阶段 | 说明 |
|------|------|
| **中断与用户 steer** | 取消 token；**`rx_steer`** 中非阻塞收到的 steer 会追加为 **user** 消息并入会话。 |
| **刷新系统提示** | 再次 **`refresh_system_prompt`**，与会话状态（含 compaction summary）对齐。 |
| **步数上限** | **`step >= max_steps`** → 结束回合。 |
| **自动 compaction** | 若配置启用且 **`should_compact`**，则在发起模型请求前可能重写 **`session.messages`**。 |
| **容量护栏** | **`run_capacity_pre_request_checkpoint`**（可能 **`continue`** 跳过本轮请求）。 |
| **上下文预算** | 预估输入 token；超限时可 **`recover_context_overflow`**（有限次数）后再试。 |
| **LSP** | **`flush_pending_lsp_diagnostics`**：把累积的诊断注入为 **user** 侧上下文（详见 **`engine/lsp_hooks.rs`**）。 |
| **分层上下文 seam** | 可选 **`layered_context_checkpoint`**（归档块追加而非替换历史）。 |
| **构造 `MessageRequest`** | **`messages_with_turn_metadata()`**（含 **`<turn_meta>`** 等工作集元数据）、**`system`**、**`tools`**、**`tool_choice`**（**`strict_tool_mode`** 时为 **`required`**）、**`reasoning_effort`**、**`stream: true`**。 |
| **流式请求** | **`create_message_stream`**；失败时可上下文恢复或标志着回合失败。 |

**流解码内层**（同一请求的 SSE/`StreamEvent` 循环，仍在 **`turn_loop.rs`**）：

- 区分 **Text / Thinking / ToolUse** 内容块；对伪造的工具标记文案做 **`filter_tool_call_delta`** 过滤。
- **空闲超时**（chunk idle）、**总字节上限**、**墙钟上限**；流级错误支持 **透明重试**（无有效内容输出时安全重发，避免双倍计费）。
- 流结束后：若存在 **tool calls** 但无 reasoning 文本，可能写入占位 **`(reasoning omitted)`** 的 **`ContentBlock::Thinking`**，以满足 **V4 thinking 模式下 tool_calls 必须带 `reasoning_content`** 的回放约定。
- 将 **assistant** 消息（thinking + text + tool_use）写入会话（需满足「可发送」内容判定）。

**无工具调用分支**（`tool_uses.is_empty()`）：

- 处理排队 **steer**、**子 Agent 完成** sentinel（若仍有 running 子 agent 会 **await** 完成或 steer）、内联 **` ```repl `** 沙箱执行（多轮直到 FINAL 或把输出喂回下一轮）；否则 **break** 结束本用户回合。

**有工具调用分支**：

- 为每个 tool 建 **`ToolExecutionPlan`**（审批需求、并行度、loop_guard 去重、MCP/幻觉名称归一等）。
- **只读且安全**时可 **`FuturesUnordered`** 并行执行，否则顺序执行；路径上调用 **`execute_tool_with_lock`** / 审批 UI / spillover 截断等。
- 工具结果写回 **`session`**（及 **LSP post-edit** 等钩子，见 **`lsp_hooks`**），**`turn.record_tool_call`**，**`turn.next_step()`**，**`continue`** 外层循环 → **下一轮模型请求**。

**其它文件关系**：流状态常量与伪工具调用清洗在 **`engine/streaming.rs`**；工具解析兜底在 **`core/tool_parser.rs`**；审批细节在 **`engine/approval.rs`** 与 **`engine/tool_execution.rs`**；循环重复调用防护在 **`engine/loop_guard.rs`**。

---

## 10. 系统提示词组装（`prompts.rs` + `prompts/`）

实现文件：**`crates/tui/src/prompts.rs`**；静态正文目录：**`crates/tui/src/prompts/`**。

### 10.1 分层常量（编译期 `include_str!`）

**默认合成顺序**（**`compose_prompt` / `compose_prompt_with_approval`**）——四层由空行拼接：

1. **`prompts/base.md`** → **`BASE_PROMPT`**：身份、语言约定、工具使用哲学、子 Agent 约定等（**`base.md`** 内 **`## Environment`** 指向运行时注入块）。
2. **人格覆盖**：**`personalities/calm.md`** / **`playful.md`**（当前默认逻辑仍以 Calm 为主）。
3. **模式增量**：**`modes/agent.md`**、**`modes/plan.md`**、**`modes/yolo.md`** —— 与 **`AppMode`** 对齐。
4. **审批策略**：**`approvals/auto.md`**、**`suggest.md`**、**`never.md`** —— 由 **`AppMode`** + **`ApprovalMode`** 组合选定（如 Agent+Suggest、Plan→Never、Yolo→Auto）。

**遗留整文件**（仍 **`include_str!`**，供未迁移调用方）：**`agent.txt`**、**`yolo.txt`**、**`plan.txt`** 以及 **`normal.txt`** / **`base.txt`** 等；新逻辑以 **`base.md` + overlays** 为准。

### 10.2 完整系统提示：`system_prompt_for_mode_with_context_skills_session_and_approval`

在四层 mode prompt 之上，运行时继续追加（顺序实现 **prefix-cache 友好**：越静态越靠前；volatile 边界后有注释），概要如下：

1. **项目上下文**：**`project_context`**（AGENTS.md 等）或自动生成目录摘要。
2. **`## Environment`**：**`render_environment_block`** —— **`locale_tag`**、OS、`SHELL`、workspace **`pwd`**（语言默认值以此为准，见 **`base.md`**）。
3. **`instructions = [...]`**（配置文件中的路径列表）：逐项读取，单文件约 **100KiB** 上限，超出截断并标注 **`[…elided]`**。
4. **用户记忆**：若启用 **`memory`**，由 **`memory::compose_block`** 注入（完整行为见 **§13**）。  
5. **会话目标**：**`goal_objective`** → **`## Current Session Goal`**。
6. **Skills 目录**：**`skills::render_available_skills_context_for_workspace`**（多路径合并 skills 目录）。
7. **Agent/Yolo 专有**：**`## Context Management`** + **`/compact`** 与 **prompt-cache** 行为说明。
8. **`prompts/compact.md`** → **`COMPACT_TEMPLATE`**：指导 **`/compact`** / **handoff** 写法。
9. **Volatile 边界之后**：**`.deepseek/handoff.md`**（**`HANDOFF_RELATIVE_PATH`**），上一轮会话交接。

引擎侧 **`refresh_system_prompt`**（**`engine.rs`**）在上述 **`SystemPrompt::Text`** 之上再 **`merge_system_prompts`** 合并 **`session.compaction_summary_prompt`**（摘要 compaction 路径写入），并用 **hash** 避免重复替换未变的 prompt。

### 10.3 其它提示相关文件

| 路径 | 用途 |
|------|------|
| **`prompts/cycle_handoff.md`** | 由 **`cycle_manager.rs`** **`include_str!`**（**`CYCLE_HANDOFF_TEMPLATE`**），用于 cycle briefing / 种子消息，而非 **`prompts.rs`** 的默认四层合成。 |
| **`prompts/subagent_output_format.md`** | 子 Agent 系统提示片段，由 **`tools/subagent/mod.rs`** 多次 **`include_str!`** 注入子 agent 运行时。 |
| **`crates/tui/src/prompts/agent.txt`** | **[TOOL_SURFACE.md](./TOOL_SURFACE.md)** 中声明的工具表与叙述仍与之对齐；改工具面时应同步检查 **`agent.txt`** / **`base.md`**。 |

维护提示词与「托管天才」协作策略的演进分析见仓库根目录 **`PROMPT_ANALYSIS.md`**（偏产品与 prompt 策略，非 API 契约）。

---

## 11. 主要 Tools：定义、触发条件、约束与实现概述

详细清单与设计取舍见 **[TOOL_SURFACE.md](./TOOL_SURFACE.md)** 与 **`crates/tui/src/prompts/agent.txt`**。本节说明 **代码层面** 如何定义工具、何时执行、常见约束及对应源码位置。

### 11.1 定义方式：`ToolSpec` → `ToolRegistry`

| 概念 | 说明 |
|------|------|
| **`ToolSpec`**（**`crates/tui/src/tools/spec.rs`**） | 每个工具是一个实现 **`async_trait::async_trait`** 的 **`ToolSpec`**：`name()`、`description()`、`input_schema()`（发给模型的 JSON Schema）、**`execute`**、**`capabilities()`**、**`approval_requirement()`**（**`Auto` / `Suggest` / `Required`**）、**`defer_loading()`**（延迟载入目录——减小首轮 schema 体积）、**`supports_parallel()`** / **`is_read_only()`**（并行批处理依据）。 |
| **`ToolContext`** | 每次执行传入的上下文：**workspace**、**`shell_manager`**、**`trust_mode`**、**`trusted_external_paths`**、**`network_policy`**、**`features`**、**`runtime`**（**`RuntimeToolServices`**：任务队列、自动化、cancellation 等）、**`cancel_token`**、**`memory_path`**、大输出 **workshop** 路由等。 |
| **`ToolRegistry`**（**`tools/registry.rs`**） | **`HashMap<String, Arc<dyn ToolSpec>>`**；**`to_api_tools()`** 按 **工具名字典序** 序列化（稳定 **KV prefix cache**）；schema 经 **`schema_sanitize`**；**`execute_full_with_context`** 可在工具返回后走 **大输出路由**（**`raw=true`** 可跳过合成）。 |
| **`deepseek-tools` crate** | 与 **`deepseek-tui`** 共用的 **`ToolCall`/`ToolError`/Approval 枚举** 等；无头 **`Runtime.invoke_tool`** 走另一套组装，但语义对齐。 |

### 11.2 触发条件：模型何时「打到」工具

1. **主路径**：流式响应里出现 **原生 `tool_calls`/`ToolUse`** → **`turn_loop`** 解析参数 → **`ToolExecutionPlan`** → **`execute_tool_with_lock`** → **`ToolRegistry::execute_full_with_context`**（见 **§9**）。  
2. **兜底**：模型在正文里伪造工具 XML/markers → **`filter_tool_call_delta`** 清洗展示文本；若仍存在标记 → **`tool_parser::parse_tool_calls`** 可解析出一组合成 **`ToolUseState`**（优先仍以 API 通道为准）。  
3. **延迟载入**：工具声明 **`defer_loading: true`** 时，可能不会出现在首轮 **`active_tools`**；模型若仍调用该名 → **`turn_loop`** 里 **`maybe_activate_requested_deferred_tool`** 把对应定义加入 **`active_tool_names`** 并重试后续步骤。  
4. **名称纠错**：**`ToolRegistry::resolve`** 对大小写、连字符、camelCase、`_tool` 后缀等做规范化，减少幻觉名导致的失败。  
5. **MCP**：启用 **`Feature::Mcp`** 且连接池可用时，引擎把 **`mcp_*`** 工具并入 catalog；执行时走 MCP 适配器（仍实现 **`ToolSpec`**）。  
6. **子 Agent**：启用 **`Feature::Subagents`** 时父会话 registry 通过 **`with_subagent_tools`** 挂载 **`agent_spawn`** 等；子会话使用 **`with_full_agent_surface`** 与父面对齐（见 **`registry.rs`** 注释）。

### 11.3 装配入口：模式 × Feature × 会话权限

主逻辑在 **`crates/tui/src/core/engine/tool_setup.rs`** 的 **`build_turn_tool_registry_builder`**：

| 维度 | 行为概要 |
|------|----------|
| **`AppMode::Plan`** | **`with_read_only_file_tools`**（仅 **`read_file`/`list_dir`**）+ **`with_search_tools`** + **`with_git_tools`** + **`with_git_history_tools`** + **`with_diagnostics_tool`** + **`with_skill_tools`** + **`with_validation_tools`** + **`with_runtime_task_tools`** + **`todo`/`plan`**；**不包含** **`with_agent_tools`** 里的写文件 / 默认整套 Web / Patch / Shell。 |
| **`AppMode::Agent` / `Yolo`** | 基底 **`with_agent_tools(allow_shell)`**：内含 **`with_file_tools`**、**`note`**、**`grep_files`/`file_search`**、**`with_web_tools`**、**`request_user_input`**、**`apply_patch`**、**git / diagnostics / project / load_skill / run_tests / validate_data / runtime_task / revert_turn`**；若 **`allow_shell`** 为真则再 **`with_shell_tools`**。 |
| **叠加（所有模式共用的 builder 尾部）** | **`with_review_tool`**、**`with_rlm_tool`**、**`with_fim_tool`**、**`with_user_input_tool`**（**`with_parallel_tool`** 当前为 **no-op**，保留 API 兼容）。 |
| **`Feature::ApplyPatch`**（非 Plan） | 再次 **`with_patch_tools`**（与 **`with_agent_tools`** 内已有注册叠加时为幂等覆盖）。 |
| **`Feature::WebSearch`** | 再次 **`with_web_tools`**。 |
| **`Feature::ShellTool`** 且 **`allow_shell`** | 再次 **`with_shell_tools`**（Plan 下如需 shell，依赖此处而非 **`with_agent_tools`**）。 |
| **`memory_enabled`** | **`with_remember_tool`**（未开启记忆则不注册，避免模型徒占 catalog）。 |

**`ToolContext`** 在引擎 **`build_tool_context`** 中根据 **`AppMode`**、**`auto_approve`**（YOLO）、**`Features`**、**`runtime_services`** 等填充；与上述 registry **独立但须一致**。

### 11.4 执行期约束（通用）

| 约束类型 | 说明 |
|----------|------|
| **工作区路径** | 多数文件类工具将路径约束在 **`workspace`**；**`trusted_external_paths`**（`/trust`）可放行额外读写根。 |
| **审批** | **`ApprovalRequirement`** 与 **`execpolicy`** / TUI 审批 UI、**`auto_approve`** 组合决定是否阻塞；**`engine/approval.rs`**、**`engine/tool_execution.rs`**。 |
| **网络** | **`fetch_url`/`web_search`/…** 受 **`network_policy`**（及会话级 **`/network allow`**）约束。 |
| **重复调用护栏** | **`engine/loop_guard.rs`** 对相同 **`(tool, input)`** 连续尝试可短路并返回结构化提示。 |
| **并行** | **`should_parallelize_tool_batch`**：仅当批次内均为 **只读 + 无审批阻塞 + 工具声明支持并行** 等条件时用 **`FuturesUnordered`**；否则顺序执行。 |
| **输出体量** | **`tools/truncate`** spillover；可选 **workshop** 大输出路由（见 **`registry::execute_full_with_context`**）。 |
| **取消** | 工具应观察 **`ToolContext.cancel_token`**（长耗时 Shell/MCP 等）。 |
| **编辑后诊断** | **`edit_file`/`write_file`/`apply_patch`** 等成功后会触发 **LSP** 管线（**`engine/lsp_hooks.rs`**），下一轮请求前注入诊断（见 **§9**）。 |

### 11.5 按工具家族的实现位置与要点（速查）

下列路径均为 **`crates/tui/src/tools/`**。

| 家族 | 工具名（示例） | 实现模块 | 约束 / 实现要点（概述） |
|------|----------------|----------|-------------------------|
| **文件** | `read_file`, `list_dir`, `write_file`, `edit_file` | **`file.rs`** | 读写路径校验；PDF 等按需抽取；写操作多为 **`Suggest`** 审批。 |
| **补丁** | `apply_patch` | **`apply_patch.rs`** | 统一 diff 应用；与 **`Feature::ApplyPatch`** / Plan 排除逻辑配合。 |
| **内容搜索** | `grep_files` | **`search.rs`** | **Rust `regex`** 扫描工作区；默认 **`MAX_RESULTS=100`**，单文件 **`MAX_FILE_SIZE=10MiB`** 跳过过大文件。 |
| **文件名搜索** | `file_search` | **`file_search.rs`** | 模糊匹配路径（非内容）。 |
| **Web** | `web_search`, `fetch_url`, `finance`, `web_run` | **`web_search.rs`**, **`fetch_url.rs`**, **`finance.rs`**, **`web_run.rs`** | 出站 HTTP；受 **network policy** 与用户审批策略约束。 |
| **Shell** | `exec_shell`, `exec_shell_wait`, … | **`shell.rs`** | **`SharedShellManager`** 管理前后台任务、超时与交互；依赖 **`allow_shell`**。 |
| **耐久 Shell** | `task_shell_start`, `task_shell_wait` | **`tasks.rs`** | 与耐久任务 / gate 证据链路衔接。 |
| **Git** | `git_status`, `git_diff` | **`git.rs`** | 只读仓库 introspection。 |
| **Git 历史** | `git_log`, `git_show`, `git_blame` | **`git_history.rs`** | 只读；深度随参数受限。 |
| **诊断 / 测试** | `diagnostics`, `run_tests` | **`diagnostics.rs`**, **`test_runner.rs`** | 聚合环境与 **`cargo test`** 类调用。 |
| **工程地图** | `project_map`（名以源码为准） | **`project.rs`** | 结构化项目概况。 |
| **Skills** | `load_skill` | **`skill.rs`** | 按 **`SKILL.md`** 约定加载技能正文与伴随文件列表。 |
| **结构化校验** | `validate_data` | **`validate_data.rs`** | 对 JSON 等做 schema 校验类辅助。 |
| **Todo / Plan** | `todo_*`, `checklist_*`, `update_plan` | **`todo.rs`**, **`plan.rs`** | **共享内存状态**（`SharedTodoList` / `SharedPlanState`），与会话绑定。 |
| **耐久任务 / Gate / PR / GitHub / 自动化** | `task_*`, `task_gate_run`, `pr_attempt_*`, `github_*`, `automation_*` | **`tasks.rs`**, **`github.rs`**, **`automation.rs`** | 依赖 **`RuntimeToolServices`**（**`task_manager`**, **`automations`**, **`active_task_id`** 等）；多数字段变更类工具 **强审批**。 |
| **子 Agent** | `agent_spawn`, `agent_wait`, … | **`subagent/mod.rs`**（大块） | 独立运行时 **`SubAgentRuntime`**；提示片段含 **`subagent_output_format.md`**。 |
| **RLM / Review / FIM** | **`rlm`**（长上下文递归处理；REPL 内_helpers：`llm_query` / …）、**`review`**、FIM 工具 | **`rlm.rs`**, **`review.rs`**, **`fim.rs`** | **`rlm`** 需 **`DeepSeekClient`** + **`run_rlm_turn_with_root`**；与 **`tools/rlm.rs`** 头注释一致——短并行扇出用 REPL 内 **`rlm_query`**，整块长输入用 **`rlm`** 工具。 |
| **撤销 / 记忆 / 用户输入** | `revert_turn`, `remember`, `request_user_input` | **`revert_turn.rs`**, **`remember.rs`**, **`user_input.rs`** | **快照侧仓**；记忆仅 **`memory_enabled`**；用户输入工具与 TUI 阻塞交互。 |
| **周期归档召回** | `recall_archive` | **`recall_archive.rs`** | 检索 checkpoint-restart 产生的归档上下文。 |

新增或修改工具时：**实现 `ToolSpec` → 在 `ToolRegistryBuilder` 或 `tool_setup` 中注册 → 更新 schema 与 `agent.txt`/`base.md` 叙述 → 按需补充 `TOOL_SURFACE.md`**。

---

## 12. 上下文管理（Context Management）

本节汇总 **会话如何把「可调用的对话长度」控制在模型窗口内**，同时尽量保住 DeepSeek **前缀 KV cache** 的收益。实现分散在 **`compaction.rs`**、**`cycle_manager.rs`**、**`working_set.rs`**、**`seam_manager.rs`**、**`core/engine/context.rs`**、**`core/capacity*.rs`** / **`engine/capacity_flow.rs`** 与 **`core/session.rs`**。

### 12.1 数据分层：消息、系统提示与 `<turn_meta>`

| 层次 | 存放位置 | 作用 |
|------|----------|------|
| **对话主序列** | **`Session.messages`**（**`core/session.rs`**） | 发给模型的 **`messages`** 主体；含 user / assistant / tool 结果等 **`Message`**。 |
| **系统提示** | **`Session.system_prompt`** + 可选 **`compaction_summary_prompt`** | 静态层（§10）与 compaction 摘要 **`merge_system_prompts`** 合并；变更频率低于逐轮 user 文段。 |
| **回合元数据** | **`<turn_meta>`**（**`messages_with_turn_metadata`**，`turn_loop.rs`） | 注入 **最后一条「真实用户」** 文本消息的 **内容块首部**（不能打在纯 tool-result 伪 user 上，否则会破坏 API 的 tool 消息配对）。内含 **当前本地日期** 与 **`WorkingSet` 摘要**，**不落系统提示**，避免抖动前缀缓存。 |

**`WorkingSet`**（**`working_set.rs`**）监听用户输入与工具活动，维护 **高优先级路径**列表；为 compaction 提供 **pinned 消息下标** 与 **`top_paths`**，并把摘要交给 **`<turn_meta>`**。

### 12.2 有损摘要压缩（Compaction）

- **实现**：**`crates/tui/src/compaction.rs`**（**`should_compact`**、**`compact_messages_safe`**、估算 token、摘要输入裁剪策略等）。  
- **触发**：  
  - **自动**：在 **`handle_deepseek_turn`** 每步构造请求前，若 **`CompactionConfig.enabled`** 且 **`should_compact`** 为真，则调用 **`compact_messages_safe`** 重写 **`session.messages`**，并把 **`summary_prompt`** 交给 **`merge_compaction_summary`**，进入 **`session.compaction_summary_prompt`**。**v0.8.11+** 自动 compaction 以 **token** 为主信号，并设有 **`MINIMUM_AUTO_COMPACTION_TOKENS`（500K）地板**：低于该估算总量时 **不自动压缩**，以减少「前缀被摘要重写 → KV cache 大幅失效」的代价（注释见 **`CompactionConfig`**）。  
  - **手动**：用户 **`/compact`** → **`Op::CompactContext`** → **`handle_manual_compaction`**；**绕过** 上述地板（用户显式意图）。  
- **用户设置**：**`settings.auto_compact`** 默认 **`false`**（**`settings.rs`**）；开启后 TUI 可在发送前根据估算上下文尝试触发自动压缩路径（见 **`tui/ui.rs`** **`should_auto_compact_before_send`**）。  
- **保留策略**：压缩规划会参考 **working set 的 pinned 下标**与 **`KEEP_RECENT_MESSAGES`** 等常量，避免把仍在编辑线上的上下文整块删掉（详见 **`plan_compaction`** / **`compact_messages_safe`** 调用链）。  
- **工具结果在进入下一轮前的瘦身**：**`core/engine/context.rs`** 中的 **`compact_tool_result_for_context`**（及相关常量）：按 **工具名**（如 shell / web 视为「嘈杂」）施加 **软/硬字符上限**；**大上下文模型**（窗口 ≥ 约 **500K tokens**）使用更宽松的配额。

### 12.3 Checkpoint-restart 周期（Cycle / 「↻ context refreshing」）

- **动机与语义**：**`cycle_manager.rs`** 文档Issue **#124** —— 长会话里 **半摘要半原文** 的「弗兰肯斯坦」上下文易导致检索与忠实度问题；周期边界改为 **同质的新上下文**：保留 **结构化状态**（todo/plan/working set/子 Agent 句柄等）+ **模型撰写的简报 `<carry_forward>`**（约 **≤3000 tokens**），上一轮verbatim **归档为 JSONL** 供 **`recall_archive`** 按需检索（Issue **#127**）。  
- **默认阈值**：下一次请求的 **活跃输入估算** 跨越 **约 768K tokens**（**`DEFAULT_CYCLE_THRESHOLD_TOKENS`**，约为 **1M 窗口的 ~75%**）；可按 **`[cycle]` / `[cycle.per_model.<id>]`** 覆盖（见 **`CycleConfig`**）。  
- **触发时机**：回合 **`TurnOutcomeStatus::Completed`** 之后 **`maybe_advance_cycle`**（**`engine.rs`**）；内部 **`should_advance_cycle`** 要求干净相位（无在途工具/流/审批）。  
- **简报生成**：若启用 **`SeamManager`**，优先用 **Flash**（默认 **`deepseek-v4-flash`**）根据已有 seam 文本 **`produce_flash_briefing`**；失败则回退 **`produce_briefing`** 走主模型。成功后 **`session.messages`** 换为 **种子消息**（含简报 + 结构化状态块），**`cycle_count`** 递增，seam 状态重置。  

### 12.4 分层上下文 / Flash Seam（Issue #159）

- **实现**：**`crates/tui/src/seam_manager.rs`**；阈值与模型来自 **`config` → `context`** 段（见 **`config.rs`** 中 Context / seam 字段）。  
- **思路**：在 **`layered_context_checkpoint`**（**`turn_loop.rs`**）中 **追加** **`<archived_context>`** 摘要块，**不替换** 早期 verbatim 消息，从而减缓「改写前缀 → KV cache 断裂」。  
- **软接缝**：默认阈值约为估算输入 **192K / 384K / 576K** 对应 **L1–L3**；**硬周期** 对齐 **~768K**。最近 **16** 轮（**`VERBATIM_WINDOW_TURNS`**）侧倾向于保留 verbatim。  
- **是否生效**：引擎在存在 **`deepseek_client`** 时会构造 **`SeamManager`**，但 **`SeamConfig.enabled`** 实际取自 **`api_config.context.enabled`，未配置时默认为 `false`**（见 **`engine.rs`** 构造处注释）——即 **需在配置中打开 layered context**，软接缝与 Flash 摘要才会运行；**`SeamConfig` 类型的 `Default` 里 `enabled: true`** 不代替上述运行时默认值。

### 12.5 容量控制器（Capacity Controller）

- **文档**：[capacity_controller.md](./capacity_controller.md)。  
- **代码**：**`core/capacity.rs`**、**`engine/capacity_flow.rs`** 等与 **`CapacityControllerConfig`**。  
- **要点**：在 **默认 V4 路径** 上倾向 **关闭** 自动改写 live prompt（以免破坏前缀缓存）；启用 **`capacity.enabled = true`** 后可在 **请求前 / 工具后 / 工具错误升级** 等检查点做 **telemetry 或实验性干预**。

### 12.6 紧急上下文回收（Overflow Recovery）

当 **预检** 或 **提供商返回 context-length 错误** 且仍未超限重试次数时：

1. **`recover_context_overflow`**（**`engine.rs`**）强制 **`compact_messages_safe`**（**`auto_floor_tokens = 0`**，绕过自动 compaction 地板）。  
2. 若仍超预算，**`trim_oldest_messages_to_budget`** 从头部丢弃消息，至少保留 **`MIN_RECENT_MESSAGES_TO_KEEP`**（**4**）条。  
3. 全局 **`MAX_CONTEXT_RECOVERY_ATTEMPTS`**（**2**）限制反复自救次数（见 **`core/engine/context.rs`**）。

### 12.7 用户侧其它入口（与 compaction/cycle 协同）

| 入口 | 说明 |
|------|------|
| **`/compact`** | 手动触发摘要压缩（引擎 **`CompactContext`**）。 |
| **`/anchor`** | **锚点**文本：压缩后可重新注入，保住关键事实（**`commands/anchor.rs`**）。 |
| **`.deepseek/handoff.md`** | 会话外交接artifact（§10）；与 **`/compact`**、退出流程配合。 |
| **`recall_archive` 工具** | 检索周期归档 JSONL，补足「已移出 live messages」的史实（**`recall_archive.rs`**）。 |

系统提示里 **Agent/Yolo** 的 **Context Management** 段落（**`prompts.rs`**）向模型解释 **`/compact`**、前缀缓存与 **`<turn_meta>`** 分工；改行为时需 **同步检查** 该段与 **`cycle_manager` / `compaction` 默认值**。

---

## 13. 用户记忆（User Memory）

面向「跨会话、跨仓库」的 **个人偏好与惯例** 持久化；与仓库内的 **`AGENTS.md`** / **`instructions = [...]`**（项目级说明）互补。完整产品与隐私说明见 **[MEMORY.md](./MEMORY.md)**；本节侧重 **代码路径与二次开发触点**。

### 13.1 定位与边界

| 维度 | 说明 |
|------|------|
| **作用域** | **用户级**：默认文件 **`~/.deepseek/memory.md`**（**`Config::memory_path()`**，支持 **`expand_path`**）；**不是** per-repo。仓库专属约定应放在 **project context / instructions**。 |
| **默认** | **关闭**（**`Config::memory_enabled()`** 默认为假），避免未授权用户承担额外 I/O 与 prompt 体积。 |
| **注入内容** | 非空文件读写后包装为 **`<user_memory source="…">…</user_memory>`**，由 **`memory::compose_block`**（**`crates/tui/src/memory.rs`**）生成。 |
| **与前缀缓存** | 在 **`prompts.rs`** 中，**`<user_memory>`** 插在 **`instructions`** 之后、**skills 目录块** 与 **Agent/Yolo 的 `## Context Management`** 之前，仍在 **`compact.md`** 模板与 **handoff 的 volatile 边界** 之前；文件被编辑或 `remember` 写入后，下一轮 **`refresh_system_prompt`** 会重新读取，前缀可能随之更新。 |

### 13.2 启用方式与配置优先级

1. **`[memory] enabled = true`**（**`~/.deepseek/config.toml`** 中的 **`MemoryConfig`**）。  
2. 环境变量 **`DEEPSEEK_MEMORY`**：truthy 值为 **`1` / `on` / `true` / `yes` / `y` / `enabled`**（在 **`config.rs`** 载入环境时写入 **`memory.enabled`**）。  
3. 路径：**顶层 `memory_path`** 或 **`DEEPSEEK_MEMORY_PATH`**（环境变量覆盖配置文件中的路径设置，见 **MEMORY.md**）。  

启动 **`deepseek-tui`** 时 **`TuiOptions.use_memory`** 取自 **`config.memory_enabled()`**（**`main.rs`**），与引擎 **`EngineConfig.memory_enabled` / `memory_path`**（**`build_engine_config`** in **`tui/ui.rs`**）一致。

### 13.3 读路径：如何进入系统提示

- **`Engine::refresh_system_prompt`**（**`core/engine.rs`**）调用 **`memory::compose_block(self.config.memory_enabled, &self.config.memory_path)`**。  
- 若有内容，作为 **`PromptSessionContext.user_memory_block`** 传入 **`prompts::system_prompt_for_mode_with_context_skills_session_and_approval`**（见 **§10.2**）。  
- **单文件 ≤100KiB** 全文载入；超出则在 **`as_system_block`** 中截断并附加 **`…(truncated, …)`** 提示（常量 **`MAX_MEMORY_SIZE`**，与 **`project_context`** 上限对齐思路一致）。

### 13.4 写路径：三种入口

| 入口 | 实现位置 | 行为概要 |
|------|----------|----------|
| **Composer `# …` 快捷追加** | **`tui/ui.rs`** **`is_memory_quick_add`** / **`handle_memory_quick_add`** | 仅当 **`config.memory_enabled()`**；单行、以 **`#`** 开头且非 **`##`/`#!`**；调用 **`memory::append_entry`**，**不提交回合**。 |
| **`/memory` 子命令** | **`commands/memory.rs`** | 依赖 **`App.use_memory`**；**show / path / clear / edit / help**，禁用时报错提示如何开启。 |
| **`remember` 工具** | **`tools/remember.rs`** | 仅在 **`tool_setup`** 中 **`memory_enabled`** 时 **`with_remember_tool`**（**§11.3**）；**`ApprovalRequirement::Auto`**；执行时 **`ToolContext.memory_path`** 须为 **`Some`**（**`build_tool_context`** 在启用时设置，否则工具报错）。 |

**`memory::append_entry`**：创建父目录、按行追加 **`- (UTC时间戳) 条目`**，去掉composer 传入的前导 **`#`**。

### 13.5 与工具栈、子 Agent 的关系

- **`remember`** 在 **`ToolSpec`** 上标记 **`WritesFiles`**，但审批为 **`Auto`**，因写入仅限用户自有记忆文件。  
- **`MEMORY.md`** 说明 **子 Agent 继承记忆** 且可使用 **`remember`**；若在子 Agent 运行时收窄 **`memory_path`** 或禁用记忆，需检查 **`SubAgentRuntime` / `build_tool_context`** 是否与父会话一致。  

### 13.6 二次开发检查清单

- 改动注入格式： **`memory.rs`** **`as_system_block`** + **`prompts.rs`** 拼接顺序。  
- 改动开关或路径解析：**`config.rs`** **`memory_enabled` / `memory_path`** 与环境变量段。  
- 改动模型写入口：**`remember.rs`** + **`core/engine/tool_setup.rs`**。  
- 改动用户交互：**`commands/memory.rs`**、**`tui/ui.rs`** 快捷追加逻辑。  
- 文档与用户-facing 文案：**[MEMORY.md](./MEMORY.md)**、**[CONFIGURATION.md](./CONFIGURATION.md)**。

---

## 14. 配置分层与环境变量（上下文 / 记忆 / 容量）

运行时配置分散在 **`~/.deepseek/config.toml`**（及 **`managed_config`** / profile）、**环境变量**、以及 **`~/.deepseek/settings.toml`**（**`Settings`**，偏 UI 与会话习惯）。二次开发时先分清 **哪一层解析你的字段**。权威键列表见 **[CONFIGURATION.md](./CONFIGURATION.md)** Key Reference；本节只做 **与 §12 / §13 直接相关** 的对照。

### 14.1 `config.toml` + 环境变量（引擎 / API / 上下文护栏）

| 主题 | 典型键 / 环境变量 | 说明 |
|------|-------------------|------|
| **Flash seam（分层上下文）** | **`[context].enabled`**（默认 `false`）、**`l1/l2/l3_threshold`**、**`verbatim_window_turns`**、**`cycle_threshold`**、**`seam_model`** | 合并进 **`Config`** 的 **`context`** 段，构造 **`SeamConfig`**（**`engine.rs`**）；与 **`seam_manager`** 一致，见 **§12.4**。 |
| **容量控制器** | **`[capacity].enabled`**（默认 `false`）及 **`capacity.*`  Risk 参数** | 见 **[capacity_controller.md](./capacity_controller.md)**、**§12.5**。 |
| **用户记忆** | **`[memory].enabled`**、**`memory_path`**；**`DEEPSEEK_MEMORY`**、**`DEEPSEEK_MEMORY_PATH`** | 见 **§13**、**[MEMORY.md](./MEMORY.md)**。 |
| **MCP / Skills 路径** | **`mcp_config_path`**、**`skills_dir`** 等 | MCP 工具池 **重启** 后生效（CONFIGURATION 文档已有说明）。 |

### 14.2 `settings.toml`（`Settings`）与 compaction 行为

**`crates/tui/src/settings.rs`** 持久化项包含 **`auto_compact`**（默认 **`false`**，保护 V4 前缀缓存）、**`default_mode`**、**`locale`**、UI 密度等。  

- **`auto_compact`**：为真时，发送前可由 **`should_auto_compact_before_send`**（**`tui/ui.rs`**）走自动压缩路径；与 **`CompactionConfig`** 的 **`enabled`** 共同决定是否 **`compact_messages_safe`**（详见 **`App::compaction_config`** / **`update_model_compaction_budget`**）。  
- **`compact_threshold`**（**`App`** 内）：来自 **`crate::models::compaction_threshold_for_model_and_effort`**（**`models.rs`**），随当前 **模型 ID** 与 **`reasoning_effort`** 解析，**不是** `settings.toml` 里手写单独一行阈值（除非后续产品改为可配置）。

### 14.3 Checkpoint 周期（Cycle）配置现状

**`cycle_manager::CycleConfig`**（阈值 **768K**、简报上限 **~3K**、per-model 表等）由 **`EngineConfig.cycle`** 传入引擎；**`build_engine_config`** 当前使用 **`app.cycle_config()`**，而 **`App::new`** 里 **`cycle`** 字段初始为 **`CycleConfig::default()`**。  

因此：**若尚未在 UI/配置加载路径把 TOML 映射到 `App.cycle`**，运行时周期边界仍以 **代码内默认值** 为准；**`[context].cycle_threshold`** 主要对齐 **SeamManager** 的硬阈值语义，与 **周期归档** 逻辑一并参阅 **`cycle_manager.rs`** / **`should_advance_cycle`**。

---

## 15. Skills 与项目说明（`instructions` / `AGENTS.md`）

与 **§10** 系统提示拼装、**§13** 用户记忆区分：**项目侧**约定走 **仓库内文档** 与 **可安装技能包**，**不**写入 **`memory.md`**。

### 15.1 项目上下文链

- **`project_context.rs`**：沿工作区向上加载 **`AGENTS.md`** 等（具体文件名与合并规则见源码与 **CONFIGURATION.md**）。  
- **`instructions = [...]`**（**`config.toml`**）：额外 Markdown 路径列表，在 **`prompts.rs`** 中 **`render_instructions_block`** 注入（单文件与 **`instructions`** 共用 **100KiB** 量级上限逻辑，见 **`prompts.rs`** / **`memory.rs`** 常量注释）。

### 15.2 源码模块一览

| 模块 | 路径 | 职责 |
|------|------|------|
| **注册表与发现** | **`crates/tui/src/skills/mod.rs`** | **`SkillRegistry`**：单根目录递归发现、多根合并、**`render_skills_block`**（写入「可用技能」摘要）。 |
| **安装与 Registry** | **`crates/tui/src/skills/install.rs`** | **`InstallSource`** 解析、 tarball 解压、网络策略与 **`NeedsApproval`**、安装元数据标记文件。 |
| **内置技能包** | **`crates/tui/src/skills/system.rs`** | **`install_system_skills`**：随启动向 **`skills_dir`** 解压 **`skill-creator`**（见 **§15.8**）。 |
| **模型工具** | **`crates/tui/src/tools/skill.rs`** | **`load_skill`**：按名称解析 **`SKILL.md`** 并附带同级 companion 文件（见 **§15.6**）。 |
| **斜杠命令** | **`crates/tui/src/commands/skills.rs`** | **`/skills`**（列表）、**`/skill`**（install/update/uninstall/trust；本地列表 **仅扫 `settings`/`config` 下的 `skills_dir`**，与 prompt 用的 **`discover_in_workspace`** 不完全一致，见 **§15.9**）。 |

扩展 Skills（新 discovery 路径、安装器或 schema）时：**同时更新** **`skills/mod.rs`**、**`prompts.rs`** 中与路径/上限相关的叙述与注释，避免系统提示、**`load_skill`** 与 **`/skills`** 列表观感不一致。

### 15.3 `SkillRegistry::discover`：遍历与 `SKILL.md` 解析

**`discover(root)`**（单根，常用于 **`app.skills_dir`**）与 **`discover_in_workspace(workspace)`**（多根合并）底层都走同一套 **`discover_recursive`**：

- **遍历方式**：对每层目录 **`read_dir`**；子目录的 **`file_type()`** 在非跟随语义下取得元数据，从 **`symlink`** 到目录时通常 **`is_dir()` 为假**，从而 **避免跟随符号链接目录**（与注释「不跟随 symlink」一致）。
- **深度**：相对传入根目录的最大深度为 **`MAX_DISCOVERY_DEPTH = 8`**。
- **隐藏目录**：任何 **子目录** 名以 **`.`** 开头则 **整棵子树跳过**（例如 **`.git`**）；**用户显式配置的根目录本身** 即使是隐藏路径也会被扫描。
- **粒度**：若某目录下存在 **`SKILL.md`**，则解析为一个 **`Skill`**，且 **不再向下递归该目录的子目录**（包内嵌套的 **`SKILL.md`** 不会当成独立技能）。

**`parse_skill(path, contents)`**：

- 优先识别 **`---`** YAML frontmatter；字段 **`name`**、**`description`** 可选。
- 若无可用 frontmatter，则用正文里 **第一个 Markdown ATX 标题**（**`# ...`**）推导 **`name`**。
- **`Skill::name`**：frontmatter 的 **`name:`** 取 **`trim`** 后的值；无 frontmatter 时取首个 **`#` 标题** 的 **`trim`** 文本。**合并与查找**均按字符串 **精确相等**（见 **`get`**），不做大小写折叠。

### 15.4 `skills_directories` 与 `discover_in_workspace`（合并顺序）

**`skills_directories(workspace: &Path) -> Vec<PathBuf>`** 按固定候选序列构造路径，再经 **`existing_skill_dirs`** 过滤为 **「存在且为目录」** 的列表，并去掉重复路径。**从前到后** 为：

1. **`workspace/.agents/skills`**
2. **`workspace/skills`**
3. **`workspace/.opencode/skills`**
4. **`workspace/.claude/skills`**
5. **`workspace/.cursor/skills`**
6. **`~/.agents/skills`**（若 home 可解析）
7. **`~/.claude/skills`**（若 home 可解析）
8. **`skills::default_skills_dir()`** —— 恒为 **`~/.deepseek/skills`** 形态（见 **`skills/mod.rs`**），**不是** 运行时 **`Config::skills_dir()`** 解析出的任意自定义路径。

**`discover_in_workspace(workspace)`** 按上述顺序逐个 **`SkillRegistry::discover`**，再把技能 **追加进合并注册表**：若已存在 **同名**（**`Skill::name` 字符串相等**），则 **跳过后来的条目**（**first-wins**）。

**与配置的落差**：用户在 **`config.toml`** 里把 **`skills_dir`** 指到 **`~/.deepseek/skills`** 以外时，**安装器、`/skills`、运行时 **`app.skills_dir`** 会以该路径为准**，但 **`discover_in_workspace` 仍不会扫描该路径** —— 除非它与 **`default_skills_dir()`** 指向同一路径或你把技能放到上述工作区 / 全局约定目录之一。

**`prompts.rs` 拼装**：优先 **`render_available_skills_context_for_workspace(workspace)`**（即上面的合并发现）；若返回 **`None`**（合并结果为空），才 **`or_else`** 回退 **`render_available_skills_context(skills_dir)`**，其中的 **`skills_dir`** 为调用方传入的 **`Config::skills_dir()`** 解析结果。因此 **「仅在自定义全局目录装了技能、且工作区合并非空」** 时，自定义目录里的技能 **不会** 与合并列表union，这是当前实现的可预期的边角。

### 15.5 `render_skills_block`：注入系统提示时的上限

内部函数 **`render_skills_block(registry: &SkillRegistry)`** 接收 **已合并的注册表**（来自 **`discover_in_workspace`** 或单目录 **`discover`**）。公开入口 **`render_available_skills_context_for_workspace`** / **`render_available_skills_context`** 负责先 discovery 再调用它。主要护栏：

| 常量 / 行为 | 值 | 含义 |
|-------------|-----|------|
| **`MAX_SKILL_DESCRIPTION_CHARS`** | **512** | 每条 **`description`**（以及 warning 行）经 **`truncate_for_prompt`**：压成单行后超长则截断并加 **`…`**。 |
| **`MAX_AVAILABLE_SKILLS_CHARS`** | **12 000** | **`### Available skills`** 下列出的条目累计字符若再加下一行会超限，则 **省略该行** 并以计数 **`... N additional skills omitted`** 收尾。 |
| **warnings** | **`.take(8)`** | **`### Skill load warnings`** 最多 **8** 条（源码字面量，无单独命名常量）。 |

这与 **§10** 中 **`refresh_system_prompt`** 衔接：**模型看到的是名称 + 简短描述 + 磁盘路径 + 使用约定**，完整 **`SKILL.md`** 正文仍靠 **`read_file`** 或 **`load_skill`**。

### 15.6 `load_skill` 工具行为

**`tools/skill.rs`**：

- 使用 **`discover_in_workspace(&context.workspace)`**，与系统提示用的合并目录集合一致。
- **`SkillRegistry::get(name)`** 按 **`Skill::name` 字符串精确匹配**（**大小写敏感**）；传入的 **`name`** 仅 **`trim()`**，不做大小写折叠。
- **`format_skill_body`**：输出 **`# Skill: …`**、可选引用块描述、**`Source:`** 路径、**`## SKILL.md`** 正文（来自解析阶段填入的 **`skill.body`**，而非重复围栏代码块）。
- **`collect_companion_files`**：**`SKILL.md` 所在目录**下 **所有同级普通文件**（**`read_dir` + `is_file`**），**排除 **`SKILL.md` 本身**；**不包含子目录**；路径排序后写入 **`## Companion files`** 无序列表，供模型按需 **`read_file`**。

用途：**渐进披露**——目录已在系统提示里出现过时，一条工具调用即可拉到正文 + 同级资源清单。

### 15.7 `install.rs`：Registry、 tarball 与信任标记

- **`InstallSource::parse`**：支持 **`github:user/repo[/path]`**、**`https://…`**（**`.tar.gz` / `.tgz`**）、以及在 **`DEFAULT_REGISTRY_URL`** 上解析的 **裸 registry key**（内部再映射到 GitHub tarball URL）。
- **体积**：**`DEFAULT_MAX_SIZE_BYTES = 5 * 1024 * 1024`**（5 MiB）下载上限。
- **解压安全**：拒绝 **`..`** 路径逃逸；临时目录解压后经 **`rename`** 原子落到目标技能目录。
- **审批**：**`NetworkPolicy`** / **`NeedsApproval`** —— 例如从未知 Host 拉 tarball、或非显式 **`github:`** 形态的 registry 解析，可走 **`needs_approval`**，由上层（Agent/YOLO 等）决定是否允许联网。
- **标记文件**（技能目录内）：**`INSTALLED_FROM_MARKER`**（**`.installed-from`**）、**`TRUSTED_MARKER`**（**`.trusted`**）、**`INSTALLED_DATE_MARKER`**（**`.installed-date`**），供 **`/skill`** 子命令展示信任与来源。

### 15.8 内置 `skill-creator`（`system.rs`）

**`install_system_skills(skills_dir)`**（**`main.rs`** 启动时调用）：若 **`skills_dir/skill-creator/`** 不存在或版本标记 **`skill-creator/.system-installed-version`** 低于 **`EMBEDDED_SKILL_CREATOR_VERSION`**，则从 **`include_bytes!`** 嵌入的 zip 解压 **`skill-creator`**。保证用户开箱即有一份与 Cursor/Claude 「创建 Skill」文档相近的模板技能，且升级二进制时可刷新内置版本。

### 15.9 斜杠命令列表 vs `discover_in_workspace`

**`/skills` 本地列表**（**`commands/skills.rs`**）当前实现为 **`SkillRegistry::discover(&app.skills_dir)`**：只扫描 **`Config::skills_dir()`** 解析出的 **`app.skills_dir`**，**不合并** 工作区 **`.agents/skills`**、**`skills/`** 等路径。

**系统提示**（在 workspace 非空且合并发现非空时）与 **`load_skill`** 使用 **`discover_in_workspace`**。再加 **§15.4** 所述：**自定义 `skills_dir`** 与 **`default_skills_dir()`** 不一致时，**CLI 与模型目录也可能分叉**。二次开发若要让 **`/skills`**、安装路径与 prompt **`load_skill`** 完全一致，需要在 **`skills_directories`** / **`discover_in_workspace`** 与 **`list_skills`** 之间 **统一路径策略**（或接受现状并在产品上写明差异）。

---

*文档随仓库演进会持续与实际 crate 边界产生细小偏差；若发现不一致，以源码与 **ARCHITECTURE.md** 为准并欢迎修正本节。*
