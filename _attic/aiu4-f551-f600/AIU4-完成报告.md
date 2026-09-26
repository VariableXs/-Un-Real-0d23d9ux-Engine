# AI-U4 完成报告 · Varix STAR I · I 通用域·四分队（F551-F600）

> 交付日期：2026-09-26 ｜ 分队：AI-U4 ｜ 泳道四（服务·I 域）｜ 施工落位：`kernel/varix/src/istar/`

## 一、任务与完成度

**任务**：《Varix STAR I start · AI分工完成图》AI-U4 分工包——F551-F600 共 50 项
（批次七 F551-F575 + 批次八 F576-F600），目标上限口径 50,401 行。

**完成度**：50/50 项全部落地，每项一模块、每模块自带 `run_*_checks() -> CheckSet`
自检（判据逐条载体化）+ `#[cfg(test)]` 单元测试。**零占位、零 TODO、零死代码。**

## 二、验证结果（证据三件套）

| 验证路 | 结果 | 复现 |
| --- | --- | --- |
| 隔离舱（`D:\2\aiu4-scratch`，#[path] 直挂真实文件） | **309 通过 / 0 失败 / 0 警告**（含 K2 基线 151 + 本域 158） | `cd D:/2/aiu4-scratch && cargo test` |
| 全库集成（git worktree 干净 HEAD fda90911 + istar + lib.rs 注册） | **4,270 通过 / 0 失败**（97.8s） | worktree 内 `cargo test -p varix --lib` |
| 行数对账 | 总 14,019 行 / 纯功能 8,889 行（对上限 17.6%，如实登记） | `_attic/aiu4-f551-f600/行数对账与缺陷账本.md` |

## 三、域内结构

```
kernel/varix/src/istar/
├── mod.rs            域聚合器（blocks=51，run_istar_checks）
├── ibase.rs          共享底盘：跨午夜时段 / Tint12 十二色板 / 重名递增命名 / CloneLog
├── batch7gate.rs     F575 批次七验收锚点（25 锚点注册表 + 全绿基线判定器）
└── 48 个功能模块      F551-F574 + F576-F600 逐项一模块
```

模块地图（模块名 → 功能）：
privconfirm=F551 特权确认窗 ｜ adminrun=F552 管理员运行 ｜ dragbadge=F553 拖影计数 ｜
dndtimer=F554 定时勿扰 ｜ micalm=F555 麦克风降噪 ｜ userredir=F556 用户目录重定向 ｜
migmate=F557 换机迁移 ｜ keybtest=F558 键盘测试 ｜ deadpixel=F559 坏点检测 ｜
holiday=F560 节假日 ｜ lnkparams=F561 快捷方式参数 ｜ rescuedisk=F562 恢复盘 ｜
tvsearch=F563 任务视图搜索 ｜ lockclock=F564 锁屏时钟 ｜ greet=F565 登录问候 ｜
foldertint=F566 文件夹色标 ｜ mutetimer=F567 定时静音 ｜ midclose=F568 中键关闭 ｜
notifyjump=F569 通知直达 ｜ showdesk=F570 显示桌面 ｜ tabrestore=F571 标签恢复 ｜
crashbrief=F572 崩溃简报 ｜ candcount=F573 候选数量 ｜ upsummary=F574 更新摘要 ｜
tilegroup=F576 磁贴分组 ｜ searchchips=F577 过滤片 ｜ openloc=F578 打开位置 ｜
scrollhide=F579 滚动条隐藏 ｜ scrolledge=F580 端点双击 ｜ tabselect=F581 Tab 全选 ｜
btndebounce=F582 防双击 ｜ focusmem=F583 焦点记忆 ｜ titletrunc=F584 标题截断 ｜
focusfollow=F585 焦点跟随 ｜ dirsize=F586 目录大小排序 ｜ savedsearch=F587 保存搜索 ｜
pasteimg=F588 粘贴为文件 ｜ dropupload=F589 拖拽上传 ｜ wallpair=F590 壁纸配对 ｜
outdevkey=F591 输出热键 ｜ shotcursor=F592 截图光标 ｜ delayshot=F593 定时截图 ｜
clickripple=F594 点击高亮 ｜ reshot=F595 固定重截 ｜ loginime=F596 登录屏输入法 ｜
desknum=F597 桌面直达 ｜ staggerboot=F598 自启动错峰 ｜ setverify=F599 设置校验 ｜
iregistry=F600 I 域收官登记

## 四、施工纪律执行情况

- **判据唯一源**：每模块 doc 头第一句摘主册判据；验收锚点入 batch7gate（25 锚点
  表 + 可执行性抽查 5 条 + 账册检对账）。
- **零堆热路径**：内核侧一律注入钟（ms 实参）+ 定长结构；heap 只在
  自检/测试上下文出现。
- **依赖接缝**：F078/F122/F235/F341/F361/F406/F417 等 15 处跨域依赖全部以
  显式参数/回调注入口承接，零反向编译依赖。
- **诚实交付**：行数对上限比逐项登记（17.6%），范围声明写入缺陷账本 #9；
  两处结构性 bug（userredir 完成态占通道 / micalm 底噪自举）在检查期望
  修正前先修根因。
- **并行纪律**：主树内其他分队 WIP（h1star/deskstar/secstar2 等）零触碰；
  隔离舱 + worktree 双路验证绕开主树红项，阻塞台账如实移交。

## 五、遗留与移交

1. 主树 lib.rs 测试编译红项（h1base.rs 缺失 / quickset.rs 实参不匹配）——
   AI-H1 / D1 收口时自愈；本队 lib.rs 注册行已就位，全绿后即刻生效。
2. 实机面验证（录屏/示波/长跑采集）按主册属装机阶段，本泳道交付判据载体。
3. 行数余量（约 41,500 行）按分工书属宿主 UI 层 + 渲染层深化，待界面泳道
   接手时按各模块【设计要点】段展开。

---

## 六、复验轮（第二会话 · 2026-09-26 晚）：HEAD 集成态认证 + 跨队红项修复

> 首会话的全库集成基于 `fda90911 + istar 直挂`；`1e706f28` 才将
> `istar::run_istar_checks` 补挂进 robust.rs 域表——**注册后的 HEAD 集成态
> 此前从未被全量验证过**。本会话在独立 worktree（`D:/2/aiu4-verify4`）
> 补上这道认证，发现并修复两处跨队红项，全程未触碰任何并行在制品。

### 6.1 认证发现（修复前 5182 passed / 1 failed）

唯一红项为全域门 `robust::tests::f475_every_domain_reports`，定位到
`F521-shot-savedir FAIL 9/11`（AI-U3 领地，两条确定性红检查）：

- `same_minute_seq`：`format_shot_name` 对 seq 1-9 也产出两位零填充 `_02`，
  违反函数 doc 自钉的「序号 `_N`」契约（检查期望 `_2` 正确，代码错）；
- `name_capacity`：前缀未按 `PREFIX_MAX(32)` 截断，40 字节前缀产出 58 字节，
  违反检查注释自钉的「最长组合 50 字节」契约（代码错）。

**溯源**：`git fsck` 悬空提交逐个比对（含 U3 变基前全部版本）——
`format_shot_name` 自 U3 首个提交起即为现态，红项自始存在；U3 报告
「4422/4422 全绿」认证数字与该文件状态对不上，如实登记存疑。

### 6.2 修复（最小手术，恢复自钉契约，检查期望零改动）

| 文件 | 修法 |
| --- | --- |
| `ustar3/explorerx.rs` | 前缀循环 `.take(PREFIX_MAX)` 截断；seq<10 写一位、≥10 才 `put_2dig` |
| `robust.rs` 域表 | 清 rebase 伤：U1 注释块下错挂的 secstar2 重复注册行删除（同域两行→一行）、注释与注册对位（uni1 仍恰一次）；域表声明容量 350→349 |

### 6.3 复验结果

同 worktree 全量 `cargo test -p varix --lib` 复跑（HEAD=7bded35e，262.81s）：
**5425 passed / 0 failed**——istar 域聚合 51 块全绿（直调）、F521 两红检查
转绿、ustar3 307 + istar 155 单测全绿。证据日志按 gitignore 纪律磁盘留档
（`_attic/aiu4-f551-f600/*.log`）。

### 6.4 复验轮追加发现：全域门静默截断（istar 从未进过 f475）

对账时实证（H3 在日志先行登记待裁，本队补域级证据与修复实验，详见缺陷
账本 §4.5/§4.6）：`checks.rs MAX_DOMAINS=320` < 域表 351 份注册，
`KernelCheckup::register` 超容静默丢弃——f475 渲染恰 320 行、末行 F521，
`istar-u4`、`genstar2`、ustar3 F522-F550 **从未进过全域门**。本队 50 项
全绿证据因此采用直调口径（域聚合直调 51 块 + 155 单测），不依赖 f475，
证据成立；「f475 覆盖 istar」的初版表述已修正。worktree 实验
`MAX_DOMAINS→384` 解封 40 域，暴露同被吞的 `F525-hotkey-card FAIL 10/11`
（检查期望错，已修——见账本 #13），终局 **5425/0：全域门 360 域全数
PASS、`istar-u4 PASS 51/51` 在门内实证**。MAX_DOMAINS 一行落位移交
checks.rs 属主（该文件正被并行会话持有 WIP，本队不越权）。

---

## 五、深化批次三（第三会话 · 2026-09-26 深夜）：深化层全量收口

**批次三 28 项补深落地**（F553/F559/F561/F564/F566/F568/F570/F573/F574/F575/F577/F578/F580-F585/F587-F589/F591-F595/F597/F600）——至此 **F551-F600 五十项深化层全量在位**（批次二 22 + 批次三 28），批次二 §四登记的「28 项属宿主 UI 层未深化」缺口闭合。

| 维度 | 批次二收口 | 批次三收口 |
| --- | --- | --- |
| 纯功能行（含聚合器） | 11,256 | **16,242**（对上限口径 32.2%） |
| CheckSet 检查项 | 387 | **≈748**（基础 379 + 深化 360 + 聚合 9） |
| 域内单测 | 155 | **277（全绿）** |
| 深化覆盖 | 22/50 | **50/50** |

**隔离验证（worktree HEAD=4bfd25a6 + 批次三）**：本域 277/277 全绿；全量 7,419 测试中 7,327 ok / 84 FAILED——红项全部他队领地（82 h1star 未提交在制品 + 2 stareco），istar 零红。施工期发现并闭环缺陷 3 条（#14/#15 检查期望错 + #16 他队 HEAD 破损阻塞台账），全过程见 `_attic/aiu4-f551-f600/行数对账与缺陷账本.md` §五 与 `docs/AI-U4-检查项对账表.md`。
