# EMBED-MANIFEST — 第三方嵌入声明协议（M-59）

> AI-15 开放工具组。应用作者在 exe 同目录放置 `variable-embed.json`，
> 声明本应用的窗口嵌入特性；引擎读取后在「窗口接管」时参考。
> 校验实现：`src-tauri/src/shell/opentools.rs`（`embed_manifest_validate` / `embed_manifest_scan`）。

## 1. Schema（version 1）

| 字段 | 类型 | 必填 | 约束 | 说明 |
|---|---|---|---|---|
| `version` | number | ✓ | 必须为 `1` | 声明格式版本 |
| `titleMatch` | string | — | ≤256 字符；空 = 不匹配；必须可编译为正则 | 窗口标题匹配正则 |
| `minWidth` | number | — | 0..16384；0 = 不限 | 嵌入容器最小宽度（px） |
| `minHeight` | number | — | 0..16384；0 = 不限 | 嵌入容器最小高度（px） |
| `multiInstance` | string | — | `single` \| `multi`；空 = 默认 single | 多实例策略 |
| `waitMs` | number | — | 0..60000；0 = 引擎默认 | 窗口出现后等待稳定的毫秒数 |

全部字段可省略（serde `default`）；非法声明整体忽略并记日志，**绝不让坏文件影响启动**。

## 2. 优先级规则（用户登记 > 应用声明 > 引擎默认）

1. 用户在「窗口登记」中手动登记的几何/参数永远最高；
2. 应用声明仅在用户未登记时生效（兜底）；
3. 两者皆无 → 引擎默认行为。
4. 用户删除登记后自动回落到应用声明（不需要重启）。

## 3. 样例

```json
{
  "version": 1,
  "titleMatch": "^Vim - ",
  "minWidth": 480,
  "minHeight": 320,
  "multiInstance": "single",
  "waitMs": 1200
}
```

## 4. 与嵌入验证工具的关系

AI-2 的 `Compat-Matrix.ps1 -Action Run-Embed/Report-Embed`（C-8）用本声明做嵌入验证；
`Variable --plugin-dev <目录>` 顺带校验同目录的 `variable-embed.json` 并打印指引。

## 5. 红线

- 声明是**建议**不是命令：引擎永远有权拒绝嵌入（反作弊进程/最小尺寸不足等）；
- 不允许任何形式的代码执行字段（纯数据声明）；
- 正则只限长度防 ReDoS，不承诺跨语言语义一致（文档明示）。
