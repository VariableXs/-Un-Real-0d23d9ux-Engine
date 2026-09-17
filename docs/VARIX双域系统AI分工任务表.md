# VARIX 双域系统 · AI 分工任务表

> 配套文档：《VARIX双域系统施工总案.md》（逻辑与验收标准）、《VARIX双域系统总施工清单.md》（进度勾选）。
> 本文件职责：**把全部工作拆成从 1 开始编号的独立任务，供多个 AI 并行分工领用**。
> 规则：AI 领任务先看「前置」是否全部完成；完成一个任务 = 代码/文档交付 + 验收全过 + 在本文件对应项打勾并注明完成日期与 AI 名。
> 全局不变量（任何任务不得违背）：见总案「全局不变量」十条（物理隔离/默认拒绝/诚实文化/行为等价/可停性/零残留/编码纪律/内核边界/体验红线/用户自由度）。

## AI 分工角色定义

| 代号 | 职责范围 | 主要工作区 |
|---|---|---|
| AI-K | 内核线：引导/缺页/用户态/驱动/文件系统 | `kernel/varix/src/` |
| AI-V | 界面线：Servo 垫片/VWM 适配/四大软件/体验专项 | `src/` |
| AI-B | 桌面后端线：命令面映射/容器/隔离/嵌入语义保留 | `src-tauri/` |
| AI-P | 便携与部署线：U 盘五分区/差分链/升级灾备 | `portable/`、`scripts/` |
| AI-S | 安全与质量线：白名单/fuzz/门禁/审计/基准 | `tools/`、`code-analysis/` |

---

## 阶段 0：引导选择页（部分已完成）

- [x] **任务 1**（AI-K）：键盘上下键选择——input 域 PS/2 轮询接出「键事件查询接口」，bootselect 倒计时中响应 ↑/↓/Enter；QEMU 实机验证三种路径（默认超时/选中 varix/选中 windows）。前置：无。✅ 2026-09-16 AI-K（新增 `kernel/varix/src/ps2.rs` 轮询+Set-1 解码；bootselect 倒计时 50 片/秒轮询、↑↓即时重画、Enter 即选；宿主 14 例+QEMU 三路径串口/截图归档 `docs/acceptance/2026-09-16-任务1-键盘选择/`；ktest 2728 绿/kcheck 0 告警）
- [x] **任务 2**（AI-K）：BootNext 一键切 Windows——UEFI 变量写入 + ResetSystem 路径；QEMU 验证写变量与重启行为。前置：任务 1。✅ 2026-09-16 AI-K（新增 `kernel/varix/src/bootnext.rs`：Limine EFI 系统表/内存映射请求接入，SetVariable 写 BootNext+GetVariable 读回校验+ResetSystem(EfiResetCold)；RS 调用前置低 4GiB 恒等映射（固件以绝对物理地址自引用）；OVMF UEFI 实机验证「写变量+复位」全链（VM 按复位退出），SeaBIOS BIOS 引导如实降级「UEFI BootNext unavailable」继续引导 varix；证据归档 `docs/acceptance/2026-09-16-任务2-BootNext切Windows/`；ktest 2732 绿/kcheck 0 告警）
- [x] **任务 3**（AI-K）：选择页视觉收口——console 清屏策略消除倒计时残影；多分辨率（800×600/1280×720/1920×1080）截图归档。前置：任务 1。✅ 2026-09-17 AI-K（`run_countdown_with` 每帧 `draw_frame`＝`banner::paint_backdrop` 重绘背板后再 draw，清屏策略单一来源；logo 阶段前补一次背板重绘消除选择页残影；新增 `frame_repaint_erases_previous_highlight` 残影断言单测；`VARIX_RENDER_MENU=1` 归档三分辨率整页渲染 `docs/acceptance/2026-09-16-任务3-选择页视觉收口/menu-{800x600,1280x720,1920x1080}.ppm/.png`；QEMU 实机 1280×800 三帧视觉验证（默认高亮 VARIX / ↓ 高亮移 WINDOWS 首卡无残影 / ↑ 回顶 WINDOWS 卡无残影），证据同目录 `menu-{default,down-windows,up-top}-1280x800.png`；ktest 2733 绿/kcheck 0 告警。**修正（任务4 连带）**：`fb.rs pack(Bgr32)` R/B 字序错误（QEMU 实机抓帧发现——定义蓝的 HL_BOX 显示成橙），修正后三分辨率归档与三帧实机证据全部重抓重归档，ktest 2749 绿）
- [ ] **任务 4**（AI-K）：`boot-select.json` 配置读取（共享分区路径抽象为参数注入），损坏走内置默认并 kwarn。前置：任务 1。✅ 2026-09-17 AI-K（新增 `kernel/varix/src/bootcfg.rs`：手写最小 JSON 子集解析 ≤400 行零 panic；容错三层=字段缺失→默认/类型错值域非法→该字段默认/整体损坏→全部内置默认+kwarn+「CONFIG RESET」角标；timeout 值域 0 合法/负值→默认/超大→钳60；未知字段忽略+深度上限；路径 `SHARED_BOOT_SELECT_PATH` 与读取函数均参数注入，目标态经 Limine 可选内模块（MODULE_REQUEST rev1 flags=0，缺失不 panic）读引导卷 boot-select.json，真盘 FS 任务17/18 落地后仅换 reader；bootopt 增字段级 customized_timeout/entry 旗标，合并优先级=cmdline 显式>共享配置>内置默认；1000 组随机字节+结构化 fuzz 单测；QEMU 实机三场景：合法配置静默生效（timeout_sec=9 实测倒计时 9）/整体损坏 kwarn+角标/文件缺失静默，证据 `docs/acceptance/2026-09-17-任务4-bootselectjson配置读取/`；ktest 2749 绿/kcheck 0 告警）
- [x] **任务 5**（AI-K）：引导页负向演练矩阵——配置损坏/键盘中途拔除/倒计时中拔盘/无帧缓冲四场景各一遍并归档记录。前置：任务 4。✅ 2026-09-17 AI-K（四场景 QEMU 实机全过：①配置损坏→kwarn+CONFIG RESET 角标+回落内置默认 ②↓后键源静默（物理拔除等价路径）→高亮冻结、归零回落默认项 ③HMP drive_del 倒计时中拔盘→内核模块已载内存，boot complete 零 panic ④-vga none→serial-only 降级、自检 10/13 如实上报、boot degraded；**发现并修复缺陷**：归零曾执行未确认的 ↑↓ 预选项（↓后拔键盘会自动切 Windows，违反默认拒绝），修复为归零返回 default_index、切 Windows/固件必须显式 Enter，宿主单测同步更正；证据+演练记录+残余风险声明 `docs/acceptance/2026-09-17-任务5-引导页负向演练/`；ktest 2749 绿/kcheck 0 告警）

## 阶段 1：U 盘五分区便携基建

- [x] **任务 6**（AI-P）：五分区 GPT 脚本（ESP/VARIX 系统/引擎 VHDX/共享 exFAT/快照区）＋幂等重跑＋对齐校验。前置：无。（2026-09-16 AI-P 完成：portable/AI-P/Create-Partitions.ps1 + partition-plan.json；自测 25/25，验收 docs/acceptance/双域-阶段1-六维验收-AI-P任务6任务9-2026-09-16.md）
- [ ] **任务 7**（AI-P）：ESP 组装——VARIX 引导器与 Windows 引导文件双链共存；VM 首启验证。前置：任务 6。
- [ ] **任务 8**（AI-P）：Deploy-To-USB 总编排适配五分区（沿用 Preflight/Stage/Verify），断点续作支持。前置：任务 7。
- [x] **任务 9**（AI-P）：SHARED 目录契约初始化＋`apps.json` schema 定版（带 version 字段与迁移说明）。前置：任务 6。（2026-09-16 AI-P 完成：portable/AI-P/Init-Shared.ps1 + README 契约三方表；幂等/损坏留证重建/负向 schema 用例全过）
- [ ] **任务 10**（AI-P）：强拔演练——各阶段（引导/倒计时/系统运行中）拔盘各 ×3，下次插入可恢复，归档。前置：任务 8。
- [ ] **任务 11**（AI-P）：多 U 盘版本管理——同盘差分版本命名与回收站区约定。前置：任务 9。

## 阶段 2：内核欠账清零

- [x] **任务 12**（AI-K）：#PF handler 接线——decide() 四路挂入 idt 路径；非法访问测试四路各走通（guard/Fault、COW、GrowStack、MapZero）。前置：无。✅2026-09-16 宿主 FakePt 单测 6 例（四路+reserved+登记表边界）；目标态 RealPt 真页表（拆大叶+invlpg）；idt isr_dispatch 接线；实机自检 MapZero/GrowStack/COW×2 全通（selftest 4/4，guard 走宿主+fatal 路径，SwapIn 如实 Fatal）。连带修复 iretq 错误码帧错位（可修复 #PF 返回必 #GP，common_entry 帧下移 8B 补 drop_error_code）与 fatal 诊断帧全 0 伪寄存器（read_trap_frame 真实化）。ktest 2757 绿/kcheck 0 告警，启动链自检 13/13 行为等价。凭证 docs/acceptance/双域-任务12-pf接线-selftest44-实机串口-2026-09-16.log。
- [x] **任务 13**（AI-K）：COW 基建——引用计数页+写时复制，计数归零竞态 ×1000 稳定。前置：任务 12。✅2026-09-16 新建 mem/cow.rs：并发安全 CowTable（F043 ticket 锁 SpinProtected，替代无锁旧 paging::CowTable 并删除后者）；语义规范化 attach=1/share=+1/release=−1，归零回收 Reclaimed 恰好一个调用者（锁内 1→0 判定+槽紧缩 O(1)），#PF 路径经 reclaim 回调归还 PMM。宿主：race_to_zero_x1000（8 线程并发 release ×1000 轮，归零者恒 1）+share_storm（4000 并发 share 计数精确 1001）+断裂 4096 字节逐字一致+饱和拒绝/表满/审计共 10 用例。溢出策略显式声明（u32 饱和即拒绝绝不回绕）；位操作全封装（paging mark_cow→Option 校验 present/is_cow 三条件/cow_flags_for_*/leaf_flags，RealPt 改走封装），零内联位算术。实机 selftest 4/4（COW 路含 512×u64 模式逐字保真+归零回收还帧），ktest 2769 绿/kcheck 0 告警，boot complete 行为等价。凭证 docs/acceptance/双域-任务13-COW基建-race1000-逐字保真-实机串口-2026-09-16.txt。
- [x] **任务 14**（AI-K）：ring3 切换 + 最小 syscall（exit/串口 write）+ 用户态 hello。前置：任务 12。✅2026-09-16 Limine→内核→iretq 进 ring3（entry=0x400000，19 用户页映射）；int 0x80（IDT dpl3+RSP0 切 int_kstack）与 syscall 指令（STAR/FMASK/LSTAR+naked stub sysretq）双通道 write 输出逐字节一致；exit(0)→Zombie→reap→Vacant 槽回收实证（pid=2，曾硬编码 pid=1 空转假通过，改 DEMO_PID 登记）；hello 用户态 CS 自检=0x1b。连带修复三处真实缺陷：ELF 同页多段覆盖 PTE（页账本共享帧+页内偏移拷贝+flags 并集）、pfh::ensure_pt 2MiB 大叶拆分缺失、诊断地址笔误；QEMU 11.1 iretq 窗口 CS 槽染 0x2b artifact 定性 TCG 问题并保留 guard_frame_cs 正式守卫。ktest 2776 绿/kcheck 0 告警/kbuild 成功，QEMU 实机串口验收无 fatal。凭证 docs/acceptance/双域-任务14-ring3切换与最小syscall-实机串口-2026-09-16.md。
- [x] **任务 15**（AI-K）：进程控制块 + spawn/wait + ELF 装载器；与 PE 共用映像装载原语抽象。前置：任务 14。✅2026-09-17 新建 `proc/loader.rs`：SegmentView/ImageFormat/UserMapper 三抽象+页账本引擎（同页多段共享帧/页内偏移拷贝/flags 并集/失败全回滚），ELF 适配器+KernelMapper(target)，格式中立性由 hello.elf≡同段表中立镜像逐页逐字节等价用例结构性保证（未来 PE 只需实现双 trait）；PCB 全字段经核内 API（审计零裸字段跨模块改）；句柄类型运行期注册扩展（16 封顶+同名幂等）；waitpid×1000 零泄漏用例。实机 QEMU：两轮 spawn→iretq→exit→wait→页账本 18/18 归还→pid=2 槽复用实证；压力探针 64 槽打满/TableFull 优雅拒绝/全量退场 live=0 无泄漏；无 fatal 优雅停机。连带修复三真实缺陷：SpaceArena destroy 槽位不复位+create 只追加致 16 轮后永久耗尽（连带 TableArena::free_user_tree 递归归还用户表帧，内核半区共享表防御跳过）；监护者 match 临时守卫自旋死锁（Err 臂再 lock）；退出账本同页重复记账。ktest 2784 绿/kcheck 0 告警/kbuild 成功。凭证 docs/acceptance/双域-任务15-PCB-spawn等待-ELF装载器-映像原语抽象-实机串口-2026-09-17.md + docs/进程生命周期状态图-2026-09-17.md。
- [ ] **任务 16**（AI-K）：块设备抽象 + NVMe 最小栈读写回环；QEMU NVMe 盘验证。前置：任务 14。
- [ ] **任务 17**（AI-K）：fs23_journal 后端接真块设备 + 掉电注入回归 ×10。前置：任务 16。
- [ ] **任务 18**（AI-K）：exFAT 只读实现挂载 SHARED 分区；写路径走快照区过渡并如实标注。前置：任务 17、任务 6。
- [ ] **任务 19**（AI-K）：PS/2 键盘事件服务化（供任务 1 复用）；xHCI 立项评估报告。前置：任务 14。
- [ ] **任务 20**（AI-K）：显示服务收编——bootselect/future shell 统一绘制接口（Surface 归口）。前置：任务 3。
- [ ] **任务 21**（AI-K）：里程碑整合——「内核可运行用户态程序读写真盘」整链 QEMU 演示脚本。前置：任务 15/17/18。

## 阶段 3：Variable 界面上内核

- [x] **任务 22**（AI-V＋AI-B）：垫片协议定版——invoke 名/参数序列化/错误码三段式规范文档。前置：无。（2026-09-16 AI-V+AI-B：规范 docs/双域-垫片协议规范-v1.md；单源 tools/shim-protocol.source.json + 同源生成；运行时 shimInvoke.ts；TS8+Rust4 用例）
- [x] **任务 23**（AI-B）：三色审计——跑 audit.cjs 产出 554 方法映射表（✅可映射/🔶需内核新服务/❌暂缺），归档 docs。前置：任务 22。（2026-09-16 AI-B：docs/shim-mapping.json 549 方法/546 命令 100% 归属，✅314/🔶222/❌13；生成器 tools/shim-mapping.cjs 未分类即门禁失败；验收记录 docs/acceptance/双域-垫片协议与三色审计-六维验收-2026-09-16.md）
- [ ] **任务 24**（AI-K）：内核 KV 存储服务（对应前端 localStorage 语义，差异公示）。前置：任务 15。
- [x] **任务 25**（AI-V）：Servo 移植评估——Variable 前端依赖的网页特性清单逐项标 Servo 支持度，产出风险表。前置：任务 22。✅ 2026-09-16 AI-V：`docs/双域-Servo移植评估风险表-2026-09-16.md`（特性逐项表+嵌入 API 对接面+补齐排序；联网查证 servo.org 2026-02/07 月报与 book.servo.org 实验特性表；结论：可承载，仅 backdrop-filter/CJK 管线两项需降级方案）。
- [ ] **任务 26**（AI-V）：输入事件管道（内核→Servo）+ 焦点模型原型。前置：任务 19、任务 25。🔶 2026-09-16 AI-V：前端侧原型 `src/lib/shim/inputBus.ts`（焦点栈/兜底路由/剪贴板白名单预留点）+ 用例 8 条全绿（含 1000 次焦点切换无串键）；**实机链路被任务 19（AI-K 内核输入服务）阻塞**，事件频道名已按协议登记 `shim://input`，19 落地后逐字段核对事件结构。
- [ ] **任务 27**（AI-V）：第一批保底命令面——桌面壳/文件管理器/设置页三件套在 Servo 内渲染成功。前置：任务 23/24/26。
- [ ] **任务 28**（AI-V）：BootScreen 进度事件对接（bootPhase 语义不变）。前置：任务 27。
- [ ] **任务 29**（AI-V）：降级提示 i18n（zh/en）全量补齐＋性能采样基线（首帧/交互延迟）。前置：任务 28。

## 阶段 4：数据总线与白名单

- [ ] **任务 30**（AI-S）：VFS 白名单裁决层（挂共享分区挂载点）＋越权审计日志。前置：任务 18。
- [ ] **任务 31**（AI-S）：路径逃逸 15 变体负向用例（../ 混淆/符号链接/大小写/UTF-8 归一化等）全拦截。前置：任务 30。
- [ ] **任务 32**（AI-S）：共享内存块原语＋授权模型（句柄不可传递默认）。前置：任务 15。
- [ ] **任务 33**（AI-S）：消息通道原语（订阅/广播）。前置：任务 32。
- [ ] **任务 34**（AI-S）：剪贴板/拖放白名单化。前置：任务 33。
- [ ] **任务 35**（AI-S）：Uxv 交换格式接入＋校验失败安全路径。前置：任务 33。
- [ ] **任务 36**（AI-V）：设置页白名单管理 UI＋审计查看页。前置：任务 30。
- [ ] **任务 37**（AI-S）：规则热更新与冲突仲裁机制。前置：任务 36。

## 阶段 5：Wine 兼容层通道

- [ ] **任务 38**（AI-B）：Wine 移植评估报告——NTAPI 对接面→VARIX syscall 映射清单，工作量分级。前置：任务 15。
- [ ] **任务 39**（AI-B）：PE 装载器＋静态链接 exe 加载运行（不含导入解析）。前置：任务 38。
- [ ] **任务 40**（AI-B）：导入表解析＋Wine 核心 DLL 绑定。前置：任务 39。
- [ ] **任务 41**（AI-B）：记事本级闭环（GDI 文本/菜单/文件对话框）。前置：任务 40。
- [ ] **任务 42**（AI-B）：Job 限额接入（内存/CPU rate/KILL_ON_JOB_CLOSE 语义对齐 src-tauri isolation.rs）。前置：任务 41。
- [ ] **任务 43**（AI-B）：prefix 模板与每进程隔离目录。前置：任务 41。
- [ ] **任务 44**（AI-B）：适配数据库建立（每软件：版本/所需 API/结论/缺失清单）。前置：任务 41。
- [ ] **任务 45**（AI-B）：10 软件并发隔离压力（无串扰/无句柄泄漏）。前置：任务 42/43。
- [ ] **任务 46**（AI-V）：分级登记写 apps.json＋桌面图标通道标记。前置：任务 44。

## 阶段 6：隐形 Windows 引擎通道

- [ ] **任务 47**（AI-P）：引擎 VHDX 挂载＋VM 拉起/保活/休眠编排。前置：任务 8、任务 12。
- [ ] **任务 48**（AI-P）：画面流通道打通（先全屏后窗口级）。前置：任务 47。
- [ ] **任务 49**（AI-P）：输入注入通道。前置：任务 48。
- [ ] **任务 50**（AI-V）：VWM 外来窗口接管（注册/snap/几何持久化，复用 embed:// 语义）。前置：任务 48。
- [ ] **任务 51**（AI-V）：引擎冷启动阶段化叙事（20-40s 拆命名进度）＋占位卡视觉。前置：任务 50。
- [ ] **任务 52**（AI-P）：ramcache 落地（LRU/关机清空断言/整盘 hash 零残留）。前置：任务 47。
- [ ] **任务 53**（AI-P）：延迟测量与画质档位（办公/均衡/游戏三档）。前置：任务 49。
- [ ] **任务 54**（AI-P）：异常三场景演练（VM 崩溃/宿主资源枯竭/引擎内蓝屏）均不影响 VARIX 本体。前置：任务 50/52。
- [ ] **任务 55**（AI-V）：IME 中文输入专项。前置：任务 49。

## 阶段 7：资源统管与设置页

- [ ] **任务 56**（AI-K）：配额服务——三方分配矩阵/水位/回收。前置：任务 21。
- [ ] **任务 57**（AI-V）：设置页四组 UI（引导行为/软件通道/白名单/资源档位）＋settings://changed 双向同步语义沿用。前置：任务 29/37/46/53。
- [ ] **任务 58**（AI-P）：拔出全链演练（运行中强拔→中断→下次插入恢复）。前置：任务 10、任务 52。
- [ ] **任务 59**（AI-K）：GPU Vulkan 加速立项评估（软件渲染兜底不变）。前置：任务 21。
- [ ] **任务 60**（AI-P）：多机型测试矩阵（≥3 台不同厂商实机）建立并跑完首轮。前置：任务 57。

## 阶段 8-10：安全/灾备/生态（可与 5-7 交错）

- [ ] **任务 61**（AI-S）：Wine 进程默认能力收敛（无网络/无宿主盘）＋申请式授权 UI。前置：任务 42。
- [ ] **任务 62**（AI-S）：PE 拒绝表（≥20 恶意样本全拒＋50 正常软件零误拦）。前置：任务 40。
- [ ] **任务 63**（AI-S）：救援 CLI 四命令适配 U 盘自救援。前置：任务 8。
- [ ] **任务 64**（AI-S）：威胁清单终审（对策＋残余风险双栏）＋诚实声明页。前置：任务 61/62。
- [ ] **任务 65**（AI-B）：保险箱迁移（privacy.rs 语义平移至内核侧）＋密钥仅内存断言。前置：任务 33。
- [ ] **任务 66**（AI-P）：差分升级＋断电中途升级演练 ×10 零变砖。前置：任务 11。
- [ ] **任务 67**（AI-P）：快照滚动调度＋灾备 SOP（半损坏/全损坏）＋「什么救不回来」诚实清单。前置：任务 66。
- [ ] **任务 68**（AI-P）：三处配置一致性校验器（md5 对齐）。前置：任务 66。
- [x] **任务 69**（AI-S）：verify 单命令门禁（三线测试+audit+基准回归收敛）——**✅ 2026-09-16 AI-S 验收通过**（scripts/verify.sh 八段全绿实跑：tsc 0 错/vitest 2671 passed/ca-core 356/kcheck 0 错/ktest --lib 2727/variable --lib 267/audit PASS/bench 回归 PASS，基线 _attic/bench/baseline.json）
- [ ] **任务 70**（AI-S）：六解析器 fuzz 常态化（boot-select/PE/ELF/exFAT/规则/Uxv）。前置：任务 4/39/18/31/35。
- [ ] **任务 71**（AI-S）：U 盘场景性能基准（引导<8s/首帧<3s/交互 P95<100ms）＋回归门禁。前置：任务 29。
- [ ] **任务 72**（AI-V）：适配看板（apps.json 可视化）＋i18n 三语扩展＋无障碍全量检查。前置：任务 46。

## 体验专项（横切，按关联阶段就位）

- [ ] **任务 73**（AI-V）：首次运行向导（专项 A，5 步）——关联阶段 3。前置：任务 27。
- [ ] **任务 74**（AI-V）：帮助系统＋快捷键速查卡同源生成（专项 B）。前置：任务 57。
- [ ] **任务 75**（AI-V）：错误恢复体验三段式错误卡＋自检页（专项 C）。前置：任务 57。
- [ ] **任务 76**（AI-V）：反馈通知规范＋加载感知骨架屏（专项 D/E）。前置：任务 51。
- [ ] **任务 77**（AI-V）：个性化打磨＋信任透明（通道徽标/隐私仪表盘/隔离演示模式）（专项 F/G）。前置：任务 57。
- [ ] **任务 78**（AI-P）：升级体验＋数据搬家向导＋退役安全清空（专项 H/I）。前置：任务 66/67。
- [x] **任务 79**（AI-S）：工程级质量基建其余项（sanitizer/供应链评审/混沌演练例行化/双确认制）（专项 J）——**✅ 2026-09-16 AI-S 实跑验收通过**（sanitizer.sh 诚实 SKIP 机制验证；supply-chain-check.sh 通过且清零 npm high/critical——vitest 2→3.2.4/vite 5.4.21→7.1.11/plugin-react 5，升级后 vitest 2671 全绿+vite build 成功；chaos-drill.sh fs:: 80 用例+WAL 硬崩 50/50 持久零泄漏；double-confirm.sh+pre-commit 关键路径标记）：`scripts/sanitizer.sh`（无 nightly 诚实 SKIP）、`scripts/supply-chain-check.sh`（锁文件冻结+npm/cargo audit）、`scripts/chaos-drill.sh`（fs23_journal 掉电注入族+WAL 硬崩持久性演练）、`scripts/double-confirm.sh` + pre-commit 关键路径双确认标记。前置：任务 69（待其转绿后联跑收口）。
- [ ] **任务 80**（AI-S）：可观测诊断（日志统一时间轴/诊断快照/性能追踪/用户可读时间线）（专项 K）。前置：任务 71。
- [ ] **任务 81**（AI-V）：桌面核心体验打磨（中文字体管线/动效统一表/全局搜索/剪贴板历史复用 ai27）（专项 L）。前置：任务 27。
- [ ] **任务 82**（AI-V）：高频场景旅程六条端到端打磨（专项 M）。前置：任务 73-78。
- [ ] **任务 83**（AI-V）：实用基础包八件（命令面板/截图/快速预览/便签/时钟农历/计算器/OCR/工具箱）（专项 N）。前置：任务 81。
- [ ] **任务 84**（AI-V）：微交互细节十项＋防丢失低挫败六项（专项 O/P）。前置：任务 83。
- [ ] **任务 85**（AI-V）：自由度定制（快捷键重绑/主题编辑器/宏/布局自由模式）（专项 Q）。前置：任务 77。
- [ ] **任务 86**（AI-V）：学习曲线＋情感化惊喜（专项 R/S）。前置：任务 82。
- [ ] **任务 87**（AI-V）：硬件场景适配七项（多屏/合盖/投影/音频/USB 外设/唤醒/老机兜底）（专项 T）。前置：任务 60。

## 收尾

- [ ] **任务 88**（全体）：终局联验——「U 盘随插随用」完整旅程实机走查 ×3 台机型；对照本文件 1-87 逐项复核勾选；残项登记三色表并注明原因。
- [ ] **任务 89**（全体）：文档收口——总案/清单/本表三处状态同步；CHANGELOG 与验收报告归档 docs/acceptance/。
