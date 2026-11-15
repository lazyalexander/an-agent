# 宿主 spool 测试夹具

[English](../../../../../../../crates/an-agent-host/tests/fixtures/spools/README.md) · 简体中文

这些固定样例用于 `discord_parley.rs` 在 `embedded-rhai` 特性下验证宿主加载、注册、挂载和分派边界。
它们是测试输入，不是内置领域库，也不是产品依赖。`chat_npc` 包装 Rhai 脚本，
`discord` 提供测试用通道声明。

生产领域知识及发布包归 an-spool；宿主保留通用加载、权限校验、生命周期及产物验收。
