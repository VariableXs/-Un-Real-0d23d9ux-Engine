# ASCENT-60 — 实施步骤（从批次到验收的完整施工手册）

**版本**：`1.1ascent.vxe`
**姊妹篇**：[`ASCENT-60-功能全景.md`](ASCENT-60-功能全景.md)（做什么）、[`ASCENT-60-AI分工图.md`](ASCENT-60-AI分工图.md)（谁来做）。
**前置阅读**：本文假设读者已读功能全景的 60 项定义与附 A 依赖矩阵。

---

## 0. 施工总纪律（全批次共同遵守）

### 0.1 验证基线（每个批次收口前必须全绿）

```powershell
npm run typecheck          # tsc --noEmit 零错误
npm run test               # vitest 全绿（禁止跳过失败用例）
node tools/audit.cjs       # i18n 完整性 + 裸色值 + 无障碍段（按批次逐步启用）
cargo check                # Rust 侧零告警
cargo test --no-run        # 测试可编译（lib check 绿 ≠ test 可编译，教训在案）
cargo test                 # 其他 AI 域的失败用例如实标注归属，不越界修
```

### 0.2 共享文件守则（血泪教训成文）

| 共享文件 | 守则 |
| --- | --- |
| `src/lib/ipc.ts` / `src-tauri/src/ipc.rs` / `src-tauri/src/lib.rs` | 改前 `rg` 核实当前状态，最小化 diff，一次只加自己的段 |
| `src/i18n/dictionaries.ts` | zh 块曾多次被并发回滚：改前 `rg` 核键、改后立即复验，提交信息注明新增键清单 |
| `src/system/windows/vwm.ts` | 新窗口类注册分批提交，注册表段保持追加式 |
| `src-tauri/src/shell/taskman.rs` / `kbdhook.rs` / `desktop.css` / `StartMenu.tsx` | 历史冲突高发区，动前先 git log 看对方最近提交 |

### 0.3 批次（AB = Ascent Batch）编号与总排布

| 批次 | 主题 | 覆盖 | 依赖 |
| --- | --- | --- | --- |
| AB-0 | 地基与令牌 2.0 | U-07 | 无 |
| AB-1 | 启动剧场（P0 心脏） | U-01…U-06 | AB-0 |
| AB-2 | 视觉语言落地 | U-08、U-09、U-10 | AB-0 |
| AB-3 | 交互与布局 | U-11、U-12、U-55、U-56 | AB-2 |
| AB-4 | 桌面工作流 | U-13、U-14、U-15、U-17 | AB-0 |
| AB-5 | 文件管理与数据面板 | U-16、U-25、U-26、U-27 | AB-4 |
| AB-6 | 传输与存档 | U-22、U-28、U-29、U-30 | AB-5 |
| AB-7 | 性能与可靠 | U-19、U-20、U-21、U-23、U-24 | AB-1 |
| AB-8 | 安全与隐私 | U-31…U-36 | AB-0 |
| AB-9 | 开放生态 | U-37…U-42 | AB-3 |
| AB-10 | 硬件协同 | U-43…U-48 | AB-7 |
| AB-11 | 氛围与品质 | U-49…U-54、U-57…U-59 | AB-3、AB-4 |
| AB-12 | 收口与门禁 | U-60 + 全域验收 | 全部 |

并行波次：`AB-0 → [AB-1 ‖ AB-4 ‖ AB-8] → [AB-2 ‖ AB-5 ‖ AB-7] → [AB-3 ‖ AB-6 ‖ AB-9 ‖ AB-10] → AB-11 → AB-12`。

---

## 1. AB-0：地基与令牌 2.0（U-07）

**目标**：令牌体系升级到位，为全部视觉工作铺轨。

**步骤**：
1. `src/design/tokens.css` 扩展五组令牌（排版比例 / 间距全档 / 圆角三档 / 遮罩三档 / 噪声参数），每组的语义命名先在 `docs/DESIGN.md` 增补词条再落 CSS；
2. `tools/audit.cjs` 升级：裸色值计数器从 info-only 改为「阈值门禁」模式（初始阈值 = 当前实测值，每批收紧，最终 0）；新增「组件层引用原始变量层」的 lint 规则（先 warning，AB-3 转 error）；
3. 存量迁移第一批（低风险文件）：`tools.css`、`boot.css`（即将被 AB-1 重写，只迁不改）、`vwm.css` —— 每文件独立 commit，`rg '#[0-9a-fA-F]{3,8}|rgba?\(' <file>` 复核为零；
4. 三主题（dark/light/high-contrast）全量走查 `src/features/settings/`，对照度不足处只调 token 值不动组件。

**验收**：
- [ ] `node tools/audit.cjs` 以门禁模式运行，迁移文件零裸色值；
- [ ] 三主题切换截图（visual-audit 15 路由）与基线 diff <0.5%；
- [ ] DESIGN.md 新词条与 tokens.css 一一对应（人工核对清单入 PR 描述）。

**回滚**：令牌扩展纯增量，revert 单 commit 即可；audit 阈值改动独立 commit，可单独回退。

---

## 2. AB-1：启动剧场（U-01…U-06）★关键路径

**目标**：3A 级开机仪式上线，本计划最高优先级。

### 阶段 1.1 字形资产（U-02，先行）
1. 建 `src/assets/wordmark/`：8 字母 SVG + `glyphs.ts` 路径表导出（路径数据 + 实测 `getTotalLength()` 常量表）；
2. 单测 `src/assets/wordmark/glyphs.test.ts`：锁定每条路径长度（路径被误改即 fail）+ viewBox 合法性；
3. 装配组件 `src/system/boot/BootWordmark.tsx`：props = { scale, tone, progress }，progress 驱动逐字母 dashoffset（沿用 12.5%/字母 区间映射）。

### 阶段 1.2 胶囊与流带（U-03、U-04）
1. `CapsuleBar.tsx`：纯展示组件，props = { progress, breathing }；CSS 走 stadium 圆角（radius=height/2）+ 白填充 + 流光头；满格呼吸由 progress===1 触发一次性 class；
2. `FileTicker.tsx`：消费现有 `LoadEventPayload` 流，内部 useMemo 派生速度/剩余项/剩余时间（3s 滑动窗口均值）；行切换动画 transform-only；
3. `boot.css` 重写：从零新文件，只允许引用语义 token；emoji 全量替换为 `src/components/icons/` 的线性 SVG（此批先内置 6 枚最小集，全量等 AB-2 的 U-10）。

### 阶段 1.3 编排与整合（U-01、U-06）
1. `BootScreen.tsx` 重构：保留全部对外契约（onDone/onExitStart/onStats、boot_replay、boot://event、跳过机制），内部换成 `entering → streaming → readyHold → exiting` 状态机（纯函数 reducer + 单测四剧本：快机/慢机/跳过/回放）；
2. `settings.ts` 增 `bootPacing: cinematic|brisk|instant`（sanitize 三值），`DEFAULT_SETTINGS` 默认 cinematic；i18n 增 pacing* 键（zh/en，audit 过）；
3. 入场/离场编排全部 CSS 化（keyframes + 延迟），reduce-motion 媒体查询整段覆盖为 ≤80ms 淡入淡出。

### 阶段 1.4 启动交响（U-05）
1. `sounds.ts` 增 `bootSwell/bootPulse/bootReady` 三合成函数 + 进度调度器（监听剧场状态机而非自建定时器）；
2. `bootSoundMode: full|mute|chime-only` 设置项 + SettingsModal 声音页一行三选；
3. 音频设备热插拔容错（devicechange 监听，失败静默不抛）。

**验收（除功能全景 U-01…U-06 各自验收项外）**：
- [ ] 人为注入事件序列（vitest mock boot_replay）跑四剧本，状态机转移与快照全部符合预期；
- [ ] 真机（或 WebView2 环境）冷启动录屏逐帧检查：入场编排 900ms 内完成、无闪白、无布局跳动；
- [ ] 全程 emoji 扫描：`rg "[\x{1F300}-\x{1FAFF}]" src/system/boot/` 为空；
- [ ] 1440p + 200% 缩放下字标/胶囊/流带三者的比例与 100% 缩放一致（token 驱动，无 px 硬编码字标尺寸）。

**回滚**：旧 BootScreen 逻辑在 git 历史（重构前打 tag `pre-boot-cinema`），整批可 revert；期间 `bootAnim=none` 用户始终有直通逃生门。

---

## 3. AB-2：视觉语言落地（U-08、U-09、U-10）

**步骤**：
1. **材质引擎（U-08）**：tokens.css 材质三档段 + `src/styles/material.css`；`bgTier` 联动：低档全局 solid；DEBUG 环境变量 `VARIABLE_DEBUG_MATERIAL=1` 时显示同屏 frosted 计数徽标；快捷面板/通知中心两处先行试点 frosted；
2. **光影系统（U-09）**：`--elev-1/2/3` 令牌 + 交互态语义（hover 抬升/active 下压/拖拽投影）；`desktop.css`、`vwm.css`、`tools.css` 的既有阴影声明分三 commit 迁移（每文件迁移后截图回归）；
3. **图标语言（U-10）**：`src/components/icons/` + `src/lib/iconRegistry.ts` 语义字典；动效图标八件套（transform-only，reduce-motion 一键停）；registry 单测扫描全部 tsx 的 lucide import 做语义冲突检测。

**验收**：
- [ ] 同屏 5 个 frosted 面板自动降级 3 个（DEBUG 徽标如实显示）；
- [ ] 抽查 10 组件三态光影全部来自令牌（rg 验证零裸 box-shadow 十六进制）；
- [ ] registry 冲突单测在故意注入冲突时 fail（自测单测有效性）。

---

## 4. AB-3：交互与布局（U-11、U-12、U-55、U-56）

**步骤**：
1. **微交互精修（U-11）**：`src/styles/interactions.css` 六类控件四态规范；各控件文件删除自制 transition（`rg 'transition:' src/system src/components` 逐个归口）；危险操作红色内闪 + error 音接入（删除/焚毁/清空回收站三处先行）；
2. **响应式重构（U-12）**：`src/lib/breakpoints.ts`（matchMedia 三档上下文）+ tokens.css 容器查询段；任务栏/开始菜单/设置三处先行；`tools/screenshot-route.cjs` 增 1280/2560 两档宽度基线；
3. **空状态（U-55）**：`EmptyState.tsx` 通用组件 + 首批 12 处接入（清单见功能全景）；i18n 全覆盖；
4. **错误叙事（U-56）**：`src/lib/errors.ts` 词典（首批 20 个高频错误码 zh/en）+ ErrorBoundary、传输、嵌入三站点接入；重试退避按钮组件化。

**验收**：
- [ ] 六控件 × 四态视觉回归截图通过；位移/缩放超限扫描脚本（临时 tools 脚本）零告警；
- [ ] 1280/1920/2560/3840 四档截图回归通过；
- [ ] 人为触发 10 类错误全部人话化（手动演练清单入 PR）。

---

## 5. AB-4：桌面工作流（U-13、U-14、U-15、U-17）

**步骤**：
1. **桌面 Profiles（U-13）**：`src/system/desktop/profiles.ts` 数据模型（壁纸/主题/图标布局/任务栏/窗口集引用）；切换服务 + 卡片 UI；与 DRESS_PRESETS 打通（预设=Profile 的一个切面）；
2. **智能吸附 2.0（U-14）**：`src/system/vwm/snap2.ts`（AI-2 域内）；幽灵预览独立图层；分区方案库（1/2、1/3、2+1、四象限、自定义 ≤3×3）；与既有 applySnap 合并重构，snap 音效复用；
3. **任务栏进化（U-15）**：`Overflow.tsx`（30 窗压测）+ `JumpList.tsx`；IPC 新增 `taskbar.recent.register`（共享文件守则：ipc.ts/ipc.rs 最小段追加）；文件管理器外壳先注册最近打开项；
4. **拖放总线（U-17）**：`src/lib/dnd/bus.ts` + `DragGhost.tsx`；Drop 目标声明式 API；收藏托盘（屏幕边缘寄存）。

**验收**：
- [ ] Profile 切换后多显示器图标位置逐个还原（脚本断言）；
- [ ] 吸附幽灵预览与落位偏差 0px；3×3 自定义网格可用；
- [ ] 30 并发窗口任务栏零像素错位；Jump List <80ms；
- [ ] 文件→便签（若 AB-11 未到则用写作占位面板）、窗口→工作区两类跨域拖放通。

---

## 6. AB-5：文件管理与数据面板（U-16、U-25、U-26、U-27）

**步骤**：
1. **Explorer 2.0（U-16）**：Tabs/DualPane/ColumnView/RenameDialog 四组件 + shell 侧列视图批量读目录 IPC；>5000 项虚拟化滚动（与 AB-6 的 IO 治理目录虚拟化对齐接口，先做分页后接治理）；视图记忆按目录持久化；
2. **版本时光机（U-25）**：`shell/versions.rs`（内容寻址 + 增量块 + 保留策略 GC）+ 保存队列挂点（钩子级，不改进各应用保存逻辑）+ `src/features/versions/` 时间轴浏览器（双栏 diff）；
3. **全局标签（U-26）**：tags 表 + explorer/桌面图标标签角点 + 智能文件夹（保存查询）；
4. **回收站 2.0（U-27）**：来源记录 + 恢复预览 + 策略引擎（容量/时间阈值，星标豁免）。

**验收**：
- [ ] 万项目录滚动 60fps（bench 项入 AB-12 基线）；
- [ ] 1000 版本文件秒开；还原后 mtime/权限保留；GC 后校验全过；
- [ ] 标签随移动/重命名跟随（path key 监听单测）；
- [ ] 断电恢复测试：回收站 DB 与文件一致性通过（人为 kill 进程模拟）。

---

## 7. AB-6：传输与存档（U-22、U-28、U-29、U-30）

**步骤**：
1. **IO 治理（U-22）**：`shell/iogov.rs`（IoPriority 尽力而为设置 + 大文件分块管道 + 暂停/恢复 + 目录分页）；explorer 虚拟化接通；
2. **传输指挥台（U-28）**：`shell/transfer.rs` 队列中枢（并发 3、冲突四策略、失败重试面板）+ `src/features/transfer/` 前端（速度迷你曲线与启动剧场流带同源风格组件复用）；explorer/拖入/版本导出三来源全接入；
3. **存档柜（U-29）**：`shell/archive.rs`（zstd + SHA-256 清单 + 挂载只读浏览）+ 柜卡片墙 + 审计修复；
4. **数据血缘（U-30）**：lineage 表 + 三类事件采集（拖入/使用/离开）+ LineageWindow + 整体导出/焚毁。

**验收**：
- [ ] 索引扫描中复制 1GB 波动 <15%；暂停/恢复校验和一致；
- [ ] 10GB 归档→挂载→还原往返校验一致；翻转 1 字节被审计捕获；
- [ ] 拖入→编辑→导出全链路事件不漏；血缘焚毁后 DB 与磁盘残留为 0。

---

## 8. AB-7：性能与可靠（U-19、U-20、U-21、U-23、U-24）

**步骤**：
1. **启动流水线（U-19）**：加载任务 P0/P1/P2 分级（boot.rs 域）；mtime 快照预热缓存（checksum 防脏读）；boot_replay 事件加 phase 标记；剧场 ready 提前、P1/P2 转托盘微型胶囊；
2. **内存守护（U-20）**：`shell/memward.rs`（5s 采样 + 泄漏看门狗 + 软回收）+ IPC `mem.snapshot`；省内存模式（<10% 可用内存联动降档）；
3. **帧预算（U-21）**：`src/lib/framebudget.ts`（三档策略 + 动画成本账本 + 长任务 idle 切片）；星场/氛围光/通知入场三站点先行接入；
4. **崩溃叙事（U-23）**：ErrorBoundary 全量铺设（窗口/面板级）+ `shell/session.rs` 30s 会话快照与恢复卡片 + 安全模式（连续 2 次异常退出自动进入）；
5. **Bench 2.0（U-24）**：`tools/bench2.cjs` 六指标 + 预算断言；归档纪律沿用 docs/bench mtime 检查。

**验收**：
- [ ] 冷启动 ready 缩短 ≥40%（bench2 对比报表）；
- [ ] 注入泄漏 10 分钟被识别；软回收后 RSS 下降且功能正常；
- [ ] 万行列表+星场+通知三并发主线程 >50ms 长任务计数 0；
- [ ] 杀渲染进程重启会话还原率 ≥95%；连续异常后安全模式自动触发；
- [ ] bench2 连续 3 次空跑方差 <10%；人为 20% 回归被拦。

---

## 9. AB-8：安全与隐私（U-31…U-36）

**步骤**：
1. **隐私仪表盘（U-31）**：`shell/privacy.rs` 审计表（剪贴板/敏感区/出站三类）+ 仪表盘前端；暂停期显式标注「记录中断」；
2. **防火墙 2.0（U-32）**：netconsent.rs 扩展应用级档案（禁网/白名单/放行）+ 出站告警中心 + 日流量配额；
3. **诱饵文件（U-33）**：privacy.rs canary 段 + 模板生成器 + 触发警报链（托盘/仪表盘/可选锁屏）+ 白名单豁免；
4. **紧急擦拭（U-34）**：`shell/panic.rs` 三级协议 + kbdhook 长按 800ms 进度环；演练模式（dry-run 文件系统 diff 断言为零）；
5. **信任链（U-35）**：`shell/trust.rs` WinVerifyTrust 封装 + 登记卡签名状态 + 信任墙；
6. **隐身会话（U-36）**：session.rs 隐身层（内存 + 加密临时盘）+ explorer/剪贴板/通知三站点开关 + 四角标识 + 焚毁。

**验收**：
- [ ] 诱饵打开 <1s 警报；误报白名单生效；
- [ ] L2 真实触发后临时数据残留 0；演练模式磁盘 diff 为空；
- [ ] 四类签名样本验证全对；100 软件并发验证 <10s；
- [ ] 隐身会话 100 文件操作后扇区级扫描为 0（复用焚毁引擎的验证工具）。

---

## 10. AB-9：开放生态（U-37…U-42）

**步骤**：
1. **协议中枢（U-37）**：`shell/deeplink.rs` 路由 + 单实例转发；HKCU 注册仅便携部署态（退出退订）；环境内按钮逐步改走协议动词；
2. **脚本安全屋（U-38）**：`shell/script/safehouse.rs`（.vxsc 清单 + 能力授予 + 5s 配额）+ 脚本管理页 + 导入确认流（契约式逐项勾选）；
3. **资源包（U-39）**：`shell/pack.rs`（.vxs 导入/导出/部分应用/校验）+ 外观包管理页 + Profile 内嵌支持；
4. **无障碍 2.0（U-40）**：audit.cjs 无障碍段转 error；ARIA 分批补齐（StartMenu/Taskbar/Explorer/Settings 四大件优先）；色觉模拟器（设置内四滤镜）；
5. **i18n 中心（U-41）**：翻译工作台（双栏对照/缺失高亮/引用跳转，写回 dictionaries.ts）；伪本地化模式 + audit 扫描；RTL 试点两面板（逻辑属性迁移）；
6. **诊断导出（U-42）**：`shell/diag.rs`（六类内容 + 预览勾选 + 脱敏）+ 关于页入口。

**验收**：
- [ ] 浏览器 `variable://open` 唤起定位成功；未注册宿主友好降级；
- [ ] 越权脚本 100% 被拒并记录；死循环 5s 被杀环境无恙；
- [ ] .vxs 往返还原一致；畸形包拒绝且叙事清晰；
- [ ] audit 无障碍段 0 告警；纯键盘完成开窗→吸附全流程；
- [ ] 伪本地化 0 截断；RTL 两面板截图正确；
- [ ] 诊断包 0 绝对路径 0 口令字段（扫描断言入 diag 单测）。

---

## 11. AB-10：硬件协同（U-43…U-48）

**步骤**：
1. **多显示器 2.0（U-43）**：monitor.rs 显示器档案（壁纸/任务栏/布局记忆）+ 拓扑可视化编辑器 + 热插拔窗口迁移（VWM 布局引擎承接）；
2. **外设快捷层（U-44）**：`shell/periph.rs` + 宏录制编辑器（与 N-17 词表共享）+ 鼠标手势 8 向识别（轨迹采样本地实现 + 渐隐光痕）+ 媒体键接管（MediaControl 扩展）；
3. **音频路由（U-45）**：IAudioSessionEnumerator 每应用音量/路由/效果三档 + 峰值表 + 音频 HUD（4 段柱）；
4. **色彩与时辰（U-46）**：`shell/circadian.rs`（2800K–6500K 时段插值 + 手动覆盖浮条 + 退出还原）；
5. **无线中心（U-47）**：`shell/wireless.rs` BLE 枚举 + 电量预警（24h 防骚扰）+ 快速配对向导；
6. **性能模式（U-48）**：`src/lib/perfmode.ts` 三档上下文 + framebudget/iogov/预热三消费点 + 切换微光提示 + 本地智能建议（可忽略）。

**验收**：
- [ ] 双屏拔插 20 次窗口零丢失；混合 DPI 清晰度合格；
- [ ] 手势 100 采样准确率 ≥97%；宏冲突即时标红；
- [ ] 路由切换 <150ms 无爆音；峰值表与系统读数一致；
- [ ] 退出后宿主 gamma 字节级还原；过渡 ≤100K/分钟；
- [ ] 三类无线设备状态与系统一致；配对可取消无残留；
- [ ] 三档切换 10 次无漂移；静音档 GPU 降 ≥30%。

**注**：本批多项需真机（多屏/蓝牙/音频外设），无真机项按既有 DoD 口径**不勾选**，挂 docs/acceptance/ascent-硬件真机清单.md 待实机验收（沿用 B-30 矩阵诚实纪律）。

---

## 12. AB-11：氛围与品质（U-49…U-54、U-57…U-59）

**步骤**：
1. **音景引擎（U-49）**：`src/lib/soundscape.ts` 五场景程序化合成（叠加/渐变/睡眠定时/番茄联动）；
2. **焦点舱（U-50）**：`src/system/focus/`（舱体 + 通知静默队列 + 白名单穿透 + 专注账本）；
3. **通知交互（U-51）**：notifyStore 模型扩展（actions/progress/组）+ 通知中心升级 + 勿扰回放；
4. **声景反馈（U-52）**：sounds.ts 参数表化 + 三主题 + 深夜自动 ×0.5（醒目开关）；
5. **环境辉光（U-53）**：`glow.ts` 壁纸主色提取 + 内发光层（省内存联动关闭）；
6. **节律助手（U-54）**：`src/system/rhythm/` 四节律 + 仪表（MiniApp 承载，依赖 AB-4 未含的 U-18——**U-18 在本批补做**：`src/apps/mini/` 五件套 + vwm mini 窗口类）；
7. **引导体系（U-57）**：onboarding spotlight 5 站 + coach marks 注册表 + 技巧卡片；
8. **键盘全景（U-58）**：`src/lib/hotkeys/` 集中注册表 + Ctrl+/ 速查浮层 + 覆盖审计表清零；
9. **触控基础（U-59）**：tokens touch 变体 + 44px 目标 + 四手势 + 平板模式询问。

**验收**：
- [ ] 音景 1h 无内存增长；场景切换无爆音；
- [ ] 舱内通知全静默、出舱有序回放；账本可导出；
- [ ] 操作通知一次点击到位；100 条积压回放 <5s；
- [ ] 辉光 <0.5ms/帧；纯白壁纸不过曝；
- [ ] 五 MiniApp 冷启动合计 <400ms；便签持久化；
- [ ] 导览全程 Esc 可退；速查表与键位 100% 同源；纯键盘 10 分钟深度操作；
- [ ] 长按呼出 450ms±50ms；触屏无死按钮。

---

## 13. AB-12：收口与门禁（U-60 + 全域验收）

**步骤**：
1. **品质关卡（U-60）**：`.github/workflows/quality.yml` 五段流水线（tsc → vitest → audit 全段 → bench2 预算 → visual-audit 15 路由 × 3 主题矩阵）；main 全阻断 + 特性分支白名单机制；品质看板 markdown 报告；
2. **文档收口**：`docs/acceptance/ascent-验收矩阵.md`（60 项 × 三宿主，未实测不勾）；CHANGELOG 顶部新条目「ASCENT-60 收口」；README 能力声明表更新（含 D-5 诚实声明表的对应新增行）；
3. **发布物三件套**：`npm run build` + `tauri build`（NSIS）+ portable 构建，沿用 B-30 发版纪律；
4. **全域回归**：四关卡全绿 + bench2 基线归档 + 三主题截图矩阵入库。

**验收**：
- [ ] CI 四关卡 <8 分钟；注入 1px 漂移被捕获；
- [ ] 验收矩阵中每项勾选均有证据链接（测试/截图/录屏归档路径）；
- [ ] 版本号升至 `1.1.0`，代号 `1.1ascent.vxe`。

---

## 14. 风险登记表

| 风险 | 概率 | 缓解 |
| --- | --- | --- |
| 启动剧场与 U-19 流水线互相牵制（同改 boot 路径） | 高 | AB-1 先冻结 phase 协议字段（预留枚举），AB-7 只填充分级逻辑不改协议 |
| 字形资产设计反复导致 AB-1 返工 | 中 | 阶段 1.1 单独验收（路径长度单测锁定）后才进 1.2 |
| Explorer 2.0 与 IO 治理接口漂移 | 中 | AB-5 开工前先与 AB-6 负责路签「目录分页接口契约」（写进两批 PR 描述） |
| dictionaries.ts 并发回滚复发 | 高 | 每次 zh 块修改后立即 `rg` 复验 + commit message 附键清单（守则 0.2） |
| 硬件批次无真机 | 确定 | 诚实纪律：不实测不勾，挂真机清单，不伪造验收 |
| safehouse JS 引擎引入体积/安全面 | 中 | 评估独立轻量引擎 crate；若不可控则降级为「受限宏脚本」并如实缩小 U-38 范围 |
| Cargo.toml 无效 feature 名再次阻塞 | 低 | 新增依赖先 `cargo check` 单独验证再合入（教训在案） |

## 15. 完成定义（DoD，每批次）

1. 功能全景对应项的全部验收框勾选（或有真机豁免标注）；
2. 0.1 验证基线全绿；
3. audit.cjs 该批新增门禁段通过；
4. 共享文件改动经 `rg` 复验无回滚冲突；
5. PR 描述含：改动文件清单 / 新增 i18n 键清单 / 共享文件触碰声明 / 回滚方式；
6. 验收矩阵（AB-12 建，之前先记在批次 PR）对应行更新。

---

*批次执行顺序与并行波次见第 0.3 节；分工到 AI 路见姊妹篇 AI 分工图。*
