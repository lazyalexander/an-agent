# an-agent

[English](../../README.md) · 简体中文

面向应用工作流的本地 agent 运行时，提供 macOS 用户守护服务、轻量调度与外部 harness 委派。

an-agent 负责任务权限、原执行者监督、spool 生命周期、活动记录和已验收产物。
模型循环交给外部 harness；领域知识和业务验收属于 spool；应用桥接属于 Shuttle。
守护服务复用 `ExecutionBackend` 和现有监督流程。

## 使用与观测

用 `cargo build --release --locked --bin an-agent-host` 构建。
[用户服务指南](doc/daemon.md)说明服务启动、LaunchAgent、观测界面、委派与结果回传。
任务权限随任务传递，只能收窄，可到期和撤销；取消由原执行者处理。
本地操作者边界信任同一系统用户；本地客户端退出不证明远端模型已终止。
参见[执行契约](doc/execution.md)和[平台验收](doc/platforms.md)。

## 分工

- an-agent：权限、调度、监督、生命周期、活动记录与产物保管。
- [an-loom](https://github.com/lazyalexander/an-loom)：开发、校验与打包。
- [an-spool](https://github.com/lazyalexander/an-spool)：成熟领域知识与实现。
- [an-shuttle](https://github.com/lazyalexander/an-shuttle)：应用快照、交互和交付。

不可变发布基线为 **silk**；获准修改或新增的文件标为 **kit**，确认结算后按规则保留或回收。
指令文件保持受保护，标签不授予执行权限。见[内容契约](doc/spool-code.md)。

## 示例：CAD → Krita

保存 CAD 版本并交付可按零件分层编辑的透明绘画，验证委派、执行与产物交付。

![CAD 绘画](https://raw.githubusercontent.com/lazyalexander/an-shuttle/0dfd646/examples/cad-paint/sample/vise.png)

[案例配置与验收](https://github.com/lazyalexander/an-shuttle) · [领域源码](https://github.com/lazyalexander/an-spool)

## 开发

运行 `cargo test --workspace`、`cargo clippy --workspace --all-targets`、
`cargo fmt --all -- --check` 和 `cargo arch`。
[架构](doc/architecture.md)、[测试](doc/testing.md)和[质量门禁](doc/quality.md)说明完整要求。
