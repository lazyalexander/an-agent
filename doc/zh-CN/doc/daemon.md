# macOS 用户级守护服务

[English](../../daemon.md) 为权威契约。守护服务复用现有原生 spool、
ExecutionBackend 所在执行路径、独立监督进程与产物验收，不增加平行 host。
业务步骤、领域知识和验收规则仍属于 spool。

## 启动与观测

使用所选 host 的真实绝对路径，服务目录父级须已存在。路径须规范化；macOS
的 `/var` 和 `/tmp` 是符号链接。服务目录属于当前用户，权限为 0700。

```sh
an-agent-host daemon init /absolute/private/service 2
an-agent-host daemon serve /absolute/private/service
an-agent-host daemon show /absolute/private/service
an-agent-host daemon request /absolute/private/service /absolute/request.json
an-agent-host daemon launchd /absolute/private/service /absolute/agent.plist
```

`serve` 前台运行，供用户级 launchd 管理；生成 plist 不代表已安装。
同一服务目录以操作系统文件锁保证单实例。SIGTERM 停止服务接单，不转移原任务
所有权；原有限监督者继续执行、处理取消并记录结算。禁止以 root 运行。

`endpoint.json` 保存本机工作台地址。片段中的私有令牌授权只读快照，数据接口
同时校验 Host。网页不提供控制接口、任意文件读取或跨域授权。Unix 通道只接收
同用户、有长度上限的请求。这不是对恶意同用户进程的安全隔离。

## 调度、生命周期与权限

请求包含 `op` 及所需字段。返回 `approval_sha256` 的操作先预览，再用相同
请求附加 `approve` 摘要。实例注册不授予执行或模型披露权限。

- `register`：注册现有实例；`submit`：绑定实例、run、输入和期限并排队。
- `cancel`：向确切任务请求取消；请求成功不代表终止已确认。
- `pause`、`resume`、`suspend`、`unload`：调用原生命周期接口。
- `accept`、`reclaim`：仍由原产物托管流程核验，不从队列状态推导许可。
- `show`：读取全量投影；`task` 按 ID 只查原任务，避免等待时扫描其他实例。
- `harness`：提交明确披露的有限客户端任务。
- `harness-result`：读取已完成提案，不能把它当成已验收的业务产物。

并发为 1–8，同一实例一次启动一个排队任务，最多注册 128 个实例、保留 256 个
调度条目。可执行任务按顺序选择；原直接入口不会隐式纳入全局调度。

先持久化启动占位再启动。服务崩溃后只核对原结算证据，不重跑已占位任务。
结果未知的任务继续占用预算，并显示“执行中或待核实”。缺失回执不能转换成
终止证明或回收许可。休眠选择确切已提交检查点，不恢复进程内存。
尚未实现调度档案收集、自动重试、自动迁移和通用业务图。

## 外部 harness

首版接入已限定的 Codex CLI 0.166.0，任务显式选择并绑定可执行文件摘要、
prompt、schema、scope、可选 model 和有限 MCP。原生客户端保管认证与上下文。
禁用 shell、编辑、插件、子 agent 等工具；唯一可选 MCP 工具是事先获准的
回环材料读取。该接口接收数据提案，不是任意代码代理或任意远程 harness 协议。

本地客户端继续由现有进程组监督，期限 180 秒。确认本地终止不表示远端模型
已停止或计费为零。提案须通过 spool 的业务验证和既有 kit、产物许可。
原生事件与请求保存在私有目录，工作台不披露 prompt、MCP 凭据或私有会话。

## 资源与验收

界面区分生命周期、执行结算与产物验收。服务及任务的活进程后代按出生身份采样，
RSS 求和可能含共享页；已退出进程、重新托管的进程和共享 CAD/Krita 应用不计入。
未知指标不写成零。采样不授予进程控制权，也不等同于历史使用量或模型账单。

`tests/cli/native_daemon.py` 用真实 macOS 流程检查并发、服务崩溃、取消、原产物
托管、生命周期、资源采样和只读界面授权。有限 harness 测试替身不证明模型服务
可用；Codex/MCP 与 CAD→Krita 在 Shuttle 独立验收。其他系统拒绝启动该服务。
