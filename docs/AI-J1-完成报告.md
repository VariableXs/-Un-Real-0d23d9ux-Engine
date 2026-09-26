# AI-J1 完成报告 · J 鼠标域·一分队（F601-F620）

> **分工包**：AI-J1 = F601-F620（20 项 · 目标 15,660 行上限口径），判据摘自主册《Varix STAR I start.md》第 8 部分 J-A/J-B。生成于本批次收口日；**深化批次二已并入（v2 对账）**；**深化批次三已并入（v3 对账，2026-09-26）——跨窗口运行时接线批次**。
> **收口状态**：20/20 项功能落地 + **119 项单测全绿**（j1 63 + j1deep 23 + j1v3 33）+ 面板/运行时/持久化/遥测/证据/动作路由六链齐全；八窗口运行时覆盖 + 动作路由中心真实接线；行数对账如实呈报（§3 v3）；实机类判据按双轨制登记「随闸门补测」（§5）。

## §1 交付物台账（v3 增量置顶）

### v3 批次新增/重构（2026-09-26 · 跨窗口接线批次）

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

## §3 行数对账（v3 · 跨窗口接线批次并入后）

- **域功能行现值：4,844 行**（23 个功能文件非空非注释行，同一过滤口径；计数脚本归档 `_attic/aij1-f601-f620/count-domain-total.sh`），较 v2 收口径（同脚本回测 HEAD=4,169）新增 **675 行**，另有跨域接线 +30 行（App/SettingsModal/ExplorerWindow/三入口）。
- **目标上限口径：15,660 行。达成率 30.9%——仍未达「≥90%」铁律，如实呈报不掩盖。**
- v3 批次的功能构成（全部为判据内真实深化，零注水）：
  - **八窗口运行时覆盖**（v2 缺口工单第 1 条）：desktop 全量层 + app-write/mind/code/fate 窗层 + explorer/taskbar/datavault headless——滚轮刻度/自适应增益/穿透/侧键/手势/慢速微调/自动滚在软件窗口与系统窗口真实生效，不再只活在桌面窗；
  - **动作路由中心**（缺口工单第 2 条）：`vx-j1-action` 从「派发后无人消费」变为有登记表、有两级路由、有扩展点、有未处理显性化的真实设施；17 内置动作全登记；Tauri 窗口管理/编辑三兄弟/声明式 nav.up 内建接线；
  - **作用域声明接线**：`data-app-id`/`data-app-class` 此前全系统无人声明——F605 应用覆盖、F616 应用档案、F615 侧键作用域的解析链永远落默认档；v3 在八个入口声明，解析链真实生效；
  - **F616 管线消费**（此前 trackCurrentApp 挂了档案但管线不消费——参数从未真正变化）：指针增益每次移动经 `effectiveParamsFor()` 取「设备档案×应用档案」合成值；
  - **F614 首交互建档**：first-pointerdown 建档 + 气泡回调（notifyOnClone 尊重）——「插入即自动挂载」在本域诚实环境（webview 不暴露 VID-PID）的落地形态；
  - **F606 真实倾斜路径**：纯横向 wheel 事件按倾斜档语义横滚（此前只有 Shift+滚轮等效入口）；
  - **F604 滚轮打断**：锚标接管期间滚轮输入即退出（Windows 语义补全）；
  - **F607/F613 Tauri 多屏缓存刷新**：availableMonitors 物理像素虚拟桌面坐标 + visibilitychange 刷新 + 失败单屏降级——多屏真机上护边与落点记忆的真实数据源；
  - **F610 --hover-delay 通道**：tooltip 旋钮偏离 500ms 基线时接管全局悬停延迟令牌（tooltip.css 真实消费），回基线即还政（AI-18 M-71 默认零迁移）；
  - **F602 blur 修饰键复位**（切窗后粘 Shift 不再拖住指针）、**deltaMode 归一化**（行/页/像素统一到行语义）、**画中实时墨迹**（F617 识别前跟手轨迹）；
  - **接线审计面板**：挂载身份/动作路由表/F609 容器数/F610 变量通道的只读快照——接线状态不让「已实现」停留在注释里。
- **副作用级修复**：v2 J1Runtime 的 useEffect 依赖 replica——每次指针移动重挂全部 window 监听器（每秒可达百次 add/removeEventListener）；v3 内核一次性挂载，listener 生命周期与组件无关。
- **剩余缺口定性**：距 90% 口径还差约 9,100 行。真实构成：① 每项判据的实机证据生产（录屏/4K 四档走查/断电注入——实机日集中产出，注行数无意义）；② explorer 标签页历史/视图刷新等跨域消费者的动作接线（属 F089/F090 域内组件，需该域协同）；③ v1/v2/v3 均未铺开的边界空态 UI 纵深。**建议处置：继续拆深化批次（每批 +3~5k 行）并推进跨域协同接线，或由 Variable 裁定上限口径修订。继续注行数就是注水，我不做。**

### 版本对账轨迹

| 版本 | 域功能行 | 单测 | 达成率 | 备注 |
| --- | --- | --- | --- | --- |
| v1 | 2,360* | 63 | 15.1% | 20/20 判据逻辑全落地 |
| v2 | 3,967* | 86 | 25.3% | +惯性/录制/打包/遥测/证据/面板纵深 |
| v3（本版） | 4,844* | 119 | 30.9% | +八窗运行时/动作路由/作用域接线/F616 消费/F610 修正（同一过滤口径回测 HEAD=4,169，净增 675） |

*注：v1/v2 行数为当批次报告自报口径；v3 起统一使用归档脚本口径（v2 收口回测 4,169），跨版本可比性以本表为准。

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
# 单测（119 项：j1.spec 63 + j1deep.spec 23 + j1v3.spec 33）
npx vitest run src/features/mouse/__tests__
# 类型检查（本域零错误；仓库既有错误见 §4-16——他人进行中未跟踪文件）
npx tsc --noEmit
# 手工走查入口：设置中心 → 「鼠标」标签页（mouseJ1），十个纵深子面板折叠展开
#   「运行时接线审计」子面板 = 八窗挂载身份/动作路由表/F609 容器/F610 变量通道快照
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

---

*AI-J1 · J 鼠标域一分队收口（v3）。判据全落地，八窗接线，数字不注水，缺口摆在台面上。*
