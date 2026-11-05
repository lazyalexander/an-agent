# an-agent-host

[English](../../../../crates/an-agent-host/README.md) · 中文译文，以英文为准。 · [中文目录](../../README.md)

宿主端授权、执行监督、模型/上下文交换、提交状态和产物保管。领域规则应放在spool发布中；Loom负责构建和打包。后端强制执行每个尝试边界。

```sh
cargo install --path crates/an-agent-host --locked
```

从[文档索引](../../doc/README.md)和
[当前支持矩阵](../../doc/platforms.md)开始。包检查不是执行权限。未知终止从未授权重放、接受或清理。

| 任务 | 合约 |
| --- | --- |
| 加载、编辑、运行和控制原生 spool | [原生运行时](../../doc/native-spool.md) |
| 选择状态或审查发布版本 | [原生状态](../../doc/native-state.md) |
| 路由固定产品并调度有限批次 | [事件](../../doc/native-events.md) |
| 检查所有权并收集私有材料 | [存储](../../doc/native-storage.md) |
| 注册有界的 stdio 可执行文件 | [部署](../../doc/native-deployment.md) |
| 构建或运行已审查的操作员工作流 | [执行配置文件](../../doc/workspace-jobs.md) |
| 版本产品并协调外部读取 | [制品](../../doc/artifacts.md) |
| 提交实例状态并跟踪接收者 | [检查点](../../doc/checkpoints.md) |
| 使用模型/上下文服务或有限编码工具 | [服务](../../doc/model-services.md) |
| 使用可选的 OpenShell 后端 | [OpenShell](../../doc/openshell.md) |

`execution/` 拥有准入、后端协调和结算；`native_spool/` 拥有发布绑定的外观。`workspace_job/`、`artifact_versions/` 和 `instance/` 保持临时数据、已接受的产品和显式状态区分开。`Host::pump` 保留其嵌入的事件语义；子进程不会在无故障的汇入点中运行。

在[AGENTS.md](../../AGENTS.zh-CN.md)中运行仓库门禁。真实 CLI 接受入口点和测试用例所有权列在[tests/cli](tests/cli/README.md)中。
