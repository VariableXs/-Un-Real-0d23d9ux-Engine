/**
 * AI04 车道 D：桌面图标本车道本地词典（V-01…V-10 / V-96 / U-13）。
 *
 * 约定：不得修改 src/i18n/dictionaries.ts（并行车道冲突），本车道新增文案
 * 全部落在这里；现有 t() 已有的 key（viewMenu/sortBy/sortType/sortName/
 * autoArrange/showIcons/hideIcons/explorerWin/recycleBin 等）继续走 t()。
 * 组件侧通过 desktopLabel(lang, key) 取词：zh-TW 回退 zh，未命中回退 key 本身。
 */
import type { Lang } from "../../i18n/dictionaries";

export const DESKTOP_LABELS: Record<"zh" | "en", Record<string, string>> = {
  zh: {
    // ---- V-01 排序方式补全 ----
    sortSize: "大小",
    sortDate: "日期",
    alignGrid: "将图标与网格对齐",
    // ---- V-02 系统图标五件套 ----
    sysNetwork: "网络",
    sysUserFiles: "用户的文件",
    sysControlPanel: "控制面板",
    sysIconsReset: "恢复默认",
    sysIconHostNA: "该入口依赖宿主系统能力，Variable 尚未接入",
    sysIconHidden: "系统图标已隐藏（仅隐藏，不会真的删除）",
    // ---- V-03 图标锁定 ----
    lockIcons: "锁定桌面图标",
    lockStateOn: "已锁定（点击解锁）",
    lockStateOff: "未锁定（点击锁定）",
    desktopLocked: "桌面已锁定",
    unlockConfirmTitle: "解锁桌面图标",
    unlockConfirmBody: "解锁后图标将恢复自由拖动与删除。",
    unlockOk: "解锁",
    // ---- V-04 双击空白动作 ----
    dblClickMenu: "双击空白动作",
    dblNone: "无动作（默认）",
    dblShowDesktop: "显示桌面",
    dblMinimizeAll: "最小化全部窗口",
    dblPalette: "打开命令面板",
    dblLock: "锁定环境",
    dblDone: "双击空白动作已触发",
    // ---- V-05 标签可读性 ----
    labelShadeMenu: "标签文字对比",
    shadeAuto: "自动（跟随壁纸）",
    shadeBlack: "总是黑字",
    shadeWhite: "总是白字",
    // ---- V-96 桌面归档 ----
    archiveAskTitle: "桌面归档",
    archiveAskBody: "桌面有点挤了，要归档吗？",
    archiveOk: "去选择",
    archivePickTitle: "选择要归档的图标",
    archiveAll: "全选",
    archiveNone: "清空",
    archiveDo: "归档所选",
    archiveDone: "已归档",
    // ---- U-13 桌面配置 ----
    profileMenu: "桌面配置",
    profileSave: "保存当前配置…",
    profileManage: "管理…",
    profileApply: "应用",
    profileDelete: "删除",
    profileDeleteTitle: "删除桌面配置",
    profileSaved: "已保存桌面配置",
    profileApplied: "已切换桌面配置",
    profileEmpty: "还没有已保存的桌面配置",
    profileHotkey: "快捷键（仅登记）",
    profileHotkeyHint: "系统级热键通道未接入：快捷键仅登记在本地配置中，不会注册系统热键。",
    profileNamePrompt: "配置名称",
  },
  en: {
    sortSize: "Size",
    sortDate: "Date",
    alignGrid: "Align icons to grid",
    sysNetwork: "Network",
    sysUserFiles: "User Files",
    sysControlPanel: "Control Panel",
    sysIconsReset: "Restore default",
    sysIconHostNA: "This entry relies on host OS capability, not yet integrated in Variable",
    sysIconHidden: "System icon hidden (hidden only, never deleted)",
    lockIcons: "Lock desktop icons",
    lockStateOn: "Locked — click to unlock",
    lockStateOff: "Unlocked — click to lock",
    desktopLocked: "Desktop icons are locked",
    unlockConfirmTitle: "Unlock desktop icons",
    unlockConfirmBody: "Icons can be dragged and deleted again after unlocking.",
    unlockOk: "Unlock",
    dblClickMenu: "Double-click blank action",
    dblNone: "None (default)",
    dblShowDesktop: "Show desktop",
    dblMinimizeAll: "Minimize all windows",
    dblPalette: "Open command palette",
    dblLock: "Lock environment",
    dblDone: "Double-click action triggered",
    labelShadeMenu: "Label readability",
    shadeAuto: "Auto (follow wallpaper)",
    shadeBlack: "Always black text",
    shadeWhite: "Always white text",
    archiveAskTitle: "Desktop archive",
    archiveAskBody: "Your desktop is getting crowded. Archive some icons?",
    archiveOk: "Choose icons",
    archivePickTitle: "Choose icons to archive",
    archiveAll: "Select all",
    archiveNone: "Clear",
    archiveDo: "Archive selected",
    archiveDone: "Archived",
    profileMenu: "Desktop profiles",
    profileSave: "Save current as profile…",
    profileManage: "Manage…",
    profileApply: "Apply",
    profileDelete: "Delete",
    profileDeleteTitle: "Delete profile",
    profileSaved: "Profile saved",
    profileApplied: "Profile applied",
    profileEmpty: "No saved profiles yet",
    profileHotkey: "Hotkey (registered only)",
    profileHotkeyHint: "No system hotkey channel yet: hotkeys are only recorded locally, never registered with the OS.",
    profileNamePrompt: "Profile name",
    tplCenter: "Template center…",
    tplSaveFrom: "Save file as template…",
    tplNameTitle: "Template name",
    tplNewTitle: "New file",
    tplCreated: "Created",
    tplSavedTpl: "Saved as template",
    tplSavedBody: "Available under desktop right-click → New",
    tplTooBig: "File too large (template limit 256KB)",
    tplLimit: "Template limit reached (50)",
    tplInvalid: "Invalid name or extension (1-12 letters/digits)",
    tplReadFail: "Failed to read file (text templates only)",
    tplRename: "Rename",
    tplDelete: "Delete",
    tplUserTpl: "Custom templates",
    tplEmpty: "No custom templates yet. Use \"Save file as template\" to add one.",
    tplPickDirNA: "Directory picker unavailable (not in Tauri)",
  },
};

/** 取词：zh-TW → zh 回退；未命中 key 原样返回（便于发现漏翻）。 */
export function desktopLabel(lang: Lang, key: string): string {
  const table = DESKTOP_LABELS[lang === "en" ? "en" : "zh"];
  return table[key] ?? key;
}