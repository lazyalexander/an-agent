# seat

[English](../../../../../../crates/an-agent-core/src/seat/README.md) · 中文译文，以英文为准。 · [中文目录](../../../../README.md)

这个进程中有谁被挂载，以及哪个可以运行。

**准入规则：** 树存在。移除一个座位会移除其子树。池一次只允许一个回合，并且不运行来宾脚本。`AgentControl` 和记录器都使用这个。两者都不拥有它。

**依赖项：** `agent`。永不：`recorder`，`control`，工作区 CAS 存储。
