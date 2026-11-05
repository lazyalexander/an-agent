# 可选的 Linux 开发机

[English](../../../../../infra/dev/lima/README.md) · 中文译文，以英文为准。 · [中文目录](../../../README.md)

`arm-linux.yaml` 在 Apple 芯片 Mac 上提供一台普通的 ARM Linux 机器。它仅用于开发基础设施。原生 Linux 用户不需要 Lima 也不需要此配置。宿主运行时策略仍保留在 `an-agent-host` 中，并使用原生内核强制执行，而不是 VM 提供商接口。

参见 [原生 Linux 接受](../../../doc/qualification.md) 了解设置、源码传输、测试和证据边界。现有的 Lima 实例会保留自己的复制配置；移动此文件不会修改、重启或删除它们。不要仅仅为了采用此仓库布局而重新创建实例。SPACE/KVM 接受仍保持独立。
