/**
 * 化境 V-11…V-14（车道 S）开始菜单本地词典。
 * 诚实边界：i18n dictionaries.ts 属共享文件（并行车道禁改，避免冲突），
 * 故本车道字符串就近放这里；主控后期统一合并进 dictionaries.ts。
 * 现有 t() key（pinned / recent / tbPin / tbUnpin / runAsAdmin / properties /
 * rename / uninstallMenu / tpRemove / tpRemoveBody 等）继续走 useI18n。
 */

export const START_LABELS = {
  zh: {
    // V-11 字母索引条
    idxBarLabel: "按字母跳转",
    idxNoGroup: "#",
    // V-12 最近添加 / 高频使用
    grpRecentAdded: "最近添加",
    grpHighFreq: "高频使用",
    grpHide: "隐藏此分组（本次会话内）",
    grpHideNote: "已隐藏「{name}」分组。恢复：重新打开开始菜单（会话级隐藏，正式开关位在设置页）",
    hfreqOnTitle: "关闭「高频使用」分组（正式开关位在设置页，属后续版本范围，故就近提供）",
    hfreqOffTitle: "开启「高频使用」分组（本地计数，默认关；正式开关位在设置页，属后续版本范围，故就近提供）",
    // V-13 固定文件夹
    folderNew: "新建文件夹",
    folderFull: "文件夹最多 24 项，无法继续添加",
    folderMergeFull: "合并后超过 24 项上限，已取消合并（保持原样）",
    folderRenameTitle: "重命名文件夹",
    folderDisband: "解散文件夹",
    folderDisbanded: "已解散文件夹，{n} 项回到固定网格",
    folderOpen: "打开文件夹",
    // V-14 右键高级操作
    ctxPinnedStart: "固定到「开始」（已固定）",
    ctxPinTaskbarNA: "固定到任务栏（暂未接入）",
    naTaskbarDetail: "官方软件暂无任务栏固定接口，本项暂不可用（正式能力见后续版本）",
    ctxOpenFileLoc: "打开文件位置",
    naFileLocDetail: "官方软件为环境内置，无独立文件位置",
    ctxAdmin: "以管理员身份运行",
    naAdminDetail: "官方软件不支持以管理员身份运行",
    propsTitle: "属性",
    propName: "名称",
    propKind: "类型",
    propPath: "路径",
    propNoPath: "—（无独立路径）",
    propAdded: "加入时间",
    propUsage: "使用次数（本机计数）",
    kindApp: "官方软件",
    kindSys: "系统入口",
    kindTool: "实用工具",
    kindTp: "第三方软件",
    kindFolder: "固定文件夹",
    propsLocalNote: "以上信息均读本机（零网络）",
  },
  en: {
    idxBarLabel: "Jump by letter",
    idxNoGroup: "#",
    grpRecentAdded: "Recently added",
    grpHighFreq: "Most used",
    grpHide: "Hide this group (this session)",
    grpHideNote: "Group \"{name}\" hidden. Reopen the Start menu to restore (session-level; the permanent switch lives in Settings)",
    hfreqOnTitle: "Turn off \"Most used\" group (the permanent switch belongs to the Settings page in a later release, so it is offered here)",
    hfreqOffTitle: "Turn on \"Most used\" group (local counting, off by default; the permanent switch belongs to the Settings page in a later release)",
    folderNew: "New folder",
    folderFull: "A folder holds at most 24 items",
    folderMergeFull: "Merge would exceed the 24-item limit — cancelled (kept as is)",
    folderRenameTitle: "Rename folder",
    folderDisband: "Disband folder",
    folderDisbanded: "Folder disbanded, {n} item(s) back on the grid",
    folderOpen: "Open folder",
    ctxPinnedStart: "Pin to Start (already pinned)",
    ctxPinTaskbarNA: "Pin to taskbar (not wired yet)",
    naTaskbarDetail: "No taskbar-pin interface for official apps yet — unavailable for now",
    ctxOpenFileLoc: "Open file location",
    naFileLocDetail: "Official apps are built in — no standalone file location",
    ctxAdmin: "Run as administrator",
    naAdminDetail: "Official apps cannot run as administrator",
    propsTitle: "Properties",
    propName: "Name",
    propKind: "Type",
    propPath: "Path",
    propNoPath: "— (no standalone path)",
    propAdded: "Added",
    propUsage: "Launch count (local)",
    kindApp: "Official app",
    kindSys: "System entry",
    kindTool: "Tool",
    kindTp: "Third-party app",
    kindFolder: "Pinned folder",
    propsLocalNote: "All values are read locally (zero network)",
  },
};

/** 语言 → 词典（zh-TW 归并 zh；en 单列）。键集合两语一致，缺键在 CI 由用例守护。 */
export function startLabels(lang: string): typeof START_LABELS.zh {
  return lang === "en" ? START_LABELS.en : START_LABELS.zh;
}
