# causal-web

[English](../../../../packages/causal-web/README.md) · 中文译文，以英文为准。 · [中文目录](../../README.md)

一个内存流磁带查看器。将一个`.jsonl`磁带放入其中，页面会通过`refs`将陈述、动作和观察转化为因果链，然后在页面内验证磁带。无运行时依赖，完全客户端运行 —— 文件永远不会离开您的机器。

## 运行

```sh
bun install
bun run serve   # → http://localhost:3777 (loads examples/sample.jsonl)
```

打开 `http://localhost:3777/?tape=examples/other.jsonl` 以加载特定的磁带。

其他脚本：`bun test`（验证单元测试），`bun run check`（tsc），`bun run build`（输出 `dist/main.js`）。

## 页面内验证

- 每一行都解析为 JSON（错误行通过行号定位）
- schema：`v = 1`，必填字段，`kind`/`from_kind` 枚举
- `seq` 从 1 开始递增，无间隔
- ids 唯一
- ids 是 ULID，其嵌入的时间戳与 `ts` 字段匹配（与 det_seam 的 `Entropy::ulid(now_ms)` 同步）
- `ts` 单调非递减（同一毫秒内的运行是合法的 —— 在实际磁带中观察到）
- 无悬空的 `refs`
- 每个观察都引用一个动作
- 每个工具动作至少被一个观察回应
- 每个磁带对应一个会话

`examples/sample.jsonl` 由 `scripts/gen-sample.ts`（`bun scripts/gen-sample.ts`）生成；其 ULID 时间戳与 `ts` 匹配，并且通过所有检查。
