# SETTINGS — 设置手册（自动生成）

> 自动生成：`node tools/gen-settings-doc.cjs`（源：`src/lib/settings.ts`）。手工修改会被下次生成覆盖。
> 新增设置项不改文档，下次发版自动出现在本手册；默认值与代码实测一致（生成时直接读取 DEFAULT_SETTINGS）。
> 键与数据开放导出（Z-51）/ 配置分享（Z-54）字段一一对应。

共 74 个设置项。

| 键 | 类型 | 默认值 | 说明 | 所有者 | Since |
|---|---|---|---|---|---|
| `language` | `Lang` | `"zh"` | — | — | 1.5xw |
| `theme` | `ThemeId` | `"deep-space"` | — | — | 1.5xw |
| `wallpaperMode` | `WallpaperMode` | `"gravity"` | 桌面壁纸模式（桌面环境 L0 显示层；与四软件内部主题互不影响）。 | — | 1.5xw |
| `bootAnim` | `BootAnim` | `"full"` | 启动动画过渡形式（真实加载完成后的阶段4/5 编排开关）。 | — | 1.5xw |
| `iconSize` | `IconSize` | `48` | 桌面图标大小三档（32/48/64）。 | — | 1.5xw |
| `winControls` | `WinControls` | `"mac"` | 窗口控制按钮位置（桌面红绿灯 + 软件窗口控制条，规格 4.3.5）。 | — | 1.5xw |
| `taskbarPos` | `TaskbarPos` | `"bottom"` | 任务栏停靠位置（批次E，规格 4.4）。 | — | 1.5xw |
| `runIndicator` | `"dot" \| "underline" \| "capsule"` | `"dot"` | AI-03 V-18：运行指示样式三选（dot=Win11 圆点 / underline=Win10 下划线 / capsule=胶囊）。 | AI-03 | 1.5xw |
| `mediaBreath` | `boolean` | `false` | AI-03 M-16：媒体呼吸（播放时时钟旁 2% 幅度 / 4s 周期；默认关；reduce-motion 自动停用）。 | AI-03 | 1.5xw |
| `clockZones` | `string[]` | `[]` | AI-03 M-12：时钟多时区（IANA 名，最多 3 个）。 | AI-03 | 1.5xw |
| `shortcutBinds` | `Record<string, string>` | `{}` | 快捷键自定义（批次E，规格 4.7）：action → accel；空 = 全默认。冲突检测在前端设置页。 | — | 1.5xw |
| `xBinds` | `{ xbutton1: string; xbutton2: string }` | `{ xbutton1: "back", xbutton2: "forward" }` | M-30 鼠标侧键编程：XBUTTON1/2 → 动作 id（默认 back/forward 语义）。 | — | 1.5xw |
| `launchApps` | `Record<string, string>` | `{}` | M-31 启动槽应用归属：launch1..launch9 → appKey（空 = 未分配）。 | — | 1.5xw |
| `keycast` | `boolean` | `false` | M-33 按键回显开关（只回显功能组合，打字内容永不出现）。 | — | 1.5xw |
| `keycastCorner` | `"tl" \| "tr" \| "bl" \| "br"` | `"br"` | M-33 按键回显位置（四角可选）。 | — | 1.5xw |
| `wheelVolume` | `boolean` | `false` | M-35 任务栏滚轮调音量（默认关；开启后任务栏滚轮 = 音量步进 + 浮标）。 | — | 1.5xw |
| `commandHintBar` | `boolean` | `true` | Z-13 命令提示条开关（默认开；容器高度 > 600px 才显示）。 | — | 1.5xw |
| `keymapProfile` | `string` | `"default"` | Z-14 当前键位方案 id。 | — | 1.5xw |
| `keyStats` | `Record<string, number>` | `{}` | M-28 键位使用统计（本地，只记 action id + 次数，隐私口径见 keymap/telemetry.ts）。 | — | 1.5xw |
| `avoidTaskbar` | `boolean` | `false` | 🟢 绿灯状态（批次D，规格 4.3.4）：true = 避让 Windows 任务栏。 | — | 1.5xw |
| `wizardDone` | `boolean` | `false` | 首次启动欢迎向导已完成（完成后不再显示）。 | — | 1.5xw |
| `oobeDone` | `boolean` | `false` | B-32 OOBE 首次初始化向导已完成（口令/三模板/介质体检/导览）。 | — | 1.5xw |
| `oobeContainerPath` | `string` | `""` | B-32：OOBE 建卷的容器文件路径（空 = 未创建）。 | — | 1.5xw |
| `oobeContainerEncrypted` | `boolean` | `false` | B-32：OOBE 容器是否启用口令加密。 | — | 1.5xw |
| `oobeTools` | `string[]` | `[]` | B-32：OOBE 选择的 AI 工具模板（claude-code/codex/zcode）。 | — | 1.5xw |
| `wallpaperDaily` | `boolean` | `false` | 批次E-6：每日自动换壁纸（本地缓存目录按日期取图，零网络）。 | — | 1.5xw |
| `wallpaperPoolDir` | `string` | `""` | 批次E-6：壁纸本地缓存目录（Bing 缓存等自备图片文件夹）。 | — | 1.5xw |
| `winTabSwitcher` | `boolean` | `true` | 批次E-6：Win+Tab 多窗口切换器开关（true = Variable 接管 Win+Tab）。 | — | 1.5xw |
| `privacyShield` | `boolean` | `false` | S-1 防截屏模式（WDA_EXCLUDEFROMCAPTURE；仅防系统截屏 API，边界见设置页声明）。 | — | 1.5xw |
| `perfMode` | `PerfMode` | `"high"` | — | — | 1.5xw |
| `showStatusBar` | `boolean` | `true` | — | — | 1.5xw |
| `editorWidthPct` | `number` | `64` | 58..72 | — | 1.5xw |
| `editorAlign` | `"center" \| "left" \| "right"` | `"center"` | — | — | 1.5xw |
| `fontFamily` | `string` | ``"Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif`` | — | — | 1.5xw |
| `fontSize` | `number` | `16` | 14..22 | — | 1.5xw |
| `lineHeight` | `number` | `1.75` | 1.5..2.2 | — | 1.5xw |
| `autosaveDelayMs` | `number` | `1200` | 400..3000 | — | 1.5xw |
| `uiZoom` | `number` | `1` | 0.85..1.3 | — | 1.5xw |
| `nightLight` | `number` | `0` | F-1 夜灯模式：0 = 关闭；10..70 = 色温遮罩强度（遮罩层，不动系统色温）。 | — | 1.5xw |
| `reduceMotion` | `boolean` | `false` | — | — | 1.5xw |
| `safeMode` | `boolean` | `false` | — | — | 1.5xw |
| `soundVolume` | `number` | `0.5` | A-4 声音设计：系统音音量 0-1；全局静音。 | — | 1.5xw |
| `soundMuted` | `boolean` | `false` | — | — | 1.5xw |
| `bgTier` | `number` | `0` | Anime starfield performance tier: 1..10 fixed, 0 = smart auto monitor. | — | 1.5xw |
| `pvzDictOverrides` | `Record<string, string>` | `{}` | 8.2 用户教学式词典：大白话解释，key 为小写术语。 | — | 1.5xw |
| `inputFeel` | `InputFeelSettings` | `structuredClone(DEFAULT_INPUT_FEEL)` | AI-06 输入手感组（U-58/U-59、V-61…V-70）：默认全部关闭或等于现状。 | AI-06 | 1.5xw |
| `winFeelOpacity` | `boolean` | `false` | AI-01 Z-36：标题栏透明度/置顶微控总开关（默认 off）。 | AI-01 | 1.5xw |
| `winShake` | `boolean` | `false` | AI-01 M-01：摇一摇最小化（Aero Shake，默认 off）。 | AI-01 | 1.5xw |
| `winGuides` | `boolean` | `false` | AI-01 M-07：拖拽对齐参考线与轻吸附（默认 off；Alt 临时禁用）。 | AI-01 | 1.5xw |
| `winGestures` | `boolean` | `false` | AI-01 Z-41：鼠标手势最小集（右键拖 下=关窗 / 上=最小化，默认 off）。 | AI-01 | 1.5xw |
| `altWheelTopmost` | `boolean` | `false` | AI-01 Z-38：Alt+滚轮在置顶窗口间循环（默认 off）。 | AI-01 | 1.5xw |
| `altTabFilter` | `"off" \| "app" \| "monitor"` | `"off"` | AI-01 M-08：精炼 Alt+Tab 过滤（off / app=同应用 / monitor=同屏；Ctrl+Alt+Tab 触发）。 | AI-01 | 1.5xw |
| `xmouse` | `0 \| 1 \| 2` | `0` | AI-01 M-09：悬停聚焦 X-Mouse（0=关 / 1=仅聚焦 / 2=聚焦并置顶）。 | AI-01 | 1.5xw |
| `desktopHotzone` | `number` | `0` | AI-01 Z-42：右缘热区呼出桌面切换预览（0=关 / 8..32 px 宽度）。 | AI-01 | 1.5xw |
| `compatLegacyShim` | `boolean` | `true` | AI-12 Z-16：遗留协议兼容 Shim（默认开，只记录不改行为）。 | AI-12 | 1.5xw |
| `compatHighRefresh` | `boolean` | `true` | AI-12 M-40：高刷（≥120Hz）动效时长 0.85× 折算（默认开）。 | AI-12 | 1.5xw |
| `compatHostOverride` | `"auto" \| "native" \| "vm" \| "remote"` | `"auto"` | AI-12 Z-20：宿主档手动覆盖（auto 服从检测；误检安全=不确定按 native）。 | AI-12 | 1.5xw |
| `compatSlowOverride` | `-1 \| 0 \| 1 \| 2 \| 3` | `-1` | AI-12 Z-21：慢速档手动覆盖（-1=auto；0..3=L0..L3；只降不升）。 | AI-12 | 1.5xw |
| `perfThreadHealth` | `boolean` | `false` | AI-13 Z-58：UI 线程健康采集（默认关 = 零采集红线）。 | AI-13 | 1.5xw |
| `perfIdleFreeze` | `boolean` | `false` | AI-13 Z-59：空闲渲染冻结（最小化/遮挡暂停 rAF；默认关）。 | AI-13 | 1.5xw |
| `perfUsageStats` | `boolean` | `false` | AI-13 Z-62：本地使用统计（零外发，只写本地；默认关）。 | AI-13 | 1.5xw |
| `updateChannel` | `"stable" \| "beta"` | `"stable"` | AI-13 Z-61：更新通道（stable/beta）。 | AI-13 | 1.5xw |
| `themeFollowHc` | `boolean` | `false` | M-74：系统高对比度跟随（默认关；开启后系统 HC 切换环境即时跟随）。 | — | 1.5xw |
| `motionScale` | `0.5 \| 1` | `1` | M-75：动效时长全局缩放（0.5 / 1；reduceMotion 优先级最高不受影响）。 | — | 1.5xw |
| `cvdSim` | `"off" \| "protanopia" \| "deuteranopia" \| "tritanopia" \| "achromatopsia"` | `"off"` | U-40：色觉模拟器（开发者向预览；默认 off）。 | — | 1.5xw |
| `focusAnnounce` | `boolean` | `false` | U-40：焦点跟随提示（Tab 移动读出目标控件名称；默认关）。 | — | 1.5xw |
| `s2tLexicon` | `Record<string, string>` | `{}` | M-77：简繁转换用户词表（键=简体词，值=目标串；上限 500 条）。 | — | 1.5xw |
| `i18nOverrides` | `Record<string, Record<string, string>>` | `{}` | U-41：i18n 运行时覆盖表 { lang: { key: value } }（工作台编辑写这里）。 | — | 1.5xw |
| `pseudoLocale` | `boolean` | `false` | U-41：伪本地化模式（dev 检查硬编码文案与溢出布局；默认关）。 | — | 1.5xw |
| `rtlPilot` | `boolean` | `false` | U-41：RTL 试点（设置/通知中心两处面板正确渲染；默认关）。 | — | 1.5xw |
| `localeFormatFollow` | `boolean` | `false` | M-78：日期/时间/数字区域格式跟随系统（默认关 = 现状硬编码）。 | — | 1.5xw |
| `byteUnit` | `"auto" \| "binary" \| "decimal"` | `"auto"` | M-78：容量单位口径（auto=系统口径即二进制 / binary / decimal）。 | — | 1.5xw |
| `customBg` | `CustomBg` | `{ type: "nebula", color: "#0a1226", gradientFrom: "#0a1638", …` | — | — | 1.5xw |
| `mindDefaults` | `MindDefaults` | `{ gridEnabled: true, snapEnabled: true, gridMode: "grid", gr …` | — | — | 1.5xw |
