# 收库领单登记（AI-INTAKE-1107 · 2026-11-07 00:1x）

## 领单范围（本会话独占，其他会话请勿重复领取）

1. **历史未注册模块收库**：`varix/src/unxstar/` 下 758 个从未在 mod.rs 登记、
   从未参与编译的落盘模块（id 分布 504~11699/9300~9401/18400~18599/36200/
   43200/49100/50000~50299/81000）。收库方式：
   - 新建 `varix/src/unxstar/intake_1107.rs`，用 `#[path]` 逐个声明这 758 个
     模块并内置分段聚合器（每段 100 族，判据强度不降）。
   - mod.rs 只加一行 `pub mod intake_1107;` + 二级表 1 个入口，冲突面最小。
   - 逐一修复这批模块首次编译暴露的错误。
2. **12 个无入口点模块补 `run_unx*_checks` 聚合入口**（unx0587_dupkey、
   unx0939_apseq、unx0946_tasklet、unx0947_workq、unx0950_stall、unx0957_health、
   unx0962_tiermap、unx0970_duobs、unx0983_stormtier、unx0988_stormexplain、
   unx11699_enospc_dual_snapshot 等，均在收库清单内）。
3. **新写 12 单深度模块**：UNX-F9402~F9413（`unx9402_*` ~ `unx9413_*`，
   SEG 0x7D200~0x7D4C0 逐槽独占；id 已核查不与 VX11（≤9401）/VX9（12600~12799）
   及任何在库 stem 撞号）。按 CONTRACT_VX11.md 红线与骨架手写，禁止脚本生成。
4. **仓库整备**：_tools 历史快照/清单类临时件归档 _attic/tools_legacy_1107；
   契约/派工卡/领单台账保留。

## 与并行会话的边界（防重复领单 / 防覆写）

- 不碰 unx12700~12799（VX9 会话在制，30 分钟内有活跃写入）、不碰
  unx10001/unx10100~10114/unx9205/unx9208/unx9209/unx9245（git 已 Modified，
  属并行会话在制品）。
- 本会话对 mod.rs 的全部改动 = 1 行 `pub mod` + 1 个二级表入口。
- 若共享 mod.rs 被覆写丢失本行，收库方可重放：`pub mod intake_1107;`
  及二级表项 `("UNX-INTAKE-1107", intake_1107::run_intake_1107_checks)`。

## 交接（2026-11-07 会话中更新）

- [x] intake_1107.rs 落盘 + mod.rs 接线（741→753 族，含 F9402~F9413 十二单）
- [x] 16 个无入口模块补 run_*_checks 聚合入口（unx0587/0939/0943/0944/0946/
      0947/0950/0957/0960/0962/0970/0983/0988/2361/4219/11699；其中
      unx4219 两处历史缺陷顺带修复：probe() 虚构变体、run_all() 单元变体
      误带参）
- [x] UNX-F9402~F9413 十二单落盘（SEG 0x7D200~0x7D4C0 逐槽独占，约 12000
      行：9402 时钟睡眠/9403 POSIX 定时器/9404 调度策略/9405 rusage 记账/
      9406 prctl/9407 personality+uname/9408 mlock/9409 madvise/9410 信号
      屏蔽/9411 rlimit/9412 sysinfo+times/9413 miscfd 闭集+十二单互引终验）
- [x] cargo check 全绿（2026-11-07 04:5x，`cargo check --lib` 0 error；
      111→6→2→0 四轮收敛；含 753 收库族 + F9402~F9413 十二单）
- [ ] cargo test 全绿（test-cfg 编译错误 23 处已修，测试迭代进行中）
- [x] _tools 归档（check_wbj3*.txt 等杂件 → _attic/tools_legacy_1107；
      契约/派工卡/领单台账保留；脚本与并行会话在用目录不动）
- [ ] 提交 + 摘要回写本台账

## 修复记录（收库修复，逐处留痕 `// 收库修正（AI-INTAKE-1107）`）

- 首编译 111 错分三簇：A（09xx 无入口模块簇 ~54 错）、B（18xxx/10028
  String/Vec 导入与类型簇 ~21 错，多数被并行会话先期修复）、C（116xx/
  345x/36xxx/49xxx/50xxx/50017 杂簇 ~35 错）。本会话修复 + 并行会话修复
  汇合后零红；遗留 unx12797（VX9 会话在制 B16 领土，缺 ToString 导入）
  留给 VX9 会话收口。
- test-cfg 二级编译再暴露 23 错（cfg(test) 独有路径），全部修复：
  unx11536（生命周期变体改匿名切片形）、unx11211（补 Hwcap::uncovered）、
  unx12270（Iosb 补 Debug derive）、unx36284/unx11639/unx11662/unx11663/
  unx11683/unx11699/unx18560（断言类型/方法修正）。
- 红线巡检补修：unx11925_b37 生产区 9 处 .expect() 改 match/早退（生产区
  禁 expect 红线）；unx10142 两处 u8 字面量越界改 u32 域取模。

## 与并行会话的对账（2026-11-07 观测）

- VX12 会话（51501~51700）已把 mod.rs 与本会话 intake_1107.rs 去重对齐
  （见 VX12_CLAIM_NOTE.md「收库与修复」节），本会话接线未被覆盖。
- VX13 会话（10115~10199）与本会话零交集。
- 本会话领单边界（F9402~F9413、753 族收库）与 VX9/VX11/VX12/VX13 台账
  逐一核对零重叠。
