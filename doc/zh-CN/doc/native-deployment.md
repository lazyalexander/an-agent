# 注册的原生部署

[English](../../native-deployment.md) · 中文译文，以英文为准。 · [中文目录](../README.md)

## 部署契约：stdio-only-v1

部署目录提供 `spool.yaml` 和 `program`。仅读取这些文件；其他开发文件不会安装。现有的 spool 解析器是权威的。宿主此外要求：

- `sort: beat`, `constructor: host`, `host: an-agent-stdio-v1`;
- `consumes: [kit.request]`, `produces: [kit.execute]`, 无依赖;
- 文件无，内存忽略，网络无，进程启动，模型无，流程两者;
- `inverse: irreversible`（无重放或回滚承诺）;
- 配置 `profile: stdio-only-v1` 和 `executable_sha256`；可选
- `source_sha256` 是生产者证据，从不作为安全或编译器证明。

未知配置将被拒绝。宿主使用一个固定入口点，无参数、shell、环境转发或包定义的策略。进程启动描述宿主的单次启动，不涉及子进程的权限。系统运行时读取是后端必需的，不是任意工作区的访问。

部署 ID 是 YAML 字节的 SHA-256 值，递归绑定程序哈希。它被有意地**不**作为 Loom 归档 ID。YAML 限制在 64 KiB，可执行文件限制在 20 MiB。更改的字节需要一个新的注册版本。相同注册是幂等的；执行不是去重的。

```sh
cargo install --path crates/an-agent-host --locked
an-agent-host check deployment
an-agent-host register deployment host-store
an-agent-host run host-store DEPLOYMENT_DIGEST input.txt
an-agent-host run host-store DEPLOYMENT_DIGEST input.txt --execute
```

默认为预览模式。显式执行会先打印解析后的计划，仅授权此受限的配置文件。`--execute` 是一个操作界面，而不是一个认证的远程授权协议。不要给未认证的模型一个无限制的shell，并将其称为受监督的执行。外部代码工具必须保留其自身的命令批准策略。

## 实际执行和证据

参见 [native Linux 验证](qualification.md) 以获取一个隔离的开发虚拟机配方和显式的 stdio 输出/上下文传递。固定文件运行时接受度在 [Linux 验证](qualification.md) 中单独记录。两者都不能作为生产虚拟机后端的资格。

参见 [模型中断](../../model-services.md#cancellation-and-deadlines) 以获取批准的 HTTP 超时、取消、保留上下文和无重试的解决方案。参见 [进程生命周期所有权](workspace-jobs.md) 以获取取消、超时处理、作用域管道清理、确认回收和崩溃限制。参见 [CLI 重启诊断](qualification.md) 以获取产品可执行文件发布者、并发宿主回归和所需 CI 管道。注册、运行快照和 macOS 构建接受使用固定的 `/bin/cp` 在一个受监督的可信过程中创建可执行文件 inode，验证其字节，并发布而不替换现有文件。嵌入过程从未在该 inode 上持有写入者。没有 shell、项目选择的辅助程序或重试。运行预览声明辅助程序的独立十秒超时；凭证分别记录出版证据和 kit 进程证据。这并不替代 kit 箱。macOS 是当前主要平台；现有的 Linux CI 保留，不扩展部署支持。

拒绝凭证包括可加性的 `error` 汇报证据（阶段、可选操作系统代码、I/O 类型和无重试标志）；标准进程 API 无法区分 spawn 从 pre-exec 政策失败。

对于 stdio 配置，macOS Seatbelt 拒绝写入、任意文件读取、网络连接和子进程创建。仅可访问选定的可执行文件、操作系统运行时库、随机设备和必要系统元数据。字面根目录对 dyld 可读；这可以暴露根目录名称，而不是子树文件内容。空环境、8 KiB UTF-8 输入、64 KiB 每个输出流和五秒墙截止时间绑定一个运行。输出与输入交付同时进行排水。Linux 使用原生 Landlock 文件系统限制、seccomp 调用允许列表和资源限制。它需要非根宿主和 Landlock ABI 3 或更高版本；缺少执行失败关闭。没有 Docker 代理或镜像。不支持的系统拒绝执行，保留拒绝凭证。没有无限制的回退。嵌入过程无法持有该 inode 上的写入者。没有 shell、项目选择的辅助程序或重试。运行预览声明辅助程序的独立十秒超时；凭证分别记录出版证据和 kit 进程证据。这并不替代 kit 箱。macOS 是当前主要平台；现有的 Linux CI 保留，不扩展部署支持。

原生穿梭只将请求事件映射到意图。实际核心注册、描述挂载、交付、意图准入和磁带记录在子进程之前。`applied` 只有在成功执行和 artifact/receipt 写入后记录。非零退出或后端拒绝永远不会获得成功的应用记录。预飞行拒绝可能在运行存在前发生；准入暂停保留核心审计，而不启动进程。

```text
host-store/
  registry/                         descriptor registry
  releases/<deployment-digest>/     spool.yaml + program
  runs/<run-id>/
    plan.json                       exact invocation, bounds, input hash
    input.txt                       explicit context snapshot
    operation.json                   host operation identity before submission
    program                         selected executable
    stdout.txt, stderr.txt          bounded results
    receipt.json                    identities, outcome, evidence paths
    agent/                          workspace events and agent tape
    audit.json                      readable tape snapshot
```

仅选中的输入字节进入 kit；它们被标记为 caller-supplied 数据，而非可信指令或验证过的事实。模型历史记录、环境密钥、其他 spool 状态以及宿主存储均不暴露。结果是数据，而非权威。原生上下文隔离通过外部文件探针进行测试。

## 限制，明确说明

这是操作系统类似宿主的责任边界，**不是容器管理器**。Linux 有针对每个进程的地址空间和 CPU 限制；macOS 目前尚未具有等效的配额。没有任意的原生依赖挂载、多用户服务保护、系统调用拒绝事件的 telemetry，或保证崩溃恢复。存储必须私有且可信；静态符号链接检查无法防止恶意同用户并发重命名。锁用于串行化共享元数据操作，而非独立子进程的生命周期；在崩溃后，应先调查再手动删除一个过时的锁。

保留是手动进行的。失败和部分运行仍可进行检查。不要因年龄而删除证据，也不要覆盖外部文档。保留输入和可执行字节有助于调查，而非精确的重新执行或外部世界的恢复。现有的内容地址映射工作区 crate 不在此处复制或静默地接入记录器。更广泛的版本控制、参考感知清理和容器后端需要各自的具体消费者和合同。
