# VX11 并行施工通用简报

你在为 Varix 内核（Rust no_std + alloc）施工功能模块。仓库根：
`D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel`

## 必读顺序

1. `_tools/VX11_CONTRACT.md` —— 施工契约，逐条遵守。
2. `_tools/_vx11_gNN.md` —— 你这一组的卡（卡号/文件名/诊断段/最小行数/标题/验收原文）。
3. `varix/src/unxstar/unx50001_gamutmap.rs` —— 已验收参考实现（1,400 行）。通读后学它的
   密度、结构、判据写法、注释风格；**不要照抄主题内容**。

## 产出

只新建 `varix/src/unxstar/` 下属于你的文件，文件名严格按卡片 `file` 字段。

每个文件必须：
- `wc -l` ≥ 卡面 `lines`（建议 +15%）；
- 两类以上真实数据结构（状态机 / 账本 / 注册表 / 解析器 / 调度器 / 协商机）；
- 显式错误枚举，`code()` 返回 `SEG_BASE + n`，全部码值都被真实构造使用；
- ≥3 类坏样本注入核，逐类断言具体诊断码；
- 8~20 条判据，优先性质（守恒/单调/幂等/确定性/往返/非法必拒）；
- 文件末尾 `#[cfg(test)]` 测试，且必须真能跑过；
- 内核路径零 panic：无 `unwrap/expect/panic!/todo!/unimplemented!`、无 `std::`、无裸越界；
- 浮点超越函数只走 `libm`；导入白名单见契约 §2；
- B21（F50002–F50020）可 `use super::unx50001_gamutmap::{...}`，B22 起完全自包含；
- 中文注释解释「为什么」，公开函数一行 `///`。

禁止：空壳、恒真断言、`todo!()`、只返回常量的假实现、改 `mod.rs`、写任务板、写生成脚本。

## 自验（必须做完）

在 `kernel/_tools` 下：

```bash
MSYS2_ARG_CONV_EXCL='*' python _vx11_check_remote.py <tag> <num> [<num> ...]
```

- 每写完 1~2 个文件跑一次；结尾必须 `TOTAL=n RED=0`，随后 `cargo test --lib` 为 `ok`。
- 编译/断言错误按行号修，不许注释掉判据或删测试。
- B21 组的探测列表必须同时带 `50001`。
- 不要在本机跑 cargo（本地工具链不可用）。

## 交付回复

每文件：路径 + 行数 + 判据条数 + 一句话主题；最后贴探针尾部两行（TOTAL 行 + test result 行）。
