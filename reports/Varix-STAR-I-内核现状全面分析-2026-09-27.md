# Varix STAR I 内核现状 · 全面分析报告

> 生成日期：2026-09-27 ｜ 调研范围：MD1/MD2/MD3/start.md（约 17,000 行）+ kernel/varix/src（346 模块）+ 验收档案

---

## 一、总体状态一句话

**图纸阶段全绿，实机阶段欠账。** STAR I 全案 25 个工作包已于 09-24 单日在宿主侧收口，内核测试 **3662 项全绿**，判据实装层覆盖约 134 个 CheckSet 域；但渲染仍是纯 CPU 软件渲染、Win32 兼容 27 个 API 大多 Stub、4K/高 DPI 主要活在设计规范里，实机（Y7000 + U 盘）闭环验收是摘星前唯一的硬缺口。

## 二、四份 MD 文档分析

### MD1 总案（工程最高纲领）
- STAR = Sovereign（自主内核）/ Three-legged（三腿）/ Assimilation（生态借力）/ Relay（接力双域）。
- 目标形态 VARIX-500：U 盘整机双域系统（VARIX 内核域 + Windows/Variable 域），同刻只活一个，Limine `LoaderEntryOneShot` 接力切换；Hypervisor 被正式否决（ADR-001）。
- 软件三分：原生自绘 + Linux syscall 翻译柜台（对标 Linuxulator/WSL1）+ Wine 支架；自有代码 60 万行封顶（ADR-007），Wine/Servo/relibc/smoltcp 等绝不重写。
- 成功判据 6 条：接力闭环 ≤30s、20 次冷启动 0 蓝屏、桌面独立服役一天、兼容首役（Neovim/git）、生态通水（VSCode Linux 版）等。

### MD2 技术详案（43 篇字段级图纸）
- 引导链/交接协议：handoff.json schema 冻结、五状态机、SHA-256 封条、四帧交接画面、WD-040 五步账 ≤25s；Windows 加固四板斧 + 防自锁闸。
- 显示栈 VXWM（B-501~507）：单线程事件泵 + "不需要就不合成"（静止 ≤ 单核 5%）；拖动 1080p ≥55fps（p95 ≤18181μs）；字形图集 LRU 60MB；合成器崩溃 3 秒恢复。
- **篇 8 图形加速定案**：CPU 软渲染打满全场，三层路径（系统位块 / Wine GL→Mesa llvmpipe / Electron 软件合成）；R3 硬加速**只摸不建**（`r3scan.rs` + `SubmitBackend` trait，GpuBackendStub 接口位）。

### MD3 任务分布
- 4 域 25 工作包（WP-101~405）+ 4 道闸门里程碑，绑定 127 项 B 判据，以判据而非行数计量。
- 第 6 章 STAR I start 四域扩展（09-25 拍板）：A 兼容（PE 直载/vxapp 侧载第一优先）、B 性能（B1 vxbench 基线铁律 + B3 GPU 加速合成器）、C 体验（桌面 UI 成型）、D 生态开放（不建软件商店）。

### start.md 主册（640 项功能深化设计 F001-F640）
- 交互词典对齐 Windows、视觉自研；**卷首·乙 尺寸基线**（Win11 实测：任务栏 48px、标题栏 32px、开始菜单 640×700px，高 DPI 等比放大）。
- **B 性能域红线**：开机动画 ≤8s、点击 100ms 反馈、**动画目标 80fps / 最低 60fps** —— 你说的"稳定 80 帧"正是此红线，四层优化路径为：脏区深化 → 冷启动预取 → **渲染路径 GPU 化**（Intel 核显 2D/3D）→ U 盘 I/O 深化。
- **C-7 4K 资产管线硬标准**：全部素材按 3840×2160 原生制作、CI 自动校验、低分屏降采样不重画；验收十二查第 4 查要求"4K 四档 DPI（100/125/150/200%）截图全绿"。

## 三、兼容性分析

**现状是双轨：Win32/PE 为主力实装，Linux 侧为设计+判据层。**

| 层 | 模块 | 状态 |
|---|---|---|
| PE 装载链 | `proc/pe.rs`、`peblock.rs`、`compatstar/peblend·pebind·persrc` | 静态 PE32+ 真装载可执行，导入绑定 + W^X |
| Win32 服务台 | `proc/winapi.rs` | 首层 27 API：**Full 4 / Partial 3 / Stub 20**；user32/gdi32 窗口面整体 Stub |
| Win32 语义面 | `compatstar/`（reghive、fsredir、condrv、clipfmt、SEH…） | 语义模型齐备，判据自检宿主全绿 |
| Win32 深化 | `compatstar2/` + deep/deep2/deep3 三批（winsock、imm32、dpistate…） | 125 检查/域，判据层完成 |
| Linux 转译 | `lxrun.rs`、`lxerrno.rs`、`lxprocfs.rs` | 设计完整，**实机 LTP 未跑** |
| Wine 支架 | `wineshim.rs` 等四路垫片 | 垫片在，**Wine 本体未入镜像** |

缺口：GUI 真渲染大量 Stub、"常用 50 件"实测未做、LTP/VSCode 深水区未验证、CJK 字形资产缺口。

## 四、性能与渲染（GPU/CPU 计划）

**当前 100% CPU 软渲染，无真实 GPU 加速**——GPU 相关全部是接口位/成本模型：
- `fb.rs`：Limine GOP 直通 + Surface 唯一绘制原语；`displaysrv.rs`：双缓冲 + 脏矩形（MAX_DIRTY=32），帧 >4MiB 诚实降级直写。
- GPU 桩：`drivers/gpu.rs`、`aurora/gpu.rs`（三阶梯命令流建模）、`r3scan.rs`（CpuBackend 实装 + GpuBackendStub）。
- "拖动 55fps" 是带宽模型推算，非实测。

**渲染方式升级的既定路线（改渲染方式用满 GPU/CPU 的方案已在文档立项）：**
1. **S406 GPU 摸底立项包**（WP-405 结项产出）：UHD 630 → Mesa iris 接管 → Vulkan 化合成器三步走；验收口径 60fps/8ms/35W；llvmpipe 退役为回退，NVIDIA 观察位。
2. start.md B-2·补 四层优化：① 合成器脏区深化 ② 应用冷启动（预取指纹/二次启动减半）③ **渲染路径 GPU 化** ④ U 盘 I/O 深化；铁律是 **B1 vxbench 基线不绿，B2-B4 不开工**。
3. 架构上已预留"换后端不换管线"：`SubmitBackend` trait 由编译期证明，CpuBackend 换 GpuBackend 只需替换后端实现。
4. CPU 侧机制已齐备（perfstar/perfstar2）：帧率账本、SIMD 图像、字形缓存、脏区深化、中断合并、大页、调度延迟预算——但真实计时标定登记为实机欠账。

## 五、80 帧稳定性

- 80fps 是 start.md B 性能域红线（动画目标 80 / 最低 60），当前状态：**机制与账本齐备，实测未标定**。拖动 55fps 是 1080p 带宽模型推算；GPU 化（第三层）落地后按 S406 口径应达 144Hz 能力。
- 稳定性工程本身是强项：panic 四环节序列器 + 四级复位阶梯、关机四相账本、断电百次 100/100（QEMU）、九类崩溃恢复矩阵、合成器 3 秒恢复、防自锁闸门——宿主侧已锁定回归。
- 但"跑所有大型软件极其稳定"目前做不到：Win32 GUI 面 20/27 Stub、Wine 未入镜像、Linux 侧未经 LTP。大型软件稳定运行依赖兼容层深水区 + GPU 渲染 + 实机烤机三件事全部落地。

## 六、4K UI 全面更新到内核

- 当前分辨率：GOP 直通三档（800×600 / 1280×720 / 1920×1080），无运行期热切换；单窗 32MiB 缓冲覆盖 2560×1440×4。
- **4K 支持目前主要在判据与规范层**：C-7 4K 资产管线、F636 DPI 自适配契约（1x 素材升采样须标"增强渲染"）、`compatstar2/dpistate.rs`（应用高 DPI 感知三态）、`moneum.rs` 多显示器前瞻接口。
- **把 4K 真正落到内核需要的动作**：① GOP/显示栈升 3840×2160 双缓冲（4MiB 帧上限解除，需走全屏直写或扩大 PMM 块——4K×4B≈33MB/帧）；② atlas 字形图集按 4K 原生重制 + CJK 资产补齐；③ designsys/aurora 素材全量按 3840×2160 出图 + CI 校验；④ 四档 DPI 缩放器实装；⑤ 实机 4K 外接走查（MD1 阶段 2 目标，尚无完成证据）。

## 七、稳定性/构建现状与欠账清单

**能构建、能跑：** `cd kernel && cargo +1.97.1 test` → 3662 全绿；`cargo check --target x86_64-unknown-none --features kernel-image` 全过；varix.iso / varix-uefi.img / QEMU 截图证据齐全。注意 `build-windows.bat` 与 CHANGELOG.md 属于 Variable 应用，不是内核。

**未完成项（文档如实登记）：**
- 实机欠账总清单：交接对练十次、LTP 实机跑批、72h 烤机、断电百次实机、六件套真人走查、桌面空闲 5% 与拖动手感实口径复测。
- 20 维度验收 13✅/3◐/4⏳，⏳ 全是实机硬项（交互/细节/兼容环境/整体感受）。
- 工程债：main.rs `boot()` 近 700 行待收拢、2425 条 clippy 存量告警、31 条 rustc 警告基线、CJK 字形缺口。

## 八、建议的下一步优先级

1. **B1 vxbench 实机基线**（一切性能数字的前提，铁律不绿不开工）
2. **S406 GPU 摸底**：UHD 630 上验证 Mesa iris → 再立项 Vulkan 化合成器（80fps 的关键路径）
3. **4K 双缓冲帧管线**改造（解除 4MiB 帧限制）+ 四档 DPI 缩放器实装
4. **Win32 兼容从 27 API 首层扩到"常用 50 件"实测**，GDI/USER 从 Stub 走向真渲染
5. 实机欠账清偿：72h 烤机、断电百次、LTP 跑批
