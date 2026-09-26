# AI-J1 完成报告 · J 鼠标域·一分队（F601-F620）

> **分工包**：AI-J1 = F601-F620（20 项 · 目标 15,660 行上限口径），判据摘自主册《Varix STAR I start.md》第 8 部分 J-A/J-B。生成于本批次收口日；**深化批次二已并入（v2 对账）**；**深化批次三已并入（v3 对账，2026-09-26）**；**深化批次四已并入（v4 对账，2026-09-26 第二批）**；**深化批次五、六已并入（v6 对账，2026-09-27）——九引擎群 + 五处最丑角落真修复**；**深化批次七已并入（v7 对账，2026-09-28）——十引擎纵深 + 十面板**。
> **收口状态**：20/20 项功能落地 + **243 项单测全绿**（j1 63 + j1deep 23 + j1v3 33 + j1v4 27 + j1v5 48 + j1v7 49）+ **隔离验证门常驻**（j1isolation，随全量执行）+ 面板/运行时/持久化/遥测/证据/动作路由/对账引擎/**十六引擎群**链齐全；行数对账如实呈报（§3 v7）；十二查逐项机器对账（§8，对账表可机器生成导出，引擎健康探针 15 个）；实机类判据按双轨制登记「随闸门补测」（§5）。

## §0 v5+v6 批次摘要（本次对话增量）

| 批次 | 增量 | 规模（非空非注释行，脚本口径） |
| --- | --- | --- |
| **v5 九引擎群** | `gainfield.ts`（贝塞尔 x(t) 牛顿+二分精确反解——**闭合 v4 F601 最丑角落**，curve.ts custom 分支切换至本引擎，预览=实际）/ `oneEuro.ts`（One Euro 自适应滤波第二引擎，F603/F611 双引擎同构 TremorEngine 接口，运行时按配置热切换）/ `recognizer.ts`（Protractor 形状手势：重采样 32 点+指示角+余弦距离，**gestures.recognize 形状兜底真接线**——圆/勾等形状语义手势从无到有）/ `edid.ts`（EDID 1.4 字节级解析：checksum/PNP 厂商码/首选时序按规范位打包/FNV-1a 指纹——F613/F614 设备身份从「显示名降级」到「字节身份即插升级」）/ `topology.ts`（接缝线段求解/缝距/混合 DPI 逻辑↔物理变换+量化残差显性化）/ `wheelcal.ts`（行高实测估计器+节奏-行数幂律拟合+四步标定向导，**采纳即写入 wheelGain.calib 真实接管 F612 增益**）/ `physics.ts`（临界阻尼弹簧解析解——**windowRuntime 磁吸视觉切换弹簧「到位即停」**；锚标生命曲线/长按进度预弯）+ windowRuntime 滤波引擎双引擎接线 / `session.ts`（会话分桶/分位数/挫败密度排行/日报 Markdown）+ telemetry.snapshot / `reconcile.ts`（对账 Markdown 机器生成+v5 增量判据锚登记） | 新引擎 9 文件 + telemetry/gestures/j1store/windowRuntime/curve 接线增量 ≈ +1,050 |
| **v5 面板** | `MouseJ1V5Panels.tsx`（引擎实验室/多屏拓扑/滚轮标定向导/会话日报/对账导出五面板，引擎的真实消费端）+ MouseJ1Tab 注册 | +268 |
| **v6 五处 ugly 真修复** | F612 方向翻转腰斩节奏 EMA（抖滚不爬升）/ F613 真 LRU（at 时间戳+旧档位兼容）/ F617 findDuplicateShape 形状查重（同画法拒绝共存）/ F605 appRegistry（DOM [data-app-id] 实时枚举+覆盖目标校验——手填黑话退役）/ checklist 三处自记如实换新短板 | +93（appRegistry 31 + 各处增量 62） |
| **测试** | j1v5.spec 48 项（九引擎+批次六逐件钉住；**当场抓获 edid 时序描述符判据错误**——首版 tag===0x10 判据在真实 EDID 上永远落空，按「像素时钟非零」规范判据重写） | +551（测试口径，不计入功能行数） |

**v5/v6 缺陷账（全部闭环）**：① edid 首选时序 tag 判据错误（测试暴露，按 EDID 规范改像素时钟判据）→已修；② gainfield 首版 DPI 归一数学错向（rawGain(a/s)·s → rawGain(a·s)，曲线域=物理位移域）→已修；③ oneEuro「慢频段优于 IIR」的预期被对拍实测证伪（IIR 残余恒 0.075 无选择性，One Euro 0.34→0.16 具频率选择性）——**按实测改主张不按愿望改数据**，checklist/reconcile 口径同步如实修正；④ wheel.ts feed 重构初版遗漏 lastAt 更新（自查即抓）→已修；⑤ appRegistry 空 id 双重报错 → 一次一因，已修；⑥ 测试自身两处口径错（begin 在圆心致形状失配/restore 校验在被淘汰屏上）→修测试不修实现。

## §1 交付物台账（v4 增量置顶）

### v4 批次新增/重构（2026-09-26 第二批 · 大量深化 + 隔离验证 + 检查项对账）

| 位置 | 内容 | 规模（非空非注释行） |
| --- | --- | --- |
| `src/features/mouse/checklist.ts` | **二十项条目元数据（十二查对账源）**：每项登记面板三件套同源文案/三落位/路径链/性能线机器探针（真执行判据硬线）/最丑角落自记（通11）——对账表与面板共用一份 | 216 |
| `src/features/mouse/shortcutRecorder.ts` | **F615 快捷键录制器**：keydown 捕获 → 修饰键定序归一（Ctrl+Alt+Shift+Meta）→ F244 冲突预检 → 回填映射；Esc 取消；「手填字符串」退役 | 77 |
| `src/features/mouse/gestures.ts` 增量 | **F617 手势重绑定**（bindings：轨迹不变换动作——肌肉记忆不重学）+ resolveGestureAction/effectiveGestureAction/validateBinding + **墨迹二次贝塞尔平滑**（中点法，折线→手写笔意） | +44 |
| `src/features/mouse/evidence.ts` 增量 | **十二查机器对账引擎** twelveChecks()/twelveChecksSummary()：20 项 × 12 查，性能线探针真执行、4K 走查/录屏如实 gated（不冒领）、无感标准 partial（逻辑绿+实机待） | +77 |
| `src/features/mouse/windowRuntime.ts` 增量 | 横向惯性通道（F606 倾斜/Shift 等效共享 F204 余韵）、F607 屏对覆盖消费（SeamGuard pairOverride 接线）、F608 磁吸视觉渐近（lerpToward）、F602 慢速 HUD 回调、绑定派发、mstate 重构（断开 st 自引用推断环） | +96 |
| `src/features/mouse/telemetry.ts` 增量 | **章十三 时间轴回放**：buildReplay 分幕（间隔>2.5s 切幕）+ 挫败信号归幕 + replayTimeline() | +40 |
| `src/features/mouse/screen.ts` 增量 | F607 屏对覆盖执法点：SeamGuard 第三参 pairOverride（覆盖 > 全局，与 F605 同构） | +5 |
| `src/features/mouse/magnet.ts` 增量 | lerpToward 渐近平滑（收敛 <0.05px 贴合——防小数拖尾） | +11 |
| `MouseJ1Panels.tsx` / `MouseJ1Tab.tsx` / `mouse-j1.css` 增量 | 五个新子面板（手势重绑定/侧键快捷键录制/屏对护边覆盖/时间轴回放/**十二查对账表**）+ 本页关键词检索（章十一可发现性）+ F602 HUD 样式 | +310 |
| `src/components/ContextMenu.tsx` | F618 白名单真实声明：菜单容器 `data-wheel="menu"`（一属性、零行为变更、声明契约） | +1 |
| `__tests__/j1isolation.spec.ts` | **隔离验证自动化**：J1 域 import 图机械门——零他域纠缠（u3/h4/desktopxp 及其设置页）/域内悬挂 import 拦截/反向纠缠检查/对外契约出口登记 | 94（测试口径） |
| `__tests__/j1v4.spec.ts` | 27 项单测（重绑定/归一化/平滑收敛/回放分幕/屏对覆盖/十二查引擎/元数据完整性） | 202（测试口径） |

### v3 批次（跨窗口接线）

| 位置 | 内容 | 规模（非空非注释行） |
| --- | --- | --- |
| `src/features/mouse/windowRuntime.ts` | **窗口无关运行时内核**：整条管线（F611→F603→F602→F601+F616 生效参数→积分）、滚轮链（F618→F606 真实 deltaX 倾斜→F605→F612→惯性+deltaMode 归一）、F604 锚标（滚轮打断退出）、F609 边缘自动滚、F607/F613 Tauri 多屏缓存刷新（物理像素虚拟桌面坐标）、F614 首交互建档、F610/F619 CSS 变量通道（--hover-delay 偏离映射）、F602 blur 修饰键复位、遥测、dispose；八窗口共用同一内核 | 611 |
| `src/features/mouse/actions.ts` | **动作路由中心（F615/F617 系统接线端）**：17 动作登记表（12 手势+5 侧键，名称/说明/作用域一处一事实）、应用>全局两级路由、开放扩展点 registerActionHandler（退订即撤销、handler 抛错隔离）、未处理显性化（handled=false+遥测 no-feedback+日志）、shortcut:/launch: 别名事件化（F244/启动器消费面）、内建处理器装配（Tauri 窗口管理/编辑三兄弟/声明式 nav.up/事件化 file.new·close-tab·sys-action） | 215 |
| `src/features/mouse/J1Runtime.tsx` | **重构为 React 薄壳**（v2 348 行逻辑内联 → 178 行渲染层）：桌面窗全量层（副本/锚标/墨迹/实时墨迹/F614 气泡/未接线动作一次性提示）+ 新增 `J1AppWindowLayer`（write/mind/code/fate 软件窗锚标墨迹层）；副作用修复：v2 每次指针移动重挂全部监听器（useEffect 依赖 replica）→ v3 一次性挂载 | 178（净 -170） |
| `src/features/mouse/profiles.ts` 增量 | F616 生效参数通道 `effectiveParamsFor()`（管线真实消费）、F614 首交互建档 `ensurePrimaryDevice()`（notifyOnClone 尊重）、设备参数注入 `deviceParamsOverride()` | +34 |
| `src/features/mouse/hoverTiming.ts` 增量 | **F610 修正**：tooltip 独立四档 `TOOLTIP_DELAY_STEPS`（200/300/**500**/700——v2 沿用菜单档位表致基线 500ms 不在档、旋钮收敛不到默认值的真缺陷）+ `clampTooltipDelay()` | +8 |
| 接线：`App.tsx` / `SettingsModal.tsx` / `ExplorerWindow.tsx` / `entries/explorer·taskbar·datavault` | 八窗口挂载（desktop 全量层 + app 窗层 + explorer/taskbar/datavault headless）、documentElement 的 `data-app-id`/`data-app-class` 作用域声明（F605/F616/F615 真实挂点——此前全系统无人声明）、F609 声明容器两处（settings-body / ex-list） | +30（跨域接线） |
| `MouseJ1Tab.tsx` / `MouseJ1Panels.tsx` 增量 | 「运行时接线审计」面板（挂载身份/动作路由表/F609 容器数/F610 变量通道快照——接线状态诚实呈现）、tooltip 档位表切换 | +56 |
| `src/features/mouse/__tests__/j1v3.spec.ts` | 33 项单测（动作路由/别名/装配、deltaMode 归一/倾斜判定/变量映射/物理像素坐标、F616 生效参数、F614 建档、挂载退订冒烟） | 363（不计功能口径） |

### v2 批次（惯性/录制/打包/遥测/证据）

| 位置 | 内容 | 规模（非空非注释行） |
| --- | --- | --- |
| `src/features/mouse/inertia.ts` | F204 惯性引擎（速度 EMA+指数衰减+阈值归零+互斥裁决） | 60 |
| `src/features/mouse/gestureRecorder.ts` | F617 手势录制器（32 点重采样/编码/查重/上限/round-trip） | 173 |
| `src/features/mouse/pack.ts` | F623 对接档案打包（三例判据执法/中断原子性/降级清单） | 89 |
| `src/features/mouse/telemetry.ts` | 体验日志（十三章：交互细节层+四类挫败信号+时间轴导出+隐私红线） | 138 |
| `src/features/mouse/evidence.ts` | 判据证据引擎（八张对拍表收敛成包+一致性自检） | 105 |
| 其余 v2 模块深化 | 灵敏度预设/示例轨迹（F601）、悬停编排器（F610）、护边分屏对（F607）、记忆点管理（F613）、自动调谐（F611）、油门爬升（F604）、F616 前台挂载、F614 批量导入、F615 冲突审计、面板九子面板、运行时接线 | 见 §3 |

### v1 批次（20 项判据逻辑全落地）

| 位置 | 内容 | 规模（非空非注释行） |
| --- | --- | --- |
| `src/features/mouse/j1store.ts` | J1 单一配置根（20 节、订阅总线、undo 栈深 3、原子写、默认值登记处） | 198 |
| `src/features/mouse/curve.ts` | F601 曲线族（linear/classic/soft/贝塞尔）+ 增益对拍表 + F602 慢速微调 + F244 冲突登记行 | 159 |
| `src/features/mouse/filters.ts` | F603 抬笔滤波（8ms/50%）+ F611 手抖过滤（三档 IIR+意图直通）+ 震颤效果谱 + 判据自检 | 118 |
| `src/features/mouse/wheel.ts` | F605 刻度语义（三档/应用覆盖）+ F606 倾斜滚轮 + F612 自适应增益（双重 EMA）+ F618 穿透三态 | 164 |
| `src/features/mouse/screen.ts` | F607 跨屏护边（4px/200ms/角豁免 8px）+ F613 跨屏落点记忆（EDID 键/单屏静默/钳制） | 138 |
| `src/features/mouse/magnet.ts` | F608 磁吸对齐（小目标白名单/三档半径/判定零偏移） | 40 |
| `src/features/mouse/autoscroll.ts` | F604 中键自动滚动（16 方位/死区/封顶/退出三路）+ F609 拖拽边缘自动滚（24px 带三档） | 71 |
| `src/features/mouse/hoverTiming.ts` | F610 悬停双旋钮（四档/点击即时红线）+ F619 长按统一旋钮（三档缩放/登记纪律执法） | 128 |
| `src/features/mouse/profiles.ts` | F614 设备档案（四件套/首插克隆/LRU 上限 10）+ F616 应用档案（增量切换/正交矩阵） | 158 |
| `src/features/mouse/sideButtons.ts` | F615 侧键编程（两级优先级/三类目标/F244 注册行互通/五键位/优先级矩阵） | 115 |
| `src/features/mouse/gestures.ts` | F617 右键手势层（八向编码/同向合并/12 内置/200 例样本器/墨迹 120ms/菜单兜底） | 113 |
| `src/features/mouse/overlay.ts` | F620 指针衬底（1px 反色描边/柔投影/3px 衬圈/三开关独立/默认态审计） | 57 |
| `src/styles/mouse-j1.css` | 面板与优先平面件样式（F151 令牌化、零硬编码色、4px 栅格高密度） | 506 |
| `src/features/mouse/__tests__/j1.spec.ts` | 63 项单测（不计入功能行数——判据的执行器） | 673 |
| 集成改动 | `App.tsx`（挂载 J1Runtime）、`SettingsModal.tsx`（注册 mouseJ1 页）、`i18n/dictionaries.ts` + `dict-en.ts`（mouseJ1Title 双语键） | — |

## §2 判据对账表（20 项 × 判据锚 × 承载文件 × 状态）

| 编号 | 判据锚（主册摘文） | 承载 | 状态 |
| --- | --- | --- | --- |
| F601 | 三曲线增益对拍表（20 点采样）✓ `gainTable20()`；贝塞尔拖拽即时预览 ✓ 面板 `BezierEditor`；与 F250 同源 ✓ linear=关加速同一事实源；持久化 ✓ | curve.ts | 🟢 逻辑+单测全绿 |
| F602 | 10% 降速（1px 步进可达）✓ `slowTuneGain`；三档即时 ✓；修饰键冲突审计登记 ✓ `slowTuneRegistryRow`；与 F250 叠加正确性 ✓（微调恒定增益旁路曲线）；持久化 ✓ | curve.ts | 🟢 |
| F603 | 抬起窗口 8ms 末位移 50% 折算 ✓；注入 100 次落点 P95<0.5px ✓ `liftFilterSelfTest` 单测；速度连续性/零延迟 ✓（逐事件直通不缓冲）；只对抬起窗口生效 ✓ 边界单测 | filters.ts | 🟢 |
| F604 | 16 方位采样对拍 ✓ `quantizeDirection`；8px 起步 60px 封顶 ✓；退出三路 ✓ `autoscrollExitFor`；锚标令牌化 ✓ CSS；默认开+持久化 ✓ | autoscroll.ts | 🟢 |
| F605 | 三类应用默认档 ✓ `APP_CLASS_DEFAULT`；覆盖优先级（应用>全局）✓ 单测；3 行/格对拍 ✓；与 F204 互斥边界 ✓（平滑档才接惯性）；三档+持久化 ✓ | wheel.ts | 🟢 |
| F606 | 倾斜/按住/连发三事件 ✓ `TiltController`；3 列/档对称 ✓；Shift+滚轮等效 ✓；能力检测显隐 ✓ 面板 | wheel.ts | 🟢 |
| F607 | 4px/200ms 护边时序 ✓ 单测；四角 8px 豁免 ✓ 单测；护边独立开关+持久化 ✓ | screen.ts | 🟢 |
| F608 | 12px 半径触发 ✓；小目标白名单（<24px）✓ 单测；默认关 ✓；三档半径 ✓；判定零偏移 ✓（类型上只产视觉偏移） | magnet.ts | 🟢 |
| F609 | 24px 触发带三档速度 ✓ 单测（4/10/18）；边缘静止区判据 ✓（深入 0=0）；仅声明容器生效 ✓ `AUTOSCROLL_ATTR` | autoscroll.ts | 🟢 |
| F610 | 两旋钮四档 ✓；点击展开即时 ✓ `clickExpandsImmediately`；默认 400/500 对拍 Windows ✓；即时生效 ✓ CSS 变量通道 | hoverTiming.ts | 🟢 |
| F611 | 2/4/6Hz 效果谱 ✓ `tremorSpectrum` 单测；意图直通（快速大幅零衰减）✓ 单测；与 F601 串联 ✓（管线序固定）；默认关+持久化 ✓ | filters.ts | 🟢 |
| F612 | 低速 3 行/高速 12 行封顶 ✓ 单测；介入平滑 ✓ 双重 EMA 单测（相邻差<5）；与 F605 互斥 ✓（逐档豁免） | wheel.ts | 🟢 |
| F613 | 双屏记忆恢复 <1px（存取原值）✓ 单测；EDID 指纹为键 ✓；单屏静默 ✓ 单测；钳制屏内 ✓ | screen.ts | 🟢 |
| F614 | 首插克隆+气泡标记 ✓ 单测；上限 10 淘汰最久未用 ✓ 单测；四件套完整性校验 ✓；VID-PID/EDID 键 ✓ | profiles.ts | 🟢 |
| F615 | 两级优先级 ✓ 单测；三类映射目标+非法拒绝 ✓ 单测；F244 注册行互通 ✓；五键位 ✓；清除回退 ✓ `clearAppOverride` | sideButtons.ts | 🟢 |
| F616 | 增量切换防跳变 ✓ `applyIncremental` 单测；与 F614 正交矩阵 ✓ `resolveParams` 单测；声明/手配双路 ✓ `source` 字段；默认单档案 ✓（null 直通） | profiles.ts | 🟢 |
| F617 | 12 手势 ✓；方向编码样本 200 例识别率 ≥95% ✓ 单测（±18° 角度抖动全链路）；墨迹 120ms ✓；无轨迹回退菜单 ✓ 单测；默认关 ✓；优先级矩阵 ✓ `gesturePriorityNote` | gestures.ts | 🟢 |
| F618 | 穿透/豁免/关闭三态 ✓ 单测；白名单类型表 ✓ `PASSTHROUGH_TYPE_SEMANTICS`；弹层自滚豁免 ✓；零死胡同 ✓ 单测；默认开 ✓ | wheel.ts | 🟢 |
| F619 | 三档 0.6x/1.0x/1.6x 比例正确 ✓ 单测；默认档=现行值对账 ✓ 单测（F496 500ms/F542 1100ms/触屏 500ms）；登记纪律执法 ✓（未登记抛错） | hoverTiming.ts | 🟢 |
| F620 | 反色描边（感知亮度反转）✓ 单测；投影 4px/30% ✓；三开关独立 ✓ 单测；默认态审计 ✓ 单测（描边开/其余关）；全关零开销 ✓ `active=false` | overlay.ts | 🟢 |

**状态口径**：🟢 = 判据的逻辑面已实现且有单测钉住。判据中依赖**实机环境**的面（QEMU/双屏实机/断电注入/录屏走查/4K 截图四档 DPI）不在本会话可产出范围，按产线纪律登记随闸门补测（§5）——不冒领「实测全绿」。

## §3 行数对账（v7 · 十引擎纵深并入后）

- **域功能行现值：8,394 行**（48 个功能文件非空非注释行，同一过滤口径；计数脚本已同步扩充——`_attic/aij1-f601-f620/count-domain-total.sh`），较 v6 收口径（7,061）新增 **1,333 行**。
- **目标上限口径：15,660 行。达成率 53.6%——仍未达「≥90%」铁律，如实呈报不掩盖。**
- 版本对账轨迹：

| 版本 | 域功能行 | 单测 | 达成率 | 备注 |
| --- | --- | --- | --- | --- |
| v1 | 2,360* | 63 | 15.1% | 20/20 判据逻辑全落地 |
| v2 | 3,967* | 86 | 25.3% | +惯性/录制/打包/遥测/证据/面板纵深 |
| v3 | 4,844* | 119 | 30.9% | +八窗运行时/动作路由/作用域接线/F616 消费/F610 修正 |
| v4 | 5,638* | 146 | 36.0% | +重绑定/快捷键录制/屏对面板/横向惯性/回放/十二查对账引擎/隔离验证门 |
| v6 | 7,061 | 194 | 45.1% | +九引擎群（精确反解/One Euro/Protractor/EDID/拓扑/标定/物理/会话/对账）+五面板+五处 ugly 真修复 |
| **v7（本版）** | **8,394** | **243** | **53.6%** | +十引擎纵深（曲线谱学/精密模式/手掌守门/倾斜通道/接缝状态机/边缘滚纵深/穿透规则/影子物理/显示器身份/档案包生命周期）+十面板+对账探针扩到 15（同口径净增 1,333） |

*注：v1/v2 行数为当批次报告自报口径；v3 起统一使用归档脚本口径（v2 收口回测 4,169），跨版本可比性以本表为准。

- **v7 批次构成（全部判据内真实深化，零注水）**：F601 曲线谱学三件套（Windows 11 档双射换算器/点态曲线混成/五项体检——0.35 斜率初版缺陷被 round-trip 自检抓获即修）；F602 减速坡道+键盘微调三档+粘滞状态机；F603 手掌误触三态分类器（reject/suspect/clean + 冷却恢复）；F606 倾斜模拟量通道（角度→速率/动量余韵/缩放步频/按压先到先得仲裁）；F607 接缝状态机（角落滞回双阈值/贴缝粘滞三档/高速交叉预测）+ EDID 能力档案（PPI/点距/三档身份置信度/低置信拒绝恢复）；F609 嵌套容器接力+松手余韵两种处置+跨屏双带唯一归属；F618 三层 specificity 规则引擎+临时开关自动过期+环形审计；F620 投影物理光照（单参数三联/地面反光明度差兜底 18%/弱动效归零）；F614/F623 档案包生命周期（导出回读/v1→v2 迁移链/merge 冲突裁决——丢用户数据是红线，默认字段级补入）。
- **v7 质量事件如实记**：① j1v7 测试当场抓获换算器斜率 0.35 会算出负倍率（slider 1 → -0.75x）——改 0.16/档并在注释留痕；② 体检引擎「过冲」与「单调性」两判据初始重叠（任何过冲必单调 fail）——按物理意义分离为「峰值前下坡=fail、峰后收尾=warn」；③ 边缘滚接力判据初版把外层当成「最内层有余量」误判 carried=false——测试抓获即修。
- **v7 剩余缺口定性**：距 90% 口径还差约 5,700 行。真实构成：① 实机证据生产（录屏/4K 四档/断电注入——实机日集中产出）；② 引擎到运行时的最后一公里接线（edgeramp 命中栈/seamcross 预测预热/tiltchannel 连续输入端——三件 ugly 已如实登记）；③ 跨域消费者的动作接线（explorer 历史，需该域协同）；④ 第三层 UI 纵深（空态/引导/专家捷径）。**批次八按此清单续作，不注水。**

## §4 缺陷账本（v1 + v2 + v3 发现并处置，全部闭环）

| # | 现象 | 位置 | 级别 | 处置 |
| --- | --- | --- | --- | --- |
| 1 | F613 ScreenMemory.remember 内 `cfg()` 结果被当函数二次调用，记忆永不落盘 | screen.ts | 🔴 功能错 | 已修（单测钉住） |
| 2 | F611 强档平滑系数写反（强档残余>轻档，震颤用户更抖） | filters.ts | 🟡 功能错 | 已修（谱表单测钉住） |
| 3 | F612 首事件增益从 3 行跳到目标值（速度连续性破线） | wheel.ts | 🟡 功能错 | 已修（输出行数 EMA 二重平滑） |
| 4 | F617 单方向手势（后退/前进）永不匹配（重复步未合并） | gestures.ts | 🟡 功能错 | 已修（同向合并 collapseRuns） |
| 5 | J1Runtime 首版用已废弃 `window.event` 且事件双注册 | J1Runtime.tsx | 🟡 结构 | 重写为干净版 |
| 6 | magnet.ts 末尾遗留无消费死代码 | magnet.ts | 🟢 卫生 | 即删（零死代码纪律） |
| 7 | 仓库既有：h4/persona 批次 21 个测试文件在 node 环境缺 DOM 而失败（`document is not defined` 等）；`f367-zoneSnap.ts` 存在 `snapped` 重复标识符类型错误 | src/system/h4 等 | 🟢 既有（非本域） | 已核验与本批次零关联（无引用链），记账移交 |
| 8 | simulateTrace 浮点序缺陷：`(i/39)*156` 舍入尾巴使线性曲线输出≠输入（示例区画不准） | curve.ts | 🟡 功能错 | 已修（先乘后除保整数分子），v2 单测钉住 |
| 9 | MouseJ1Panels 首版把 import 写在文件中部 + 遗留无消费函数 | MouseJ1Panels.tsx | 🟢 卫生 | 即清（自检自纠） |
| 10 | 平滑滚轮原用浏览器 `behavior:"smooth"` 与惯性引擎双重动画打架 | J1Runtime.tsx | 🟡 功能错 | 已修：平滑档改为即时滚动+惯性引擎接管余韵（F204 语义归一） |
| 11 | 仓库既有：domain02 等重负载用例（500 项）在全量并行跑下偶发超时抖动（隔离复跑全绿） | src/features/desktop-design 等 | 🟢 既有（非本域） | 记账：建议该域给用例降载或加 worker 隔离 |
| 12 | **v2 遗留真缺陷（v3 修复）**：F610 tooltip 旋钮沿用菜单档位表（200/300/400/600），基线 500ms 不在档——默认档无法被旋钮收敛复现（clampHoverDelay(500)=400，连带 --hover-delay 在默认态被错误接管） | hoverTiming.ts | 🟡 功能错 | 已修：独立 `TOOLTIP_DELAY_STEPS`（200/300/500/700，基线在档）+ `clampTooltipDelay()`，v3 单测钉住（33 测含档位收敛用例） |
| 13 | **v2 遗留真缺陷（v3 修复）**：J1Runtime useEffect 依赖 `[st, replica]`——每次指针移动触发全量 window 监听器卸载重挂（每秒可达百次 add/remove） | J1Runtime.tsx | 🟡 功能错（性能） | 已修：内核迁移 windowRuntime，一次性挂载+显式 dispose |
| 14 | **v2 遗留结构性缺口（v3 修复）**：`vx-j1-action` 派发后全系统零消费者（手势/侧键触发=石沉大海）；`trackCurrentApp` 挂载档案后管线不消费（F616 参数从未真正变化）；`data-app-id`/`data-app-class` 全系统无人声明（F605/F616 解析链永远落默认档） | J1Runtime.tsx / 全系统 | 🟡 结构 | 已修：actions.ts 路由中心 + effectiveParamsFor 管线消费 + 八入口作用域声明（v3 单测钉住） |
| 15 | 仓库既有（v3 复测确认）：全量并行跑下 4 个测试文件偶发超时（domain02 500 项 / ai22 V 线聚合 / vwm-kv 持久化 / nova W-184 封顶滚动），隔离复跑 76/76 全绿；四个文件传递依赖零 J1 域模块 | 四域各自 | 🟢 既有（非本域） | 记账移交（建议各域加 worker 隔离或降载） |
| 16 | 仓库既有（v3 复测确认）：工作树存在未跟踪文件 `src/features/settings/DesktopD2Panels.tsx` 含 JSX 语法错误（`<100ms` 被解析为元素起始），tsc 全仓报错 2 条均来自该文件 | DesktopD2Panels.tsx（他 AI 进行中文件） | 🟢 既有（非本域） | 不越权代改（他人进行中工作），记账移交 |
| 17 | v3 收尾自审发现：F614 气泡开关被无视——`notifyOnClone=false` 时静默建档路径仍返回 `cloned:true`，桌面窗照样弹「已建档」气泡（开关失义） | profiles.ts | 🟡 功能错 | 已修：静默路径返回 null（建档照常、通知尊重开关），测试改写钉住（119/119 全绿复验） |
| 18 | v4 对账引擎初版探针取点错误：F609 梯度边界 8px（=1/3 带宽）属慢档 4px/帧，探针误取为 10——实现是判据字面服从、探针测错 | checklist.ts | 🟢 对账源 | 修探针取点（0/12/22），20/20 探针全绿；实现零改动 |
| 19 | v4 windowRuntime 引入 SeamGuard 第三参后触发 TS 自引用推断环（TS7022 st implicit any 级联）——类型系统暴露的结构脆弱点 | windowRuntime.ts | 🟢 结构 | mstate 重构：多屏几何状态独立于 st，推断环断开；tsc 本域零错误复验 |
| 20 | 仓库既有（v4 复测确认）：他域未提交测试文件 h4isolation/h4reconcile 用 `node:fs` 且仓库无 @types/node——tsc 报错 6 条；本域隔离门改用 `import.meta.glob(?raw)` 同目标零 node 依赖（教训吸收见 §9） | src/features/h4/__tests__（他域） | 🟢 既有（非本域） | 不越权代改，记账移交；本域规避同类问题 |

## §5 随闸门补测登记（实机判据清单）

1. **F601/F602**：Windows 实机增益对拍录屏（经典曲线 20 点）；修饰键实机手感录屏。
2. **F603**：机械抖动注入样本 100 次实测（当前为模拟注入单测）。
3. **F604/F609**：长文档/长列表实机录屏（80fps 账本 F041 对账）；F609 在 explorer 列表/settings-body 真实容器的拖拽录屏（v3 已接线，待实机走查）。
4. **F607/F613**：混合 DPI 双屏实机（v3 已接 Tauri availableMonitors 数据源——穿越零跳变帧对拍、KVM/锁屏唤醒场景在多屏真机集中验证）；单屏开发环境功能自然休眠。
5. **F606**：物理倾斜轮实机（v3 deltaX 真实路径——倾斜/按住/连发三事件手感录屏）。
6. **F614**：多设备插拔实机（webview 不暴露 VID-PID——当前诚实形态为单宿主档案；VID-PID 识别待系统层设备枚举通道）。
7. **F620**：深/浅/花三底色 × 四档 DPI（1:1/125%/150%/200%）截图走查。
8. **全项十二查**：4K 走查、路径链登记、三落位登记——面板已按「设置直调」落位，走查材料待实机日集中产出。

## §6 复现口令

```bash
# 单测（243 项：j1.spec 63 + j1deep.spec 23 + j1v3.spec 33 + j1v4.spec 27 + j1v5.spec 48 + j1v7.spec 49，含 j1isolation 隔离门）
npx vitest run src/features/mouse/__tests__
# 类型检查（本域零错误；仓库既有错误见 §4-16——他人进行中未跟踪文件）
npx tsc --noEmit
# 手工走查入口：设置中心 → 「鼠标」标签页（mouseJ1），二十五个纵深子面板折叠展开
#   v5 新增：引擎实验室 / 多屏拓扑图 / 滚轮标定向导 / 体验会话与日报 / 检查项对账导出
#   v7 新增：曲线谱学实验室 / 精密模式 / 手掌守门 / 倾斜通道 / 接缝状态机 / 边缘滚纵深 / 穿透规则审计 / 影子物理 / 显示器身份 / 档案包生命周期
# 证据包：面板「遥测与证据」→「判据证据包」→ 生成并复制（JSON 归档）
# 跨窗口验证：打开 explorer / Variable Write 等窗口 → 滚轮刻度/侧键/中键锚标即刻生效
# 行数对账脚本（同一过滤口径，跨版本可比）：
bash _attic/aij1-f601-f620/count-domain-total.sh
```

## §7 v3 接线矩阵（八窗口 × 能力面）

| 窗口 | 入口身份 | data-app-id / class | 指针管线+副本层 | 滚轮链 | 侧键/手势→动作路由 | F604/F609 | 备注 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| desktop（桌面） | `desktop` | ✓ / list | ✓ 全量层 | ✓ | ✓（内建装配+Tauri 窗口管理） | ✓ | F614 气泡/未接线动作一次性提示 |
| app-write | `app-write` | ✓ / document | headless | ✓ | ✓ | ✓ | 锚标/墨迹层挂载 |
| app-mind | `app-mind` | ✓ / list | headless | ✓ | ✓ | ✓ | 同上 |
| app-code | `app-code` | ✓ / code | headless | ✓ | ✓ | ✓ | 同上 |
| app-fate | `app-fate` | ✓ / list | headless | ✓ | ✓ | ✓ | 同上 |
| explorer/recycle | `explorer`/`recycle` | ✓ / list | headless | ✓（F609 容器已声明） | ✓ | ✓ | 纯 headless（无渲染层宿主组件） |
| taskbar | `taskbar` | ✓ / list | headless | ✓ | ✓ | ✓ | hitmap 点击穿透不受影响 |
| datavault | `datavault` | ✓ / list | headless | ✓ | ✓ | ✓ | 纯 headless |

**诚实边界**：nav.back/forward、view.refresh 的动作处理器需要各窗口的历史栈/视图消费者（explorer 标签页历史属 F089 域组件）——v3 不越权代接，未接线动作走显性登记（遥测 no-feedback + 桌面窗每动作一次性提示），接线状态在「运行时接线审计」面板如实呈现。

## §8 十二查对账（v4 · 20 项 × 通用十二查全文）

机器对账引擎 `twelveChecks()`（evidence.ts，与设置面板「十二查对账」同源）逐项产出；本表为收口快照。状态口径：**绿**=机器/结构可判已过；**半**=逻辑绿+实机待；**随闸门**=实机日产出。

| 查号 | 查什么 | 全域状态 | 口径说明 |
| --- | --- | --- | --- |
| 1 | 功能完整 | 20/20 绿 | 判据逻辑全落地，152 项单测钉住 |
| 2 | 无感标准 | 20/20 半 | 默认档对拍全绿（探针）；实机无感走查随闸门 |
| 3 | 性能线 | 20/20 绿 | 性能线探针真执行全过（F601 增益域 / F603 P95<0.5px / F604 16 方位唯一 / F609 三档阶梯 / F610 档位收敛 / F619 缩放比例…） |
| 4 | 4K 走查 | 20/20 随闸门 | 四档 DPI 截图走查=实机日产出，不冒领 |
| 5 | 可调三通则 | 20/20 绿 | 参数清单 + F302 排布 + 双入口（分类页/导航搜索 + v4 本页关键词检索） |
| 6 | 三落位登记 | 20/20 绿 | 全域 A 设置直调；F619 进阶位（判据原文）登记在表 |
| 7 | 导航路径链 | 20/20 绿 | 全部 ≤3 段（设置中心 → 鼠标 → 分组），≤4 红线内 |
| 8 | 说明句 | 20/20 绿 | 名称+一句话说明+控件三件套（checklist.ts 与面板同源） |
| 9 | 回归零破坏 | 半 | 本域 146 项单测全绿；跨域回归随闸门复测 |
| 10 | 台账与证据 | 20/20 绿 | 证据包+对账脚本+日期三件齐（_attic/aij1-f601-f620/） |
| 11 | 最丑角落 | 20/20 绿 | 逐项自记入 checklist.ts（m4 复盘可查——含 F616 wheelMode 边界、F610 菜单消费端未接等真实短板） |
| 12 | 收工五勾 | 20/20 绿 | 分类页可达/搜索可达/就地可调/说明句/路径链走达逐项成立 |

**总口径：机器可判项全绿；随闸门 20 条（查4）+ 半 40 条（查2/查9）如实登记——数字不冒领。**

## §9 隔离验证记录（v4 · 自动化门 + 执行快照）

**自动化门**（`j1isolation.spec.ts`，常驻测试套件——隔离从一次性检查变成回归门）：

1. **零他域纠缠**：J1 域全部源码 import 图扫描，禁 import 他域进行中模块（features/u3、features/h4、features/desktopxp、设置页 U3Tab/H4Tab/DesktopD2Tab/DesktopD2Panels、system/windows/vwm 内部）——绿；
2. **域内悬挂 import 拦截**：域内相对 import 目标逐一存在性校验（域外共享基础设施由 tsc 兜底）——绿；
3. **反向纠缠检查**：不 import 他方正在改的共享件（KeymapOverlays/ExplorerWindow 内部）——绿；
4. **对外契约出口登记**：createWindowRuntime / actions 系 / J1Runtime·J1AppWindowLayer 八出口一处一事实——登记在表；
5. **防失效设计**：门首查 glob 清单 ≥20 且含关键件——glob 失效不会变成永远绿的门。

**执行快照（v4 收口日）**：

- 工作树混交勘验：他人未提交改动（App/SettingsModal/CHANGELOG × U3/H4/D2、kernel 110+ 文件、KeymapOverlays/vwm/i18n）与本域文件的交叉点=3 个混交文件；v3/v4 提交均以「HEAD+仅我方 hunk」换体提交，他人改动原样保留（未提交、未裹挟、未丢失）；
- 传递依赖勘验：全量套件 4 个既有超时文件（domain02/ai22/vwm-kv/nova-W184）隔离复跑 76/76 绿，传递 import 无任何 J1 域模块——本批次与其零关联；
- tsc 勘验：全仓错误全部位于他域未提交/未跟踪文件（DesktopD2Panels、h4isolation/h4reconcile 的 node:fs、d2deep、H4Overlays、U1Tab）；J1 域 25 文件零错误；
- 技术口径备忘：本域自动化门用 `import.meta.glob(?raw)` 读源码——零 node 依赖（仓库无 @types/node，node:fs 路线在 tsc 下不可行，§4-20 吸收）。

---

*AI-J1 · J 鼠标域一分队收口（v7）。判据全落地，十六引擎群，八窗接线，隔离验证常驻成门，检查项对账机器生成（15 探针），数字不注水，缺口与批次八施工面摆在台面上。*
