# macOS 用户级守护服务

## 任务级权限

操作者入口通过 `grant` 签发任务授权。`scope` 包含 `task`、`subject`、
`expires_ms`、`delegate` 和 `permissions`，预览与批准绑定其完整内容。
宿主返回私有凭据 `{id, secret}`，活动和执行记录仅保留授权标识与摘要。
`subject` 用于归属记录，持有凭据用于认证。原有同用户控制入口承担可信管理操作。

子任务和 Shuttle 使用 `daemon task-request ROOT JSON`，向同一服务的 `tasks.sock`
提交 `{credential, request}`。每次调用都校验凭据和授权范围，共用现有队列、监督器与
产物保管。执行配置负责阻止工作负载访问管理入口、宿主存储和其它凭据。原生 harness
客户端与应用适配器仍属于可信同用户软件。

权限逐项绑定资源，每份授权最多 64 项。

| 类型 | 范围 |
| --- | --- |
| `run` | 已注册实例、运行名、规范输入路径及最大时限，原有启动审批固定字节和后端策略 |
| `harness` | `harness-permission` 返回的配置摘要，绑定程序、模型、schema、scope 与有限 MCP |
| `read-input` | 规范路径和 SHA256，返回匹配的输入字节 |
| `accept` | 指定实例和运行的原有验收，以及提案采纳关联 |
| `artifact` | 指定实例、运行和输出，核验原有保管记录后返回字节与清单 |

字节读取上限为 512 KiB。应用负责材料披露与领域检查，每次 harness 提交仍批准完整的
确切提示内容。配置摘要覆盖既有范围声明，提示文字的来源需要应用保障。

`derive-grant` 允许有委派权的父任务签发子集授权。任务标识相同、到期时间不增加，
原生执行时限可以缩短，子任务可以移除继续委派的能力。宿主核对完整祖先链，最多
16 层，服务保留最多 256 份授权。调用方把子凭据明确传给选定适配器。

`revoke-grant` 撤销本授权及其后代。带授权的任务使用 `cancel` 时，预览显示将撤销的
授权，同一授权下的其它工作也会停止接单。需要独立取消的工作使用独立子授权。
原有监督器在服务退出后仍检查撤销与过期，队列重启时转发取消。撤销后可以查看自身任务
状态及继续取消，新任务、提案领取和产物获取会拒绝。已验收产物按原有规则保留。
本地进程终止由原有监督器确认，远程模型终止情况仍为未知。

`native_task_permissions.py` 覆盖伪造凭据、权限收窄、跨任务访问、输入变化、原生文件
越权拒绝、取消、持久撤销和服务退出后的执行过期。`document_activity.py` 用两个真实
Codex agent 验证 PDF→Markdown→XLSX、权限拒绝、取消及产物保留，指定 `--shuttle-client`
可通过编译后的 Shuttle daemon 客户端完成任务调用。

[English](../../daemon.md) 为权威契约。守护服务复用现有原生 spool、
ExecutionBackend 所在执行路径、独立监督进程与产物验收，不增加平行 host。
业务步骤、领域知识和验收规则仍属于 spool。

## 启动与观测

使用所选 host 的真实绝对路径，服务目录父级须已存在。路径须规范化；macOS
的 `/var` 和 `/tmp` 是符号链接。服务目录属于当前用户，权限为 0700。

```sh
an-agent-host daemon init /absolute/private/service 2
an-agent-host daemon serve /absolute/private/service
an-agent-host daemon show /absolute/private/service
an-agent-host daemon request /absolute/private/service /absolute/request.json
an-agent-host daemon launchd /absolute/private/service /absolute/agent.plist
```

`serve` 前台运行，供用户级 launchd 管理；生成 plist 不代表已安装。
同一服务目录以操作系统文件锁保证单实例。SIGTERM 停止服务接单，不转移原任务
所有权；原有限监督者继续执行、处理取消并记录结算。禁止以 root 运行。

`endpoint.json` 保存本机工作台地址。片段中的私有令牌授权只读快照，数据接口
同时校验 Host。网页不提供控制接口、任意文件读取或跨域授权。Unix 通道只接收
同用户、有长度上限的请求。这不是对恶意同用户进程的安全隔离。

## 调度、生命周期与权限

请求包含 `op` 及所需字段。返回 `approval_sha256` 的操作先预览，再用相同
请求附加 `approve` 摘要。实例注册不授予执行或模型披露权限。

- `register`：注册现有实例；`submit`：绑定实例、run、输入和期限并排队。
- `cancel`：向确切任务请求取消；请求成功不代表终止已确认。
- `pause`、`resume`、`suspend`、`unload`：调用原生命周期接口。
- `accept`、`reclaim`：仍由原产物托管流程核验，不从队列状态推导许可。
- `show`：读取全量投影；`task` 按 ID 只查原任务，避免等待时扫描其他实例。
- `harness`：提交明确披露的有限客户端任务。
- `harness-result`：读取已完成提案，不能把它当成已验收的业务产物。

并发为 1–8，同一实例一次启动一个排队任务，最多注册 128 个实例、保留 256 个
调度条目。可执行任务按顺序选择；原直接入口不会隐式纳入全局调度。

先持久化启动占位再启动。服务崩溃后只核对原结算证据，不重跑已占位任务。
结果未知的任务继续占用预算，并显示“执行中或待核实”。缺失回执不能转换成
终止证明或回收许可。休眠选择确切已提交检查点，不恢复进程内存。
尚未实现调度档案收集、自动重试、自动迁移和通用业务图。

## 外部 harness

首版接入已限定的 Codex CLI 0.166.0，任务显式选择并绑定可执行文件摘要、
prompt、schema、scope、可选 model 和有限 MCP。原生客户端保管认证与上下文。
禁用 shell、编辑、插件、子 agent 等工具；唯一可选 MCP 工具是事先获准的
回环材料读取。该接口接收数据提案，不是任意代码代理或任意远程 harness 协议。

本地客户端继续由现有进程组监督，默认期限 180 秒。确认本地终止不表示远端模型
已停止或计费为零。提案须通过 spool 的业务验证和既有 kit、产物许可。
原生事件与请求保存在私有目录，工作台不披露 prompt、MCP 凭据或私有会话。

## 资源与验收

界面区分生命周期、执行结算与产物验收。服务及任务的活进程后代按出生身份采样，
RSS 求和可能含共享页；已退出进程、重新托管的进程和共享 CAD/Krita 应用不计入。
未知指标不写成零。采样不授予进程控制权，也不等同于历史使用量或模型账单。

`tests/cli/native_daemon.py` 用真实 macOS 流程检查并发、服务崩溃、取消、原产物
托管、生命周期、资源采样和只读界面授权。有限 harness 测试替身不证明模型服务
可用；Codex/MCP 与 CAD→Krita 在 Shuttle 独立验收。其他系统拒绝启动该服务。

## 异步委派

`delegate` 接收 `parent` 和原有有限 harness 的 `task`，精确披露审批后立即返回任务 ID。
`parent` 仅用于关联，不授予权限，也不是隔离边界，仍适用同用户访问规则。
`update-delegation` 接收 `id`、`strategy: "enqueue"` 和 `instruction`，保持原有 harness、
范围、schema 和 MCP 绑定，追加指令后重新审批。这是新的有限任务，不是恢复原会话，
不会自动注入前序结果。仅在前序成功且确认终止后启动；失败、取消或结果不明时保持排队，
可明确取消后续任务。不支持中断重放或自动递归派生。

查询和取消复用 `task`、`cancel`。`delegation-inbox` 按 `parent` 返回持久任务状态与完成的
提案；重复读取、服务重启都不消费消息或重放执行。当前采用主动查询回传，尚不自动唤醒
协调者或确认消息消费。提案仍为不可信数据，必须经过业务验收。所有任务共享现有队列
上限，没有新增调度器或模型循环。

## Agent 活动与采纳

`activity` 按 `id` 查询原有提交、认领、归属、取消、结算和提案记录，附原始来源摘要。
记录分类不是推断出的时间线。spool 的产物状态仍来自原有状态和验收接口，工作台详情
使用同一个鉴权后的只读查询。

外部 Codex JSONL 只投影固定事件和动作分类，保留原始行号和来源摘要，最多返回 128 条，
显式计数省略行和损坏行。提示词、参数、正文、URL 和工具输出不进入活动视图。
harness 报告不等于宿主确认的文件修改，也不赋予权限；缺失事件表示不可见。
原有私有事件文件仍是来源，不另建活动账本。

`record-adoption` 接收 `id`、已注册 `instance` 和 `run`，预览提案摘要与原有正式验收
产物的操作员关联。精确审批后，每个任务写入一份保留的关联记录；它不验收产物、不改变
保管权，也不证明语义等价。未验收或取消的产物不能关联。重启后关联仍可查询。

可选的 `document_activity.py` 验收使用一张合成 PDF 表格、两次真实 Codex 委派和操作员
选定的成熟表格工具。PDF 提取和表格生成留在应用适配层；原有 native spool 发布并保管
PDF、Markdown 和 XLSX 的精确字节。明确审批的采纳关联连接提案与已验收产物，另行检查
导出 OOXML 的数值与公式。这仅验证一个文本 PDF 样例，不代表 OCR 或原生 Excel 验收。

文档验收显式接收 `--host`、新 `--evidence`、合成 `--pdf`、已验证 `--codex`、`--node`
和应用自有 `--builder` 路径。控制端需要 `pypdf`。构建器接收行数据 JSON 和输出目录，
写出 `result.xlsx`：A6:C8 为源数据，D6:D8 为金额公式，D10 为合计；不会安装工具或开通
服务。样例含 Bolt（12×0.50）、Plate（3×8.00）、Bearing（2×4.25），合计 EUR 38.50。

## 原生教学应用

三个机械教学包现在可作为普通 Python spool 加载，继续走原有守护队列、任务授权和验收。操作者运行时可显式选择最多六个额外只读目录、八个辅助快照文件，以及 `graphics = macos-agx-metal-v1`。辅助文件通过 `AN_SPOOL_AUX_0` 等变量访问，内容变化会使旧批准失效。要求 Metal 的包缺少明确图形选择时拒绝加载；仅该选择可使用现有 900 秒上限，其它任务仍为 300 秒。

下游 `submit` 可提供 `artifact: {catalog, revision}`；`input` 必须匹配该已验收版本的原始内容位置。原监督进程保留并复核版本，输入读者和引用仍走现有流程，任务权限继续绑定实例、运行、输入和期限。

an-spool 的 `examples/mechanical-animation/product` 提供本地教学操作页。每次预览确认后由应用申请收窄的任务授权，凭据仅存在于该调用内存。阶段进度来自原始队列与验收证据，不新增执行队列，也不自动重放。原有观测页保持只读；操作页另用本机访问令牌并要求同源 POST。最终以 Shuttle 的原有 Krita 接口打开已验收 ORA；它是动画首帧参考层。

`native_daemon_inputs.py` 覆盖辅助输入变化拒绝、图形授权缺失拒绝，以及原队列中两个实例的固定二进制产物版本交接。真实 Blender 和 Krita 仍需单独从产品入口验收。

工作台默认显示英文，可用语言按钮切换到简体中文。两种语言共用原状态、权限与任务记录，语言切换不会提交运行。

教学入口仅为测试／使用示例，不进入主要运行时代码。新调用者的 `submit` 只传 `artifact: {catalog, revision}`，省略 `input`；`run-artifact` 权限固定目录引用、版本、接收实例、运行名及期限，允许收窄期限。宿主负责解析内部文件位置。旧路径型调用仍保留，但两种输入同时出现时必须一致。

## 内嵌 app-server

现有 harness 请求可显式设置 `task.app_server=true`，该选择进入权限摘要。
宿主管理固定版本 Codex 子进程，每次任务建立新的临时会话，只注入选定指令和
获准工具的结果。harness 负责模型循环与上下文，宿主负责授权、任务监督、终止证据
和产物保管。原生 shell、编辑、插件和自动子代理关闭。

模型通过 `host_request` 回到原 daemon，继续使用原有预览、批准、队列和能力检查。
只有允许委派的父任务能选取已有后代授权的 ID，不能生成凭据或管理授权。
子任务建立独立会话，不继承父任务历史。每次工具操作重新检查有效授权。
撤权和取消由原独立监督者处理；本地终止不证明远端推理已停止。
结果未知时保留原任务，不自动重试。已提交给另一个 spool 的工作由其监督者负责，
需要显式取消。客户端使用独立目录并仅引用原生文件登录，不复制用户配置或私人会话。
本期尚未验收仅钥匙串登录、会话恢复和任意原生工具。

## HarnessAdapter 与重连

`HarnessAdapter` 接入现有调度、独立任务宿主、权限和产物验收，负责启动外部
harness、声明能力以及转换观测事件。交互任务选择
`task.adapter="codex-app-server"`，有限执行使用 `codex-exec`。
旧的省略字段与 `app_server=true` 请求保留原有身份和权限摘要，冲突选择会被拒绝。
两种 Codex 模式均固定为 CLI 0.166.0，并核验授权时的程序哈希。

通过原任务权限入口调用 `harness-connect`，传入 `id` 和可选的事件 `cursor`，
取得执行句柄、能力、状态、增量事件及补充指令回执。控制端断开或守护服务重启后，
继续观察原有任务进程。`resume_execution=false` 表明执行进程丢失后需要保留未知状态。
撤销后的凭据仍可观察和取消其所属任务。

`harness-steer` 接收 `id`、`execution`、唯一 `command` 和 `instruction`。
预览与精确批准将指令摘要绑定到原执行句柄，调用者必须拥有任务及其 harness 权限。
原任务进程在交付前再次核验授权，按接纳顺序处理最多 32 条、各不超过 64 KiB 的指令。
相同命令 ID 与相同内容复用原回执，内容不同则拒绝。发送标记先于传输落盘，
标记之后、确认之前崩溃的结果保留为未知。`queued` 表示本地接纳，`acknowledged`
表示 Codex 已接纳到原 turn，最终结果仍需验收。

Codex 使用携带原 `expectedTurnId` 的 `turn/steer`。取消沿用授权撤销与进程组监督，
结算要求本地释放证据，远程服务是否停止仍标记为未确认。最终结构化输出进入既有
spool 产物发布和验收流程。未来改造后的 DeepSeek Harness 需实现同一契约、明确声明
能力并通过相同验收，保留宿主任务身份、授权链和原有生命周期。

`native_harness_adapter.py` 通过协议进程验收双后台任务、补充指令、旧句柄拒绝、
同级越权拒绝、命令去重、独立取消与守护服务重启。
`document_activity.py --app-server` 使用真实 Codex 和指定的应用侧工作簿工具，
复验 PDF→Markdown→Excel 交付、公式及产物归属。本地证据区保留真实模型验收记录。

独立工作区界面使用 `python3 packages/workbench/build.py` 构建。
TypeScript 与 HTML 源码使用英文，简体中文界面翻译保存在
`packages/workbench/zh-CN.json` 中并在构建时嵌入。
用户填写的工作区名称、目标及记录保留原始语言。

### 有界续轮与采集

交互任务可声明 1 至 8 的 `turn_limit` 和 1 至 3600 的 `wall_seconds`，默认单轮、180 秒。
显式设置进入 harness 权限摘要。所有轮次共用原 owner、执行句柄、thread、schema、授权与监督器。
有限 exec 支持单轮。补充指令绑定当前活动 turn，迟到指令拒绝，空闲会话需显式续轮。

`harness-turn-result` 接收 `id` 与从 1 开始的 `turn`，返回不可变的已完成轮次提案，
其中 `artifact_accepted=false`、`termination_confirmed=false`。
`harness-continue` 接收 `id`、`execution`、`after_turn`、唯一 `command` 和 `instruction`。
精确批准绑定指令及已完成轮次，每轮在原上限内接纳一次续轮。重复命令核对原回执，
发送标记先于传输落盘，结果未知时保留原身份。等待续轮的任务仍受取消、授权撤销和总期限约束。
最终 `harness-result` 继续要求原任务完成结算。

`activity.harness.metrics` 从已有记录汇总提供方累计计数和授权披露。缓存输入属于输入，
推理输出属于输出，同会话累计更新去重，倒退或非法计数降低覆盖率，缺失值为 `null`。
资源回执记录摘要、字节数和宿主工具耗时，模型实际消费情况未知。
`read-input` 与 `artifact-read` 可选择 `encoding="utf-8"`，复用原授权检查，默认保留字节响应。

### 宿主任务会话边界

内部 task-session 层负责续轮准入、命令对账、观察和授权工具分派。
适配器能力使用类型化声明，补充指令与续轮分别检查。Codex 原生事件与用量解析
留在 Codex 适配器中。现有 `harness-*` 操作、持久身份和权限摘要保持兼容。
此次提取不增加调度器，提供方或 guest 报告不能授权产物验收。

## 任务会话的 Cocoon 传输

任务会话中间层通过现有 `host_request` 工具接收 `cocoon-call`、
`cocoon-status` 和 `cocoon-reconcile`。当前提供有界传输基础，尚未构成已验收的
spool 执行后端或产物导入器；不会启动 VM，也不会将导出文件接纳为交付产物。

可信操作员提供权限为 0600 的私有 JSON 配置，字段为 `socket`、
`grant`、`caller`、`vm` 和 `generation`，只保留工作授权，不含 Cocoon 管理凭据。模型只得到配置路径和 SHA-256，不能读取
其内容。任务授权需包含 `kind=cocoon`、`profile_sha256` 和 `operation_sha256`；
后者精确绑定序列化后的 Cocoon 操作。授权继续经过宿主原有预览与审批流程，
不得同时授予模型读取凭据配置的权限。Cocoon 每次仍独立检查自身工作授权。

`cocoon-call` 请求包含 `profile`、`profile_sha256`、UUID 格式的 `operation_id`
以及 `operation`。仅允许 show、context、stop、guest 和 export；
guest/export 的 VM、调用者和 generation 必须与配置一致。模型不能签发授权或
创建 VM。宿主生成独立的 Cocoon 请求 UUID，并在传输前持久化绑定。同一操作 ID
再次提交只观察原记录，内容变化则拒绝。传输丢失后保持未知，不重放操作。
记录属于原任务会话，同一授权下的其他会话不可借此读取；意图记录不含凭据和客体载荷。

`cocoon-reconcile` 使用相同配置、操作身份和精确操作授权，仅查询 Cocoon 原请求 ID，
保留已确认的成功结果。工具中的 `cocoon-status` 只需操作 ID；控制端也可通过任务授权
端点提供 `id` 和 `operation_id`，在任务授权撤销后观察本任务记录。这不恢复执行权限。
`harness-connect` 也返回本会话保留的 `cocoon_operations` 回执。
所有响应保持 `artifact_accepted=false`；`observed` 只表示获得对应后端回执，
VM 终止和产物验收仍需独立证据。

`cocoon-transfer` 通过相同配置和精确操作授权暂存有界二进制输入输出。
`operation` 计划绑定 `direction`（upload 或 download）、位于 `/workspace/` 下的
客体 `path`、当前 VM `attempt`、`sha256` 和 `bytes`（最多 4 MiB）。上传另绑定
`source`，并要求匹配的 ReadInput 权限；下载不能选择宿主目标路径。宿主填写任务身份，
核对每个 8192 字节分块的回执身份、游标以及最终大小和摘要。每个分块都有持久化提交栅栏，
重入只观察或查询原操作，不重发未知写入；分块之间及暂存候选前均检查取消和撤销。
下载只生成会话私有的 `candidate.bin`，`staged` 不代表已验收、可执行或已交付，
也不证明 VM 终止或领域产物正确。

spool 的 `submit` 可用 `candidate={session,transfer,sha256}` 选择已暂存下载，
代替 `input` 或 `artifact`。调用者须拥有来源会话，持有绑定实例、run、transfer、摘要和
最长秒数的 `run-candidate` 权限，来源授权也须有效。宿主在预览和预留前核实完整暂存回执
及文件字节，不接受调用者指定宿主路径。原 spool 监督程序负责输入快照、执行和终止；
其授权跟随来源会话的授权链，使预留后的取消和撤销继续有效。任务观察保留
`candidate_input` 来源信息。正式保管仍经过原 spool `accept`，`artifact-read` 核实保管
后才把字节返回继续执行的主会话。此桥接验证候选输入和保管流程，不表示领域程序已迁入
Cocoon，也不证明 PDF/Excel 内容正确。
