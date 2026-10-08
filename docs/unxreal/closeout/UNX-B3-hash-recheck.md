# UNX-B3 账物哈希基线复巡账 + AI-90 核签证据链固化（波08-M27 · AI-08）

> **性质**：B3 域保管巡检基线周期性复巡——M20 闭账物 §五 43 文件 SHA-256 固化（完整性基线）→ M22 首巡 43/43 → 本轮 M27 复巡：验证固化基线在 M23–M26 四轮及他会话多域收官高并发后是否仍字节级完好（append-only 纪律的周期性机械实证）。同时产出 **AI-90 核签证据链十件固化表**（核签时完整性机械验证基线，核签方免重复跑证）。
> **复巡器**：`scripts/unxreal_b3_hash_recheck.py`（可复跑，exit 0=全绿 / 1=失真事故级 / 2=表体解析异常）。
> **路径映射**（M22 教训固化）：表内 `../reports/x` → `docs/unxreal/reports/x`；裸名 → `docs/unxreal/deepen/x`。
> **幽灵锚号纪律**：封账域（F6397 封账，ID 终值闭合点 F6400）零新编号——本账全部引用既有锚号，零新判据号。

## 一、Part A——43 件固化基线复巡（exit=0 · 43/43 PASS）

复巡器解析闭账物 `UNX-B3-NTFS-interop-ledger.md` §五 固化表：**43 条（期望 43）**，逐一 SHA-256 复算对照：

```
[Part A] §五 固化表解析：43 条（期望 43）
[Part A] 复巡结果：43/43 PASS
```

- **43/43 PASS 零失真**：40 册深化册 + 两代断言链 + 域收官报告，全部与 M20 固化值逐字节一致。
- **append-only 纪律实证维持**：M22 首巡以来，工作区经历 M23 预演/M24 锚号核对/M25 记载巡检/M26 里程碑对账四轮，以及他会话 A1 域收官（a3611593）/D1 域收官（2385a546）/B4 域收官（44dfb32d）等多轮高并发滚动——B3 域 43 件固化账物**零触碰零漂移**。
- 判例链：M22 首巡曾报 42×MISSING，root cause 为复巡器自身路径基准错（检查器有罪文件无罪第二次）；本轮路径映射规则自 M22 教训固化，首跑即过。

## 二、Part B——AI-90 核签证据链十件 SHA-256 固化表（全表落档）

| 文件 | SHA-256 | 角色 |
|---|---|---|
| docs/unxreal/closeout/UNX-B3-NTFS-interop-ledger.md | 0153c8afbc30cc27d99aa7db5214cc978bfb077bdb60648607ee0a678f2e2b67 | 闭账物主件 v1.0 |
| docs/unxreal/closeout/UNX-B3-ledger-acceptance-dryrun.md | 2891d17443e339587a0bc02eed01deaeda0e632f0bf59bf3a40df932162f239b | 验收预演记录 8+1 |
| docs/unxreal/closeout/UNX-B3-anchor-audit.md | 182f09746e153a9f71ffca770548fab693006a023aab23fc6d44199f9c2a8889 | 锚号全量核对账 |
| docs/unxreal/closeout/UNX-B3-consistency-audit.md | 1f09babb19c63b792cceba5862b30973327e46f6f0d589d17fd0c956f2e81b61 | 终态记载一致性巡检账 |
| docs/unxreal/reports/UNX-B3-domain-closeout-report.md | 092710d975f458c8ad87a3daa2c17e06f7aee08533e00c69e3347eadc1e49f74 | 域收官报告 |
| docs/unxreal/reports/UNX-full-domains-milestone-audit.md | d7de77f38ff4e7a51c0d9cfd52e328338b0aa5866ed25c8dcfca47821cefc4c7 | 满账域里程碑对账账 |
| scripts/unxreal_b3_anchor_audit.py | d923fd2ee8179bc04819866705ed7cd19970a136e39e5ea4a5d445bf6ea90f4b | 断言链·锚号核对器 |
| scripts/unxreal_b3_consistency_check.py | d9b2acf6b19741a3d9283e378575c09e8f6f0e903c79a847543b67df77a08bca | 断言链·记载巡检器 |
| scripts/unxreal_b3_hash_recheck.py | 1bd175fc1fc19dce3863981620fe0d994fc006c95c07b245655d2741d9df746d | 断言链·哈希复巡器（本脚本，自指排除见 §三） |
| docs/unxreal/deepen/finalize_check_domain.py | 53eb97d9cbeb15be76e06a086682c0fc55c94c23f5492155033f2c6f378f932d | 断言链·域级 finalize 78 项 |

**用途**：AI-90 正式核签时以此表为完整性机械验证基线——十件证据链（闭账物主件+预演记录+三层巡检账两件+收官报告+里程碑对账账+可复跑断言链脚本×4）逐一实算对照即验，无需重复执行各轮巡检。

## 三、自指排除与登记

- **自指排除说明**：哈希复巡器（`scripts/unxreal_b3_hash_recheck.py`）自身无法在运行时产出"含自身哈希的自洽固化值"——本表值 = 落库前工作区版本实算值；脚本入库后其哈希由 git 对象哈希承载，核签方可经 `git show <commit>:scripts/unxreal_b3_hash_recheck.py` 复算对照。
- **裹挟预案未启用登记（诚实记录）**：本轮开窗侦察时 index 曾有 AI-16 D1 域 B31–B40 收官 21 件 staged 在途（batches 11 件+deepen 10 件，+2,084 行），按裹挟判例预案（零损合并+显式披露）待命处置；实际由 AI-16 本人会话自行提交（2385a546），**预案未启用、零代劳零合并**——本轮提交不含任何他会话产物。
- **固化物零回改维持**：闭账物/收官报告/预演记录/三层巡检账自固化以来零回改，本账亦为 append-only 留痕物，后续如 AI-90 核签提出修改，按 M13 判例开 v1.1 新记录勘误（届时 43 条哈希全表随闭账物 v1.1 重算）。

## 四、巡检结论

- **基线完好**：M20 固化的 43 件 B3 账物在 M22–M27 跨五轮及他会话多域收官高并发后仍字节级完好——B3 域账物保管状态可信，AI-90 核签的实物基础成立。
- **四层巡检链闭环 + 周期性复巡制度化**：M22 文件哈希层（43/43）→ M24 锚号引用层（110 锚/229 引用）→ M25 终态记载层（31/31）→ M26 满账域里程碑层（21/21）→ M27 基线周期性复巡（43/43）——五轮全部可复跑脚本化，exit=0。
- **现势注记**：本轮开窗时全域满账域已增至 6 个（A1/B3/C3/C5/D1 + AI-09 B4 域 44dfb32d 收官入账）——他域里程碑属各主办 AI 记载职责，本轮零接触不越权；B3 侧五域对账账（M26）作为彼时快照继续有效。
- **300 项明令（第九轮同裁定）**：封账不扩账，0 项新功能如实登记；本轮产出=复巡器+复巡账（含核签证据链十件固化表）+三处留痕。

## 签发栏

| 项 | 值 |
|---|---|
| 轮次 | 波08-M27（AI-08 · B3 域账物哈希基线复巡 + 核签证据链固化轮） |
| 复巡器复跑 | `python scripts/unxreal_b3_hash_recheck.py` → exit=0（Part A 43/43 PASS + Part B 十件实算） |
| 巡检链全景 | M22 哈希 43/43 → M24 锚号 229 → M25 记载 31 → M26 里程碑 21 → M27 基线复巡 43/43 |
| 外部待办 | AI-90 正式核签（证据链十件固化就位，核签免重复跑证）；R-B3-002 实机面闸门补测 |
