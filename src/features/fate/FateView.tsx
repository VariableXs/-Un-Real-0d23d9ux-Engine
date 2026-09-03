/**
 * 命运推演空间（FTPE · 第三大独立功能空间）。
 * - 三大维度字典（人格/性格/思想）+ 事件库 + 随机因素，全部无模板、-100..+100 无级滑块
 * - 推演：确定性种子 → 多分支命运树（主干/分支/结局印章）
 * - 续分：双击事件节点 → 子树再推演 → 反向替换主干下游（10.1/10.2）
 * - 档案：.fatetree 与 .mindmap/.project 平级保存，含种子可重现
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save as saveDialog, open as fileDialog } from "@tauri-apps/plugin-dialog";
import {
  PenLine, Save, FileDown, Maximize2, Play, Sparkles, Users, Heart,
  BookOpen, Zap, Dices, ListEnd, FolderOpen, GitBranch, Plus, Trash2,
  Shuffle, Repeat, ListPlus, ChevronRight, ChevronLeft,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast, uiStore } from "../../state/uiStore";
import {
  simulate, drillDown, continueSimulate, layoutTree, treeStats, collectTraits, catColor, catName,
  emptyProfile, defaultParams, ideologyConflicts, type TraitSource,
} from "./engine";
import { enName, tagLabel } from "./dictionaries.en";
import {
  PRESET_PERSONALITIES, PRESET_CHARACTERS, PRESET_IDEOLOGIES, PRESET_EVENTS, PRESET_RANDOMS, PRESET_BUFFS,
} from "./dictionaries";
import type {
  CharacterEntry, EventEntry, FateDoc, FateNode, IdeologyEntry, PersonalityEntry,
  RandomFactor, RoleProfile, SimParams,
} from "./types";

/** 会话单例：切换空间不丢推演状态。 */
const fateSession: { doc: FateDoc | null; filePath: string | null; vp: Vp | null } = { doc: null, filePath: null, vp: null };
/** 自定义字典的本地持久化（规范 14.2/14.4：持续扩展、离线保存）。 */
const CUSTOM_LS = "fate.customDict.v1";

/** 行为树总结框架：移入的构建项（人格/性格/思想/事件/随机因素），随会话持久。 */
interface SummaryItem {
  key: string;
  kind: "personality" | "character" | "ideology" | "event" | "random";
  id: string;
  name: string;
  value: number;
  custom: boolean;
}
const SUMMARY_LS = "fate.summary.v1";
const fateSessionSummary: { items: SummaryItem[] } = { items: loadSummary() };

function loadSummary(): SummaryItem[] {
  try {
    const raw = localStorage.getItem(SUMMARY_LS);
    if (raw) {
      const v = JSON.parse(raw) as { items?: SummaryItem[] };
      if (Array.isArray(v.items)) return v.items;
    }
  } catch { /* corrupt → empty */ }
  return [];
}

function loadCustomDict(): FateDoc["customDict"] {
  try {
    const raw = localStorage.getItem(CUSTOM_LS);
    if (raw) {
      const v = JSON.parse(raw) as Partial<FateDoc["customDict"]>;
      return { personalities: [], characters: [], ideologies: [], events: [], randoms: [], ...v };
    }
  } catch { /* corrupt overrides fall back to presets */ }
  return { personalities: [], characters: [], ideologies: [], events: [], randoms: [] };
}

interface Vp { x: number; y: number; z: number }
type Tab = "personality" | "character" | "ideology" | "events" | "factors" | "params";

/** v3.0 1.2：从推演结果里抽将来可能发生的核心事件关键词（供舞台闪光）。 */
function stageKeywords(root: FateNode, n: number): string[] {
  const pool: string[] = [];
  const walk = (node: FateNode, depth: number): void => {
    if (depth >= 1 && node.name && node.cat !== "ending" && node.cat !== "root") pool.push(node.name);
    if (depth > 2) return;
    for (const c of node.children) walk(c, depth + 1);
  };
  walk(root, 0);
  // deterministic shuffle so the same seed lights the same stage
  for (let i = pool.length - 1; i > 0; i--) {
    const j = (i * 7 + root.id.length * 13) % (i + 1);
    [pool[i], pool[j]] = [pool[j]!, pool[i]!];
  }
  return pool.slice(0, n);
}

export function FateView(): React.ReactElement {
  const { lang } = useI18n();
  const [profile, setProfile] = useState<RoleProfile>(fateSession.doc?.profile ?? emptyProfile());
  const [params, setParams] = useState<SimParams>(fateSession.doc?.params ?? defaultParams());
  const [custom, setCustom] = useState<FateDoc["customDict"]>(fateSession.doc?.customDict ?? loadCustomDict());
  const [root, setRoot] = useState<FateNode | null>(fateSession.doc?.root ?? null);
  const [tab, setTab] = useState<Tab>("personality");
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<{ kind: Tab; id: string } | null>(null);
  const [vp, setVp] = useState<Vp>(fateSession.vp ?? { x: 0, y: 0, z: 0.75 });
  const [panActive, setPanActive] = useState(false);
  /** v3.0 1.2 中央舞台推演动画（Stage of Fate）状态。 */
  const [stage, setStage] = useState<{ keywords: string[] } | null>(null);
  /** 行为树总结框架：固定停靠面板，不遮挡主画布。 */
  const [summary, setSummary] = useState<SummaryItem[]>(fateSessionSummary.items);
  const [summaryOpen, setSummaryOpen] = useState(true);
  /** 模块二：节点/连线的选中编辑状态（连线选中 = 其子节点）。 */
  const [selNode, setSelNode] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editProb, setEditProb] = useState("");
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null);
  const vpRef = useRef(vp);
  vpRef.current = vp;
  const filePathRef = useRef(fateSession.filePath);

  const personalities = useMemo(() => [...PRESET_PERSONALITIES, ...custom.personalities], [custom.personalities]);
  const characters = useMemo(() => [...PRESET_CHARACTERS, ...custom.characters], [custom.characters]);
  const ideologies = useMemo(() => [...PRESET_IDEOLOGIES, ...custom.ideologies], [custom.ideologies]);
  const events = useMemo(() => [...PRESET_EVENTS, ...custom.events], [custom.events]);
  const randoms = useMemo(() => PRESET_RANDOMS.map((p) => custom.randoms.find((c) => c.id === p.id) ?? p).concat(custom.randoms.filter((c) => !PRESET_RANDOMS.some((p) => p.id === c.id))), [custom.randoms]);

  // ---------- 行为树总结框架（第 N 章）：移入 / 删除 / 随机构成 ----------
  useEffect(() => {
    fateSessionSummary.items = summary;
    try { localStorage.setItem(SUMMARY_LS, JSON.stringify({ items: summary })); } catch { /* quota */ }
  }, [summary]);

  /** 英文模式下预置条目的本地化显示名。 */
  function LName(kind: "personality" | "character" | "ideology" | "random" | "event", id: string, fallback: string): string {
    if (lang !== "en") return fallback;
    return enName(kind, id)?.n ?? fallback;
  }

  /** 数值卡片唯一写回入口：不存在则先加入 profile，再写值（修复滑条/输入失效）。 */
  function setTraitValue(kind: Tab, id: string, v: number): void {
    setProfile((pr) => {
      if (kind === "personality") {
        const has = pr.personalities.some((x) => x.id === id);
        return { ...pr, personalities: has ? pr.personalities.map((x) => x.id === id ? { ...x, weight: v } : x) : [...pr.personalities, { id, weight: v }] };
      }
      if (kind === "character") {
        const has = pr.characters.some((x) => x.id === id);
        return { ...pr, characters: has ? pr.characters.map((x) => x.id === id ? { ...x, value: v } : x) : [...pr.characters, { id, value: v }] };
      }
      const has = pr.ideologies.some((x) => x.id === id);
      return { ...pr, ideologies: has ? pr.ideologies.map((x) => x.id === id ? { ...x, value: v } : x) : [...pr.ideologies, { id, value: v }] };
    });
    // 同步总结框架中的该项数值，构建与推演数据始终一致。
    setSummary((s) => s.map((it) => it.kind === kind && it.id === id ? { ...it, value: v } : it));
  }

  /** 单项移入总结框架（幂等：已存在则仅更新数值）。 */
  function moveToSummary(kind: SummaryItem["kind"], id: string): void {
    let name = "";
    let value = 0;
    let isCustom = false;
    if (kind === "personality") {
      const e = personalities.find((x) => x.id === id);
      if (!e) return;
      name = LName("personality", e.id, e.name);
      value = profile.personalities.find((x) => x.id === id)?.weight ?? 60;
      isCustom = !!e.custom;
    } else if (kind === "character") {
      const e = characters.find((x) => x.id === id);
      if (!e) return;
      name = LName("character", e.id, e.name);
      value = profile.characters.find((x) => x.id === id)?.value ?? 0;
      isCustom = !!e.custom;
    } else if (kind === "ideology") {
      const e = ideologies.find((x) => x.id === id);
      if (!e) return;
      name = LName("ideology", e.id, e.name);
      value = profile.ideologies.find((x) => x.id === id)?.value ?? 50;
      isCustom = !!e.custom;
    } else if (kind === "event") {
      const e = events.find((x) => x.id === id);
      if (!e) return;
      name = LName("event", e.id, e.name);
      value = Math.round(e.base * 100);
      isCustom = !!e.custom;
    } else {
      const f = randoms.find((x) => x.id === id);
      if (!f) return;
      name = LName("random", f.id, f.name);
      value = f.strength;
      isCustom = !!f.custom;
    }
    setSummary((s) => {
      const key = `${kind}:${id}`;
      const rest = s.filter((it) => it.key !== key);
      return [...rest, { key, kind, id, name, value, custom: isCustom }];
    });
    pushToast("info", lang === "zh" ? "已移入总结框架" : "Moved into summary", name);
  }

  /** 总结框架内实时删除：同时从 profile / 自定义字典移除，构建与推演数据同步。 */
  function removeFromSummary(item: SummaryItem): void {
    setSummary((s) => s.filter((it) => it.key !== item.key));
    if (item.custom) {
      deleteCustom(item.kind === "random" ? "factors" : (item.kind === "event" ? "events" : (item.kind as Tab)), item.id);
    } else if (item.kind !== "event" && item.kind !== "random") {
      setProfile((p) => ({
        ...p,
        personalities: item.kind === "personality" ? p.personalities.filter((x) => x.id !== item.id) : p.personalities,
        characters: item.kind === "character" ? p.characters.filter((x) => x.id !== item.id) : p.characters,
        ideologies: item.kind === "ideology" ? p.ideologies.filter((x) => x.id !== item.id) : p.ideologies,
      }));
    }
    pushToast("info", lang === "zh" ? "已从总结框架删除并同步" : "Removed from summary and synced", item.name);
  }

  /** 随机构成：一键随机生成人格/性格/思想组合，并全部放入总结框架。 */
  function randomCompose(): void {
    const nP = 1 + Math.floor(Math.random() * 3);
    const nC = 3 + Math.floor(Math.random() * 4);
    const nI = Math.floor(Math.random() * 3);
    const chosenP = [...personalities].sort(() => Math.random() - 0.5).slice(0, nP);
    const chosenC = [...characters].sort(() => Math.random() - 0.5).slice(0, nC);
    const chosenI = [...ideologies].sort(() => Math.random() - 0.5).slice(0, nI);
    const pList = chosenP.map((e) => ({ id: e.id, weight: 30 + Math.floor(Math.random() * 61) }));
    const cList = chosenC.map((e) => ({ id: e.id, value: Math.floor(Math.random() * 201) - 100 }));
    const iList = chosenI.map((e) => ({ id: e.id, value: Math.floor(Math.random() * 201) - 100 }));
    setProfile((pr) => ({ ...pr, personalities: pList, characters: cList, ideologies: iList }));
    setSummary((s) => [
      ...s.filter((it) => !(it.kind === "personality" || it.kind === "character" || it.kind === "ideology")),
      ...pList.map((x) => {
        const e = personalities.find((y) => y.id === x.id)!;
        return { key: `personality:${x.id}`, kind: "personality" as const, id: x.id, name: LName("personality", x.id, e.name), value: x.weight, custom: !!e.custom };
      }),
      ...cList.map((x) => {
        const e = characters.find((y) => y.id === x.id)!;
        return { key: `character:${x.id}`, kind: "character" as const, id: x.id, name: LName("character", x.id, e.name), value: x.value, custom: !!e.custom };
      }),
      ...iList.map((x) => {
        const e = ideologies.find((y) => y.id === x.id)!;
        return { key: `ideology:${x.id}`, kind: "ideology" as const, id: x.id, name: LName("ideology", x.id, e.name), value: x.value, custom: !!e.custom };
      }),
    ]);
    pushToast("success", lang === "zh" ? "随机构成完成" : "Random compose done",
      lang === "zh" ? `${nP} 人格 · ${nC} 性格 · ${nI} 思想` : `${nP} personas · ${nC} traits · ${nI} ideologies`);
  }

  /** 行为树继续推演：在已有树的全部结局叶上再延伸若干步，不清空原树。 */
  function continueSim(): void {
    if (!root) return;
    const traits: TraitSource[] = collectTraits(profile, personalities, characters, ideologies);
    const enabledFactors = randoms.filter((f) => f.enabled);
    const randomMult = 1 + (enabledFactors.length > 0
      ? enabledFactors.reduce((a, f) => a + f.strength / 100, 0) / enabledFactors.length * 0.5 : 0);
    const struggleBoost = ideologyConflicts(profile, ideologies).length * 0.15;
    const activeBuffs = PRESET_BUFFS.filter((b) => params.buffs?.includes(b.id));
    const next = continueSimulate(root, params, events, traits, randomMult, struggleBoost, Math.min(3, Math.max(1, params.years >> 2)), lang, activeBuffs);
    setRoot({ ...next });
    syncDoc({ ...next });
    requestAnimationFrame(() => fitTree(next));
    const st = treeStats(next);
    pushToast("success", lang === "zh" ? "行为树已继续推演" : "Behavior tree extended",
      lang === "zh" ? `共 ${st.count} 个节点 · 深度 ${st.maxDepth}` : `${st.count} nodes · depth ${st.maxDepth}`);
  }

  const markSession = useCallback((doc: FateDoc | null, path: string | null): void => {
    fateSession.doc = doc;
    fateSession.filePath = path;
  }, []);

  function syncDoc(r: FateNode | null): void {
    const doc: FateDoc = {
      app: "variable-fatetree",
      formatVersion: 1,
      name: profile.name || "未命名推演",
      savedAt: Date.now(),
      profile, params, root: r ?? (root as never), versions: [],
      customDict: custom, notes: "", viewport: vpRef.current,
    };
    fateSession.doc = doc;
  }

  useEffect(() => {
    try { localStorage.setItem(CUSTOM_LS, JSON.stringify(custom)); } catch { /* quota */ }
  }, [custom]);

  // 内置工作区双击 .fatetree 文件 → 完整载入（规范 五：点开=完整加载，非只改标题）。
  useEffect(() => {
    const onOpen = (ev: Event): void => {
      const path = (ev as CustomEvent<{ path: string }>).detail?.path;
      if (path) void openDocAt(path);
    };
    window.addEventListener("variable:fate-open-file", onOpen);
    return () => window.removeEventListener("variable:fate-open-file", onOpen);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lang, profile, params, custom]);

  // 定位打开：工作区把 fatePendingOpen 交给本空间，挂载后完整载入对应档案。
  useEffect(() => {
    const pending = uiStore.getState().fatePendingOpen;
    if (!pending) return;
    uiStore.setState({ fatePendingOpen: null });
    void openDocAt(pending);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ---------- 推演（第九章） ----------
  function runSim(): void {
    if (profile.personalities.length === 0 && profile.characters.length === 0 && profile.ideologies.length === 0) {
      pushToast("error", lang === "zh" ? "请至少选择一个人格、性格或思想" : "Pick at least one trait");
      return;
    }
    const traits: TraitSource[] = collectTraits(profile, personalities, characters, ideologies);
    const enabledFactors = randoms.filter((f) => f.enabled);
    const randomMult = 1 + (enabledFactors.length > 0
      ? enabledFactors.reduce((a, f) => a + f.strength / 100, 0) / enabledFactors.length * 0.5 : 0);
    const struggles = ideologyConflicts(profile, ideologies);
    const struggleBoost = struggles.length * 0.15;
    const activeBuffs = PRESET_BUFFS.filter((b) => params.buffs?.includes(b.id));
    const r = simulate(profile, params, events, traits, randomMult, struggleBoost, lang, activeBuffs);
    // v3.0 1.2：不直接跳转——先进入"中央舞台"编织逻辑链，绽放后无缝转入命运树空间。
    setStage({ keywords: stageKeywords(r, 9) });
    window.setTimeout(() => {
      setStage(null);
      setRoot(r);
      syncDoc(r);
      setProfile({ ...profile });
      requestAnimationFrame(() => fitTree(r));
      pushToast("success", lang === "zh" ? "推演完成" : "Simulation done",
        lang === "zh" ? `共 ${treeStats(r).count} 个节点 · ${struggles.length > 0 ? `内心挣扎 ×${struggles.length}` : "无思想冲突"}` : "");
    }, 2700);
  }

  /** 10.1/10.2 续分：双击事件节点 → 子树再推演 → 反向替换主干下游。 */
  function drill(node: FateNode): void {
    if (!root || node.cat === "ending" || node.cat === "root") return;
    const traits: TraitSource[] = collectTraits(profile, personalities, characters, ideologies);
    const activeBuffs = PRESET_BUFFS.filter((b) => params.buffs?.includes(b.id));
    const sub = drillDown(node, params, events, traits, 1, 0, (Date.now() & 0xffff), 3, lang, activeBuffs);
    const replace = (n: FateNode): boolean => {
      if (n.id === node.id) { n.children = sub.children; return true; }
      return n.children.some(replace);
    };
    replace(root);
    setRoot({ ...root });
    syncDoc(root);
    pushToast("success", lang === "zh" ? "子树已续分并反馈主干" : "Subtree drilled into trunk");
  }

  /** ---------- 模块二：节点/连线 100% 可交互（单击选中 → 就地改名/改概率/续分/删除） ---------- */
  function selectNode(id: string | null): void {
    setSelNode(id);
    if (!id || !root) { setEditName(""); setEditProb(""); return; }
    const found = findNode(root, id);
    if (found) {
      setEditName(found.name);
      setEditProb(String(Math.round(found.prob * 100)));
    }
  }

  function findNode(n: FateNode, id: string): FateNode | null {
    if (n.id === id) return n;
    for (const c of n.children) {
      const r = findNode(c, id);
      if (r) return r;
    }
    return null;
  }

  /** 修改选中节点（就地改写树并同步会话档案）。 */
  function patchNode(patch: { name?: string; prob?: number }): void {
    if (!root || !selNode) return;
    const walk = (n: FateNode): void => {
      if (n.id === selNode) {
        if (patch.name !== undefined) n.name = patch.name;
        if (patch.prob !== undefined) n.prob = Math.round(patch.prob * 1000) / 1000;
      }
      n.children.forEach(walk);
    };
    walk(root);
    setRoot({ ...root });
    syncDoc(root);
  }

  /** 删除选中节点所在的整个分支（根节点不可删）。 */
  function removeSelBranch(): void {
    if (!root || !selNode) return;
    if (selNode === root.id) return;
    const cut = (n: FateNode): boolean => {
      const before = n.children.length;
      n.children = n.children.filter((c) => c.id !== selNode);
      if (n.children.length !== before) return true;
      return n.children.some(cut);
    };
    if (cut(root)) {
      setSelNode(null);
      setRoot({ ...root });
      syncDoc(root);
      pushToast("info", lang === "zh" ? "分支已删除" : "Branch removed");
    }
  }

  function fitTree(r: FateNode | null = root): void {
    if (!r) return;
    const pos = layoutTree(r);
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    for (const p of pos.values()) {
      minX = Math.min(minX, p.x); maxX = Math.max(maxX, p.x);
      minY = Math.min(minY, p.y); maxY = Math.max(maxY, p.y);
    }
    const el = containerRef.current;
    const pad = 90;
    const zw = ((el?.clientWidth ?? 900) - pad * 2) / Math.max(1, maxX - minX + 200);
    const zh = ((el?.clientHeight ?? 640) - pad * 2) / Math.max(1, maxY - minY + 160);
    const z = Math.min(1.1, Math.max(0.12, Math.min(zw, zh)));
    // 模块一：左移视口留出总结框架长条的位置，默认视野不压住缝隙区
    setVp({ z, x: -minX * z + 250, y: -(minY + maxY) / 2 * z });
  }

  // ---------- .fatetree 存取（第十三章） ----------
  async function saveAs(): Promise<void> {
    const r = root;
    if (!r) return;
    const p = await saveDialog({ defaultPath: `${profile.name || "命运推演"}.fatetree`, filters: [{ name: "Fate Tree", extensions: ["fatetree"] }] });
    if (typeof p !== "string") return;
    await writeDoc(p);
  }

  async function save(): Promise<void> {
    if (filePathRef.current) await writeDoc(filePathRef.current);
    else await saveAs();
  }

  async function writeDoc(path: string): Promise<void> {
    try {
      const doc: FateDoc = {
        app: "variable-fatetree", formatVersion: 1,
        name: profile.name || "未命名推演", savedAt: Date.now(),
        profile, params, root: root!, versions: [], customDict: custom, notes: "", viewport: vpRef.current,
      };
      await ipc.saveTextFile(path, JSON.stringify(doc), true);
      filePathRef.current = path;
      markSession(doc, path);
      pushToast("success", lang === "zh" ? "命运档案已保存" : "Fate archive saved", path);
    } catch (e) {
      pushToast("error", lang === "zh" ? "保存失败" : "Save failed", errMessage(e).message);
    }
  }

  async function openDoc(): Promise<void> {
    try {
      const p = await fileDialog({ multiple: false, filters: [{ name: "Fate Tree", extensions: ["fatetree"] }] });
      if (typeof p !== "string") return;
      await openDocAt(p);
    } catch (e) {
      pushToast("error", lang === "zh" ? "档案打开失败" : "Open failed", errMessage(e).message);
    }
  }

  async function openDocAt(p: string): Promise<void> {
    try {
      const parsed = JSON.parse(await ipc.wsReadText(p)) as FateDoc;
      if (parsed.app !== "variable-fatetree") throw new Error(lang === "zh" ? "不是有效的 .fatetree 文件" : "not a fatetree");
      setProfile(parsed.profile);
      setParams({ ...defaultParams(), ...parsed.params });
      setCustom({ ...{ personalities: [], characters: [], ideologies: [], events: [], randoms: [] }, ...parsed.customDict });
      setRoot(parsed.root);
      filePathRef.current = p;
      markSession(parsed, p);
      // v3.0 1.3：瞬间恢复到上次编辑的完整状态和视口位置（无存档视口才 fit）。
      if (parsed.viewport) setVp(parsed.viewport);
      else requestAnimationFrame(() => fitTree(parsed.root));
      pushToast("success", lang === "zh" ? "命运档案已打开" : "Opened", parsed.name);
    } catch (e) {
      pushToast("error", lang === "zh" ? "档案打开失败" : "Open failed", errMessage(e).message);
    }
  }

  /** 13.2 命运树 → 思维导图：一键完整转换并保存为 .mindmap 文件。 */
  async function exportMindmap(): Promise<void> {
    const r = root;
    if (!r) return;
    try {
      const pos = layoutTree(r);
      const nodes: Record<string, unknown>[] = [];
      const edges: Record<string, unknown>[] = [];
      const walk = (n: FateNode): void => {
        const p = pos.get(n.id);
        if (!p) return;
        nodes.push({
          id: n.id,
          textHtml: `<p><strong>${escapeHtml(lang === "en" ? enName("event", n.id)?.n ?? n.name : n.name)}</strong></p><p><span style="color:#8fb0ff">${escapeHtml(catName(n.cat, lang))} · ${Math.round(n.prob * 100)}%${n.ending ? ` · ${escapeHtml(n.ending.type)}` : ""}</span></p>`,
          x: p.x, y: p.y, width: 210, height: 68,
        });
        for (const c of n.children) {
          edges.push({ id: `${n.id}->${c.id}`, sourceNodeId: n.id, targetNodeId: c.id });
          walk(c);
        }
      };
      walk(r);
      const p = await saveDialog({
        defaultPath: `${profile.name || "命运推演"}.mindmap`,
        filters: [{ name: "Mindmap", extensions: ["mindmap"] }],
      });
      if (typeof p !== "string") return;
      await ipc.saveTextFile(p, JSON.stringify({ app: "variable-mindmap", formatVersion: 1, name: profile.name, nodes, edges }, null, 2), true);
      pushToast("success", lang === "zh" ? "已导出为思维导图" : "Exported as mindmap", p);
    } catch (e) {
      pushToast("error", lang === "zh" ? "导出失败" : "Export failed", errMessage(e).message);
    }
  }

  // ---------- 画布交互（平移/缩放，与主软件同套手感） ----------
  function onPointerDown(e: React.PointerEvent): void {
    if ((e.target as HTMLElement).closest("button, input, textarea, select, .ft-node, .ft-panel, .ctx-menu, .card-pop, .ft-node-edit")) return;
    containerRef.current?.focus({ preventScroll: true });
    if (e.button === 0) { setPanActive(true); selectNode(null); }
    dragRef.current = { sx: e.clientX, sy: e.clientY, ox: vpRef.current.x, oy: vpRef.current.y };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: React.PointerEvent): void {
    const d = dragRef.current;
    if (!d) return;
    setVp({ ...vpRef.current, x: d.ox + (e.clientX - d.sx), y: d.oy + (e.clientY - d.sy) });
  }
  function onPointerUp(): void {
    dragRef.current = null;
    setPanActive(false);
  }
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent): void => {
      e.preventDefault();
      if (dragRef.current) return;
      const r = el.getBoundingClientRect();
      const px = e.clientX - r.left - r.width / 2;
      const py = e.clientY - r.top - r.height / 2;
      const cur = vpRef.current;
      const z2 = Math.min(2.2, Math.max(0.1, cur.z * Math.pow(1.0016, -e.deltaY)));
      const wx = (px - cur.x) / cur.z;
      const wy = (py - cur.y) / cur.z;
      setVp({ z: z2, x: px - wx * z2, y: py - wy * z2 });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [root]);

  // ---------- v3.0 1.3 · WASD 平滑漫游（与思维导图同套手感，输入时不抢键） ----------
  const fateKeys = useRef<Set<string>>(new Set());
  const fateRaf = useRef(0);
  useEffect(() => {
    const MOVES = new Set(["w", "a", "s", "d"]);
    const ALIAS: Record<string, string> = { ArrowUp: "w", ArrowDown: "s", ArrowLeft: "a", ArrowRight: "d" };
    const trackDown = (e: KeyboardEvent): void => {
      const ae = document.activeElement as HTMLElement | null;
      if (ae && (ae.tagName === "INPUT" || ae.tagName === "TEXTAREA" || ae.isContentEditable)) return;
      const alias = ALIAS[e.key];
      const k = e.key.toLowerCase();
      if (MOVES.has(k) || alias) {
        fateKeys.current.add(MOVES.has(k) ? k : alias ?? k);
        if (!e.repeat) {
          let last = performance.now();
          cancelAnimationFrame(fateRaf.current);
          const tick = (now: number): void => {
            const dt = Math.min(0.05, (now - last) / 1000);
            last = now;
            const ae2 = document.activeElement as HTMLElement | null;
            const typing = !!ae2 && (ae2.tagName === "INPUT" || ae2.tagName === "TEXTAREA" || ae2.isContentEditable);
            if (!typing && fateKeys.current.size > 0) {
              const boost = (e.shiftKey ? 2.4 : 1) * 420;
              let dx = 0, dy = 0;
              if (fateKeys.current.has("w")) dy += boost;
              if (fateKeys.current.has("s")) dy -= boost;
              if (fateKeys.current.has("a")) dx += boost;
              if (fateKeys.current.has("d")) dx -= boost;
              setVp((v) => ({ ...v, x: v.x + dx * dt, y: v.y + dy * dt }));
            }
            fateRaf.current = fateKeys.current.size > 0 ? requestAnimationFrame(tick) : 0;
          };
          fateRaf.current = requestAnimationFrame(tick);
        }
      }
    };
    const trackUp = (e: KeyboardEvent): void => {
      const k = e.key.toLowerCase();
      fateKeys.current.delete(k);
      const alias = ALIAS[e.key];
      if (alias) fateKeys.current.delete(alias);
    };
    const stop = (): void => fateKeys.current.clear();
    window.addEventListener("keydown", trackDown, true);
    window.addEventListener("keyup", trackUp, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", trackDown, true);
      window.removeEventListener("keyup", trackUp, true);
      window.removeEventListener("blur", stop);
      cancelAnimationFrame(fateRaf.current);
    };
  }, []);

  // ---------- v3.0 1.3 · 视口位置持久化：切空间/保存/关闭均不丢漫游位置 ----------
  useEffect(() => {
    fateSession.vp = vpRef.current;
  }, [vp]);
  useEffect(() => () => { fateSession.vp = vpRef.current; }, []);

  // ---------- 字典编辑助手 ----------
  function uid(prefix: string): string {
    return `${prefix}-u${Date.now().toString(36)}${Math.floor(Math.random() * 1e6).toString(36)}`;
  }

  /** 14.4 添加全新条目（人格/性格/思想/事件）。 */
  function addCustomEntry(kind: Tab): void {
    const name = query.trim();
    if (kind === "personality") {
      const e: PersonalityEntry = { id: uid("p"), name: name || (lang === "zh" ? "新人格" : "New personality"), desc: "", effects: [{ tag: "内心", weight: 0.3 }], compat: [], conflict: [], custom: true, enabled: true };
      setCustom((c) => ({ ...c, personalities: [...c.personalities, e] }));
      setEditing({ kind, id: e.id });
    } else if (kind === "character") {
      const e: CharacterEntry = { id: uid("c"), name: name || (lang === "zh" ? "新特质" : "New trait"), opposite: "—", desc: "", effects: [{ tag: "冒险", weight: 0.3 }], custom: true, enabled: true };
      setCustom((c) => ({ ...c, characters: [...c.characters, e] }));
      setEditing({ kind, id: e.id });
    } else if (kind === "ideology") {
      const e: IdeologyEntry = { id: uid("i"), name: name || (lang === "zh" ? "新思想" : "New ideology"), desc: "", effects: [{ tag: "觉醒", weight: 0.3 }], conflict: [], custom: true, enabled: true };
      setCustom((c) => ({ ...c, ideologies: [...c.ideologies, e] }));
      setEditing({ kind, id: e.id });
    } else if (kind === "events") {
      const e: EventEntry = { id: uid("e"), name: name || (lang === "zh" ? "新事件" : "New event"), cat: "L2", base: 0.3, desc: "", tags: ["冒险"], custom: true, enabled: true };
      setCustom((c) => ({ ...c, events: [...c.events, e] }));
      setEditing({ kind, id: e.id });
    }
    setQuery("");
  }

  /** 14.2 删除自定义条目（预置条目不可删，恢复默认即可）。 */
  function deleteCustom(kind: Tab, id: string): void {
    setCustom((c) => ({
      ...c,
      personalities: kind === "personality" ? c.personalities.filter((x) => x.id !== id) : c.personalities,
      characters: kind === "character" ? c.characters.filter((x) => x.id !== id) : c.characters,
      ideologies: kind === "ideology" ? c.ideologies.filter((x) => x.id !== id) : c.ideologies,
      events: kind === "events" ? c.events.filter((x) => x.id !== id) : c.events,
    }));
    setProfile((p) => ({
      ...p,
      personalities: p.personalities.filter((x) => x.id !== id),
      characters: p.characters.filter((x) => x.id !== id),
      ideologies: p.ideologies.filter((x) => x.id !== id),
    }));
    setEditing(null);
    pushToast("info", lang === "zh" ? "已删除自定义条目" : "Custom entry deleted");
  }

  const TABS: Array<{ id: Tab; label: string; icon: React.ReactNode }> = [
    { id: "personality", label: lang === "zh" ? "人格" : "Persona", icon: <Users size={12} /> },
    { id: "character", label: lang === "zh" ? "性格" : "Character", icon: <Heart size={12} /> },
    { id: "ideology", label: lang === "zh" ? "思想" : "Ideology", icon: <BookOpen size={12} /> },
    { id: "events", label: lang === "zh" ? "事件库" : "Events", icon: <Zap size={12} /> },
    { id: "factors", label: lang === "zh" ? "随机" : "Random", icon: <Dices size={12} /> },
    { id: "params", label: lang === "zh" ? "参数" : "Params", icon: <ListEnd size={12} /> },
  ];

  const pos = useMemo(() => (root ? layoutTree(root) : null), [root]);
  const stats = useMemo(() => (root ? treeStats(root) : null), [root]);
  const struggles = useMemo(() => ideologyConflicts(profile, ideologies), [profile, ideologies]);

  return (
    <div className="fate-view">
      <div className="pv-toolbar fate-toolbar">
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "返回写作" : "Back"} onClick={() => uiStore.setState({ mode: "write" })}>
          <PenLine size={14} />
        </button>
        <span className="pv-title"><Sparkles size={14} /> {lang === "zh" ? "命运推演" : "Fate"}</span>
        <input
          className="ft-name"
          value={profile.name}
          placeholder={lang === "zh" ? "角色名…" : "role name…"}
          onChange={(e) => setProfile({ ...profile, name: e.target.value })}
          onKeyDown={(e) => e.stopPropagation()}
        />
        {struggles.length > 0 && (
          <span className="pv-badge warn">{lang === "zh" ? `内心挣扎 ×${struggles.length}` : `conflicts ×${struggles.length}`}</span>
        )}
        <span className="flex-1" />
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "随机构成（随机生成人格/性格/思想组合并放入总结）" : "Random compose (random trait set into summary)"} onClick={randomCompose}>
          <Shuffle size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "行为树继续推演（在原树上延伸，不清空）" : "Continue simulating (extend the tree, nothing is cleared)"} disabled={!root} onClick={continueSim}>
          <Repeat size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "打开 .fatetree" : "Open .fatetree"} onClick={() => void openDoc()}>
          <FolderOpen size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "保存 .fatetree" : "Save .fatetree"} disabled={!root} onClick={() => void save()}>
          <Save size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "另存为" : "Save as"} disabled={!root} onClick={() => void saveAs()}>
          <FileDown size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "导出为思维导图" : "Export as mindmap"} disabled={!root} onClick={() => void exportMindmap()}>
          <GitBranch size={14} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "适应全部" : "Fit all"} disabled={!root} onClick={() => fitTree()}>
          <Maximize2 size={14} />
        </button>
        <button type="button" className="btn tiny primary ft-run" onClick={runSim}>
          <Play size={12} /> {lang === "zh" ? "开始推演" : "Simulate"}
        </button>
      </div>

      <div className="pv-body">
        {/* ---------- 左：六域编辑面板（独立空间，互不干扰） ---------- */}
        <div className="pv-tree ft-panel">
          <div className="ft-tabs">
            {TABS.map((t) => (
              <button key={t.id} type="button" className={`ft-tab ${tab === t.id ? "on" : ""}`} onClick={() => setTab(t.id)}>
                {t.icon} {t.label}
              </button>
            ))}
          </div>
          <div className="pv-tree-head">
            <input className="pv-tree-filter" value={query} placeholder={lang === "zh" ? "搜索…" : "Search…"} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
            {(tab === "personality" || tab === "character" || tab === "ideology" || tab === "events") && (
              <button type="button" className="icon-btn tiny" data-tip={lang === "zh" ? "以当前搜索词新建条目" : "New entry from query"}
                onClick={() => addCustomEntry(tab)}>
                <Plus size={13} />
              </button>
            )}
          </div>
          <div className="pv-tree-list">
            {tab === "personality" && personalities.filter((p) => p.name.includes(query)).map((p: PersonalityEntry) => (
              <div key={p.id} className="ft-entry">
                <div className="pv-tree-row" onClick={() => setEditing(editing?.id === p.id && editing.kind === "personality" ? null : { kind: "personality", id: p.id })}>
                  <span className="pv-tree-name ellipsis">{LName("personality", p.id, p.name)}</span>
                  {profile.personalities.some((x) => x.id === p.id) && <span className="pv-tag">✓</span>}
                  <button type="button" className="icon-btn tiny ft-move" title={lang === "zh" ? "移入总结框架" : "Move into summary"}
                    onClick={(e) => { e.stopPropagation(); moveToSummary("personality", p.id); }}>
                    <ListPlus size={11} />
                  </button>
                </div>
                {editing?.kind === "personality" && editing.id === p.id && (
                  <TraitEditor
                    name={LName("personality", p.id, p.name)} desc={p.desc} value={profile.personalities.find((x) => x.id === p.id)?.weight ?? 60}
                    valueLabel={lang === "zh" ? "权重 %" : "weight %"} min={0} max={100}
                    effects={p.effects}
                    onValue={(v) => setTraitValue("personality", p.id, v)}
                    onDelete={p.custom ? () => deleteCustom("personality", p.id) : undefined}
                  />
                )}
              </div>
            ))}
            {tab === "character" && characters.filter((c) => c.name.includes(query) || c.opposite.includes(query)).map((c: CharacterEntry) => {
              const chosen = profile.characters.find((x) => x.id === c.id);
              return (
                <div key={c.id} className="ft-entry">
                  <div className="pv-tree-row" onClick={() => setEditing(editing?.id === c.id && editing.kind === "character" ? null : { kind: "character", id: c.id })}>
                    <span className="pv-tree-name ellipsis">{LName("character", c.id, c.name)} <span className="dim small">↔ {lang === "en" ? enName("character", c.id)?.o ?? c.opposite : c.opposite}</span></span>
                    {chosen && <span className="pv-tag">{chosen.value > 0 ? "+" : ""}{chosen.value}</span>}
                    <button type="button" className="icon-btn tiny ft-move" title={lang === "zh" ? "移入总结框架" : "Move into summary"}
                      onClick={(e) => { e.stopPropagation(); moveToSummary("character", c.id); }}>
                      <ListPlus size={11} />
                    </button>
                  </div>
                  {editing?.kind === "character" && editing.id === c.id && (
                    <TraitEditor
                      name={`${LName("character", c.id, c.name)} ↔ ${lang === "en" ? enName("character", c.id)?.o ?? c.opposite : c.opposite}`} desc={c.desc}
                      value={chosen?.value ?? 0} valueLabel={lang === "zh" ? c.opposite : (enName("character", c.id)?.o ?? c.opposite)} min={-100} max={100}
                      effects={c.effects}
                      onValue={(v) => setTraitValue("character", c.id, v)}
                      onDelete={c.custom ? () => deleteCustom("character", c.id) : undefined}
                    />
                  )}
                </div>
              );
            })}
            {tab === "ideology" && ideologies.filter((i) => i.name.includes(query)).map((i: IdeologyEntry) => {
              const chosen = profile.ideologies.find((x) => x.id === i.id);
              return (
                <div key={i.id} className="ft-entry">
                  <div className="pv-tree-row" onClick={() => setEditing(editing?.id === i.id && editing.kind === "ideology" ? null : { kind: "ideology", id: i.id })}>
                    <span className="pv-tree-name ellipsis">{LName("ideology", i.id, i.name)}</span>
                    {chosen && <span className="pv-tag">{chosen.value > 0 ? "+" : ""}{chosen.value}</span>}
                    <button type="button" className="icon-btn tiny ft-move" title={lang === "zh" ? "移入总结框架" : "Move into summary"}
                      onClick={(e) => { e.stopPropagation(); moveToSummary("ideology", i.id); }}>
                      <ListPlus size={11} />
                    </button>
                  </div>
                  {editing?.kind === "ideology" && editing.id === i.id && (
                    <TraitEditor
                      name={LName("ideology", i.id, i.name)} desc={i.desc} value={chosen?.value ?? 50}
                      valueLabel={lang === "zh" ? "强度" : "intensity"} min={-100} max={100}
                      effects={i.effects}
                      onValue={(v) => setTraitValue("ideology", i.id, v)}
                      onDelete={i.custom ? () => deleteCustom("ideology", i.id) : undefined}
                    />
                  )}
                </div>
              );
            })}
            {tab === "events" && events.filter((e) => e.name.includes(query)).slice(0, 400).map((e: EventEntry) => (
              <div key={e.id} className="pv-tree-row" title={e.desc}>
                <span className="ft-cat-dot" style={{ background: catColor(e.cat) }} />
                <span className="pv-tree-name ellipsis">{LName("event", e.id, e.name)}</span>
                <span className="dim small">{Math.round(e.base * 100)}%</span>
                <button type="button" className="icon-btn tiny ft-move" title={lang === "zh" ? "移入总结框架" : "Move into summary"}
                  onClick={(e2) => { e2.stopPropagation(); moveToSummary("event", e.id); }}>
                  <ListPlus size={11} />
                </button>
                {e.custom && (
                  <button type="button" className="icon-btn tiny" onClick={() => deleteCustom("events", e.id)}>
                    <Trash2 size={11} />
                  </button>
                )}
              </div>
            ))}
            {tab === "factors" && randoms.map((f: RandomFactor) => (
              <div key={f.id} className="ft-entry">
                <div className="pv-tree-row">
                  <span className="pv-tree-name ellipsis">{LName("random", f.id, f.name)}</span>
                  <input type="range" min={-100} max={100} value={f.strength} className="ft-slider"
                    onChange={(e) => setCustom((c) => ({ ...c, randoms: c.randoms.some((x) => x.id === f.id) ? c.randoms.map((x) => x.id === f.id ? { ...x, strength: Number(e.target.value) } : x) : [...c.randoms, { ...f, strength: Number(e.target.value) }] }))} />
                  <button type="button" className="icon-btn tiny ft-move" title={lang === "zh" ? "移入总结框架" : "Move into summary"}
                    onClick={() => moveToSummary("random", f.id)}>
                    <ListPlus size={11} />
                  </button>
                </div>
              </div>
            ))}
            {tab === "params" && (
              <div className="ft-entry ft-params">
                {([
                  ["years", lang === "zh" ? "推演年数" : "years", 1, 30],
                  ["branching", lang === "zh" ? "每步分支数" : "branching", 1, 4],
                  ["chaos", lang === "zh" ? "混沌强度" : "chaos", 0, 100],
                  ["startAge", lang === "zh" ? "起始年龄" : "start age", 1, 80],
                ] as const).map(([k, label, lo, hi]) => (
                  <label key={k} className="ft-param">
                    <span>{label}: {params[k]}</span>
                    <input type="range" min={lo} max={hi} value={params[k]} className="ft-slider"
                      onChange={(e) => setParams({ ...params, [k]: Number(e.target.value) })} />
                  </label>
                ))}
                <label className="ft-param">
                  <span>{lang === "zh" ? "随机种子（可重现）" : "seed"}</span>
                  <input className="pv-tree-filter" type="number" value={params.seed}
                    onChange={(e) => setParams({ ...params, seed: Number(e.target.value) >>> 0 })} />
                </label>
                <div className="ft-param">
                  <span>{lang === "zh" ? "增益（buff）· 点击启用/关闭" : "Buffs · click to toggle"}</span>
                  <div className="ft-buff-grid">
                    {PRESET_BUFFS.map((b) => {
                      const on = params.buffs?.includes(b.id) ?? false;
                      return (
                        <button key={b.id} type="button"
                          className={`ft-buff-chip ${on ? "on" : ""}`}
                          title={`${b.desc}\n${Object.entries(b.boost).map(([t, m]) => `${t} ×${m}`).join("　")}`}
                          onClick={() => setParams((p) => {
                            const cur = p.buffs ?? [];
                            return { ...p, buffs: on ? cur.filter((x) => x !== b.id) : [...cur, b.id] };
                          })}>
                          {b.name}
                        </button>
                      );
                    })}
                  </div>
                </div>
              </div>
            )}
          </div>
          <div className="pv-tree-foot dim small">
            {root && stats ? `${stats.count} ${lang === "zh" ? "节点 · 结局" : "nodes · endings"} ${stats.endings.length}` : (lang === "zh" ? "选好特质后点「开始推演」" : "Pick traits, then simulate")}
          </div>
        </div>

        {/* ---------- 中：命运树无限画布 ---------- */}
        <div
          ref={containerRef}
          className={`pv-canvas fate-canvas ${panActive ? "panning" : ""}`}
          tabIndex={-1}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
        >
          {!root && (
            <div className="pv-empty">
              <p>{lang === "zh" ? "在左侧挑选人格、性格与主义思想，按下「开始推演」" : "Pick traits on the left, then simulate"}</p>
            </div>
          )}
          {root && pos && (
            <div className="pv-world" style={{ transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.z})`, transformOrigin: "0 0" }}>
              <svg className="pv-edges" width={1} height={1}>
                {(() => {
                  const edges: React.ReactElement[] = [];
                  const walk = (n: FateNode): void => {
                    const p1 = pos.get(n.id);
                    for (const c of n.children) {
                      const p2 = pos.get(c.id);
                      if (p1 && p2) {
                        const x1 = p1.x + 90;
                        const y1 = p1.y + 30;
                        const x2 = p2.x;
                        const y2 = p2.y + 30;
                        const dx = Math.max(40, Math.abs(x2 - x1) / 2);
                        edges.push(
                          <path
                            key={`${n.id}-${c.id}`}
                            className={`pv-edge ${panActive ? "" : "pv-edge-anim"} ${selNode === c.id ? "sel" : ""}`}
                            d={`M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`}
                            stroke={catColor(c.cat)}
                            opacity={0.35 + c.prob * 0.6}
                            strokeWidth={1 + c.prob * 1.6}
                          />,
                          // 模块二：加宽透明命中区，让每条连线均可点击（点连线 = 选中其下游节点编辑）
                          <path
                            key={`${n.id}-${c.id}-hit`}
                            className="pv-edge-hit"
                            d={`M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`}
                            stroke="transparent"
                            strokeWidth={16}
                            fill="none"
                            style={{ pointerEvents: "stroke", cursor: "pointer" }}
                            onClick={(e) => { e.stopPropagation(); selectNode(c.id); }}
                          />,
                        );
                      }
                      walk(c);
                    }
                  };
                  walk(root);
                  return edges;
                })()}
              </svg>
              {(() => {
                const out: React.ReactElement[] = [];
                const walk = (n: FateNode): void => {
                  const p = pos.get(n.id);
                  if (p) {
                    const isEnding = n.cat === "ending";
                    out.push(
                      <div
                        key={n.id}
                        className={`ft-node ${isEnding ? "ending" : ""} ${n.cat === "root" ? "root" : ""} ${selNode === n.id ? "sel" : ""}`}
                        style={{ left: p.x, top: p.y, borderColor: catColor(n.cat) }}
                        title={`${catName(n.cat, lang)} · ${lang === "zh" ? "概率" : "prob"} ${Math.round(n.prob * 100)}%\n${n.desc}\n${lang === "zh" ? "单击编辑 · 双击续分子树" : "click to edit · double-click to drill"}`}
                        onClick={(e) => { e.stopPropagation(); selectNode(n.id); }}
                        onDoubleClick={(e) => { e.stopPropagation(); drill(n); }}
                      >
                        <div className="ft-node-head">
                          <strong>{n.name}</strong>
                          {n.prob < 1 && n.cat !== "root" && <span className="dim small">{Math.round(n.prob * 100)}%</span>}
                        </div>
                        <div className="dim small">{n.ageLabel}{n.ending ? ` · ${n.ending.type}` : ""}</div>
                      </div>,
                    );
                  }
                  for (const c of n.children) walk(c);
                };
                walk(root);
                return out;
              })()}
              {/* 模块二：就地编辑浮窗（改名 / 概率 / 续分 / 删除分支），随画布缩放平移 */}
              {selNode && pos && root && (() => {
                const n = findNode(root, selNode);
                const p = selNode ? pos.get(selNode) : undefined;
                if (!n || !p) return null;
                return (
                  <div className="ft-node-edit" style={{ left: p.x + 200, top: p.y }} onPointerDown={(e) => e.stopPropagation()}>
                    <div className="ft-edit-row">
                      <input
                        className="ft-num ft-edit-name"
                        value={editName}
                        placeholder={lang === "zh" ? "节点名称" : "node name"}
                        onChange={(e) => { setEditName(e.target.value); patchNode({ name: e.target.value }); }}
                        onKeyDown={(e) => e.stopPropagation()}
                      />
                    </div>
                    <div className="ft-edit-row">
                      <input
                        className="ft-num"
                        inputMode="numeric"
                        value={editProb}
                        onChange={(e) => {
                          setEditProb(e.target.value);
                          const v = Number(e.target.value);
                          if (e.target.value.trim() !== "" && Number.isFinite(v)) patchNode({ prob: Math.max(0, Math.min(100, v)) / 100 });
                        }}
                        onKeyDown={(e) => e.stopPropagation()}
                      />
                      <span className="dim small">%</span>
                      <span className="dim small ellipsis" style={{ flex: 1 }}>{catName(n.cat, lang)}{n.ageLabel ? ` · ${n.ageLabel}` : ""}</span>
                    </div>
                    <div className="ft-edit-actions">
                      {n.cat !== "root" && n.cat !== "ending" && (
                        <button type="button" className="icon-btn tiny" title={lang === "zh" ? "续分子树" : "Drill subtree"} onClick={() => { drill(n); selectNode(null); }}>
                          <GitBranch size={11} />
                        </button>
                      )}
                      {n.id !== root.id && (
                        <button type="button" className="icon-btn tiny" title={lang === "zh" ? "删除该分支" : "Delete branch"} onClick={removeSelBranch}>
                          <Trash2 size={11} />
                        </button>
                      )}
                      <span className="flex-1" />
                      <button type="button" className="icon-btn tiny" title={lang === "zh" ? "关闭" : "Close"} onClick={() => selectNode(null)}>
                        ✕
                      </button>
                    </div>
                  </div>
                );
              })()}
            </div>
          )}

          {/* ---------- 结局全览（9.4） ---------- */}
          {root && stats && stats.endings.length > 0 && (
            <div className="pv-refs card-pop ft-endings">
              <div className="pv-info-head">
                <ListEnd size={13} />
                <strong>{lang === "zh" ? "结局全览" : "Endings"}</strong>
              </div>
              {stats.endings.slice(0, 12).map((e) => (
                <div key={e.id} className="pv-ref-row" title={e.ending?.text}>
                  <span className="pv-ref-dot" style={{ background: "#f8d4e4" }} />
                  <span className="ellipsis">{e.ending?.type} · {e.name}（{e.ageLabel}）</span>
                </div>
              ))}
            </div>
          )}

          <div className="pv-status card-pop">
            <span>{stats ? `${stats.count} ${lang === "zh" ? "节点" : "nodes"} · ${lang === "zh" ? "深度" : "depth"} ${stats.maxDepth}` : (lang === "zh" ? "未推演" : "idle")}</span>
            <span className="sep">·</span>
            <span>{lang === "zh" ? "双击事件节点续分子树" : "double-click a node to drill"}</span>
            <span className="sep">·</span>
            <span>{Math.round(vp.z * 100)}%</span>
          </div>
        </div>

        {/* ---------- 行为树总结框架：停靠在字典面板与画布交界的纵向缝隙，玻璃拟态、不遮挡主视野 ---------- */}
        <div className={`ft-summary ${summaryOpen ? "open" : "folded"}`}>
          <div className="ft-summary-head" onClick={() => setSummaryOpen(!summaryOpen)} title={lang === "zh" ? "展开 / 收起总结框架" : "Toggle summary"}>
            {summaryOpen ? <ChevronLeft size={12} /> : <ChevronRight size={12} />}
            {summaryOpen && (
              <>
                <strong>{lang === "zh" ? "总结框架" : "Summary"}</strong>
                <span className="dim small">{summary.length}</span>
                <span className="flex-1" />
                {summary.length > 0 && (
                  <button type="button" className="icon-btn tiny" title={lang === "zh" ? "清空总结" : "Clear summary"}
                    onClick={(e) => { e.stopPropagation(); setSummary([]); }}>
                    <Trash2 size={11} />
                  </button>
                )}
              </>
            )}
          </div>
          {summaryOpen && (
            <div className="ft-summary-list">
              {summary.length === 0 && (
                <p className="dim small">{lang === "zh" ? "点击数值卡片上的「移入」按钮，把人格/性格/思想等加入这里。" : "Use the move button on any trait card to add personas / traits / ideologies here."}</p>
              )}
              {summary.map((it) => (
                <div key={it.key} className="ft-summary-row" title={`${it.kind} · ${it.value}`}>
                  <span className={`ft-sum-dot k-${it.kind}`} />
                  <span className="ft-sum-name ellipsis">{it.name}</span>
                  <span className="pv-tag">{it.kind === "personality" ? `${it.value}%` : (it.value > 0 ? "+" : "") + it.value}</span>
                  <button type="button" className="icon-btn tiny" title={lang === "zh" ? "从总结删除（同步构建与推演数据）" : "Remove from summary (syncs build & sim data)"}
                    onClick={() => removeFromSummary(it)}>
                    <Trash2 size={11} />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* ---------- v3.0 1.2 · Stage of Fate：中央舞台逻辑链编织 ---------- */}
      {stage && (
        <div className="fate-stage" role="status" aria-live="polite">
          <div className="fate-stage-veil" />
          <div className="fate-stage-beams">
            {stage.keywords.map((_k, i) => (
              <div key={i} className={`fate-beam fate-beam-${i % 5}`} style={{ animationDelay: `${(i * 0.22).toFixed(2)}s` }} />
            ))}
          </div>
          <div className="fate-stage-core">
            <Sparkles size={30} className="fate-stage-icon" />
            <div className="fate-stage-title">{lang === "zh" ? "命运演算中" : "Weaving fate"}</div>
          </div>
          <div className="fate-stage-keywords">
            {stage.keywords.map((k, i) => (
              <span key={k + i} className="fate-kw" style={{ animationDelay: `${(0.5 + i * 0.24).toFixed(2)}s` }}>{k}</span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

/** 6.3 单条特质编辑器：数值输入框 + 微调滑块（双向实时同步写回 profile）。 */
function TraitEditor(props: {
  name: string;
  desc: string;
  value: number;
  valueLabel: string;
  min: number;
  max: number;
  effects: { tag: string; weight: number }[];
  onValue: (v: number) => void;
  onDelete?: () => void;
}): React.ReactElement {
  const { lang } = useI18n();
  // 直接输入框持有本地草稿字符串：允许「-」「空」「12.」等中间态，
  // 失焦或回车时才钳位提交——修复无法输入的问题。
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? String(props.value);
  const commit = (): void => {
    if (draft === null) return;
    const n = Number(draft);
    if (draft.trim() !== "" && Number.isFinite(n)) {
      props.onValue(Math.max(props.min, Math.min(props.max, Math.round(n))));
    }
    setDraft(null);
  };
  return (
    <div className="ft-trait-editor">
      {/* v3.0 1.1 双轨输入：直接数值框（无边框透明）+ 微调滑块，二者实时同步 */}
      <div className="ft-numrow">
        <input
          type="text" inputMode="numeric" className="ft-num"
          value={shown}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") { commit(); (e.target as HTMLInputElement).blur(); }
            if (e.key === "Escape") setDraft(null);
          }}
        />
        <span className="dim small ft-num-hint">{lang === "zh" ? "直接输入数值" : "type a value"}</span>
      </div>
      <div className="ft-slider-row">
        <span className="dim small">{lang === "zh" ? "反面" : "−"}</span>
        <input type="range" min={props.min} max={props.max} value={props.value} className="ft-slider"
          onChange={(e) => { setDraft(null); props.onValue(Number(e.target.value)); }} />
        <span className="dim small">{lang === "zh" ? "正面" : "+"}</span>
        <span className="ft-val">{props.value > 0 ? "+" : ""}{props.value}</span>
      </div>
      <p className="dim small">{props.desc}</p>
      {props.onDelete && (
        <button type="button" className="icon-btn tiny ft-del" onClick={props.onDelete} title={lang === "zh" ? "删除该自定义条目" : "Delete custom entry"}>
          <Trash2 size={12} />
        </button>
      )}
      <div className="ft-weights">
        <span className="dim small">{lang === "zh" ? "影响权重表：" : "weights:"}</span>
        {props.effects.map((w) => (
          <span key={w.tag} className="pv-tag">
            {tagLabel(w.tag, lang)} {w.weight > 0 ? "+" : ""}{Math.round(w.weight * 100)}%
          </span>
        ))}
        <span className="dim small">→ {props.valueLabel}</span>
      </div>
    </div>
  );
}
