# spools — spool库

[English](../../../spools/README.md) · 中文译文，以英文为准。 · [中文目录](../README.md)

已发布的spool主体库，每个spool对应一个目录。一个spool
是一个YAML授权信封加上它所包装的脚本：作者将其作为
独立文件进行编辑，并且主体通过`script: "@file.rhai"`来指定其脚本。

货架规则：

- 已发布的主体是自包含的：加载时内联脚本
  (`an_agent_spool::library::load`)，因此注册表对一个文档进行哈希处理
  并且从不追踪文件。分离是为了作者，而不是注册表。
- 脚本引用是干净的相对路径 —— 不使用绝对路径，不使用`..`，不使用`~`。
- 名称是小写蛇形/短横线形式，版本是精确的`x.y.z`，并且一个
  已发布的（名称，版本）永远不会改变：一个修复是一个新版本。
- `requires`条目必须已经发布 —— 发布顺序是
  有向无环图顺序。
- `consumes` / `produces`是必需的（使用`[]`）：spool对
  工作区事件的声明。它们在挂载时与工作区注册表进行匹配（`AgentControl::mount_spool`），双向进行 —— 消耗的事件必须注册并路由到spool，产生的事件必须注册，并且一个命名spool的路由必须被它消费。
- 脚本可以看到一个磁带投影（剪辑ID + 预览）和挂载配置
  (`config`)；它们产生延续（`halt / utter / invoke_model /
  invoke_tool / approve`）；它们从不持有完整文本、句柄或
  密码。
- `sort`（可选，默认为`beat`）指定了挂载主体的类型：
  - **beat** — 一个工作区事件输入，字符串回复输出
    (`an_agent_spool::beat`)。Beat在挂载时被编译：语法错误会导致挂载失败，而不是第一个事件。编写时的注意事项：rhai的`trim()`会就地修改并返回单位 —— 不要在表达式中使用它。
  - **policy** — 一个代理主体：磁带投影输入，延续输出
    (`an_agent_spool::policy`)，由核心逐步驱动；每个模型调用都会针对线程的卡片进行验证，并被记录为该线程的独立锚定动作 + 观察。在交付路径上，`approve`被拒绝（它属于线程循环）且`invoke_tool`尚未被允许。
  - **gate** — 钩子链机制，由宿主的钩子布线（`HostHooks`）构建；在挂载时被拒绝。

当前条目：

- `echo/` — 一个简单的宿主构造spool，库的“你好，世界”。
- `chat_npc/` — 一个Discord角色（rhai beat）：以人格化身份回答一个
  `discord.message`，回复变为`discord.post`。一个主体，多个挂载 —— 人格是挂载配置。
- `discord/` — 通道信封（宿主构造器）：消耗
  `discord.post`以发送，产生入站的`discord.message`，网络出站。脚本化的对话案例（`an-agent-spool/tests/
  discord_parley.rs`）代表了真实的桥梁。

通用的`agent_loop`策略已随着树内工具辅助程序一同退役。使用现有的编码辅助程序来实现其模型/工具循环。领域特定策略
仍可以使用上述的显式嵌入式合同。参见
[辅助程序所有权和遗留源代码](../../model-services.md#native-harness-ownership)。
