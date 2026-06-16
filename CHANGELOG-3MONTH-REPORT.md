# ANOLISA 仓库近3月变更综述（2026.03–2026.06）

## 一、总览

| 指标 | 数值 |
|------|------|
| 总提交数 | **872** |
| 特性提交 | 284 |
| 修复提交 | 340 |
| 重构提交 | 39 |
| CI/测试/文档 | 21 / 19 / 41 |
| 代码增量 | +1,072,028 行 |
| 代码删减 | -93,913 行 |

月度节奏：4月 323 → 5月 285 → 6月 262，整体活跃度保持高位，逐步从「密集建模块」转入「打磨与稳定」阶段。

---

## 二、模块投入分布

| 模块 | 提交数 | 特性数 | 定位 |
|------|--------|--------|------|
| **sec-core** | 185 | 68 | 安全中间件与可观测性 |
| **sight** | 170 | 60 | eBPF 可观测 + AgentSight Dashboard |
| **cosh** | 115 | 40 | AI Shell 交互层 |
| **tokenless** | 94 | 11 | Token 优化压缩引擎 |
| **ckpt** | 78 | 19 | 工作区快照与回滚 |
| **anolisa** | 41 | 19 | 平台安装/分发/生命周期 |
| **agentsight** | 19 | 11 | Dashboard Web UI 与 API |
| **agent-sec-core** | 32 | 11 | 安全 agent 打包 |
| **skill/os-skills** | 15+8 | 9+4 | 技能定义与扩展 |
| **memory** | — | 2 | MCP 记忆服务（5月新增） |
| **adapter** | 6 | 6 | 适配器安装框架 |
| **sandbox** | — | 1 | 沙箱安装管线（6月新增） |

---

## 三、核心架构演进

### 1. sec-core — 安全可观测体系全面成型

**4月：安全扫描器基础设施**
- L1 规则引擎（YAML 规则） + L2 ML 分类器（Prompt Guard 2）
- Prompt 注入 / 越狱检测架构与预处理管线
- Skill Ledger CLI → OpenClaw 插件 → cosh hook 全链路集成
- 技能签名、沙箱守卫与失败处理器 hooks

**5月：安全可观测 + Hermes 扩展**
- 安全可观测性 CLI + JSONL Writer + SQLite ORM
- 事件关联（correlate security events with observability events）
- Hermes 插件框架：code-scan、skill-ledger、PII checker、prompt-scan
- PII 检测器（PIIChecker）hooks / CLI / middleware
- Benchmark 套件（prompt injection detection）+ HTML 报告
- 多平台开放策略（enableBlock hook policies）

**6月：运行态激活 + 守护进程化**
- Skill Ledger 激活守护进程（daemon process）
- 运行态激活解析器（runtime activation resolver）
- 会话报告命令（session report）
- 自保护规则（self-protect rules for OpenClaw & Hermes）

> **里程碑：sec-core v0.1 → v0.6.0，从单机工具升级为全链路安全中间件平台。**

---

### 2. sight / agentsight — eBPF 可观测体系 + Dashboard

**4月：核心 BPF 探针铺设**
- tcpsniff 探针（HTTP 透明捕获）+ TLS SNI 模块
- filewatch / filewrite eBPF 探针
- HTTP/2.0 支持、ATIF 语义适配
- SLS 上传 + Logtail 文件导出
- /metrics Prometheus 端点
- C FFI API + cbindgen 头文件生成

**5月：智能发现 + 加密 + 数据管线**
- User-Agent 检测 + DNS 探针替代 SNI → 配置驱动发现规则
- HTTP/1.1 分片 SSL 写重组 + HTTP/2 HPACK 状态解码器
- 客户端混合加密（敏感消息字段）
- traceEnabled 开关（运行态热重载）
- uid 字段 + OnceLock 缓存 + startup 校验
- skill metrics 分析（cosh 文件系统发现）

**6月：稳定 + 架构治理**
- **Breaking Change：默认 traceEnabled=false，不再上传对话内容到 SLS**
- OpenAI Responses API 支持
- BPF HTTP 协议过滤器（通配捕获）
- OOM / 死循环检测与自动终止
- 崩溃检测（agent_crash trace mode）
- cgroup v1/v2 事件过滤
- QwenCode 技能发现（per-user home scanning）
- Dashboard：健康监控 UX、TTL 清理、P1/P2 角色徽章、Session ID 提示
- **架构治理：Footprint Ladder（代码面增长控制）、架构边界 CI 门禁、genai/builder.rs 拆分为 4 模块、FFI drift guard**
- agentsight-code-review skill / pr-body skill / auto-format develop-skill

> **里程碑：sight v0.1 → v0.6.1，从 eBPF 原型升级为完整可观测引擎 + Web Dashboard。**

---

### 3. cosh — AI Shell 交互平台

**4月：交互基础 + Hook 体系**
- BeforeModel / AfterModel / BeforeToolSelection hooks
- API Key 检测（从已配置 agent，用户授权）
- Tab 补全（shell mode）、fzf 异步优化
- 可配置状态栏、Secret 脱敏、cd 命令、/bug 命令
- cosh-extension.json 兼容、skill 路径自定义
- Skills TUI Panel（交互式启停）
- 首次启动引导 banner
- STS auth（ECS RAM role）
- FHS 目录布局安装

**5月：扩展 + Hook 深化**
- 多 Provider 支持、Hook systemMessages [name] 拼接
- Extension TOML 变量替换与显示控制
- 即时 Hook 激活（install/uninstall）
- ask 决策（UserPromptSubmit / PreToolUse）
- 沙箱守卫安装 + 绕过审批流
- nvm-aware Node.js 检测
- 会话导出 / 重命名 / 起始 bash 入口
- run_id 暴露（per-run event correlation）

**6月：稳定性打磨**
- ESC 取消运行中的 slash 命令
- Dashscope token plan provider
- 自动记忆后台提取系统
- WebFetch 子模型输出验证 / 拒绝检测
- 回复语言与模型身份规则
- 键盘快捷键提示（footer status bar）
- 全 shell 命令显示（hook-ask / exec confirm）

> **里程碑：cosh v2.0.1 → v2.5.0，从 v2 大版本重写到成熟交互 Shell 平台。**

---

### 4. tokenless — Token 压缩引擎

**4月：引入与核心能力**
- TOON 上下文压缩支持
- 压缩统计（auto-record from real data）
- 压缩跳过策略（skill / content-retrieval / zero savings）
- 安全加固（shell 变量插值、binary cache invalidation）
- RPM 安装路径 / Debian FHS 支持

**5月：多适配器扩展**
- Claude Code 适配器插件
- Hermes agent 插件
- 选择性 claw context engine 插件
- 分阶段安装支持
- crates.io 替换 submodule + inline toon
- rtk v0.42.3 + toon-format 0.5.0

**6月：生态扩展**
- Qwen Code 适配器（qwencode）
- Codex 适配器插件
- Qoder CLI 适配器
- OpenClaw CLI 安装/卸载集成
- --json 输出（stats summary）
- 4-phase env pre-check（cosh extension 集成）

> **里程碑：tokenless v0.1 → v0.5.1，从零到多 agent 生态的 Token 优化引擎。**

---

### 5. ckpt (ws-ckpt) — 工作区快照

**4月：引入与基础**
- ws-ckpt 引入 ANOLISA（#343）
- OpenClaw / Hermes 插件
- overlayFS 后端 placeholder

**5月：自动化 + 配置**
- 有状态守护进程 + 自动清理（auto_cleanup_keep）
- 自动推送调度
- 时间/数字双模式配置
- 超限预警（workspace > 1000 / 文件 > 90%）
- 插件 RPM / Makefile / manifest 支持
- overlayFS placeholder 接口移除

**6月：策略 + 安全**
- Per-workspace 策略覆盖（E2E）
- Hermes/OpenClaw per-ws policy via -w JSON
- Cron 定时快照
- /proc cwd 占用守卫（init/rollback）
- BtrfsLoop / InPlace 安全初始化（reflink / rename-before-rsync）

> **里程碑：ckpt v0.1.0 → v0.3.3，从概念原型到生产级工作区保护系统。**

---

### 6. anolisa — 平台安装/分发

**5月：命令面与脚手架**
- 工作区脚手架 + CLI 命令面

**6月：分发与生命周期**
- 远程分发 registry（消费端）+ 离线回退
- 组件生命周期管线 + 原始后端安装
- 分发索引默认开启 + 离线回退
- CLI updater（archive-based）
- Bug report 命令 + self update 别名
- gVisor 沙箱安装支持
- OpenClaw adapter MVP
- 订阅同意管理（#743）
- RPM 观察到的所有权模型
- install --all 全量安装
- 适配器摘要（component status）
- 帮助分组（tier subcommands）

> **里程碑：anolisa-cli v0.1.8，从零构建出完整的组件分发/安装/生命周期平台。**

---

### 7. 新模块诞生

| 模块 | 诞生时间 | 说明 |
|------|----------|------|
| **agent-memory** | 2026-05-27 | MCP 记忆服务器 v0.1.0 + OpenClaw memory-anolisa 插件 |
| **sandbox** | 2026-06-05 | 沙箱安装管线（firecracker / e2b 后端） |

---

## 四、架构治理与质量建设

| 举措 | 说明 |
|------|------|
| **Footprint Ladder** | sight 代码面增长控制策略，防止模块膨胀 |
| **架构边界 CI 门禁** | sight 模块间依赖自动检查，KNOWN_VIOLATIONS 白名单 |
| **genai/builder.rs 拆分** | 单文件拆为 4 focused modules，降低认知负载 |
| **FFI drift guard** | cbindgen 头文件 drift 检测，防止 C/Rust 接口失同步 |
| **scoped AGENTS.md** | ffi / unified / storage 独立文档上下文 |
| **ADR + PITFALLS.md** | 架构决策记录 + 非显而易见运行问题速查表 |
| **clippy + cargo-deny + rustfmt CI** | Rust lint / 供应链安全 / 格式化三道门禁 |
| **单元测试覆盖率门禁** | agentsight coverage gate in CI |
| **安全加固** | shell 变量插值保护、binary cache invalidation、seccomp arch prune |

---

## 五、版本发布时间线

```
2026-04  cosh/v2.0→v2.3 | sight/v0.1→v0.3.1 | sec-core/v0.1→v0.4 | skill/v0.1→v0.3 | tokenless/v0.1→v0.3.2
2026-05  cosh/v2.3→v2.4.1 | sight/v0.3.1→v0.5.0 | sec-core/v0.4→v0.5.0 | ckpt/v0.1→v0.3.2 | tokenless/v0.3.2→v0.4.1 | memory/v0.1.0（新）
2026-06  cosh/v2.4.1→v2.5.0 | sight/v0.5.0→v0.6.1 | sec-core/v0.5.0→v0.6.0 | ckpt/v0.3.2→v0.3.3 | tokenless/v0.4.1→v0.5.1 | skill/v0.3→v0.5.0
```

---

## 六、Breaking Changes

1. **sight：默认 traceEnabled=false**（6月11日） — 默认不再将对话内容上传到 SLS，隐私保护优先
2. **sight：system_instructions 上传条件收紧** — 仅 traceEnabled=true 时上传 gen_ai.system_instructions
3. **anolisa：移除 legacy capability 概念**（6月12日） — 重构为 component lifecycle 模型

---

## 七、趋势与阶段判断

| 阶段 | 时间 | 特征 |
|------|------|------|
| **密集筑基** | 4月 | 大量新模块引入（tokenless/ckpt/sec-core 全链路），cosh v2 重写 |
| **功能纵深** | 5月 | 安全可观测成型、sight BPF 探针深度化、memory 新生、cosh hook 深化 |
| **治理与打磨** | 6月 | 架构边界门禁、Footprint Ladder、模块拆分、breaking change（隐私优先）、CI 全面铺开 |

整体趋势：**从「快建模块」转向「治结构、守边界、稳交付」**，工程成熟度显著提升。

---

*报告生成时间：2026-06-16 | 数据源：git log 872 commits*
