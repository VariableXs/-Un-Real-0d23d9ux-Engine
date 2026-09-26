# Varix STAR I start · src 视觉与功能设计全量清点报告（一次找完）

> 清点范围：src 全目录（1,424 个 TS/TSX/CSS 文件）×《Varix STAR I start.md》五域 F001–F640 ×《AI 分工完成图》21 分队 × kernel/varix 现有 22 个 star 系列模块
> 结论先行：**640 项功能的"逻辑本体"大头已在内核（22 个 star 模块），"视觉呈现层"确实被写进了 src**——这正是需要移植回内核的部分。下表按分队逐一列出 src 位置、内容、内核落点、移植状态。

---

## 一、总账：21 分队的 src↔内核 对应关系全图

| 分队 | F 段 | 域 | src 落点（视觉/装配层） | kernel 落点（逻辑本体） | 内核侧现状 | 待移植内容 |
|---|---|---|---|---|---|---|
| AI-C1 | F001-F020 | A 兼容前段 | src/features/compat/（16 文件） | compatstar/（21 件） | ✅ 已在内核 | 兼容层 UI 卡片（诚实提示卡/加载占位窗）视觉 |
| AI-C2 | F021-F040 | A 兼容后段 | src/features/compat/ | compatstar2/（22 件 + deep/deep2/deep3） | ✅ 已在内核 | 安装器进度视觉、卸载清扫报告 UI |
| AI-K1 | F041-F057 | B 性能前段 | src/system/perf/（13） | perfstar/（12 件 mech_*，19,114 行） | ✅ 已在内核 | 性能仪表盘图表视觉（曲线/热力图） |
| AI-K2 | F058-F075 | B 性能后段 | src/system/perf/ | perfstar2/（18 域） | ✅ 已在内核 | 同上 + 电池/热管理面板视觉 |
| AI-D1 | F076-F092 | C 桌面前段 | src/features/desktopxp/（13）+ src/system/explorer | deskstar/（八模块 + 51 块聚合） | ✅ 已在内核 | 资源管理器窗格实绘（树/列表/面包屑） |
| AI-D2 | F093-F110 | C 桌面后段 | src/features/desktop-design/ + src/system/tools/ + desktopxp | deskstar/ | ✅ 已在内核 | 设计中心/工具页 UI 实绘 |
| AI-V1 | F111-F130 | I 服务前段 | （无独立 src 域，经 U3 装配） | svstar/（11,729 行，批次九） | ✅ 已在内核 | 服务守护面板/专注模式浮层视觉 |
| AI-V2 | F131-F150 | I 服务后段 | src/features/ecosystem/（16） | stareco/（+deep/） | ✅ 已在内核 | 星图前端/评级徽章视觉 |
| AI-D2b | F076-F110 视觉 | C 桌面视觉 | src/features/desktop-design/（17）| —（src 独有） | ❌ **纯 src** | **整域移植**：设计中心/仪式覆盖层/剧场覆盖层/粒子引擎 |
| AI-E1 | F151-F170 | E 个性化 | src/system/theme-studio/（7）+ src/system/persona/（93）| —（src 独有） | ❌ **纯 src** | **整域移植**：主题工坊/人格化系统/定制包 |
| AI-H1 | F201-F250 | H 体验一 | src/features/（借力 uikit） | h1star/（在制品 82 项红） | ⚠️ 在制品 | 其视觉件依赖令牌/控件件（F151/F216 已就位） |
| AI-H2 | F251-F300 | H 体验二 | （经 uikit/组件） | h2star/（25,894 行，51.3%） | ✅ 已在内核 | 引擎对应的设置窗格视觉 |
| AI-H3 | F301-F350 | H 体验三 | src/features/（搜可达/分类页） | h3star/（六件功能核 + pwbtn） | ✅ 已在内核 | 三问队列/导航路径链 UI |
| AI-H4 | F351-F400 | H 体验四 | src/features/h4/（22）+ src/system/h4/（53） | （活契约在 src） | ⚠️ 半在 src | **任务中心/启动徽章/活动时间线视觉移植** |
| AI-S1 | F171-F185 | 安全前段 | src/features/security/（29） | secstar/（15 模块） | ✅ 已在内核 | 安全中心仪表/双签徽标视觉 |
| AI-S2 | F186-F200 | 安全后段 | src/features/security/ | secstar2/ | ✅ 已在内核 | 同上 |
| AI-U1 | F401-F450 | I 通用一 | src/features/u1/（14） | uni1/（17 模块 v6） | ✅ 已在内核 | U1Tab 活体件实绘 |
| AI-U2 | F451-F500 | I 通用二 | （经 U3 装配） | genstar2/ | ✅ 已在内核 | 判据账本可视化 |
| AI-U3 | F501-F550 | I 通用三 | **src/features/u3/（61 文件，28,739 行，61.8%）** | deskstar/ 挂接（despaint/expui/lockmount） | ✅ 引擎+装配双在 | **最大单域**：桌面实绘/资源管理器装配/锁屏横幅/复制队列/时钟面板 |
| AI-U4 | F551-F600 | I 通用四 | （经 U3Lab 挂接） | istar/（16,242 行，50/50 深化） | ✅ 已在内核 | 拖影徽标/涟漪/飞掠等动效视觉 |
| AI-J1 | F601-F620 | J 鼠标一 | src/features/mouse/（53）+ jstar 先例 | jstar2/ | ✅ 已在内核 | 鼠标手感可视化（轨迹/手势预览） |
| AI-J2 | F621-F640 | J 鼠标二 | src/features/mouse/ | jstar2/ | ✅ 已在内核 | 同上 |

**总量口径**：src 1,424 文件 ≈ 46,000+ 行（含 CSS/测试）；内核 star 系列已约 20+ 万行。**需要"搬进内核"的不是逻辑（逻辑已在内核），而是 src 的视觉呈现层**——这正是前几轮 NOVA 移植的延续。

---

## 二、视觉领域逐域清点（你点名的每一类，全在这）

### 1. 开头动画（开机/启动视觉）
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| src/system/boot/BootScreen.tsx | 开机屏总装 | novaboot.rs（designsys） | ✅ 状态机/节奏/字标/胶囊条已移植 |
| src/system/boot/ceremony.ts | 五阶段状态机 + pacing 三档 + 30% skip | novaboot.rs `ceremony_reducer` | ✅ 1:1 全绿 |
| src/system/boot/BootWordmark.tsx | VARIABLE 八字母字标（12.5% 区间） | novaboot.rs `letter_progress_milli` | ✅ |
| src/system/boot/CapsuleBar.tsx | 胶囊进度条（扫光/光点/满格呼吸） | novaboot.rs 常量组 | ✅ |
| src/system/boot/BootAtmosphere.tsx | 开机氛围（环境光） | 待移植 → novaboot 批二 | ⏳ |
| src/system/boot/FileTicker.tsx | 文件名滚动条 | 待移植 → novaboot 批二 | ⏳ |
| src/system/boot/theater/ | 开机剧场 | 待移植 → novaboot 批二 | ⏳ |
| src/features/oobe/（11） | 首次开机引导（OOBE） | 待移植 → deskstar 新件 | ⏳ |
| src/features/onboarding/（4） | 二次引导 | 同上 | ⏳ |

### 2. 界面设计（桌面/窗口/浮层骨架）
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| src/App.tsx + src/system/desktop/DesktopShell.tsx | 桌面壳总装 | deskstar/mod.rs | ⏳ 实绘接线 |
| src/components/（12 件：TitleBar/WindowControls/Modal/ToastHost/ContextMenu/CloseLight…） | 窗口基础件 | deskstar + compositor/wtree | ⏳ |
| src/system/windows/（35） | 窗口系统视觉 | compositor/wtree + deskstar | ⏳ |
| src/system/vwm/ | VWM 窗口管理器前端 | compositor | ⏳ |
| src/system/taskbar/（16） | 任务栏（含 IME 指示/媒体控制/便签） | deskstar/taskbar 系 | ⏳ 实绘 |
| src/system/startmenu/（10） | 开始菜单（分组/文件夹/索引栏/每日摘要） | deskstar/deskmenu 系 | ⏳ 实绘 |
| src/system/explorer/（6）+ src/features/files/（17） | 资源管理器 | deskstar/tabexplorer + U3 expui | ⏳ 实绘 |
| src/system/lockscreen/（4） | 锁屏 | deskstar/lockmount | ⏳ |
| src/system/tray/（5） | 托盘 | deskstar/notifctr 系 | ⏳ |
| src/system/widgets/（10） | 桌面小组件 | deskstar | ⏳ |
| src/system/notify/ | 通知中心 | deskstar/notifctr + notifchain | ⏳ |
| src/features/u3/（61 文件 28.7k 行） | U3 全部活体件（DeskPaint/ExplorerPane/ClockPanel/CopyQueue…） | U3 engines 已在内核，**活体件实绘待接** | ⏳ 最大单件 |

### 3. UI 设计（控件/组件库）
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| src/design/tokens.css | 全部设计令牌 | **nova4k.rs** | ✅ 全绿 |
| src/features/uikit/（30） | 控件基类（F216/F436 依赖件） | designsys 待新件 novaui.rs | ⏳ |
| src/components/icons/ | 图标组件 | **novaassets.rs**（包模型）+ atlas | ✅ 模型/⏳ 图集 |
| src/system/iconpacks/（6） | 图标包系统 | **novaassets.rs** | ✅ 模型全绿 |
| Z-02 控件规格（按钮 32/菜单 36/触控 44） | 按钮设计规格 | **nova4k.rs** CTL_* | ✅ |
| src/features/mouse/（53） | 鼠标手感（J 域视觉） | jstar2 + 待实绘 | ⏳ |
| src/features/inputFeel/（13） | 输入手感 | deskstar/candwin 系 | ⏳ |

### 4. 布局设计（栅格/几何/密度）
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| src/system/desktop-design/ai11-icons.ts | 图标栅格 25 档实测几何 | **novaassets.rs** GRID_* | ✅ 五档全绿 |
| src/system/desktop-icons/（15） | 桌面图标引擎 | deskstar/icongrid | ⏳ 接线 |
| src/system/desktop-design/ai12-wallpaper.ts | 壁纸引擎 25 档 + 取色 25 项 | **novaassets.rs** WallpaperEngine | ✅ 目录全绿 |
| src/system/wallpaper/（11） | 壁纸引擎本体 | display/wallpaper.rs + galaxy | ⏳ 接线 |
| src/system/desktop-design/ai13-widgets.ts | 小组件布局 | deskstar 待新件 | ⏳ |
| src/system/scene/（6） | 场景布局 | 待移植 | ⏳ |

### 5. 设置界面设计
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| src/features/settings/（53） | 设置中心全部窗格（五查/三落位/路径链） | deskstar/quickset + settings 新件 | ⏳ |
| src/system/h4/（53，f369 任务中心/f371 启动徽章/f372 活动时间线/f373 键盘布局） | H4 设置页群 | deskstar 待新件 | ⏳ |
| src/system/persona/（93） | 人格化/个性化全部档位 | E 域 → designsys 待新件 novapersona.rs | ⏳ |
| src/system/theme-studio/（7） | 主题工坊 | E 域 → novatheme.rs | ⏳ |
| src/system/ambience/（17） | 氛围系统（色温/时段） | svstar（F116 色温已在） | ⏳ 视觉接线 |
| src/features/ambience/（11） | 氛围前端 | 同上 | ⏳ |

### 6. 按钮设计
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| tokens.css Z-02（--ctl-btn 32/minw 120/menu 36/touch 44） | 按钮全部规格 | **nova4k.rs** | ✅ |
| tokens.css Z-05/U-09（焦点环三样式/hover 抬升/按压下压） | 按钮状态规范 | **nova4k.rs** | ✅ |
| src/features/uikit/ 按钮件 | 按钮实绘 | novaui.rs 待新件 | ⏳ |
| src/system/h3star 相关 pwbtn | 电源按钮 | h3star/pwbtn（内核已在） | ✅ |

### 7. 功能设计（非视觉但属五域）
| src 位置 | 内容 | 内核落点 | 状态 |
|---|---|---|---|
| src/features/compat/（16） | 兼容域功能面 | compatstar/2 | ✅ 逻辑/⏳ UI |
| src/features/security/（29） | 安全功能面 | secstar/2 | ✅ 逻辑/⏳ UI |
| src/features/hardware/（17） | 硬件管理面 | drivers/ + drvframe | ✅ 逻辑/⏳ UI |
| src/features/sound/（16） | 声音功能面 | audio/ + fdmix | ✅ 逻辑/⏳ UI |
| src/features/vision/（11） | 视觉辅助（a11y） | a11y/ + a11ygate | ✅ 逻辑/⏳ UI |
| src/features/a11y-l10n/（22） | 无障碍与国际化 | a11y + font/i18n 系 | ⏳ |
| src/features/ecosystem/（16） | 生态开放域 | stareco | ✅ 逻辑/⏳ UI |
| src/features/tools/（16）+ src/system/tools/（27） | 工具集 | 各 star 域 | ✅ 逻辑/⏳ UI |
| src/apps/（code/fate/mind/mini/write） | 五原生应用 | editor/fileman/galaxy 系 | ⏳ 实绘 |

---

## 三、已完成的移植（前三轮成果，全部独立验证全绿）

| 内核模块 | 覆盖 | 验证 |
|---|---|---|
| designsys/nova4k.rs | tokens.css 全量令牌 + 三主题 OKLCH→sRGB + 4K dp 引擎 | 4 单测 + 26 检查全绿 |
| designsys/novaassets.rs | 图标包/vicon 校验/栅格密度/壁纸引擎目录/4K 资产管线红线 | 3 单测 + 31 检查全绿 |
| designsys/novaboot.rs | 开机仪式状态机 1:1 + 节奏档 + 字标 + 胶囊条 | 4 单测 + 21 检查全绿 |

## 四、剩余移植路线（按依赖序）

1. **novaui.rs**：uikit 30 件控件基类 → 内核实绘原语（按钮/菜单/对话框/复选/输入）——F216/F436 依赖件，解锁一切界面；
2. **deskstar 实绘接线**：taskbar/startmenu/explorer/lockscreen 用 nova4k 令牌 + novaassets 图集真画上屏（经 compositor）；
3. **novapersona/novatheme**：E 域 93+7 文件的个性化档位表；
4. **U3 活体件实绘**：28.7k 行装配层逐件接 deskstar；
5. **GPU 合成**：virtio-gpu 驱动 + 显存 blit，4K@80fps 上屏（判据升级 55→80fps）。
