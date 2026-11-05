# 开发者资格配方

[English](../../qualification.md) · 中文译文，以英文为准。 · [中文目录](../README.md)

当前支持在[平台](platforms.md)上具有权威性。这些配方使用现有的强制措施；它们不添加Linux兼容性、虚拟机后端或新的部署目标。原生无头spool工作目前优先考虑macOS。

## 可选ARM Linux开发机器

原生Linux用户不需要Lima或Docker。在Apple硅片开发机器上，查看[Lima配置](../../../infra/dev/lima/arm-linux.yaml)，并在从仓库根目录运行之前明确安装Lima：

```sh
limactl validate infra/dev/lima/arm-linux.yaml
limactl start --tty=false --name=an-agent-arm-validation infra/dev/lima/arm-linux.yaml
limactl shell --workdir=/home/runner an-agent-arm-validation uname -srmo
```

这是一个普通的开发 guest，不是 spool 的隔离边界。
配置固定了其镜像并禁用了宿主挂载、代理转发、动态端口转发和 containerd。请在实际实例中验证这些设置。已有的实例保留了复制的配置；不要仅仅因为仓库文件移动了就重新创建或重启实例。

在 guest 中安装编译器、链接器、OpenSSL 开发包、Git、Python 以及仓库绑定的 Rust 工具链。审查安装程序和依赖构建脚本作为受信任的开发者操作。仅将 Git 源代码快照和显式锁文件转移到新目录中；记录修订版本并归档哈希值。不要传输凭证、私有家目录或无关文件。构建本地 guest 二进制文件；Mac 可执行文件不是 Linux 可执行文件。

以非 root 用户身份运行，确保仓库工具链在 PATH 中：

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo build -p an-agent-host --bin an-agent-host --locked
```

Landlock ABI 3 或更新版本、seccomp 和必需的文件系统操作必须可用。缺少强制措施将拒绝；不要禁用限制以将拒绝转换为通过。现有的 stdio/文件执行测试不适用于原生工作区任务、隔离的 Linux 源码构建、GPU、SPACE 或通用微虚拟机。Loom 配对测试选择显式兼容的宿主版本；使用其当前接受映射，而不是已弃用的 Waver 命令示例。

保留收据、精确的源码/二进制标识、选定的输入、终止确认和已验收的产物哈希。上下文导出是独立的有限选择；它不会调用模型或授权另一次运行。只复制显式的证据文件，比较传输哈希，绝不要将客户路径视为可信宿主路径。复制的收据不是经过认证的远程证明。只停止操作员拥有的开发实例，在保留证据之后。

## 历史证据

详细的 ARM Linux 运行、取消/下次启动拒绝调查、OpenShell 冷启动时间以及多 silk 压力实验仍保留在 [预整合源代码树](https://github.com/lazyalexander/an-agent/tree/b44b3c538eb9428975d38e7cbc3669ddf9b0f827) 中。
这些日期测量结果不是当前平台认证或性能 SLA。原始日志、机器配置和提供商凭证仍保持私密。

从启动拒绝调查中获得的重要产品修复被保留：宿主分叉可能会继承一个可写文件节点上的写入器，导致 Linux `ETXTBSY`。宿主拥有的可执行文件发布现在使用受监督的固定复制辅助程序，独立的数据/候选文件节点、精确验证和无覆盖发布。隔离测试或重试启动并不是修复方法。请参阅 [部署合同](native-deployment.md) 以及 [执行并发性](../../../crates/an-agent-host/tests/execution/concurrency.rs) 中的实际回归问题。
