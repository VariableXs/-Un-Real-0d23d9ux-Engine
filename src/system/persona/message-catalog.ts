/**
 * 通用十二查深化 · 空态/错误态/确认态双语消息目录（说明句 F474 同源三件套）。
 *
 * 主册判据延伸：
 * - 通用十二查 8「说明句：名称+一句话说明+调节控件三件套齐全」、
 *   13「文案与语言：全量文案走查」——文案不能靠散装字符串：本目录是
 *   E 域二十页**空态/错误态/破坏性确认**的唯一词源（一处一事实）；
 * - 十三章补「异常三要素：发生了什么/为什么/下一步怎么办」——错误
 *   消息结构化成三要素字段，缺一编译期即报（类型即门禁）；
 * - 双语（zh/en）同键强制——TS 类型保证两语言键集一致（漏翻编译错）。
 */

// ---------- 消息结构（三要素类型即门禁） ----------

export interface TriadMessage {
  /** 发生了什么（人话，一句）。 */
  what: string;
  /** 为什么（原因方向，一句）。 */
  why: string;
  /** 下一步怎么办（可操作，一句）。 */
  next: string;
}

export interface EmptyStateMessage {
  /** 空态标题（不是空白——空态是设计资源）。 */
  title: string;
  /** 第一次能做什么。 */
  guide: string;
  /** 入口动作文案。 */
  action: string;
}

export interface ConfirmMessage {
  /** 破坏性操作描述。 */
  what: string;
  /** 后果（可撤销？撤销路径）。 */
  consequence: string;
  /** 确认钮文案（动词具体——不写"确定"）。 */
  confirm: string;
  /** 取消永远是安全出路（显性标注）。 */
  cancel: string;
}

/** 目录条目：三态齐备（类型强制三语言键一致）。 */
export interface PageMessages {
  empty: EmptyStateMessage;
  error: TriadMessage;
  confirm: ConfirmMessage;
}

// ---------- 二十页目录（zh） ----------

const ZH: Record<string, PageMessages> = {
  tokens: {
    empty: { title: "令牌表空空如也", guide: "从 24 色语义集改任意一枚开始——改完即时全桌生效", action: "恢复出厂配色" },
    error: { what: "令牌写入失败", why: "可能被并行会话锁定或值非法", next: "检查值的格式（#rrggbb）后重试，或先放弃本地更改" },
    confirm: { what: "恢复全部令牌为出厂值", consequence: "你的 24 色自定义将全部还原（可先导出档案留底）", confirm: "恢复出厂配色", cancel: "先不恢复" },
  },
  preview: {
    empty: { title: "还没有改动可预览", guide: "在令牌表或任一设置页做一处修改，这里立即出现差异", action: "去令牌表改一个颜色" },
    error: { what: "预览渲染失败", why: "差异快照可能已过期（令牌表刚被其他面板改写）", next: "刷新预览会话——未应用的更改不会丢失" },
    confirm: { what: "放弃当前全部未应用更改", consequence: "逐令牌回滚到上次应用点，差异清零（不可撤销）", confirm: "放弃这些更改", cancel: "继续编辑" },
  },
  autodark: {
    empty: { title: "深浅自动切换未启用", guide: "开启后按日落日出或定时在深浅主题间过渡", action: "立即启用" },
    error: { what: "日落时刻获取失败", why: "F116 城市数据未接入且天文兜底遇到极昼极夜", next: "改用定时模式，或设置所在时区后重试" },
    confirm: { what: "今晚跳过自动切换", consequence: "保持当前主题直到明天日出（手动切主题不受影响）", confirm: "今晚跳过", cancel: "照常切换" },
  },
  exceptions: {
    empty: { title: "还没有应用例外", guide: "上限 10 个——例外应用在全局切换时保持自己的深浅与强调色", action: "添加第一个例外" },
    error: { what: "例外添加失败", why: "应用 id 重复或已到 10 个上限", next: "换一个唯一 id，或先移除一个不再需要的例外" },
    confirm: { what: "移除此应用例外", consequence: "该应用将重新跟随全局深浅切换", confirm: "移除例外", cancel: "保留" },
  },
  wallpaper: {
    empty: { title: "图片池是空的", guide: "加入至少 3 张图片轮换——近 7 天用过的自动排除", action: "载入示例图片池" },
    error: { what: "壁纸预载失败", why: "空闲窗口内未完成下载或校验不通过", next: "已降级沿用今日壁纸；可缩小图片档位后重试" },
    confirm: { what: "清空图片池", consequence: "池内全部图片移出轮换（磁盘文件不会被删除）", confirm: "清空图片池", cancel: "保留图片" },
  },
  icons: {
    empty: { title: "当前无第三方图标包", guide: "载入示例包体验热更换——全程 <2s 不重启", action: "载入示例包" },
    error: { what: "图标包热更换失败", why: "包结构未通过 F133 规范校验或覆盖度低于安全线", next: "查看审计报告中的逐条问题；旧包引用未动，桌面显示不受影响" },
    confirm: { what: "卸载图标包并还原官方包", consequence: "该包的自定义图标全部还原（包可随时重新载入）", confirm: "卸载并还原", cancel: "保留当前包" },
  },
  pointer: {
    empty: { title: "尚未自定义任何指针", guide: "15 枚标准指针全可编辑——热点 1px 级标定", action: "编辑默认箭头" },
    error: { what: "指针方案保存失败", why: "热点未设置或帧数超出 16 帧上限", next: "为当前指针设置热点，或删减动画帧后重试" },
    confirm: { what: "还原全部指针为系统默认", consequence: "所有自定义指针与热点标定将被清除", confirm: "还原默认指针", cancel: "保留自定义" },
  },
  sound: {
    empty: { title: "全部音量未调节", guide: "六事件六档位独立调节——50% 滑杆=半响（感知对数）", action: "调节提示音音量" },
    error: { what: "试听播放失败", why: "音频设备不可用或被独占", next: "检查输出设备后重试；总闸开启时试听同样无声" },
    confirm: { what: "全部静音（总闸）", consequence: "包括系统级提示音在内全部静音（通知横幅仍会出现）", confirm: "全部静音", cancel: "保持有声" },
  },
  startmenu: {
    empty: { title: "尚无自定义预设", guide: "三套官方预设起步，改完可保存为自定义", action: "浏览官方预设" },
    error: { what: "预设切换失败", why: "部分固定项已卸载", next: "已自动跳过卸载项并应用其余部分——可在消息中查看跳过清单" },
    confirm: { what: "删除此自定义预设", consequence: "预设本身删除（开始菜单当前布局不变）", confirm: "删除预设", cancel: "保留" },
  },
  font: {
    empty: { title: "未选择待检字体", guide: "选字体后自动扫描缺字率——三档判定安全上线", action: "选择字体检查" },
    error: { what: "字体扫描超时", why: "字表过大（3500 字预算 <100ms 未达成）", next: "已给部分结论先行；全量结果将在后台批扫完成后补齐" },
    confirm: { what: "强行应用缺字字体", consequence: "缺字将以回退字体显示（可能混排不齐）", confirm: "我了解，强行应用", cancel: "换个字体" },
  },
  motion: {
    empty: { title: "动效强度未调节", guide: "完整/减弱/关闭三档——预演三联即时可感", action: "预演三档差异" },
    error: { what: "帧时采样失败", why: "渲染管线忙（前台有重任务）", next: "稍后重试；预演自身掉帧时会诚实提示性能受限" },
    confirm: { what: "关闭全部动效", consequence: "动画瞬显、进度类转为显性进度数字（WP-207 无障碍关联）", confirm: "关闭动效", cancel: "保留动效" },
  },
  archive: {
    empty: { title: "还没有导出过档案", guide: "一键打包令牌/壁纸/指针全部个性——跨机迁移与备份", action: "导出我的档案" },
    error: { what: "档案导入失败", why: "签名校验不通过或版本过旧无法迁移", next: "确认包来源；V0 包会自动迁移，更早版本请先在原机升级导出" },
    confirm: { what: "用档案包覆盖当前全部个性化", consequence: "当前设置先自动留底（可在导入记录中一键回退）", confirm: "导入并覆盖", cancel: "先看看差异" },
  },
  widgets: {
    empty: { title: "桌面还没有小组件", guide: "时钟/天气/监控三组件——8px 网格摆放，越界自动拉回", action: "添加时钟组件" },
    error: { what: "组件数据更新失败", why: "数据源连续失败进入退避或网络中断", next: "组件显示「数据截至」并继续重试；也可手动拉取一次" },
    confirm: { what: "移除此小组件", consequence: "组件从桌面移除（数据源设置保留，可随时加回）", confirm: "移除组件", cancel: "保留" },
  },
  lock: {
    empty: { title: "锁屏为默认样式", guide: "壁纸/时钟布局/通知隐私三组定制——唤醒到可见 ≤2s", action: "定制锁屏" },
    error: { what: "锁屏预览失败", why: "壁纸文件不可读", next: "已回退跟随桌面壁纸模式" },
    confirm: { what: "关闭通知在锁屏的显示", consequence: "锁屏只显示计数徽标，不显示任何应用名与内容", confirm: "只保留计数", cancel: "保持现状" },
  },
  boot: {
    empty: { title: "开机动画为出厂方案", guide: "改色/密度/幕三底图三处可调——8.0s 结构与时长不可改", action: "个性化开机动画" },
    error: { what: "烘帧预算超限", why: "所选密度在该分辨率下字节总量超预算", next: "已推荐降一档密度（时长与结构不变）；也可降低预览分辨率" },
    confirm: { what: "还原开机动画出厂方案", consequence: "改色/密度/底图设置全部还原（8.0s 结构不受影响）", confirm: "还原出厂方案", cancel: "保留自定义" },
  },
  ime: {
    empty: { title: "输入法皮肤未定制", guide: "跟随主题或独立配色——16ms 候选延迟红线在自定义下同样执行", action: "定制输入法皮肤" },
    error: { what: "皮肤包校验失败", why: "JSON Schema 校验不通过或字段越界", next: "按校验报告逐条修正；官方皮肤始终可用不受影响" },
    confirm: { what: "重置输入法皮肤", consequence: "回到跟随主题模式（自定义配色清除）", confirm: "重置皮肤", cancel: "保留" },
  },
  ctxmenu: {
    empty: { title: "右键菜单为默认结构", guide: "隐藏项进入二级完整保留——删除/重命名等关键项锁定不可隐", action: "整理右键菜单" },
    error: { what: "菜单项隐藏失败", why: "该项为系统锁定项（防自残保护）", next: "锁定项不可隐藏是设计保证；可隐藏的是应用注册项" },
    confirm: { what: "恢复右键菜单默认排序", consequence: "使用频率自动排序的累计计数清零", confirm: "恢复默认", cancel: "保留排序" },
  },
  taskbar: {
    empty: { title: "任务栏为默认配置", guide: "图标大小/对齐/自动隐藏三选项独立生效", action: "调整任务栏" },
    error: { what: "任务栏几何计算失败", why: "屏幕分辨率返回异常（热切换瞬间）", next: "下帧自动重算；若持续异常请检查显示设置" },
    confirm: { what: "开启自动隐藏", consequence: "光标贴底 200ms 唤出；全屏应用下强制隐藏", confirm: "开启自动隐藏", cancel: "保持常驻" },
  },
  shortcuts: {
    empty: { title: "无自定义快捷键", guide: "全部条目可重录——冲突即录即查，系统保留项除外", action: "重录一个快捷键" },
    error: { what: "重录失败", why: "新组合与现有条目冲突或含系统保留键", next: "冲突面板列出了占用者——先改走那条，或换组合" },
    confirm: { what: "恢复全部默认快捷键", consequence: "所有自定义重录清除（含撤销栈）", confirm: "恢复全部默认", cancel: "保留自定义" },
  },
  verdict: {
    empty: { title: "尚未执行域总检", guide: "19 项三步验收（改→生效→回退）——30 分钟预算内跑完", action: "执行域总检" },
    error: { what: "域总检中断", why: "某项探针在「生效」步观测失败", next: "报告标出了卡住的项与证据——修复后可单独重跑该项" },
    confirm: { what: "导出证据包", consequence: "证据包含全部三步记录与配置哈希（可第三方独立复核）", confirm: "导出证据包", cancel: "先看报告" },
  },
};

// ---------- en（真翻译 · 键集与 zh 一致——TS 类型保证不漏翻） ----------

const EN: Record<string, PageMessages> = {
  tokens: {
    empty: { title: "No tokens customized yet", guide: "Start by editing any of the 24 semantic colors — changes go live desk-wide instantly", action: "Restore factory palette" },
    error: { what: "Token write failed", why: "The value may be locked by a parallel session or malformed", next: "Check the value format (#rrggbb) and retry, or discard local changes first" },
    confirm: { what: "Restore all tokens to factory values", consequence: "All 24 color customizations will be reverted (export an archive first to keep a copy)", confirm: "Restore factory palette", cancel: "Not now" },
  },
  preview: {
    empty: { title: "No changes to preview yet", guide: "Make one edit in the token table or any settings page — the diff appears here instantly", action: "Edit a color in the token table" },
    error: { what: "Preview rendering failed", why: "The diff snapshot may be stale — the token table was just rewritten by another panel", next: "Refresh the preview session — unapplied changes are not lost" },
    confirm: { what: "Discard all unapplied changes", consequence: "Tokens roll back one-by-one to the last applied point and the diff clears (cannot be undone)", confirm: "Discard changes", cancel: "Keep editing" },
  },
  autodark: {
    empty: { title: "Auto dark switching is off", guide: "Enable to transition between dark and light themes at sunset/sunrise or on a schedule", action: "Enable now" },
    error: { what: "Sunset time unavailable", why: "F116 city data is not connected and the astronomical fallback hit polar day/night", next: "Switch to scheduled mode, or set your time zone and retry" },
    confirm: { what: "Skip auto switching tonight", consequence: "Current theme is kept until tomorrow's sunrise (manual switching unaffected)", confirm: "Skip tonight", cancel: "Switch as usual" },
  },
  exceptions: {
    empty: { title: "No per-app exceptions yet", guide: "Up to 10 — exception apps keep their own dark/light and accent across global switches", action: "Add the first exception" },
    error: { what: "Could not add exception", why: "Duplicate app id or the 10-item limit was reached", next: "Use a unique id, or remove an exception you no longer need" },
    confirm: { what: "Remove this app exception", consequence: "The app will follow global dark/light switching again", confirm: "Remove exception", cancel: "Keep it" },
  },
  wallpaper: {
    empty: { title: "The image pool is empty", guide: "Add at least 3 images to rotate — anything used in the last 7 days is auto-excluded", action: "Load the sample pool" },
    error: { what: "Wallpaper preload failed", why: "The download did not finish within the idle window, or verification failed", next: "Today's wallpaper is kept as fallback; try a smaller image tier and retry" },
    confirm: { what: "Clear the image pool", consequence: "All images leave the rotation (files on disk are not deleted)", confirm: "Clear pool", cancel: "Keep images" },
  },
  icons: {
    empty: { title: "No third-party icon pack", guide: "Load the sample pack to try hot-swap — under 2 seconds, no restart", action: "Load sample pack" },
    error: { what: "Icon pack hot-swap failed", why: "The pack failed F133 spec validation or coverage is below the safety line", next: "Check the audit report for itemized issues; the old pack reference is untouched so the desktop is unaffected" },
    confirm: { what: "Uninstall pack and restore official icons", consequence: "All custom icons from this pack revert (the pack can be reloaded anytime)", confirm: "Uninstall & restore", cancel: "Keep current pack" },
  },
  pointer: {
    empty: { title: "No pointers customized yet", guide: "All 15 standard pointers are editable — hotspot calibration to 1px", action: "Edit the default arrow" },
    error: { what: "Pointer scheme save failed", why: "A hotspot is unset or the frame count exceeds the 16-frame limit", next: "Set the hotspot for the current pointer, or trim animation frames and retry" },
    confirm: { what: "Restore all pointers to system defaults", consequence: "All custom pointers and hotspot calibrations will be cleared", confirm: "Restore defaults", cancel: "Keep custom" },
  },
  sound: {
    empty: { title: "No volumes adjusted yet", guide: "Six events, six independent sliders — 50% slider = half loudness (perceived log curve)", action: "Adjust notification volume" },
    error: { what: "Preview playback failed", why: "No audio device available, or it is exclusively occupied", next: "Check your output device and retry; with the master gate on, preview is silent too" },
    confirm: { what: "Mute everything (master gate)", consequence: "Everything including system-level alert sounds is muted (notification banners still appear)", confirm: "Mute all", cancel: "Keep sound on" },
  },
  startmenu: {
    empty: { title: "No custom presets yet", guide: "Start from the three official presets, edit, then save as your own", action: "Browse official presets" },
    error: { what: "Preset switch failed", why: "Some pinned items have been uninstalled", next: "Uninstalled items were skipped and the rest applied — see the message for the skip list" },
    confirm: { what: "Delete this custom preset", consequence: "Only the preset is deleted (your current Start menu layout is unchanged)", confirm: "Delete preset", cancel: "Keep it" },
  },
  font: {
    empty: { title: "No font selected for checking", guide: "Pick a font to scan its missing-glyph rate — three-tier verdict for safe adoption", action: "Choose a font to check" },
    error: { what: "Font scan timed out", why: "The glyph table is too large (3500-char budget under 100ms was missed)", next: "Partial results are shown first; the full scan continues in the background" },
    confirm: { what: "Force-apply a font with missing glyphs", consequence: "Missing characters will fall back to another font (mixed rendering possible)", confirm: "I understand, force apply", cancel: "Pick another font" },
  },
  motion: {
    empty: { title: "Motion intensity not adjusted", guide: "Full / reduced / off — preview all three tiers side by side instantly", action: "Preview the three tiers" },
    error: { what: "Frame-time sampling failed", why: "The render pipeline is busy (a heavy task is in the foreground)", next: "Try again shortly; when preview itself drops frames it says so honestly" },
    confirm: { what: "Turn off all motion", consequence: "Animations appear instantly and progress bars become explicit numbers (WP-207 accessibility)", confirm: "Turn off motion", cancel: "Keep motion" },
  },
  archive: {
    empty: { title: "No archive exported yet", guide: "Bundle tokens, wallpaper, pointers — everything personal — for backup and migration", action: "Export my profile" },
    error: { what: "Archive import failed", why: "Signature verification failed, or the version is too old to migrate", next: "Verify the source; V0 archives migrate automatically, older ones must re-export from the original machine" },
    confirm: { what: "Overwrite current personalization with this archive", consequence: "Current settings are auto-backed-up first (one-click rollback in import history)", confirm: "Import & overwrite", cancel: "Show the diff first" },
  },
  widgets: {
    empty: { title: "No desktop widgets yet", guide: "Clock, weather, monitor — placed on an 8px grid, off-screen ones are pulled back", action: "Add a clock widget" },
    error: { what: "Widget data refresh failed", why: "The source is in failure backoff or the network is down", next: "The widget shows 'data as of' and keeps retrying; you can also fetch manually once" },
    confirm: { what: "Remove this widget", consequence: "It leaves the desktop (source settings are kept and can be re-added anytime)", confirm: "Remove widget", cancel: "Keep it" },
  },
  lock: {
    empty: { title: "Lock screen is factory style", guide: "Three groups to customize: wallpaper, clock layout, notification privacy — visible within 2s of wake", action: "Customize the lock screen" },
    error: { what: "Lock screen preview failed", why: "The wallpaper file is unreadable", next: "Fell back to follow-desktop-wallpaper mode" },
    confirm: { what: "Hide notifications on the lock screen", consequence: "Only count badges show — no app names, no content", confirm: "Counts only", cancel: "Keep as is" },
  },
  boot: {
    empty: { title: "Boot animation is factory", guide: "Three knobs: color, density, act-3 backdrop — the 8.0s structure and duration are immutable", action: "Personalize boot animation" },
    error: { what: "Bake budget exceeded", why: "The selected density exceeds the byte budget at this resolution", next: "A lower density was recommended (duration and structure unchanged); or lower the preview resolution" },
    confirm: { what: "Restore factory boot animation", consequence: "Color, density and backdrop settings revert (the 8.0s structure is unaffected)", confirm: "Restore factory", cancel: "Keep custom" },
  },
  ime: {
    empty: { title: "IME skin not customized", guide: "Follow the theme or use independent colors — the 16ms candidate latency line holds for custom skins too", action: "Customize the IME skin" },
    error: { what: "Skin package validation failed", why: "JSON Schema validation failed or a field is out of range", next: "Fix items per the validation report; the official skin always remains available" },
    confirm: { what: "Reset the IME skin", consequence: "Returns to follow-theme mode (custom colors cleared)", confirm: "Reset skin", cancel: "Keep it" },
  },
  ctxmenu: {
    empty: { title: "Context menu is default", guide: "Hidden items move to the 'Show more options' tier — Delete/Rename stay locked", action: "Organize the context menu" },
    error: { what: "Could not hide menu item", why: "This item is a locked system item (self-harm protection)", next: "Locked items cannot be hidden by design; app-registered items can be" },
    confirm: { what: "Restore default context menu ordering", consequence: "Usage-frequency auto-sort counters reset to zero", confirm: "Restore default", cancel: "Keep ordering" },
  },
  taskbar: {
    empty: { title: "Taskbar is default", guide: "Icon size, alignment and auto-hide — three options that work independently", action: "Adjust the taskbar" },
    error: { what: "Taskbar geometry failed", why: "Screen resolution returned an anomaly (mid hot-switch)", next: "It recalculates next frame; if it persists, check display settings" },
    confirm: { what: "Enable auto-hide", consequence: "Reveals when the cursor touches the bottom edge for 200ms; force-hidden in fullscreen", confirm: "Enable auto-hide", cancel: "Keep always visible" },
  },
  shortcuts: {
    empty: { title: "No custom shortcuts", guide: "Every entry is rebindable — conflicts detected as you record, except system-reserved", action: "Rebind a shortcut" },
    error: { what: "Rebind failed", why: "The new combo conflicts with an existing entry or uses a reserved key", next: "The conflict panel lists the occupant — rebind that one first, or pick another combo" },
    confirm: { what: "Restore all default shortcuts", consequence: "All custom rebinds are cleared (including the undo stack)", confirm: "Restore all defaults", cancel: "Keep custom" },
  },
  verdict: {
    empty: { title: "Domain audit not run yet", guide: "19 items, three steps each (mutate → effect → rollback) — within a 30-minute budget", action: "Run the domain audit" },
    error: { what: "Domain audit interrupted", why: "A probe failed at the 'effect' step", next: "The report names the stuck item with evidence — fix it and re-run just that item" },
    confirm: { what: "Export the evidence package", consequence: "It contains all three-step records and config hashes (independently verifiable)", confirm: "Export evidence", cancel: "Read the report first" },
  },
};

export const MESSAGE_CATALOG: Record<"zh" | "en", Record<string, PageMessages>> = { zh: ZH, en: EN };

export const MESSAGE_PAGES = Object.keys(ZH);

export function pageMessages(page: string, lang: "zh" | "en"): PageMessages | null {
  if (lang !== "zh" && lang !== "en") return null; // 未知语言显性拒绝（不静默给错词）。
  return MESSAGE_CATALOG[lang][page] ?? null;
}

// ---------- 文案审计（禁词表 + 标点一致性 + 三要素完整性） ----------

const BANNED_WORDS = ["确定", "OK", "成功！", "错误代码", "请重试"]; // 禁词：动词不具体/裸异常码/敷衍重试。
const ZH_ASCII_PUNCT = /[,?!;]|(?<![A-Za-z0-9])[.!;]/; // zh 文案混入 ASCII 标点（数字小数点除外）。
const EN_CJK_PUNCT = /[，。！？；]/; // en 文案混入中文标点。

export interface CopyAuditIssue {
  page: string;
  lang: "zh" | "en";
  slot: string;
  kind: "banned-word" | "punct-mix" | "empty-field";
  snippet: string;
}

/** 全目录文案审计（十二查 13 的机械化——发布前清零）。 */
export function auditCopy(): { issues: CopyAuditIssue[]; checked: number; clean: boolean } {
  const issues: CopyAuditIssue[] = [];
  let checked = 0;
  const scan = (lang: "zh" | "en"): void => {
    for (const [page, m] of Object.entries(MESSAGE_CATALOG[lang])) {
      const entries: Array<[string, string]> = [
        ["empty.title", m.empty.title], ["empty.guide", m.empty.guide], ["empty.action", m.empty.action],
        ["error.what", m.error.what], ["error.why", m.error.why], ["error.next", m.error.next],
        ["confirm.what", m.confirm.what], ["confirm.consequence", m.confirm.consequence], ["confirm.confirm", m.confirm.confirm], ["confirm.cancel", m.confirm.cancel],
      ];
      for (const [slot, text] of entries) {
        checked++;
        if (!text || text.trim() === "") {
          issues.push({ page, lang, slot, kind: "empty-field", snippet: "" });
          continue;
        }
        for (const w of BANNED_WORDS) {
          if (text.includes(w)) issues.push({ page, lang, slot, kind: "banned-word", snippet: text });
        }
        // 标点分语种：zh 不得混入 ASCII 标点、en 不得混入 CJK 标点（确定性规则）。
        if (lang === "zh" ? ZH_ASCII_PUNCT.test(text) : EN_CJK_PUNCT.test(text)) {
          issues.push({ page, lang, slot, kind: "punct-mix", snippet: text });
        }
      }
    }
  };
  scan("zh");
  scan("en");
  return { issues, checked, clean: issues.length === 0 };
}

/** 三要素完整性编译期兜底：运行期再验一次（数据可能来自旧版目录）。 */
export function triadComplete(m: TriadMessage): boolean {
  return [m.what, m.why, m.next].every((s) => typeof s === "string" && s.trim().length > 0);
}
