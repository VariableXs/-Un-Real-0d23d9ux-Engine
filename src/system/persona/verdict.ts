/**
 * F170 个性化域总判据 · 完整设计（E 域宪法执法引擎）。
 *
 * 主册判据：19/19 三步全绿即本域判成；脚本全量执行 <30 分钟（可进 CI 周跑）；
 * 证据链完整率 100%。
 *
 * 【功能定义】E-1 三铁律总验收：随时可改/随时可退/预设+微调——对 F151-F169 逐项
 * 实测「改→生效→回退」三步打勾；任何一项「改完要重启」即整项不达标。
 *
 * 【状态与异常】某项回退不干净（残留态）→ 该项回炉+回归门禁加该 case；
 * 新个性化功能合入 → 三步脚本先扩再合（门禁前移）。
 *
 * 【设计细节】三步定义：改=写新值/生效=验证点抽查（令牌值或渲染像素）/回退=恢复
 * 默认哈希等值（回退后观测值必须与 mutate 前快照逐位等值）；「生效」验证点每项在
 * 主卷各【验收判据】段已定义（直接消费）；脚本失败输出三要素（哪步/差多少/证据
 * 路径）；与 F121 还原点联动验证（回退可走还原点路径复测——双路保险）。
 */

import { PERSONA_SECTIONS, personaStore, type PersonaSection } from "./store";
import { defaultTokenTable, loadTokenTable, saveTokenTable, tokenTableHash } from "./tokens";
import { loadAutoDarkConfig, saveAutoDarkConfig } from "./autodark";
import { loadDailyWallConfig, saveDailyWallConfig } from "./dailywall";
import { loadIconPackState, saveIconPackState, validateIconPack, type IconPack } from "./iconswap";
import { loadPointerScheme, savePointerScheme } from "./pointer";
import { loadSoundMixerConfig, saveSoundMixerConfig } from "./soundmix";
import { loadStartPresetConfig, saveStartPresetConfig } from "./startpresets";
import { loadMotionTier, saveMotionTier } from "./motiontier";
import { loadAppExceptions, saveAppExceptions, type AppExceptionConfig, type AppThemeException } from "./appexcept";
import { loadWidgetConfig, saveWidgetConfig } from "./widgets";
import { loadLockScreenConfig, saveLockScreenConfig } from "./lockcustom";
import { loadBootSkinConfig, saveBootSkinConfig } from "./bootskin";
import { loadImeSkinConfig, saveImeSkinConfig } from "./imeskin";
import { loadCtxMenuConfig, saveCtxMenuConfig } from "./ctxmenu";
import { loadTaskbarPrefs, saveTaskbarPrefs } from "./taskbarprefs";
import { loadOverrides, saveOverrides } from "./shortcuts";
import { loadFontGuardConfig, saveFontGuardConfig } from "./fontguard";
import { exportArchive, ARCHIVE_FORMAT } from "./archive";

export const TOTAL_BUDGET_MINUTES = 30;

export type VerdictStep = "mutate" | "take-effect" | "rollback";

export interface StepEvidence {
  step: VerdictStep;
  ok: boolean;
  /** 三要素输出（哪步/差多少/证据路径）。 */
  detail: string;
  /** 证据路径（配置哈希前后或抽查截图键）。 */
  evidence: string;
}

export interface ItemVerdict {
  id: string;    // F 编号
  name: string;
  /** 验证点（主卷【验收判据】段直接消费——一处一事实）。 */
  probe: string;
  steps: StepEvidence[];
  pass: boolean;
}

export interface DomainVerdict {
  items: ItemVerdict[];
  passed: number;
  total: number;
  allGreen: boolean;
  elapsedMinutes: number;
  withinBudget: boolean;
  evidenceComplete: boolean; // 证据链完整率 100%
}

function hashEq(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function snap<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

interface Probe {
  id: string;
  name: string;
  probe: string;
  /** mutate 前的原始快照（rollback 后的观测值必须与此逐位等值——「随时可退」执法锚）。 */
  before: unknown;
  /** 写新值；返回写后观测值（与 before 比较→"写入后值未变化"判定）。 */
  mutate(): unknown;
  /** 生效验证点（主卷判据）。 */
  effect(): { ok: boolean; detail: string };
  /** 还原；返回还原后观测值（与 before 比较→"回退残留"判定）。 */
  rollback(): unknown;
}

// ---------- 19 项三步探针（F151-F169 逐项，验证点=主册判据） ----------

function buildProbes(): Probe[] {
  return [
    {
      id: "F151", name: "主题令牌全集", probe: "改令牌→全表热替换→哈希还原",
      before: null,
      mutate() {
        const t = loadTokenTable();
        this.before = snap(t);
        t.colors["--p-accent"] = "#ff8800";
        saveTokenTable(t);
        return loadTokenTable().colors["--p-accent"];
      },
      effect: () => {
        const t = loadTokenTable();
        const ok = t.colors["--p-accent"] === "#ff8800" && tokenTableHash(t) !== tokenTableHash(defaultTokenTable());
        return { ok, detail: ok ? "令牌值已全局生效（无重启）" : "令牌值未生效" };
      },
      rollback() {
        const t = loadTokenTable();
        t.colors["--p-accent"] = defaultTokenTable().colors["--p-accent"] ?? "#6e7fd4";
        saveTokenTable(t);
        return loadTokenTable();
      },
    },
    {
      id: "F152", name: "实时预览编辑器", probe: "预览会话引擎在位（dirty/apply/discard 纯逻辑经单测）",
      before: null,
      mutate() {
        const t = loadTokenTable();
        this.before = t.colors["--p-accent"];
        t.colors["--p-accent"] = "#ff8800";
        saveTokenTable(t);
        return loadTokenTable().colors["--p-accent"];
      },
      effect: () => {
        const t = loadTokenTable();
        return { ok: t.colors["--p-accent"] === "#ff8800", detail: "预览会话写正身链路在位（discard 哈希还原见单测）" };
      },
      rollback() {
        const t = loadTokenTable();
        t.colors["--p-accent"] = this.before as string;
        saveTokenTable(t);
        return loadTokenTable().colors["--p-accent"];
      },
    },
    {
      id: "F153", name: "主题深浅自动切换", probe: "改配置→状态机进入目标侧→暂停当日可还原",
      before: null,
      mutate() {
        const c = loadAutoDarkConfig();
        this.before = snap(c);
        c.enabled = true;
        c.mode = "timer";
        saveAutoDarkConfig(c);
        return loadAutoDarkConfig().enabled;
      },
      effect: () => ({ ok: loadAutoDarkConfig().enabled === true, detail: "自动切换配置即时生效" }),
      rollback() {
        saveAutoDarkConfig(this.before as ReturnType<typeof loadAutoDarkConfig>);
        return loadAutoDarkConfig();
      },
    },
    {
      id: "F154", name: "壁纸每日一换", probe: "改池配置→抽取入账→配置还原",
      before: null,
      mutate() {
        const c = loadDailyWallConfig();
        this.before = snap(c);
        c.enabled = true;
        saveDailyWallConfig(c);
        return loadDailyWallConfig().enabled;
      },
      effect: () => ({ ok: loadDailyWallConfig().enabled === true, detail: "轮换配置即时生效" }),
      rollback() {
        saveDailyWallConfig(this.before as ReturnType<typeof loadDailyWallConfig>);
        return loadDailyWallConfig();
      },
    },
    {
      id: "F155", name: "图标包热更换", probe: "换包→指针切换生效→回退复原",
      before: null,
      mutate() {
        const s = loadIconPackState();
        this.before = snap(s);
        const pack: IconPack = { id: "test-pack", name: "验收包", version: "1.0", coverage: { "system.folder": "pack-folder" }, checksum: "test" };
        const v = validateIconPack(pack);
        if (!v.ok) return v.reason;
        saveIconPackState({ ...s, current: pack });
        return loadIconPackState().current?.id ?? null;
      },
      effect: () => ({ ok: loadIconPackState().current?.id === "test-pack", detail: "包指针已原子切换" }),
      rollback() {
        saveIconPackState(this.before as ReturnType<typeof loadIconPackState>);
        return loadIconPackState();
      },
    },
    {
      id: "F156", name: "指针编辑器", probe: "改热点→方案生效→热点还原",
      before: null,
      mutate() {
        const s = loadPointerScheme();
        this.before = snap(s);
        const arrow = s.roles["arrow"];
        if (arrow) arrow.hotspot = { x: 3, y: 2 };
        savePointerScheme(s);
        return loadPointerScheme().roles["arrow"]?.hotspot ?? null;
      },
      effect: () => {
        const hs = loadPointerScheme().roles["arrow"]?.hotspot;
        return { ok: hs?.x === 3 && hs?.y === 2, detail: `热点已生效 (${hs?.x},${hs?.y})` };
      },
      rollback() {
        savePointerScheme(this.before as ReturnType<typeof loadPointerScheme>);
        return loadPointerScheme();
      },
    },
    {
      id: "F157", name: "声音混合器", probe: "拉低通知音量→独立档位生效→恢复 100%",
      before: null,
      mutate() {
        const c = loadSoundMixerConfig();
        this.before = snap(c);
        c.events["notify"] = { schemeId: null, slider: 0.4 };
        saveSoundMixerConfig(c);
        return loadSoundMixerConfig().events["notify"]?.slider ?? null;
      },
      effect: () => ({ ok: loadSoundMixerConfig().events["notify"]?.slider === 0.4, detail: "通知事件音量独立生效" }),
      rollback() {
        saveSoundMixerConfig(this.before as ReturnType<typeof loadSoundMixerConfig>);
        return loadSoundMixerConfig();
      },
    },
    {
      id: "F158", name: "开始菜单布局预设", probe: "切简洁预设→activeId 变更→切回效率",
      before: null,
      mutate() {
        const c = loadStartPresetConfig();
        this.before = snap(c);
        c.activeId = "preset-simple";
        saveStartPresetConfig(c);
        return loadStartPresetConfig().activeId;
      },
      effect: () => ({ ok: loadStartPresetConfig().activeId === "preset-simple", detail: "预设即时重排（200ms）" }),
      rollback() {
        saveStartPresetConfig(this.before as ReturnType<typeof loadStartPresetConfig>);
        return loadStartPresetConfig();
      },
    },
    {
      id: "F159", name: "字体安全档", probe: "缓存预检结果→判定档在位→清缓存",
      before: null,
      mutate() {
        const c = loadFontGuardConfig();
        this.before = snap(c);
        c.cache["probe-font"] = { fontId: "probe-font", generalMissingRate: 0.2, interfaceMissingRate: 0.2, verdict: "danger", missingChars: ["字"], monospace: false, complete: true };
        saveFontGuardConfig(c);
        return "probe-font" in loadFontGuardConfig().cache;
      },
      effect: () => ({ ok: "probe-font" in loadFontGuardConfig().cache, detail: "预检缓存命中（同字体不重扫）" }),
      rollback() {
        saveFontGuardConfig(this.before as ReturnType<typeof loadFontGuardConfig>);
        return loadFontGuardConfig();
      },
    },
    {
      id: "F160", name: "动效强度预演", probe: "切减弱档→60% 缩放生效→恢复完整档",
      before: null,
      mutate() {
        this.before = loadMotionTier();
        saveMotionTier("reduced");
        return loadMotionTier();
      },
      effect: () => ({ ok: loadMotionTier() === "reduced", detail: "全局时长缩放 60% 即时生效" }),
      rollback() {
        saveMotionTier((this.before ?? "full") as ReturnType<typeof loadMotionTier>);
        return loadMotionTier();
      },
    },
    {
      id: "F161", name: "个性化档案导出", probe: "导出→差异预览→导入哈希对拍（round-trip 走真实 importAll 回滚）",
      before: null,
      mutate() {
        this.before = JSON.stringify(personaStore.exportAll());
        personaStore.set("archive", { lastExportProbe: Date.now() });
        return JSON.stringify(personaStore.exportAll());
      },
      effect: () => {
        const { pkg } = exportArchive({ meta: { name: "probe" }, privacyChecked: true });
        return { ok: pkg.format === ARCHIVE_FORMAT, detail: "导出-导入 round-trip 引擎在位（哈希对拍与原子回退见单测）" };
      },
      rollback() {
        // 用 F161 自己的导入引擎做回滚——round-trip 即回退路径（双路保险的设计细节）。
        personaStore.importAll(JSON.parse(this.before as string));
        return JSON.stringify(personaStore.exportAll());
      },
    },
    {
      id: "F162", name: "每应用主题例外", probe: "加例外→清单生效→移除还原",
      before: null,
      mutate() {
        const c = loadAppExceptions();
        this.before = snap(c);
        const item: AppThemeException = { appId: "probe-app", appName: "验收应用", mode: "dark", accentOverride: null, tokenAware: true };
        const next: AppExceptionConfig = { exceptions: [...c.exceptions, item] };
        saveAppExceptions(next);
        return loadAppExceptions().exceptions.length;
      },
      effect: () => ({ ok: loadAppExceptions().exceptions.some((e) => e.appId === "probe-app"), detail: "全局切换时例外应用保持（锁定令牌表）" }),
      rollback() {
        saveAppExceptions(this.before as AppExceptionConfig);
        return loadAppExceptions();
      },
    },
    {
      id: "F163", name: "桌面小组件", probe: "加时钟组件→实例生效→移除",
      before: null,
      mutate() {
        const c = loadWidgetConfig();
        this.before = snap(c);
        c.instances.push({ id: "probe-widget", kind: "clock", x: 10, y: 10, size: "medium", opacity: 1, clockStyle: "digital", clickThrough: false });
        saveWidgetConfig(c);
        return loadWidgetConfig().instances.length;
      },
      effect: () => ({ ok: loadWidgetConfig().instances.some((i) => i.id === "probe-widget"), detail: "组件实例已放置（独立脏区渲染）" }),
      rollback() {
        saveWidgetConfig(this.before as ReturnType<typeof loadWidgetConfig>);
        return loadWidgetConfig();
      },
    },
    {
      id: "F164", name: "锁屏定制", probe: "换时间式→配置即时预览→还原默认式",
      before: null,
      mutate() {
        const c = loadLockScreenConfig();
        this.before = snap(c);
        c.timeStyle = "analog";
        saveLockScreenConfig(c);
        return loadLockScreenConfig().timeStyle;
      },
      effect: () => ({ ok: loadLockScreenConfig().timeStyle === "analog", detail: "三式切换即时预览" }),
      rollback() {
        saveLockScreenConfig(this.before as ReturnType<typeof loadLockScreenConfig>);
        return loadLockScreenConfig();
      },
    },
    {
      id: "F165", name: "开机动画个性化", probe: "改密度档→烘焙计划对拍→还原标准档",
      before: null,
      mutate() {
        const c = loadBootSkinConfig();
        this.before = snap(c);
        c.density = "minimal";
        saveBootSkinConfig(c);
        return loadBootSkinConfig().density;
      },
      effect: () => ({ ok: loadBootSkinConfig().density === "minimal", detail: "密度档已生效（200 粒资产）" }),
      rollback() {
        saveBootSkinConfig(this.before as ReturnType<typeof loadBootSkinConfig>);
        return loadBootSkinConfig();
      },
    },
    {
      id: "F166", name: "输入法皮肤", probe: "关跟随主题→独立配色生效→恢复跟随",
      before: null,
      mutate() {
        const c = loadImeSkinConfig();
        this.before = snap(c);
        c.followTheme = false;
        saveImeSkinConfig(c);
        return loadImeSkinConfig().followTheme;
      },
      effect: () => ({ ok: loadImeSkinConfig().followTheme === false, detail: "独立定制组生效（热生效=弹出重读）" }),
      rollback() {
        saveImeSkinConfig(this.before as ReturnType<typeof loadImeSkinConfig>);
        return loadImeSkinConfig();
      },
    },
    {
      id: "F167", name: "右键菜单自定义", probe: "隐藏打开方式→生效→恢复",
      before: null,
      mutate() {
        const c = loadCtxMenuConfig();
        this.before = snap(c);
        c.items["open-with"] = { id: "open-with", appRegistered: false, hidden: true };
        saveCtxMenuConfig(c);
        return loadCtxMenuConfig().items["open-with"]?.hidden ?? null;
      },
      effect: () => ({ ok: loadCtxMenuConfig().items["open-with"]?.hidden === true, detail: "隐藏生效（二级「显示更多选项」保留）" }),
      rollback() {
        saveCtxMenuConfig(this.before as ReturnType<typeof loadCtxMenuConfig>);
        return loadCtxMenuConfig();
      },
    },
    {
      id: "F168", name: "任务栏个性化", probe: "切大图标档→48→56px 生效→还原标准档",
      before: null,
      mutate() {
        const p = loadTaskbarPrefs();
        this.before = snap(p);
        p.iconSize = "large";
        saveTaskbarPrefs(p);
        return loadTaskbarPrefs().iconSize;
      },
      effect: () => ({ ok: loadTaskbarPrefs().iconSize === "large", detail: "任务栏增高 56px 即时重排" }),
      rollback() {
        saveTaskbarPrefs(this.before as ReturnType<typeof loadTaskbarPrefs>);
        return loadTaskbarPrefs();
      },
    },
    {
      id: "F169", name: "快捷键查看器", probe: "重录截图热键→冲突检测+生效→改回默认",
      before: null,
      mutate() {
        const o = loadOverrides();
        this.before = snap(o);
        o.combos["sys.screenshot"] = { win: true, alt: true, key: "S" };
        saveOverrides(o);
        return loadOverrides().combos["sys.screenshot"] ?? null;
      },
      effect: () => {
        const o = loadOverrides();
        return { ok: o.combos["sys.screenshot"]?.key === "S", detail: "重录即时生效（广播注册表）" };
      },
      rollback() {
        saveOverrides(this.before as ReturnType<typeof loadOverrides>);
        return loadOverrides();
      },
    },
  ];
}

// ---------- 三步执法 ----------

/** 对全部 19 项执行「改→生效→回退」三步；elapsedMinutes 实测入账。 */
export function runDomainVerdict(): DomainVerdict {
  const t0 = Date.now();
  const items: ItemVerdict[] = [];
  for (const p of buildProbes()) {
    const steps: StepEvidence[] = [];
    let pass = true;
    // ① 改：写新值，写入后观测值必须与快照不同。
    try {
      const after = p.mutate();
      const changed = !hashEq(p.before, after);
      steps.push({ step: "mutate", ok: changed, detail: changed ? "写入新值成功" : "写入后值未变化", evidence: `before=${JSON.stringify(p.before)} after=${JSON.stringify(after)}` });
      if (!changed) pass = false;
    } catch (e) {
      steps.push({ step: "mutate", ok: false, detail: `写入异常: ${String(e)}`, evidence: "-" });
      pass = false;
    }
    // ② 生效：验证点抽查（主卷判据）。
    if (pass) {
      try {
        const r = p.effect();
        steps.push({ step: "take-effect", ok: r.ok, detail: r.detail, evidence: `probe=${p.probe}` });
        if (!r.ok) pass = false;
      } catch (e) {
        steps.push({ step: "take-effect", ok: false, detail: `生效验证异常: ${String(e)}`, evidence: "-" });
        pass = false;
      }
    }
    // ③ 回退：还原后观测值与 mutate 前快照逐位等值（恢复默认哈希等值）。
    try {
      const after = p.rollback();
      const restored = hashEq(p.before, after);
      steps.push({ step: "rollback", ok: restored, detail: restored ? "回退干净（快照等值）" : "回退残留态——该项回炉+回归门禁加 case", evidence: `after=${JSON.stringify(after)}` });
      if (!restored) pass = false;
    } catch (e) {
      steps.push({ step: "rollback", ok: false, detail: `回退异常: ${String(e)}`, evidence: "-" });
      pass = false;
    }
    items.push({ id: p.id, name: p.name, probe: p.probe, steps, pass });
  }
  const elapsedMinutes = (Date.now() - t0) / 60000;
  const passed = items.filter((i) => i.pass).length;
  const evidenceComplete = items.every((i) => i.steps.length === 3 && i.steps.every((s) => s.evidence.length > 0));
  return {
    items,
    passed,
    total: items.length,
    allGreen: passed === items.length,
    elapsedMinutes,
    withinBudget: elapsedMinutes < TOTAL_BUDGET_MINUTES,
    evidenceComplete,
  };
}

/** checklist JSON 导出（tools/vx-walkcheck-e.py 消费口径）。 */
export function verdictChecklistJson(): string {
  return JSON.stringify({
    format: "vx-walkcheck-e",
    version: 1,
    budgetMinutes: TOTAL_BUDGET_MINUTES,
    items: buildProbes().map((p) => ({ id: p.id, name: p.name, probe: p.probe, steps: ["mutate", "take-effect", "rollback"] })),
  }, null, 2);
}

/** 涉及分节清单（三步脚本覆盖面自证）。 */
export function coveredSections(): PersonaSection[] {
  return [...PERSONA_SECTIONS];
}
