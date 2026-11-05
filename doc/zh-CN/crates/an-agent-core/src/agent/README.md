# agent

[English](../../../../../../crates/an-agent-core/src/agent/README.md) · 中文译文，以英文为准。 · [中文目录](../../../../README.md)

一个实时卡片及其私有目录，用于存储其磁带。

**准入规则：** `spawn_with` 将 `BuiltAgent` 绑定到一个新目录。内核不注册任何工具。这不是宿主面，也不是文件工作区。

**依赖项：** `principal`, `memstream`, `act`。绝不要使用：`recorder`, `control`，工作区 CAS 存储。
