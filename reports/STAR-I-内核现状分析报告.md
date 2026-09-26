# Varix STAR I 内核现状详细分析报告

> 分析对象：VARIX-500 内核（kernel/varix）+ Varix STAR I 四册图纸（MD1 总案 / MD2 技术详案 / MD3 任务分布 / STAR I start）+ src 前端设计资产
> 分析日期：2026-09-27

---

## 一、内核总体现状（现在怎么样了）

**一句话：VARIX-500 已是一套能从 U 盘引导到桌面、规模约 36 万行的自研 Rust no_std 内核，骨架完整、域覆盖极广，当前处在 STAR I「深化批次」施工期，而不是从零起步期。**

量化盘点（以当前工作树为准）：

| 维度 | 现状 |
|---|---|
| kernel/varix/src 顶层模块 | **346 个**（35 MB 源码） |
| 顶层 src 行数（单层统计） | 约 20 万行，加上 deep*/deskstar/compatstar 等子目录总量约 36 万行 |
| 启动链 | Limine → 内核 ELF → HHDM/页表自举 → boot complete（QEMU 全链路可跑，varix.iso / varix.img / varix-uefi.img 均在库） |
| 驱动层 | ahci、nvme、usb/xhci、msc、pci、blk、thermal、irqdma、netstack、bt —— 存储栈（含断电一致性方向）已成形 |
| 合成器 | compositor/：帧调度（5 档）、脏区刷新、碰撞、路由、窗口树、崩溃恢复、看门狗拉起 |
| 兼容层 | compatstar（21 件：GDI/GDI+、注册表 hive、WOW64、PE 绑定、.lnk、拖放、字体链…）+ compatstar2（22 件：winsock、winmm、imm32、DPI、安装器、游戏前门…）+ deep/deep2/deep3 深化批次 |
| 性能域 | perfstar 12 件算法本体（伙伴分配、CLOCK+LRU-K、时间轮、令牌桶、DEFLATE/PNG 全链、DAG 调度），CheckSet 587 项全绿 |
| 桌面 | deskstar 全套（任务栏、开始菜单、通知中心、Alt+Tab、窗口贴靠、回收站、剪贴板、锁屏…） |

**测试面**：全仓约 7,400+ 单测在跑；各域隔离验证（worktree）纪律已成惯例——最近批次 U3 252/252、K1 322/322、U4 277/277 全绿。红项基本是他队在途的 h1star 在制品，不是本域缺陷。

**结论：内核已经"活着"，且进入了按 MD 图纸逐 F 条目（F001–F600）深化的阶段。**

---

## 二、四册 MD 图纸的定位与关系

1. **MD1 总案（2640 行，宪法）**：Sovereign/Three-legged/Assimilation/Relay 四支柱。定死了三件事对本次分析最关键：
   - **三腿战略**：原生应用腿 + 兼容层腿（Wine 支架/Linuxulator/Servo）+ Windows 域兜底（接力重启切换，不是虚拟化）；
   - **接力式双域**：VARIX 域与 Windows 域同刻只活一个，切换即重启交接（Hypervisor 路线已被否决）；
   - **生态借力**：自有代码收敛 60 万行量级，靠"翻译插座"接入亿行开源生态。
2. **MD2 技术详案（2206 行，图纸）**：合成器主循环、帧生命周期四步（收集→合成→后处理→提交）、脏区三档策略、共享内存缓冲双缓冲、帧率策略（**拖动判据 55fps，"掉帧可接受、手感迟滞不可接受"**）、看门狗恢复路径。
3. **MD3 任务分布（437 行，施工组织）**：四域 25 个工作包、四道闸门、单编制（AI01 起算）顺序施工、"债不过波"铁律。
4. **STAR I start（11684 行，深化总册）**：A 兼容域（F001–F040）、B 性能域（F041–F070）、C 体验域（F071–F125，**含 C-7 4K 资产管线硬标准**）、D 生态开放域、E 个性化域——这就是当前各 AI 分队正在逐条兑现的账本，与 CHANGELOG 里的 U3/K1/U4/V1 批次一一对应。

---

## 三、兼容性分析（能不能跑 .exe）

### 已在位的
- **PE 侧模型完整**：pebind.rs（符号自愈命中率 600‰ 阈值、全解析 1μs/符号、命中 100ns/符号的成本模型）、peblend/pebind 双文件、reghive 注册表、wow64 32 位兼容、fontchain 字体链、fsredir 文件重定向；
- **API 面**：gdiface/gdiplus（图形）、winsock/winmm/imm32（网络/多媒体/输入法）、comdlg/comloc、lnkfile、dragdrop、installr/uninstall（安装卸载链）、gamefront（游戏前门）、dpistate（DPI 感知）、codepage（代码页）。

### 诚实边界（按 MD 图纸口径）
- **compatstar 目前是"兼容策略与垫片模型"，不是完整的 Win32 PE 执行器**。真正的 .exe 运行需要：ELF/PE 混合装载器 → 用户态进程 → 系统调用层 → Wine 级 API 垫片，这条链在 MD1 第 23/24 章规划为 **Wine 支架 + Linuxulator** 路线，属于兼容域工作包的后续波次。
- 换句话说：**"在 Varix 桌面里打开任意 .exe"是三腿战略的第二腿的终态目标，当前具备的是 PE 识别、绑定、DPI、注册表、资源解析等地基；直接双击运行大型 Windows 程序尚需 Wine 支架落地。** MD1 对此的口径是"存量软件通吃靠兼容层腿 + 跑不动的走 Windows 域兜底接力"，这个设计本身就是对"任何 .exe"的诚实回答。

---

## 四、性能与渲染方式（GPU/CPU 用满、80 帧）

### 现有渲染栈
- **三阶梯图形（aurora/gpu.rs）**：Real GPU → Virtual GPU → Soft 光栅的降级链已建模，命令流编码器（环形命令缓冲 + NOP/DRAW/FLIP/WAIT 包）、显存分配器（首适应 + 释放合并 + 泄漏检测）、着色器字节码校验器都在，**真实硬件寄存器访问留了接口位但未接**——目前合成主体是 CPU 软渲染 + 脏区搬运。
- **帧调度（compositor/framesched.rs）**：5 档（vsync/adaptive/balanced/lowpower/immediate），默认预算 16ms（≈60fps 上限），零堆、纯整数。
- **4K**：display.rs/dispout.rs 已有 3840×2160 模式与虚拟桌面边界断言，4K 是一等公民不是补丁。

### 改造路线（要更好用 GPU/CPU，建议按此顺序）
1. **接真 GPU 驱动**：在 drivers/gpu.rs 的接口位落第一个实驱动——QEMU 里选 **virtio-gpu**，真机选 Bochs/VMware SVGA 或 Intel GEM 简化路径。把 aurora 的命令流包（DRAW/FLIP）真编到硬件 ring buffer，软光栅降为第三阶梯兜底。
2. **合成器走显存路径**：后备帧从"CPU 内存 memcpy 到 fb"改为"表面常驻显存 + GPU blit"，脏区并集交给 GPU，CPU 只做提交；这一步做完 4K 下合成成本降一个量级。
3. **80 帧的真相**：帧调度预算现在钉在 16ms（60fps）。要稳 80fps 需要：① 显示器跑 80Hz+（或 adaptive-sync），② 预算改 12.5ms 档，③ 高分屏必须先完成第 2 步（4K@80 用 CPU 拷贝带宽算不过来，这在 MD2 的带宽分析里已经写明）。**"稳定 80 帧"在软渲染 + 4K 下不可行，接 GPU 后可行**；MD 图纸的既有判据是拖动 55fps P95，80fps 属于判据升级，需要同步改 B 判据表。
4. **CPU 侧压满**：perftstar 的时间轮/令牌桶/EDF-WFQ 已就位，接上合成器做帧预算调度即可；脏区三档（平移模式、全帧模式）保留，静止桌面长眠策略不变。

---

## 五、src 视觉设计 → 内核 4K 移植

现状盘点：
- **src 侧**（React/TS/Tauri 桌面域）：design/tokens.css + system/ 下 30+ 子系统（desktop-design、taskbar、startmenu、explorer、lockscreen、palette、iconpacks、ambience、persona…），视觉规格按 Windows 11 对齐测量值（start.md 卷首·乙）。
- **内核侧**：designsys/mod.rs 目前基本是空壳；deskstar 已有一套内核自绘桌面（任务栏/开始菜单/通知中心等）。

移植方案（建议）：
1. **令牌先行**：把 tokens.css 的颜色/间距/圆角/字号栅格转成 no_std Rust 常量表，落进 designsys，全部用设备无关单位 ×4K 缩放因子（dpistate 已有 DPI 感知地基）。
2. **4K 资产管线按 start.md C-7 执行**：图标多尺寸源文件（512px 母版 → 各档位），进 atlas.rs 图集，禁用位图放大；这已经是图里的"硬标准，不是口号"。
3. **逐面移植**：taskbar → startmenu → explorer(deskstar/tabexplorer) → lockscreen → notify，每面走"src 设计稿为视觉真相、deskstar 自绘为运行时"的对应关系，验收用 4K 截图对比。

---

## 六、稳定性（80 帧稳定运行的前提）

已有底子：合成器崩溃恢复（表面注册表 + 共享内存缓冲与合成器解耦、三秒回位判据 D-04）、看门狗拉起、驱动生命周期"probe 失败不阻断 / remove 可重入 / 引用零悬空"、断电对练测试金字塔。**要"所有大型软件工程极稳"，缺的是用户态进程隔离 + 每应用内存配额（quota.rs 已有骨架）+ Wine 层崩溃不拖垮合成器的进程边界——这些都在 MD 的阶段规划里，是兼容域落地时一并兑现的。**

---

## 七、总结：现在在哪，接下来往哪打

| 诉求 | 现状判定 | 路径 |
|---|---|---|
| 内核现状 | 约 36 万行、346 模块、可引导到桌面，深化施工期 | 按 MD3 波次继续 |
| 兼容性 | PE/注册表/WOW64/GDI/网络/输入法垫片模型齐 | 落 Wine 支架 + 用户态进程，才能真跑 .exe |
| GPU 渲染 | 三阶梯模型在、真实驱动接口位留空 | 先 virtio-gpu，再显存合成路径 |
| 80 帧稳定 | 默认 16ms/60fps 预算、4K 模式已支持 | 12.5ms 预算 + GPU 合成 + 80Hz 屏 |
| src 设计 4K 上内核 | tokens/design 稿齐全，designsys 空壳 | 令牌表 → 4K 图集 → 逐面对照移植 |
| 打开任意 .exe | 是终态目标（三腿战略第二腿） | 兼容层腿落地 + Windows 域兜底接力 |
