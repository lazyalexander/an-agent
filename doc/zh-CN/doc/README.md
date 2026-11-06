# an-agent 文档

[English](../../README.md) · 中文译文，以英文为准。 · [中文目录](../README.md)

架构和操作契约在此。包和目录的 README 文件提供方向和链接，而非并行规范。命令默认假设在仓库根目录下执行，除非另有说明。当前的保证受限于[实现状态](status.md)和[平台矩阵](platforms.md)。

## 产品与开发

| 话题 | 参考 |
| --- | --- |
| 职责、权限和依赖方向 | [架构](architecture.md) |
| 入口、监督和后端边界 | [执行](execution.md) |
| 状态、产品、临时空间和历史 | [状态和制品](state-and-artifacts.md) |
| 稳定的 silk 和可编辑的 kit 角色 | [Spool 代码](spool-code.md) |
| 审查过的 harness 开发 | [Weaver](weaver.md) |
| 实现的能力和剩余工作 | [状态](status.md) |
| 运行时支持和资格 | [平台](platforms.md) |
| 会话协调和剩余保留工作 | [会话和保留计划](session-and-retention.md) |
| 测试、CI 和跨仓库所有权 | [测试](testing.md) |
| 代码和文档质量门 | [质量](quality.md) |

## 运行时契约

| 任务 | 参考 |
| --- | --- |
| 加载/运行/编辑、轮次控制、Python 环境选择 | [原生 spool](native-spool.md) |
| 选择/提交显式状态和比较版本 | [原生状态](native-state.md) |
| 观察、路由和分发固定版本事件 | [原生事件](native-events.md) |
| 私有集合、输入退役和引用发布 | [原生存储](native-storage.md) |
| 注册和执行有界的 stdio 部署 | [原生部署](native-deployment.md) |
| 审查过的构建/工作区和进程所有权 | [执行配置文件](workspace-jobs.md) |
| 不可变的产品版本、输入克隆、中介读取 | [制品](artifacts.md) |
| 旧实例提交和检查点接收者 | [检查点](checkpoints.md) |
| 模型 CLI、显式上下文和有限编码工具访问 | [模型服务](model-services.md) |
| 可选的后端设置和合格的恢复限制 | [OpenShell](openshell.md) |
| 嵌入式交付、磁带和环境身份 | [核心契约](core-contracts.md) |
| 可选的开发者设置和历史资格 | [资格食谱](qualification.md) |

当行为发生变化时，一起更新所属契约和状态。通过链接而非重复命令、支持矩阵或安全保证。已完成的计划和过时的架构替代方案将被移除；Git 历史保留原始理由和测量数据。私有机器配置和原始操作记录不应出现在公共文档中。历史配置文件和线缆名称保持兼容性标识。
