# PRD: 多适配器 + Wrapper 模式 + 自动脱敏

> Execution Trace 模块的基础设施增强

---

## 目录

1. [多适配器扩展](#1-多适配器扩展)
2. [Wrapper 实时捕获模式](#2-wrapper-实时捕获模式)
3. [自动脱敏](#3-自动脱敏)

---

## 1. 多适配器扩展

### 1.1 背景

当前 Execution Trace 仅有 DeepSeek 适配器。需要扩展到 Claude Code 和 OpenAI，并建立一个可扩展的适配器注册机制。

### 1.2 设计决策

| # | 决策 | 选择理由 |
|---|------|---------|
| D1 | `#[trace_adapter]` 属性宏自动注册 | 加新适配器只需写 impl，不修改注册表代码 |

### 1.3 宏实现

```rust
//! 文件: src/trace/adapter_registry.rs
//!
//! `#[trace_adapter]` 属性宏：自动将适配器注册到全局注册表。

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct};

/// 标记一个结构体为 TraceAdapter 实现，自动注册到 `inventory` 收集器。
///
/// # 用法
///
/// ```ignore
/// use Paporot::trace_adapter;
///
/// #[trace_adapter]
/// pub struct ClaudeAdapter;
///
/// impl TraceAdapter for ClaudeAdapter {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn trace_adapter(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let name_str = name.to_string();

    let expanded = quote! {
        #input

        // 自动生成注册入口
        inventory::submit! {
            crate::trace::adapter_registry::AdapterEntry {
                name: #name_str,
                factory: || Box::new(#name::new()),
            }
        }
    };

    TokenStream::from(expanded)
}

/// 适配器注册表条目。
pub struct AdapterEntry {
    pub name: &'static str,
    pub factory: fn() -> Box<dyn crate::trace::adapter::TraceAdapter>,
}

inventory::collect!(AdapterEntry);

/// 获取所有通过宏注册的适配器（替代手动 all_adapters()）。
pub fn all_adapters() -> Vec<Box<dyn crate::trace::adapter::TraceAdapter>> {
    inventory::iter::<AdapterEntry>
        .into_iter()
        .map(|entry| (entry.factory)())
        .collect()
}
```

### 1.4 新增适配器：Claude Code

```rust
//! 文件: src/trace/adapters/claude.rs

#[trace_adapter]
pub struct ClaudeAdapter;

impl TraceAdapter for ClaudeAdapter {
    fn name(&self) -> &str { "claude-code" }
    fn version(&self) -> &str { "1.0.0" }

    fn can_handle(&self, raw: &str) -> bool {
        let head = &raw[..raw.len().min(4096)];
        // Claude session 日志格式特征
        head.contains("\"type\":\"assistant\"") && head.contains("\"tool_use\"")
    }

    fn parse(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        // 解析 Claude 的 tool_use / tool_result 结构
        // ...
    }

    fn description(&self) -> &str {
        "Parses Claude Code session logs into Paporot BehaviorTraces"
    }
}
```

### 1.5 新增适配器：OpenAI

```rust
//! 文件: src/trace/adapters/openai.rs

#[trace_adapter]
pub struct OpenAIAdapter;

impl TraceAdapter for OpenAIAdapter {
    fn name(&self) -> &str { "openai" }
    fn version(&self) -> &str { "1.0.0" }

    fn can_handle(&self, raw: &str) -> bool {
        let head = &raw[..raw.len().min(4096)];
        // OpenAI response 格式特征
        head.contains("\"object\":\"chat.completion\"") && head.contains("\"tool_calls\"")
    }

    fn parse(&self, raw: &str, file_path: &str) -> Result<Vec<BehaviorTrace>, TraceError> {
        // 解析 OpenAI Chat Completion response（与 DeepSeek 相似但字段名有差异）
        // ...
    }

    fn description(&self) -> &str {
        "Parses OpenAI Chat Completion responses into Paporot BehaviorTraces"
    }
}
```

### 1.6 新增依赖

```toml
[dependencies]
inventory = "0.3"          # 编译时适配器收集

# Paporot 自身作为 proc-macro crate
# 需要拆分为 paporot（lib） + paporot-macros（proc-macro）
# 或使用 linkme 作为替代（更轻量）
linkme = { version = "0.3", features = ["used_linker"] }
```

---

## 2. Wrapper 实时捕获模式

### 2.1 背景

当前 Trace 数据只能通过事后导入（`paporot trace import`）。Wrapper 模式让 Paporot 能在 Agent 执行时实时捕获轨迹。

### 2.2 设计决策

| # | 决策 | 选择理由 |
|---|------|---------|
| D1 | 子进程模式（主） + SDK API（可选） | 子进程通用无侵入；SDK 给需要精确控制的用户 |

### 2.3 子进程模式

```rust
//! 文件: src/trace/wrapper.rs

use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use crate::trace::types::{BehaviorTrace, ToolCall, Observation, TraceSource};
use crate::trace::storage::TraceStorage;

/// Wrapper 配置。
pub struct WrapperConfig {
    /// Agent CLI 命令（如 "claude "fix this bug""）
    pub agent_command: Vec<String>,
    /// 输出格式: "deepseek" | "claude" | "openai" | "auto"
    pub output_format: String,
    /// 可选的要关联的 Capability ID
    pub capability_id: Option<String>,
    /// 标签
    pub tags: Vec<String>,
}

/// 子进程 wrapper：启动 Agent，实时采集 trace。
pub fn run_wrapper(
    storage: &TraceStorage,
    config: &WrapperConfig,
) -> Result<BehaviorTrace, TraceError> {
    // 1. 解析 command
    let mut cmd = Command::new(&config.agent_command[0]);
    if config.agent_command.len() > 1 {
        cmd.args(&config.agent_command[1..]);
    }

    // 2. 捕获 stdout/stderr
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| TraceError::Io {
        message: format!("Failed to start agent process: {}", e),
    })?;

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    let mut raw_lines = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap_or_default();
        raw_lines.push(line);
    }

    child.wait().unwrap(); // 等待 Agent 执行完成

    // 3. 解析输出
    let raw = raw_lines.join("\n");
    let adapter = if config.output_format == "auto" {
        crate::trace::adapter::auto_detect(&raw)
            .ok_or_else(|| TraceError::ParseError {
                message: "Cannot auto-detect agent output format".into(),
                adapter: "auto".into(),
            })?
    } else {
        crate::trace::adapter::find_adapter(&config.output_format)
            .ok_or_else(|| TraceError::ParseError {
                message: format!("Unknown format: {}", config.output_format),
                adapter: config.output_format.clone(),
            })?
    };

    let mut traces = adapter.parse(&raw, "stdout_capture")?;

    // 4. 保存
    if let Some(mut trace) = traces.pop() {
        trace.source = TraceSource::Captured {
            agent_name: config.agent_command[0].clone(),
        };
        if let Some(ref cap_id) = config.capability_id {
            trace.capability_ids.push(cap_id.clone());
        }
        trace.tags = config.tags.clone();
        storage.save(&trace)?;
        Ok(trace)
    } else {
        Err(TraceError::ParseError {
            message: "No trace parsed from agent output".into(),
            adapter: adapter.name().into(),
        })
    }
}
```

### 2.4 SDK API（可选，给有嵌入需求的用户）

```rust
/// SDK 风格 API：手动记录 tool 调用前后。
pub struct TraceRecorder {
    trace: BehaviorTrace,
    trace_start: std::time::Instant,
}

impl TraceRecorder {
    /// 开始记录一次 Agent 执行。
    pub fn start(session_id: &str, prompt: &str) -> Self {
        Self {
            trace: BehaviorTrace {
                id: String::new(),
                session_id: session_id.into(),
                prompt: prompt.into(),
                tool_calls: Vec::new(),
                observations: Vec::new(),
                final_output: String::new(),
                token_usage: Default::default(),
                started_at: chrono::Utc::now().to_rfc3339(),
                finished_at: String::new(),
                source: TraceSource::Captured {
                    agent_name: "sdk".into(),
                },
                tags: Vec::new(),
                capability_ids: Vec::new(),
                deleted: false,
            },
            trace_start: std::time::Instant::now(),
        }
    }

    /// 记录一次 tool 调用。
    pub fn record_tool_call(&mut self, tool_name: &str, args: serde_json::Value) -> String {
        let call_id = format!("call_{}_{:03}", self.trace.session_id,
            self.trace.tool_calls.len() + 1);
        self.trace.tool_calls.push(ToolCall {
            id: call_id.clone(),
            tool_name: tool_name.into(),
            args,
            timestamp: chrono::Utc::now().to_rfc3339(),
            duration_ms: 0,
            result_id: None,
        });
        call_id
    }

    /// 记录 tool 调用的返回结果。
    pub fn record_observation(&mut self, call_id: &str, content: &str) {
        let obs_id = format!("obs_{}_{:03}", self.trace.session_id,
            self.trace.observations.len() + 1);
        self.trace.observations.push(Observation {
            id: obs_id,
            tool_call_id: call_id.into(),
            content: content.into(),
            truncated: false,
            truncated_at_bytes: None,
        });
    }

    /// 结束记录并持久化。
    pub fn finish(mut self, storage: &TraceStorage,
        final_output: &str) -> Result<BehaviorTrace, TraceError> {
        self.trace.finished_at = chrono::Utc::now().to_rfc3339();
        self.trace.final_output = final_output.into();
        storage.save(&self.trace)?;
        Ok(self.trace)
    }
}
```

### 2.5 CLI 子命令

```rust
/// Wrapper 子命令。
Trace {
    // ... 现有子命令 ...

    /// 包裹 Agent 执行，实时采集 trace
    Record {
        /// Agent 命令（-- 分隔，后面跟完整的 Agent CLI 命令）
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        agent_args: Vec<String>,

        /// Agent 输出格式: deepseek | claude | openai | auto
        #[arg(short, long, default_value = "auto")]
        format: String,

        /// 关联的 Capability ID
        #[arg(long)]
        capability: Option<String>,

        /// 标签
        #[arg(short, long)]
        tag: Vec<String>,
    },
}
```

使用示例：

```bash
# 包裹 Claude Code 执行
paporot trace record --format claude -- claude "fix the login bug"

# 自动检测格式
paporot trace record --capability cap_bug_fix_001 -- deepseek-cli "add tests for auth"
```

---

## 3. 自动脱敏

### 3.1 背景

当前脱敏是手动命令（`paporot trace redact`）。自动脱敏在导入时应用。

### 3.2 设计决策

| # | 决策 | 选择理由 |
|---|------|---------|
| D1 | 可配置开关，默认关闭 | 安全：用户主动开启后才生效 |

### 3.3 配置文件

```toml
# .Paporot/config.toml

[trace.redact]
# 是否在 trace import 时自动脱敏（默认 false）
auto_redact = true

# 脱敏规则（可覆盖默认）
redact_auth_header = true
redact_api_keys = true
redact_env_values = false

# 自定义脱敏正则规则
[[trace.redact.custom_rules]]
pattern = "sk-\\w{20,}"
replacement = "sk-***REDACTED***"

[[trace.redact.custom_rules]]
pattern = "ghp_\\w{20,}"
replacement = "ghp_***REDACTED***"
```

### 3.4 脱敏触发时机

```rust
// 在 cmd_import 中
pub fn cmd_import(storage: &TraceStorage, file_path: &str,
    adapter_name: Option<&str>) -> anyhow::Result<ImportResult> {

    let config = crate::config::Config::load()?;

    // ...

    let mut traces = adapter.parse(&raw, file_path)?;

    // 如果开启了自动脱敏
    if config.trace.redact.auto_redact {
        let redact_config = RedactConfig {
            redact_auth_header: config.trace.redact.redact_auth_header,
            redact_api_keys: config.trace.redact.redact_api_keys,
            redact_env_values: config.trace.redact.redact_env_values,
            custom_rules: config.trace.redact.custom_rules.clone(),
        };
        for trace in &mut traces {
            apply_redact(trace, &redact_config);
        }
        eprintln!("  [INFO] Auto-redaction applied to {} traces", traces.len());
    }

    // ...
}
```

### 3.5 脱敏范围

- `BehaviorTrace.prompt` —— 全文扫描
- `ToolCall.args` —— JSON 字符串值扫描
- `Observation.content` —— 全文扫描
- 不脱敏 `ToolCall.tool_name`、`session_id`、时间戳等元数据

### 3.6 默认脱敏模式

```rust
/// 默认的脱敏正则模式。
pub fn default_redact_patterns() -> Vec<(&'static str, &'static str)> {
    vec![
        // API Key 模式
        ("(?i)(api_key|apikey|api-key)[=:]\s*[^\s,;]+", "api_key=***REDACTED***"),
        // Authorization Header
        ("(?i)authorization[=:]\s*[^\s,;]+", "Authorization=***REDACTED***"),
        // Bearer Token
        ("(?i)bearer\s+[^\s,;]+", "Bearer ***REDACTED***"),
        // 通用密钥模式 (key=value)
        ("(?i)(secret|password|token)[=:]\s*[^\s,;]+", "$1=***REDACTED***"),
    ]
}
```

---

## 4. 测试策略

### 适配器

| 测试 | 内容 |
|------|------|
| 宏注册 | 两个带 `#[trace_adapter]` 的结构体出现在 `all_adapters()` 中 |
| Claude can_handle | Claude session JSON 识别 |
| OpenAI can_handle | OpenAI response JSON 识别 |
| Claude parse | 最小 Claude session → BehaviorTrace |
| OpenAI parse | 最小 OpenAI response → BehaviorTrace |

### Wrapper

| 测试 | 内容 |
|------|------|
| 子进程 | 启动 `echo '{"id":"test",...}'` 模拟 Agent 输出 |
| SDK API | record_tool_call → record_observation → finish 完整流 |

### 自动脱敏

| 测试 | 内容 |
|------|------|
| 配置读取 | `auto_redact = true` 触发，`false` 跳过 |
| Prompt 脱敏 | `api_key=sk-abc123` → `api_key=***REDACTED***` |
| ToolCall args 脱敏 | JSON 字符串值中的密钥被替换 |
| 元数据不脱敏 | tool_name、session_id 等保持不变 |
