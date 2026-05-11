# OpenTelemetry / SigNoz 接入指南

DeepSeek-TUI 从 v0.8.16 起内置 **OpenTelemetry 追踪**，把每一次 LLM 请求（普通 `chat` 与流式 `chat.stream`）按 [GenAI 语义约定][semconv] 导出为 OTLP span，方便在自托管的 [SigNoz][signoz] 或任何 OTLP 兼容后端（Jaeger、Tempo、Grafana Cloud、Honeycomb、Langfuse、Phoenix …）里观测**模型调用延迟、Token 用量、错误率、推理 effort 分布**。

> **默认行为**：开关关闭。需在 `config.toml` 或环境变量里显式开启。

---

## 1. 快速开始（Docker SigNoz + DeepSeek-TUI）

### 1.1 启动本地 SigNoz

[SigNoz 官方仓库](https://github.com/SigNoz/signoz) 提供了开箱即用的 docker-compose。最少步骤：

```bash
git clone -b main https://github.com/SigNoz/signoz.git
cd signoz/deploy/docker
docker compose up -d
```

启动完成后：

| 服务 | 默认端口 | 用途 |
|------|---------|------|
| SigNoz Web UI | http://localhost:8080 | 仪表板 / Trace 浏览 |
| OTLP/HTTP | http://localhost:4318 | DeepSeek-TUI 默认上报端点 |
| OTLP/gRPC | http://localhost:4317 | 可选（需开启 `grpc-tonic` 编译特性） |

首次登录 SigNoz 会引导你创建管理员账号；该账号仅本地有效。

### 1.2 在 DeepSeek-TUI 里启用

任选其一：

**方式 A — 环境变量（推荐，零配置）**

在 shell 或 `.env` 里加入：

```bash
export DEEPSEEK_OTEL_ENABLED=true
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
export OTEL_SERVICE_NAME=deepseek-tui
# 可选：标记环境
export DEEPSEEK_OTEL_ENVIRONMENT=local
```

**方式 B — `~/.deepseek/config.toml`**

```toml
[telemetry]
enabled = true
endpoint = "http://localhost:4318"
protocol = "http/protobuf"        # 默认；改成 "grpc" 需要 grpc-tonic 编译特性
service_name = "deepseek-tui"
environment = "local"
redact_content = true             # 保持 true，除非你信任所有能看到 trace 的人
```

### 1.3 验证

```bash
deepseek -p "测试一下 OTel 链路"
```

完成后打开 http://localhost:8080，进入 **Traces** → 选服务 `deepseek-tui`，应能看到名为 `chat.stream <model>` 的 span，其中包含 `gen_ai.system`、`gen_ai.usage.input_tokens` 等属性。如果没有，按第 4 节排查。

---

## 2. 上报内容

每次 LLM 调用产生一个 span（名字为 `gen_ai.chat` 或 `gen_ai.chat.stream`）。span 上挂载以下 [GenAI 语义约定][semconv] 属性：

| 属性 | 示例 | 说明 |
|------|------|------|
| `gen_ai.system` | `deepseek` / `nvidia-nim` / `openrouter` | 提供方标识 |
| `gen_ai.operation.name` | `chat` / `chat.stream` | 操作类型 |
| `gen_ai.request.model` | `deepseek-v4-pro` | 请求模型 ID |
| `gen_ai.request.max_tokens` | `8192` | 最大输出长度 |
| `gen_ai.request.temperature` / `gen_ai.request.top_p` | `0.7` / `0.9` | 采样参数（缺省 `NaN`） |
| `gen_ai.request.reasoning_effort` | `max` / `high` / `off` | DeepSeek V4 推理 effort |
| `gen_ai.request.streaming` | `true` | 是否走 SSE 流式 |
| `gen_ai.request.tool_count` | `12` | 这一步暴露给模型的工具数 |
| `gen_ai.response.model` | `deepseek-v4-pro` | 实际返回的模型 ID（NIM 等可能带前缀） |
| `gen_ai.response.id` | `chatcmpl-xxx` | 上游返回的请求 ID |
| `gen_ai.response.finish_reasons` | `stop` / `tool_calls` / `length` | 终止原因 |
| `gen_ai.usage.input_tokens` | `4231` | 上行 token |
| `gen_ai.usage.output_tokens` | `812` | 下行 token |
| `gen_ai.usage.reasoning_tokens` | `512` | 思考 token（仅 thinking 模式） |
| `gen_ai.stream.bytes` | `38421` | 流总字节数 |
| `gen_ai.stream.duration_ms` | `4823` | 流总耗时（毫秒） |
| `error` / `error.message` | `true` / `"HTTP 429: ..."` | 失败时记录 |

**Resource 属性**（所有 span 共享）：

- `service.name` — 默认 `deepseek-tui`，可通过 `service_name` / `OTEL_SERVICE_NAME` 覆盖。
- `service.version` — 编译时从 `Cargo.toml` 读取（如 `0.8.16`）。
- `deployment.environment.name` — 由 `environment` / `DEEPSEEK_OTEL_ENVIRONMENT` 提供。
- `telemetry.sdk.language` — `rust`。

此外，每次 turn-step 还会发出一条 `tracing::info!` 事件 `starting model request`，携带 `turn_id` / `turn_step` / `model` / `mode` 字段，方便在 SigNoz 的 **Logs** 视图里把同一 turn 的多步请求拉到一起。

---

## 3. 内容脱敏

**默认 `redact_content = true`**：span 上只导出**元数据**（模型 ID、Token 数、耗时、错误），不导出 prompt / completion 正文。

若你确认 collector 在本地或在受信网络里、且需要排查模型“为什么这么回”，可以关掉脱敏：

```toml
[telemetry]
redact_content = false
```

或：

```bash
export DEEPSEEK_OTEL_REDACT_CONTENT=false
```

> **注意**：当前实现只是为后续 prompt/completion 事件准备了开关，正文事件尚未启用。开启它**不会**立刻让 prompt 出现在 SigNoz；后续版本会按 `gen_ai.prompt` / `gen_ai.completion` 事件写入。

---

## 4. 排错

### 4.1 SigNoz 里看不到任何 trace

按这个顺序检查：

1. **开关**：`deepseek doctor` 会读取并打印当前 config；确认 `[telemetry] enabled = true`，或者 `DEEPSEEK_OTEL_ENABLED=true` 在当前 shell 里生效。
2. **端点**：默认 `http://localhost:4318`；若 SigNoz 在远程 / 容器内，把 `endpoint` 改成对应主机名。**注意路径**：endpoint 可以写到根 (`http://host:4318`) 也可以写到信号路径 (`http://host:4318/v1/traces`)，DeepSeek-TUI 会自动补全。
3. **日志**：用 `RUST_LOG=info deepseek …` 启动；若 OTLP 初始化失败，日志里会出现 `OpenTelemetry exporter init failed (…)`。
4. **网络**：在主机上 `curl -v http://localhost:4318/v1/traces` 应返回 405（GET 不允许）而不是连接被拒——否则就是 SigNoz 没起来或端口被防火墙拦了。
5. **服务名**：SigNoz UI 默认按服务名过滤，先确认下拉里是否有 `deepseek-tui`；自定义了 `service_name` 就按你设置的名字找。

### 4.2 关掉编译 OTLP

如果只是想完全去掉 OTLP 依赖（例如压缩二进制体积），目前 OTLP 编入了默认编译——可以在 `crates/tui/Cargo.toml` 把相关 dep 标成可选，并加 `cfg(feature = "otel")` 包裹 `crate::telemetry` 的使用点。本仓库尚未把 OTel 做成可选 feature，如有强需求请提 issue。

### 4.3 想用 OTLP/gRPC

`grpc-tonic` 特性默认**未编译**，原因是引入 `tonic` 会显著增加构建时间且需要保证 TLS 后端一致。改用 gRPC：

```toml
# crates/tui/Cargo.toml
opentelemetry-otlp = { version = "0.31", default-features = false, features = [
  "http-proto",
  "reqwest-blocking-client",
  "trace",
  "grpc-tonic",
  "tls-webpki-roots",
] }
```

然后把 `crates/tui/src/telemetry.rs` 的 `build_span_exporter` 改成调用 `.with_tonic()`。当前实现遇到 `protocol = "grpc"` 会打 warning 并回退到 HTTP。

### 4.4 想发到 SigNoz Cloud（托管版）

```toml
[telemetry]
enabled = true
endpoint = "https://ingest.<region>.signoz.cloud:443"
protocol = "http/protobuf"

[telemetry.headers]
"signoz-ingestion-key" = "YOUR_KEY"
```

或：

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="https://ingest.<region>.signoz.cloud:443"
export OTEL_EXPORTER_OTLP_HEADERS="signoz-ingestion-key=YOUR_KEY"
```

---

## 5. 实现位置（给二次开发者）

| 文件 | 作用 |
|------|------|
| `crates/tui/src/telemetry.rs` | 设置 OTLP exporter、`TracerProvider`、`tracing` 订阅链；`TelemetryGuard` 负责 shutdown 时 flush |
| `crates/tui/src/config.rs` (`TelemetryConfig`) | `[telemetry]` 配置项与 env override |
| `crates/tui/src/main.rs` (`init_telemetry_from_cli`) | `main()` 启动早期初始化；guard 持有到进程结束 |
| `crates/tui/src/client/chat.rs` | 在 `create_message_chat` / `handle_chat_completion_stream` 上挂 `gen_ai.*` span |
| `crates/tui/src/core/engine/turn_loop.rs` | 每个 turn-step 之前发出 `deepseek.turn` info 事件，便于 trace ↔ turn 对齐 |

**贡献新 attribute**：

- 协议级（适用于所有 provider）的属性放在 `client/chat.rs` 创建 span 时声明；
- 引擎级（依赖会话 / turn 状态）的字段放在 `turn_loop.rs` 的 `tracing::info!` 事件里；
- 受信内容（prompt / tool output）请加在 `redact_content` 守卫之内，并在文档明示风险。

修改之前先跑：

```bash
cargo build --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features
```

---

[semconv]: https://opentelemetry.io/docs/specs/semconv/gen-ai/
[signoz]: https://signoz.io/
