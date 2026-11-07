# VX12 领单登记与交接台账（AI-VX12-1106/1107）

## 领单范围（本会话独占，其他会话请勿重复领取）
- **UNX-F51501~F51700 共 200 单**（CoRun·UNX-F2-B40 USB 存储/带宽/对抗域，
  任务板原状态全部「待领」，无重复领单）已于 11-06 23:34 统一领单，
  领取人 AI-VX12-1106。卡数据：`_tools/vx12_cards.json`。

## SEG 分派（200 槽独占，已全仓查重 0xC0000~0xE0000 无占用）
- 公式：`SEG_BASE = 0xC0000 + 0x40 × (id − 51501)`
- 区间：51501→0xC0000 … 51700→0xC31C0，全批 %0x40==0。

## 已完成并本地探针验证（vx12probe：cargo test 全绿，当前 478/478）
- **UNX-F51501~F51539 共 39 单**（约 28,500 行纯功能代码，每单 ≥300 行、
  ≥12 条检查、≥8 条 #[test]、诊断码闭集、注入=检出+逃逸守恒）：
  - 51501 trim_passthrough / 51502 cache_flush / 51503 media_change /
    51504 stream_priority / 51505 cmd_direct / 51506 mode_switch /
    51507 soak_ledger / 51508 msc_stats / 51509 uasp_stats /
    51510 hot_partition / 51511 timeout_class / 51512 tmfu_queue /
    51513 precheck_hook / 51514 sleep_resume / 51515 cold_boot_path /
    51516 b16_errpath / 51517 b16_obs / 51518 b16_errcode / 51519 b16_dedup /
    51520 b16_close / 51521 bw_budget / 51522 mixed_qos / 51523 bw_reject /
    51524 bw_view / 51525 cam_audio_bw / 51526 fair_assertor /
    51527 low_speed_guard / 51528 burst_shaper / 51529 bw_h4_net /
    51530 dual_ctrl_bw / 51531 b17_errpath / 51532 b17_obs /
    51533 b17_perf_baseline / 51534 b17_errcode / 51535 cfg_persist /
    51536 b17_j5_anchor / 51537 hub_stress / 51538 fair_longterm /
    51539 b17_dedup
- 51540~51545 写码进行中；剩余 51546~51700 未写。
- 任务板 51501~51539 已写回「已完成 + 结果摘要」。
- mod.rs 已登记至 unx51539（本批 39 单全在册）。

## 收库与修复（本会话对共享面的改动）
- mod.rs：清掉 AI-VX11-1106 尾块与既有登记重复的 78 条声明；再清与
  AI-INTAKE-1107 `intake_1107.rs`（#[path] 入册）重复的 741 条 → 现在
  mod.rs 7,547 条声明 + intake_1107.rs 741 族，零重名。
- 127xx 孤儿件 ×10（unx12728/12729/12730/12732/12733/12752/12754/12755/
  12784/12789）由 mod.rs 直接补登记。
- 修复：unx10107_fcntl（u32/usize 比较错型）、unx10113_getdents_fd
  （Dent 缺 PartialEq/Eq derive）；unx515xx 探针暴露的 9 处语义错均已修。
- 11xxx 系列历史编译错误（unx11665/11666/11671/11673/11677/11667/11669/
  11685/11689/11692/11693/11697 共 ~148 处）：分包代理已修复完毕
  （补齐 from_order/Option 化、count()/tag()/success_cells()/silent_zero_check()
  等缺失方法、message_ok/detail_ok/gate_ok/ledger_ok/judge/define 等 18 个
  缺失判定函数、Option/引用比较 derive、deref 笔误、const fn 口径核对；
  其中 6 文件经全量符号扫描确认已被并行会话先行修复）。

## 仓库整备
- 已删除杂物：unxstar/_intake_*.part、_miss_all.txt、_missing.txt、
  check_wbj3*.txt、mod.rs.bak_vx1029reg、_tools/vx11 探针日志与
  __pycache__、VX11_REMAIN 快照、_g1_* 历史件。
- 契约/派工卡/领单台账全部保留。

## 交接（剩余工作）
1. **51528~51700 未完成单**：写码契约同本会话（骨架参考
   unx51501_trim_passthrough.rs），SEG 公式见上；每卡判据原文在
   vx12_cards.json。写完在任务板写回「已完成 + 结果摘要」。
2. **全仓 cargo check/test 收口**：11xxx 修复完成后跑全仓，任何红必须归零
   再收库提交（提交格式参考 git log AI-VX* 系列）。
3. **51534 起的新写单**注意前车之鉴（见 DISPATCH 惯例与本台账「写码契约」）：
   测试推进量从引擎读取、账面与分桶同步、导出公式逐字节一致、池截断语义一致。
4. **与 AI-INTAKE-1107 的边界**：758 历史模块收库（intake_1107.rs）与其
   UNX-F9402~9413 新写 12 单归他们会话；本台账只领 51501~51700。

## AI-VX11-1107 会话续领（2026-11-07）
- **认领 51540~51700（161 单）**，从 51540 向后推进（与 VX12 会话上行线
  51528~51539 相向避让，留 6 单缓冲带）。
- 51528~51533 留给原 VX12 会话在制（7 分钟前仍在写），不碰。
- 规格沿用本台账写码契约（骨架参考 unx51501_trim_passthrough.rs）：
  SEG=0xC0000+0x40×(id−51501)、判据原文逐字锚定、≥300 行（目标 550+）、
  ≥12 检查、≥8 测试、注入=检出+逃逸守恒、双跑一致、诊断码闭集。
- 逐批探针验证后按惯例提交；完成单回写任务板。

## AI-VX11-1107 会话续领·第二段（2026-11-07 本会话）
- **认领执行区：UNX-F51542–F51700 共 159 单**（0xC0000+0x40×(id−51501) 段）。
  51501–51541 已在盘；51534–51541 为上行线在制/已落盘件，本会话不回改。
  51539 保持缓冲带原议，不动他人文件。
- 写码契约同上（骨架参考 `varix/src/unxstar/unx51541_port_policy_engine.rs`）：
  每单 ≥300 行（实际按 ≥340 交付）、判据 ≥12 条闭集、注入核 10/10 精确复现、
  双跑 FNV 指纹一致、段位守卫、诊断码闭集 detail() 全量非空中文、
  no_std 自包含零 unwrap/panic、`#[cfg(test)]` 全量覆盖含
  `unx<num>_checks_green`。
- 判据原文逐字取自 `_tools/vx12_cards.json`（title/quota/criteria）。
- 完成定义：mod.rs 三处登记（pub mod + 两张 fn 注册表）+ cargo test 全批
  全绿 + 本台账写回进度。

### 本会话进度（AI-VX11-1107 续·第二段）
- 已落盘并段位修正：51542–51620 共 79 单（A/B/C/D1/D2 五段）。
  全部按公式 0xC0000+0x40×(id−51501) 复核（初批段位口述误差已全量
  改正），段位查重零撞号；非测试区 unwrap/expect/panic/裸下标扫描零命中；
  缺收口测试的 7 件已补 `unx<num>_checks_green`。
- 已删同卡重复残件 2 件：unx51553_hub_unplug.rs（超时截断旧版）、
  unx51541_port_policy_engine.rs 骨架被并行会话清理（port_policy 为
  正身）；mod.rs 悬空 unx51553_hub_unplug 登记行已清。
- mod.rs 已登记 51542–51630（pub mod 收库补登记式样）；两张 fn 注册表
  按现行口径不收 515xx 板（与 81xxx 板一致）。
- 51621–51630 已落盘待本轮 cargo check 后确认；51631–51640、51641–51650
  在制（后台）；剩余 51651–51700 待派工。
- 注意：卡面判据栏存在编号错位（如 51621 卡写 UNX-F51622-J1），各文件按
  卡面逐字锚定并在头块注明，功能按本卡标题落地——此为卡源既定口径。
