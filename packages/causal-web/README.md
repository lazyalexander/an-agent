# causal-web

memstream 思考线索演示页：拖入一份 memstream `.jsonl` tape，页面把
utterance / action / observation 按 refs 穿成因果线，并在页内对这份
tape 做完整性与一致性校验。零运行时依赖，纯客户端运行，文件不出本机。

## 运行

```sh
bun install
bun run serve   # → http://localhost:3777（自动加载 examples/sample.jsonl）
```

其他脚本：`bun test`（校验逻辑单测）、`bun run check`（tsc）、
`bun run build`（产出 dist/main.js）。

## 页内校验

- 每行可解析为 JSON（坏行定位到行号）
- schema：v=1、必填字段、kind/from_kind 枚举
- seq 从 1 无洞递增
- id 唯一
- id 为 ULID，且 **ULID 内嵌时间与 ts 字段一致**（对应 det_seam 的 `Entropy::ulid(now_ms)`）
- ts 单调不减（同毫秒合法——活跑 tape 中出现过同毫秒三连）
- refs 无悬空
- observation 必引用 action
- 每个 tool action 至少有一个 observation 应答
- 单 session

`examples/sample.jsonl` 由 `scripts/gen-sample.ts` 生成（`bun scripts/gen-sample.ts`），
fixture 的 ULID 时间与 ts 一致，全部校验通过。
