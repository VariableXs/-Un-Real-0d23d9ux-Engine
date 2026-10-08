/**
 * 车道 W 锁屏本地词典（不改 i18n/dictionaries.ts）。
 */

export const LOCK_LABELS = {
  zh: {
    ritualHint: "仪式锁 · 任意键或点击返回",
    ritualFocus: "本轮专注",
    realHint: "输入解锁口令",
    unlock: "解锁",
    wrongPw: "口令不正确",
    tooMany: "尝试过于频繁，请稍候",
    recoveryEntry: "用恢复码重置口令",
    setupTitle: "设置解锁口令",
    setupPw: "新口令（≥4 位）",
    setupPw2: "再输一次",
    setupPwMismatch: "两次输入不一致",
    setupSave: "保存并生成恢复码",
    recoveryShow: "恢复码（仅显示一次，请记下）",
    recoveryCopied: "已复制",
    recoveryDone: "我已记下",
    notifTitle: "未读通知",
    notifPrivacy: "隐私",
    notifHardware: "硬件",
    notifSystem: "系统",
    clockSize: "时钟字号",
    boundary: "应用层锁屏 · 不替代 Windows 锁屏",
    pwReset: "口令已重置，请设置新口令",
  },
  en: {
    ritualHint: "Ritual lock · any key or click to return",
    ritualFocus: "Focus round",
    realHint: "Enter passcode",
    unlock: "Unlock",
    wrongPw: "Incorrect passcode",
    tooMany: "Too many attempts, please wait",
    recoveryEntry: "Reset with recovery code",
    setupTitle: "Set unlock passcode",
    setupPw: "New passcode (≥4 chars)",
    setupPw2: "Repeat",
    setupPwMismatch: "Passcodes do not match",
    setupSave: "Save & generate recovery code",
    recoveryShow: "Recovery code (shown once, write it down)",
    recoveryCopied: "Copied",
    recoveryDone: "Saved it",
    notifTitle: "Unread",
    notifPrivacy: "Privacy",
    notifHardware: "Hardware",
    notifSystem: "System",
    clockSize: "Clock size",
    boundary: "App-level lock · not a replacement for the Windows lock screen",
    pwReset: "Passcode reset; please set a new one",
  },
} as const;

export type LockLabelKey = keyof (typeof LOCK_LABELS)["zh"];

export function lockT(lang?: string): (k: LockLabelKey) => string {
  const l = lang ?? (typeof navigator !== "undefined" ? navigator.language : "zh");
  const dict = LOCK_LABELS[l && l.toLowerCase().startsWith("zh") ? "zh" : "en"];
  return (k) => dict[k] ?? k;
}