# anolisa osbase sandbox 使用指南

本文档面向 ANOLISA 用户，指导如何通过 `anolisa osbase sandbox` 命令管理沙箱运行时。

---

## 前置条件

### 系统要求

| 操作系统 | 架构 |
|----------|------|
| Alibaba Cloud Linux 4 (alinux4) | x86_64 / aarch64 |
| Alibaba Cloud Linux 3 (alinux3) | x86_64 |
| Ubuntu 24.04 | x86_64 |

### 权限要求

所有 `osbase` 命令必须以 root 权限运行：

```bash
sudo anolisa osbase sandbox <命令>
```

非 root 执行会直接报错退出（exit code 5）。

### RPM 仓库配置

确保 `/etc/yum.repos.d/anolisa-sandbox.repo` 包含以下内容：

```ini
[anolisa-sandbox]
name=ANOLISA Sandbox Repository
baseurl=https://mirrors.openanolis.cn/anolisa/sandbox/$basearch/
enabled=1
gpgcheck=1
gpgkey=https://mirrors.openanolis.cn/anolisa/RPM-GPG-KEY-ANOLISA
```

---

## 可用场景（Scenario）

| Scenario | 隔离类型 | 适用场景 |
|----------|----------|----------|
| `runc` | 容器隔离（namespace + cgroup） | 通用容器工作负载，轻量快速 |
| `rund` | 轻量级虚拟机隔离 | 需要强隔离的容器化应用，启动速度敏感 |
| `kata-fc` | 轻量级虚拟机隔离 | 需要 Firecracker 级别的安全边界 |
| `kata-clh` | 轻量级虚拟机隔离 | 需要 Cloud Hypervisor 高性能虚拟化 |
| `kata-qemu` | 虚拟机隔离 | 调试用途或需要 GPU 透传 |
| `firecracker` | 独立微虚拟机隔离 | 不接 containerd，通过 REST API 独立管理 |
| `gvisor` | 用户态内核隔离 | 多租户场景，无需硬件虚拟化支持 |
| `gvisor-substrate` | 用户态内核隔离（高密度模式） | 高密度沙箱部署，配合 Substrate 控制面 |
| `landlock` | 内核安全模块隔离 | 仅需文件/网络访问限制，无独立运行时 |

多个场景可以共存安装，互不影响。

---

## 基本操作

### 查看可装场景

列出当前系统上可以安装的所有场景：

```bash
sudo anolisa osbase sandbox list --available
```

列出已安装的场景：

```bash
sudo anolisa osbase sandbox list --installed
```

### 安装场景

```bash
sudo anolisa osbase sandbox install <scenario>
```

**示例**：安装 rund 场景

```bash
sudo anolisa osbase sandbox install rund
```

**常用选项**：

| 选项 | 说明 |
|------|------|
| `--default` | 安装后设为默认运行时 |
| `--register-runtimeclass` | 同时创建 K8s RuntimeClass 资源 |
| `--register-handler <containerd\|none>` | 指定 containerd handler 注册方式，默认 containerd |
| `--config <FILE>` | 注入自定义配置文件 |
| `--force` | 忽略非致命的前置检查警告 |
| `--no-verify` | 跳过安装后验证检查 |
| `--dry-run` | 仅输出安装计划，不实际执行 |

### 查看已装状态

查看所有场景的状态：

```bash
sudo anolisa osbase sandbox status
```

查看指定场景的状态：

```bash
sudo anolisa osbase sandbox status rund
```

### 设置默认运行时

将已安装的场景切换为默认：

```bash
sudo anolisa osbase sandbox set-default <scenario>
```

**示例**：

```bash
sudo anolisa osbase sandbox set-default rund
```

此命令仅翻转配置，不安装/卸载任何二进制。

### 卸载场景

```bash
sudo anolisa osbase sandbox remove <scenario>
```

**示例**：

```bash
sudo anolisa osbase sandbox remove kata-fc
```

| 选项 | 说明 |
|------|------|
| `--purge` | 同时删除 ANOLISA 写入的配置文件 |
| `--force` | 跳过"是否仍有工作负载在使用"检查 |
| `--dry-run` | 仅输出卸载计划，不实际执行 |

### 健康检查

对指定场景执行端到端验证（创建→执行→销毁一个微型沙箱）：

```bash
sudo anolisa osbase sandbox doctor <scenario>
```

**示例**：

```bash
sudo anolisa osbase sandbox doctor rund
```

使用 `--fix` 尝试自动修复发现的问题：

```bash
sudo anolisa osbase sandbox doctor rund --fix
```

---

## 典型操作流程

### 流程 1：在 alinux4 上安装 gVisor 沙箱

```bash
# 1. 确认 gvisor 可安装
sudo anolisa osbase sandbox list --available

# 2. 安装 gvisor 场景
sudo anolisa osbase sandbox install gvisor

# 3. 确认安装状态
sudo anolisa osbase sandbox status gvisor

# 4. 执行健康检查
sudo anolisa osbase sandbox doctor gvisor
```

### 流程 2：安装 rund 并注册 K8s RuntimeClass

```bash
# 安装 rund，注册 RuntimeClass，并设为默认运行时
sudo anolisa osbase sandbox install rund \
    --register-runtimeclass --default

# 确认状态：rund 应显示为默认
sudo anolisa osbase sandbox status
```

### 流程 3：Firecracker 独立部署（不接 containerd）

```bash
# 安装 firecracker，跳过 containerd handler 注册
sudo anolisa osbase sandbox install firecracker --register-handler none

# 确认安装成功
sudo anolisa osbase sandbox status firecracker

# 健康检查
sudo anolisa osbase sandbox doctor firecracker
```

---

## 常见问题

### Q: 非 root 运行报错？

**A**: 所有 osbase 命令必须以 root 权限执行。在命令前加 `sudo`：

```bash
sudo anolisa osbase sandbox install rund
```

### Q: dnf install 报找不到包？

**A**: 检查仓库配置文件是否存在且内容正确：

```bash
cat /etc/yum.repos.d/anolisa-sandbox.repo
```

确认 `baseurl` 地址可达，`enabled=1`。

### Q: 安装后 containerd 没有新的 runtime handler？

**A**: 检查 containerd 配置目录下是否生成了 ANOLISA 的 drop-in 文件：

```bash
ls /etc/containerd/config.toml.d/anolisa-*.toml
```

如果文件存在但未生效，尝试重新加载 containerd：

```bash
sudo systemctl reload containerd
```

### Q: --dry-run 输出的计划和实际不一致？

**A**: `--dry-run` 反映执行瞬间的环境快照。如果在查看计划和实际执行之间环境发生了变化（如其他人安装了软件包），结果可能不同。建议在同一时间窗口内完成 dry-run 与实际执行。

### Q: 提示当前 OS/架构不支持该场景？

**A**: 不是所有场景都支持所有系统。使用 `list --available` 确认当前机器可用的场景列表：

```bash
sudo anolisa osbase sandbox list --available
```

---

## 参考

### Exit Code 含义

| Exit Code | 含义 |
|-----------|------|
| 0 | 执行成功 |
| 1 | 通用失败（前置检查/包安装/服务配置/安装后验证未通过） |
| 2 | 当前系统/架构/内核不支持该场景 |
| 3 | 健康检查失败 |
| 5 | 权限不足（未使用 root）或显式传入了 --install-mode=user |

### 全局选项速查

| 选项 | 说明 |
|------|------|
| `--json` | 机器可读 JSON 输出 |
| `--dry-run` | 仅输出计划，不执行任何变更 |
| `-v, --verbose` | 输出更多详细信息（可叠加 -vv / -vvv） |
| `-q, --quiet` | 静默模式，仅输出错误 |
