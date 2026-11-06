# an-agent-workspace

[English](../../../../crates/an-agent-workspace/README.md) · 中文译文，以英文为准。 · [中文目录](../../README.md)

内容寻址存储。这不是记录器工作区（`Wp` 在内核中）。

**准入规则：** blob、tree 和 commit 的持久化，加上领导检查。无准入决策（`act`），无磁带（`memstream`）。

**权限词：** 如果一个效果超出了工作区权限，则拒绝。代理许可和 `ActKind` 说明拒绝。

**单一领导纪律：** `NotLead` 是调用者纪律，不是锁。两个写入者持有相同的领导 id 会竞争。提交过程中发生崩溃可能导致 HEAD、分支引用和 `meta` 指向不同的树。对象是内容寻址的，因此恢复需要重建。

**依赖项：** `an-agent-core` 用于 `Resource`、`ResourceKind` 和 `det_seam`。资源助记符保留在内核中。
