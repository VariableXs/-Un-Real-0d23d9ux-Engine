/**
 * I 通用域 · AI-U1 一分队（F401-F450）共用底座。
 *
 * 职责（与 U3 u3store / J1 j1store 同源纪律，一处一事实）：
 * - 单一配置根：U1 全部配置收在 localStorage `variable:u1:v1` 一个键下，
 *   按 F 编号语义分节存储——对账与自检直接逐节取表。
 * - 订阅制总线：set() 写入后逐节广播；运行时/面板按节订阅，不轮询。
 * - 原子写 + 异常零静默：先写内存再落盘；落盘失败抛 U1StoreError，
 *   会话内配置仍生效，重启回退上次成功落盘值（十三章红线）。
 * - 可回退：每节写前留一帧快照，undoSection() 一键还原，栈深 3。
 *
 * 边界声明（主册 I-0 · 分工书泳道四）：本底座只管「判据逻辑层」的
 * 开关与参数；内核侧判据实装层在 kernel/varix/src/uni1/（AI-U1 v1，
 * 五十项语义核 6,982 行）；指针渲染平面归 F335（AI-J1 领地）、主题
 * 令牌归 F151（AI-E1 领地）、F244 键位注册唯一落位在 uni1::ubase
 * （本域领地）——均不在此重复。
 */

export const U1_LS_KEY = "variable:u1:v1";
export const U1_FORMAT = "u1-config";
export const U1_VERSION = 1;

/** U1 域全部分节（键名 = F 编号语义，与各模块文件头注释一一对应）。 */
export const U1_SECTIONS = [
  "autoArrange",   // F401 桌面自动排列
  "taskmgrHot",    // F402 任务管理器快捷入口
  "sysHotkeys",    // F403/F405/F407/F408 系统快捷键族
  "ctxHelp",       // F409 F1 上下文帮助
  "explorerKeys",  // F404/F410-F415/F431 资源管理器键位族
  "startMenu",     // F416-F419 开始菜单与任务栏
  "indicators",    // F420-F423 指示器与浮层
  "escStack",      // F424 Esc 通用关闭语义
  "shake",         // F425 Aero Shake
  "docOps",        // F413/F426-F428 文档操作键位族
  "viewFx",        // F429/F430/F432 视图族
  "diskTools",     // F437-F440 磁盘工具族
  "wizards",       // F441-F444 向导族
  "displayAv",     // F445-F447 显示与声音
  "a11yKeys",      // F450 粘滞键与筛选键
] as const;

export type U1Section = (typeof U1_SECTIONS)[number];

/** 落盘失败（十三章：异常显性化——不静默吞）。 */
export class U1StoreError extends Error {
  constructor(public readonly causeErr: unknown) {
    super("U1 配置落盘失败——会话内仍生效，重启回退上次成功值");
  }
}

type Snapshot = { section: U1Section; prev: Record<string, unknown> };

const UNDO_DEPTH = 3;

function readRoot(): Record<string, Record<string, unknown>> {
  try {
    const raw = localStorage.getItem(U1_LS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as { format?: string; version?: number; sections?: Record<string, Record<string, unknown>> };
    if (parsed.format !== U1_FORMAT || parsed.version !== U1_VERSION) return {};
    return parsed.sections ?? {};
  } catch {
    return {}; // 损坏配置按空起（不报错打断会话——F189 自愈口径）
  }
}

function writeRoot(sections: Record<string, Record<string, unknown>>): void {
  const payload = JSON.stringify({ format: U1_FORMAT, version: U1_VERSION, sections });
  localStorage.setItem(U1_LS_KEY, payload); // 失败由调用方捕获转 U1StoreError
}

class U1StoreImpl {
  private sections: Record<string, Record<string, unknown>> = readRoot();
  private undoStack: Snapshot[] = [];
  private listeners = new Set<() => void>();
  /** 落盘失败账（显性化——面板红点消费）。 */
  public persistErrors = 0;

  get<T extends Record<string, unknown>>(section: U1Section): Partial<T> {
    return (this.sections[section] ?? {}) as Partial<T>;
  }

  set(section: U1Section, patch: Record<string, unknown>): void {
    const prev = { ...(this.sections[section] ?? {}) };
    this.undoStack.push({ section, prev });
    if (this.undoStack.length > UNDO_DEPTH) this.undoStack.shift();
    this.sections[section] = { ...prev, ...patch };
    this.broadcast();
    try {
      writeRoot(this.sections);
    } catch (err) {
      this.persistErrors += 1; // 异常零静默：计数 + 抛出，会话内不回滚
      throw new U1StoreError(err);
    }
  }

  /** 一键还原该节最近一次改动（栈深 3）。 */
  undoSection(section: U1Section): boolean {
    for (let i = this.undoStack.length - 1; i >= 0; i--) {
      const snap = this.undoStack[i];
      if (snap && snap.section === section) {
        this.undoStack.splice(i, 1);
        this.sections[section] = { ...snap.prev };
        this.broadcast();
        try {
          writeRoot(this.sections);
        } catch {
          this.persistErrors += 1;
        }
        return true;
      }
    }
    return false;
  }

  resetSection(section: U1Section): void {
    this.set(section, U1_DEFAULTS[section] as Record<string, unknown>);
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private broadcast(): void {
    for (const fn of this.listeners) fn();
  }

  /** 导出（F305 同源白名单：只有配置，无凭据无路径）。 */
  exportAll(): string {
    return JSON.stringify({ format: U1_FORMAT, version: U1_VERSION, sections: this.sections }, null, 2);
  }

  /** 导入（校验失败拒载——F305 判据）。 */
  importAll(json: string): boolean {
    try {
      const parsed = JSON.parse(json) as { format?: string; version?: number; sections?: Record<string, Record<string, unknown>> };
      if (parsed.format !== U1_FORMAT || parsed.version !== U1_VERSION || !parsed.sections) return false;
      for (const key of Object.keys(parsed.sections)) {
        if (!(U1_SECTIONS as readonly string[]).includes(key)) return false;
      }
      this.sections = parsed.sections;
      this.broadcast();
      writeRoot(this.sections);
      return true;
    } catch {
      return false;
    }
  }
}

/* ------------------------------- 默认档（= 主册判据默认） ------------------------------- */

export const U1_DEFAULTS: Record<U1Section, Record<string, unknown>> = {
  autoArrange: { order: "name", enabled: false },            // F401：默认关（自由摆放为主）
  taskmgrHot: { hotkey: "Ctrl+Shift+Esc", refreshMs: 1000 }, // F402：1s 刷新判据
  sysHotkeys: {
    lockWin: true, altF4Confirm: true, winI: true, winX: true, // F403/F405/F407/F408
  },
  ctxHelp: { f1Enabled: true, neverFocusSteal: true },       // F409
  explorerKeys: {
    winEMode: "tab",                                          // F404：默认标签页
    enterOpensInTab: true, ctrlEnterNewWindow: true,          // F410
    backspaceUp: true,                                        // F411
    dragToTrash: true,                                        // F414
    trashBadge: true,                                         // F415
    quickNewFolder: true,                                     // F431
  },
  startMenu: { winKey: true, openBudgetMs: 150, tileGrid: "medium" }, // F416-F419
  indicators: { imeBadge: true, volumeFly: true, batteryFly: true },  // F421-F423
  escStack: { enabled: true, budgetMs: 100 },                // F424
  shake: { enabled: true, windowMs: 150, amplitudePx: 40 },  // F425：参数=内核唯一登记点
  docOps: {
    printScreenMode: "region", savePrompt: true,             // F413
    ctrlSave: true, ctrlShiftS: true, f12AsSave: false,      // F426
    cutDelayed: true,                                        // F427
    printPreview: true,                                      // F428
  },
  viewFx: {
    fullscreenHint: true,                                    // F429
    zoomStep: 250, zoomMin: 100, zoomMax: 8000,              // F430：=内核 ZOOM_* 常量
    pageKeepRelative: true,                                  // F432
  },
  diskTools: {
    formatCancelWindowMs: 2000,                              // F437：=内核 CANCEL_WINDOW_MS
    lnkAutofix: true,                                        // F438
    vaultRequireExport: true,                                // F439：强制导出判据（不可关）
    isoCap: 4,                                               // F440：=内核 MAX_MOUNTED
  },
  wizards: {
    taskIdleMinutes: 15,                                     // F441
    restoreBudgetMs: 30000,                                  // F442：=内核 CREATE_BUDGET_MS
    btReconnectMs: 3000,                                     // F443：=内核 RECONNECT_BUDGET_MS
    printerTestPage: true,                                   // F444
  },
  displayAv: {
    snapPx: 8, numberOverlayMs: 3000,                        // F445：=内核 SNAP/NUMBER 常量
    confirmWindowMs: 15000, blackoutBudgetMs: 2000,          // F446：=内核 CONFIRM/BLACKOUT
    customMaxMs: 3000,                                       // F447：=内核 CUSTOM_MAX_MS
  },
  a11yKeys: {
    stickyEnabled: false, filterEnabled: false,              // F450：默认全关
    filterMinHoldMs: 50, neverRemind: false,                 // =内核 FILTER_MIN_HOLD_MS
  },
};

export const u1Store = new U1StoreImpl();
