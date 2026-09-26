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
