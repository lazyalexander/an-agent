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

现有原生执行与分层产物验收参见 [CAD → Krita 集成](https://github.com/lazyalexander/an-shuttle/blob/main/docs/cad-paint.zh-CN.md)。

## 开发

运行 `cargo test --workspace`、`cargo clippy --workspace --all-targets`、
`cargo fmt --all -- --check` 和 `cargo arch`。
[架构](doc/architecture.md)、[测试](doc/testing.md)和[质量门禁](doc/quality.md)说明完整要求。

## 案例 · 机械动画

### 水墨动画与手绘蒸汽机对比

| 水墨动画 | 手绘版本 · 6 秒 |
| --- | --- |
| ![机械水墨动画](../../assets/demos/mechanical-ink.gif) | [![手绘蒸汽机](../../assets/demos/painted-steam-engine.jpg)](../../assets/demos/painted-steam-engine.mp4) |

对比线条式机械表达与 AI 材质、手绘背景结合后的画面，理解几何与运动如何约束动画。
[教学流程与复现证据](doc/cases.md)。

### 补充案例 · 四缸发动机

<a href="../../assets/demos/painted-inline-four.mp4"><img src="../../assets/demos/painted-inline-four.jpg" alt="四缸发动机" width="420"></a>

40 个 CAD 零件与 8 秒运动周期复用手绘制作流程。
[运动、检查与接入阶段](doc/cases.md#补充案例--四缸发动机)。
