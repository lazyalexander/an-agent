# det_seam

[English](../../../../../../crates/an-agent-core/src/det_seam/README.md) · 中文译文，以英文为准。 · [中文目录](../../../../README.md)

确定性缝隙 —— 唯一允许触碰操作系统时间与熵的代码。

**准入规则：** 仅允许 clock/entropy 源及其可注入的包装器。不允许任何策略，也不允许任何 *使用* 时间的逻辑 —— 这属于调用者。

**依赖项：** 本 crate 内无依赖。一切都可以依赖此 crate。

**执行方式：** `clippy.toml` 禁止的方法会拒绝裸露的 `SystemTime::now`、`Uuid::new_v4`、`rand::random` 等，无论出现在何处 —— 可重复性由编译器强制执行，而非由审查者强制执行。
