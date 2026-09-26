/**
 * E 域页组⑦ · 深化实验室二（批次五）：十个引擎实验室面板。
 * 与 pages-lab 同纪律：预览即真话——跑引擎本体，不贴图不假数据。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton, Notice, useT } from "./ui";
import { pointerSpritePair, type PointerShape } from "./png-encode";
import { bakeJobs, downgradeDensity, actCoverageCheck, bakeFrame } from "./boot-bitmap";
import { accentRamp, pickRampSlot, simulateVision, auditStateDistinction, suggestDistinctionFix, deltaE } from "./accent-ramp";
import { auditCopy, triadComplete, MESSAGE_PAGES, pageMessages } from "./message-catalog";
import { sealEnvelope, unsealEnvelope, planAtomicWrite, MigrationChain, quotaVerdict, cleanupCandidates } from "./persistence";
import { renderFrame, dirtyRects, SCENE_SIZE, type SceneKind } from "./preview-render";
import { dueTasks, fairOrder, executionPlan, nextRunAt, wakeRecoveryPlan, MISSED_POLICY_NOTE, type ScheduledTask } from "./scheduler";
import { rankEvents, withPinned, exportRanking, reconcileWithExternal, type UsageEvent } from "./usage-model";
import { buildCheatSheet, printableGrid, occupancyMatrix, suggestFreeCombos, blindWalkScript } from "./cheatsheet";
import { structuralDiff, describeDiff, threeWayMerge } from "./archive-diff";
import { shardJobs, runShard, incrementalJobs, type ProbeJob } from "./walkcheck-runner";
import { loadTokenTable } from "./tokens";

const bytesToDataUrl = (png: Uint8Array): string => {
  let bin = "";
  for (const b of png) bin += String.fromCharCode(b);
  return `data:image/png;base64,${btoa(bin)}`;
};

// ---------- 素材管线面板 ----------

export function AssetPipelineCard(): React.ReactNode {
  const t = useT();
  const [out, setOut] = useState<{ shape: PointerShape; url1x: string; url2x: string; bytes1x: number; bytes2x: number; head: string } | null>(null);
  const jobs = useMemo(() => bakeJobs(42), []);
  const advice = useMemo(() => downgradeDensity(jobs, 12 * 1024 * 1024), [jobs]); // 12MB 预算示例。
  const coverage = useMemo(() => actCoverageCheck(), []);
  const determinism = useMemo(() => {
    // 烘帧确定性对拍：小分辨率同帧烘两次——逐位一致（预览即真话的素材面）。
    const opts = { width: 160, height: 90, backdrop: [10, 10, 18] as [number, number, number], particleRadius: 2, palette: { base: "#6e7fd4", highlight: "#ffffff" } };
    const a = bakeFrame("minimal", 42, 20, opts);
    const b = bakeFrame("minimal", 42, 20, opts);
    const same = a.png.length === b.png.length && a.png.every((v, i) => v === b.png[i]);
    return { same, bytes: a.png.length, elapsedMs: a.elapsedMs };
  }, []);
  function bake(shape: PointerShape): void {
    const pair = pointerSpritePair(shape, 24, "#6e7fd4", "#ffffff");
    setOut({ shape, url1x: bytesToDataUrl(pair.png1x), url2x: bytesToDataUrl(pair.png2x), bytes1x: pair.bytes1x, bytes2x: pair.bytes2x, head: [...pair.png1x.slice(0, 8)].map((b) => b.toString(16).padStart(2, "0")).join(" ") });
  }
  return (
    <Card title={t("assetTitle")}>
      <Row label={t("assetSprites")} sub="SDF 光栅 + 纯 TS PNG 编码（CRC32 分块 + zlib stored）——真字节可落盘，任何工具可读">
        <span style={{ display: "flex", gap: 4 }}>
          {(["arrow", "ibeam", "cross", "hand"] as PointerShape[]).map((s) => (
            <PButton key={s} onClick={() => bake(s)}>{s}</PButton>
          ))}
        </span>
      </Row>
      {out ? (
        <>
          <Row label={`${out.shape}: 1x ${out.bytes1x}B / 2x ${out.bytes2x}B`} sub={`PNG 头 ${out.head}（89504e47…签名验证——产物真实性自证）`}>
            <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
              {/* 1x/2x 并排 + 2x 放大显示（4K 放大检查的页面内预演——放大不糊） */}
              <img src={out.url1x} width={24} height={24} alt={`${out.shape} 1x`} />
              <img src={out.url2x} width={48} height={48} alt={`${out.shape} 2x`} />
            </span>
          </Row>
        </>
      ) : null}
      <Row label={t("assetBakeBudget")} sub={jobs.map((j) => `${j.density}: ${(j.totalBytes / 1024 / 1024).toFixed(1)}MB`).join(" · ")}>
        <span style={{ color: advice.chosen ? "var(--p-success)" : "var(--p-warn)" }}>{advice.note}</span>
      </Row>
      <Row label="幕覆盖对拍（240 帧四幕）" sub={coverage.ok ? `每幕帧数 ${coverage.perAct.join("/")}——结构契约达成` : `缺幕: ${coverage.missingActs.join(",")}`}>
        <span />
      </Row>
      <Row label="烘帧确定性对拍（160×90 · 第 20 帧 ×2）" sub={determinism.same ? `逐位一致 · ${determinism.bytes}B · ${determinism.elapsedMs}ms——同 seed 同像素` : "两次烘帧不一致——必须修"}>
        <span style={{ color: determinism.same ? "var(--p-success)" : "var(--p-danger)" }}>{determinism.same ? "确定性达成" : "不一致"}</span>
      </Row>
    </Card>
  );
}

// ---------- 强调色阶梯面板 ----------

export function AccentRampCard(): React.ReactNode {
  const t = useT();
  const [accent, setAccent] = useState("#6e7fd4");
  const ramp = useMemo(() => accentRamp(accent), [accent]);
  const slots = useMemo(() => pickRampSlot(ramp, accent), [ramp, accent]);
  const audits = useMemo(() => auditStateDistinction(loadTokenTable().colors), []);
  const fixes = useMemo(() => suggestDistinctionFix(loadTokenTable().colors, "deuteranopia"), []);
  const bad = audits.filter((a) => !a.allDistinguishable);
  return (
    <Card title={t("rampTitle")}>
      <Row label="强调色" sub="三预设一键换源——阶梯/槽位/色觉审计全部随源重算">
        <span style={{ display: "flex", gap: 4 }}>
          {["#6e7fd4", "#22aa88", "#d4685f"].map((c) => (
            <PButton key={c} kind={accent === c ? "primary" : "ghost"} onClick={() => setAccent(c)}>{c}</PButton>
          ))}
        </span>
      </Row>
      <Row label={t("rampSlots")} sub={`hover ${slots.hover} · active ${slots.active} · disabled ${slots.disabled} · soft ${slots.soft} · 浅底可读 ${slots.text}`}>
        <span style={{ display: "flex", gap: 3 }}>
          {ramp.map((r) => (
            <span key={r.step} title={`step ${r.step} · 对白 ${r.contrastOnWhite}:1`} style={{ width: 14, height: 22, background: r.hex, display: "inline-block", borderRadius: 3 }} />
          ))}
        </span>
      </Row>
      <Row label={t("visionAudit")} sub={audits.map((a) => `${a.vision}: ${a.allDistinguishable ? "全可分" : "存在混淆对"}`).join(" · ")}>
        <span style={{ color: bad.length === 0 ? "var(--p-success)" : "var(--p-warn)" }}>{bad.length === 0 ? "四类色觉全过" : `${bad.length} 类色觉有混淆对`}</span>
      </Row>
      {fixes.length > 0 ? (
        <Notice tone="warn">{`${t("visionFix")}：${fixes.map((f) => `${f.key} → ${f.to}`).join("；")}`}</Notice>
      ) : null}
      <Row label="ΔE 抽样" sub={`success/warn 正常视觉 ΔE=${Math.round(deltaE(simulateVision(loadTokenTable().colors["--p-success"] ?? "#5fbf8a", "normal"), simulateVision(loadTokenTable().colors["--p-warn"] ?? "#d4b45f", "normal")) * 10) / 10}（≥20 可分线）`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 文案目录审计面板 ----------

export function CopyAuditCard(): React.ReactNode {
  const t = useT();
  const audit = useMemo(() => auditCopy(), []);
  const sample = pageMessages("tokens", "zh");
  const triadOk = sample ? triadComplete(sample.error) : false;
  return (
    <Card title={t("copyAuditTitle")}>
      <Row label={`目录规模：${MESSAGE_PAGES.length} 页 × 2 语言 × 10 槽位`} sub={`${t("copyTriad")}：error/confirm 三要素类型强制（漏字段 = 编译错误）——运行期复验 ${triadOk ? "通过" : "失败"}`}>
        <span style={{ color: triadOk ? "var(--p-success)" : "var(--p-danger)" }}>{audit.checked} 槽位已查</span>
      </Row>
      <Row label={t("copyBanned")} sub={audit.clean ? "禁词表零命中、标点零混用——发布口径达标" : audit.issues.slice(0, 3).map((i) => `${i.page}.${i.slot}: ${i.kind}`).join("；")}>
        <span style={{ color: audit.clean ? "var(--p-success)" : "var(--p-warn)" }}>{audit.clean ? "干净" : `${audit.issues.length} 处`}</span>
      </Row>
    </Card>
  );
}

// ---------- 持久化模型面板 ----------

export function PersistenceCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const env = sealEnvelope("persona/tokens", 1, { accent: "#6e7fd4" }, 1000);
    const okRead = unsealEnvelope<{ accent: string }>(JSON.parse(JSON.stringify(env)));
    const tampered = { ...JSON.parse(JSON.stringify(env)), payload: { accent: "#ff0000" } };
    const badRead = unsealEnvelope(tampered);
    const chain = new MigrationChain<Record<string, unknown>>()
      .register(0, 1, (o) => ({ ...(o as Record<string, unknown>), accent: "#8888aa" }), "v0 强调色桥接")
      .register(1, 2, (o) => ({ ...(o as Record<string, unknown>), radius: 8 }), "v1 增加圆角档");
    const migrated = chain.migrate({ theme: "dark" }, 0, 2);
    const plan = planAtomicWrite("persona/tokens", 2, { accent: "#6e7fd4" }, 2000);
    const usages = [{ slot: "tokens", bytes: 4200, updatedAt: 9000 }, { slot: "wallpaper", bytes: 102400, updatedAt: 1000 }, { slot: "widgets", bytes: 900, updatedAt: 9500 }];
    return {
      okRead: okRead.ok,
      badRead: badRead.ok ? "漏放——必须修" : (badRead as { reason: string }).reason,
      migrated: migrated.ok ? migrated.applied.join("；") : "失败",
      recovery: plan.recovery.map((r) => `${r.crashedAt}→${r.do}`).join(" · "),
      quota: quotaVerdict(usages, 200_000),
      stale: cleanupCandidates(usages, 10_000, 5_000, new Set(["tokens"])),
    };
  }, []);
  return (
    <Card title={t("persistTitle")}>
      <Row label={t("persistSeal")} sub={`合法信封 ${demo.okRead ? "通过" : "拒绝"} · 篡改信封 ${demo.badRead}`}>
        <span style={{ color: demo.okRead ? "var(--p-success)" : "var(--p-danger)" }}>{demo.okRead ? "双向通过" : "失败"}</span>
      </Row>
      <Row label={t("persistMigrate")} sub={demo.migrated}>
        <span />
      </Row>
      <Row label={t("persistAtomic")} sub={`断电恢复表：${demo.recovery}`}>
        <span />
      </Row>
      <Row label={`配额 ${demo.quota.usedBytes}/${demo.quota.limitBytes}B（${demo.quota.within ? "内" : "超"}）`} sub={`最大三槽: ${demo.quota.top3.map((u) => `${u.slot}@${u.bytes}B`).join(" ")} · 过期清理候选: ${demo.stale.map((u) => u.slot).join(",") || "无"}（可清≠自动清）`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 预览脏区面板 ----------

export function DirtyRectCard(): React.ReactNode {
  const t = useT();
  const [scene, setScene] = useState<SceneKind>("desktop");
  const demo = useMemo(() => {
    const base = loadTokenTable();
    const changed = JSON.parse(JSON.stringify(base));
    changed.colors["--p-accent"] = "#ff8800";
    const prev = renderFrame(scene, base);
    const next = renderFrame(scene, changed);
    const d = dirtyRects(prev, next);
    const unchanged = dirtyRects(prev, renderFrame(scene, base));
    return { d, unchanged, sceneSize: SCENE_SIZE };
  }, [scene]);
  return (
    <Card title={t("dirtyTitle")}>
      <Row label="场景" sub={`${demo.sceneSize.width}×${demo.sceneSize.height} 逻辑分辨率 · 三场景确定性几何`}>
        <span style={{ display: "flex", gap: 4 }}>
          {(["desktop", "explorer", "settings"] as SceneKind[]).map((s) => (
            <PButton key={s} kind={scene === s ? "primary" : "ghost"} onClick={() => setScene(s)}>{s}</PButton>
          ))}
        </span>
      </Row>
      <Row label={`改强调色 → 脏区 ${demo.d.dirty.length} 块（${demo.d.changedIds.join(", ") || "无"}）`} sub={`${t("dirtyRatio")} ${Math.round(demo.d.cleanRatio * 100)}%（F056 同源口径——只重画该重画的）`}>
        <span />
      </Row>
      <Row label="还原图逐位对拍" sub={`未改动帧 diff = ${demo.unchanged.dirty.length} 块脏区（0 = 放弃零残留的结构保证）`}>
        <span style={{ color: demo.unchanged.dirty.length === 0 ? "var(--p-success)" : "var(--p-danger)" }}>{demo.unchanged.dirty.length === 0 ? "零残留" : "有残留"}</span>
      </Row>
    </Card>
  );
}

// ---------- 统一调度面板 ----------

export function SchedulerCard(): React.ReactNode {
  const t = useT();
  const now = Date.now();
  const demo = useMemo(() => {
    const tasks: ScheduledTask[] = [
      { id: "wallpaper-rotate", intervalMs: 86_400_000, priority: 1, missed: "skip", lastRunAt: now - 90_000_000, enabled: true },
      { id: "widget-refresh", intervalMs: 1_800_000, priority: 3, missed: "once", lastRunAt: now - 7_200_000, enabled: true },
      { id: "usage-count-sync", intervalMs: 3_600_000, priority: 2, missed: "catchup", lastRunAt: now - 14_400_000, enabled: true },
      { id: "disabled-demo", intervalMs: 60_000, priority: 9, missed: "skip", lastRunAt: null, enabled: false },
    ];
    const due = dueTasks(tasks, now);
    const order = fairOrder(due);
    const plan = executionPlan(due, now);
    const wake = wakeRecoveryPlan(tasks, now - 14_400_000, now);
    const next = nextRunAt({ ...tasks[0]!, lastRunAt: now - 1000 }, now);
    return { due, order, plan, wake, next };
  }, [now]);
  return (
    <Card title={t("schedTitle")}>
      <Row label={`就绪 ${demo.due.length} 项 · 公平序: ${demo.order.map((d) => d.task.id).join(" → ")}`} sub="优先级降序 → 同级最饿先吃（F057 零饥饿同思想）">
        <span />
      </Row>
      {demo.plan.map((p) => (
        <Row key={p.id} label={`${p.id} × ${p.runs}`} sub={p.note}>
          <span />
        </Row>
      ))}
      <Row label={t("schedWake")} sub={demo.wake.map((w) => `${w.id}: ${w.action}`).join(" · ")}>
        <span />
      </Row>
      <Row label="下次时刻" sub={`wallpaper-rotate → ${new Date(demo.next).toLocaleTimeString()}（anchor + n×interval 首个未来点；抖动由调用方叠加）`}>
        <span />
      </Row>
      <Notice tone="info">{Object.entries(MISSED_POLICY_NOTE).map(([k, v]) => `${k}: ${v.split("——")[0]}`).join(" · ")}</Notice>
    </Card>
  );
}

// ---------- 使用排名面板 ----------

export function UsageRankCard(): React.ReactNode {
  const t = useT();
  const now = Date.now();
  const demo = useMemo(() => {
    const events: UsageEvent[] = [];
    const day = 86_400_000;
    // 30 天事件流：archiver 早期狂点、editor 近期稳定使用——EMA 让近期习惯赢。
    for (let d = 30; d >= 1; d--) {
      const n = d > 20 ? 6 : 1;
      for (let i = 0; i < n; i++) events.push({ itemId: "app.archiver", at: now - d * day + i * 1000 });
      events.push({ itemId: "app.editor", at: now - d * day + 500 });
      if (d % 3 === 0) events.push({ itemId: "app.scan", at: now - d * day + 800 });
    }
    const ranked = rankEvents(events, now);
    const pinned = withPinned(ranked, new Set(["app.scan"]), 3);
    const exported = exportRanking(ranked, now);
    const reconcile = reconcileWithExternal(ranked, exported);
    return { ranked: ranked.slice(0, 5), pinned, reconcile, rawEvents: events.length };
  }, [now]);
  return (
    <Card title={t("usageTitle")}>
      <Row label={t("usageDecay")} sub={`30 天事件流（${demo.rawEvents} 条）→ ${demo.ranked.map((r) => `${r.itemId}:${r.score}`).join(" · ")}`}>
        <span />
      </Row>
      <Row label="钉选语义（钉选是承诺）" sub={`top3 含钉选: ${demo.pinned.map((r) => r.itemId).join(" > ")}`}>
        <span />
      </Row>
      <Row label={t("usageExport")} sub={`导出只含 id/分数/计数（零内容字段——结构即隐私）· 两源对账: ${demo.reconcile.consistent ? "一致" : demo.reconcile.mismatches.join("；")}`}>
        <span style={{ color: demo.reconcile.consistent ? "var(--p-success)" : "var(--p-warn)" }}>{demo.reconcile.consistent ? "一致" : "有偏差"}</span>
      </Row>
    </Card>
  );
}

// ---------- 速查表与走查脚本面板 ----------

export function CheatSheetCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const bindings = [
      { actionId: "system.save", sequence: ["Ctrl+S"], enabled: true },
      { actionId: "system.quit", sequence: ["Ctrl+Q"], enabled: true },
      { actionId: "app.copy-line", sequence: ["Ctrl+K Ctrl+C"], enabled: true },
      { actionId: "app.comment", sequence: ["Ctrl+K Ctrl+U"], enabled: true },
    ];
    const sheet = buildCheatSheet(bindings);
    const grid = printableGrid(sheet[0]?.entries ?? []);
    const matrix = occupancyMatrix(bindings, ["Ctrl+Alt+Del", "Alt+F4"]);
    const suggest = suggestFreeCombos(matrix, "Ctrl+K", ["Ctrl+C", "Ctrl+X", "Ctrl+Z"]);
    const walk = blindWalkScript("settings", 4);
    return { sheet, grid, matrix, suggest, walk };
  }, []);
  return (
    <Card title={t("cheatTitle")}>
      <Row label={t("cheatGrid")} sub={demo.grid.map((col) => `${col.length} 行`).join(" / ") + ` · 组: ${demo.sheet.map((g) => g.title).join("/")}`}>
        <span />
      </Row>
      <Row label={t("cheatOccupancy")} sub={`占用 ${demo.matrix.occupancy.size} 组合 · 冲突 ${demo.matrix.conflicts.length} · 保留 ${demo.matrix.reserved.join(", ")} · Ctrl+K 空闲建议: ${demo.suggest.free.join(", ") || "无"}`}>
        <span />
      </Row>
      <Row label={t("cheatWalk")} sub={`盲走脚本 ${demo.walk.steps.length} 步（Tab 正向 ${demo.walk.steps.length - 3} + Shift+Tab 反向 + Enter + Esc）· 证据字段: ${demo.walk.evidenceFields.join("/")}`}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 三方合并面板 ----------

export function ArchiveMergeCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const base = { accent: "#6e7fd4", density: "standard", flags: { elder: false } };
    const mine = { accent: "#ff8800", density: "standard", flags: { elder: true } };
    const theirs = { accent: "#22aa88", density: "dense", flags: { elder: false } };
    const r = threeWayMerge(base, mine, theirs);
    const diffs = structuralDiff(base, mine);
    return { r, describe: describeDiff(diffs) };
  }, []);
  return (
    <Card title={t("mergeTitle")}>
      <Row label={`${t("mergeAuto")} ${demo.r.autoMerged} 处 · ${t("mergeConflicts")} ${demo.r.conflicts.length} 处`} sub={demo.r.conflicts.map((c) => `${c.path}: 本 ${JSON.stringify(c.mine)} / 对 ${JSON.stringify(c.theirs)}`).join("；") || "无冲突"}>
        <span style={{ color: demo.r.conflicts.length === 0 ? "var(--p-success)" : "var(--p-warn)" }}>{demo.r.conflicts.length === 0 ? "全自动" : "需人工裁决"}</span>
      </Row>
      <Row label="结构 diff（递归到叶子键）" sub={demo.describe.join("；")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 分片 runner 面板 ----------

export function ShardRunnerCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const jobs: ProbeJob[] = [
      { id: "F151", estMs: 2000, resourceGroup: "tokens", run: () => ({ ok: true, detail: "ok" }) },
      { id: "F152", estMs: 1500, resourceGroup: "tokens", run: () => ({ ok: true, detail: "ok" }) },
      { id: "F160", estMs: 3000, resourceGroup: "motion", run: () => ({ ok: false, detail: "注入失败样本——演示红项" }) },
      { id: "F165", estMs: 2500, resourceGroup: "boot", run: () => { throw new Error("探针异常演示"); } },
      { id: "F170", estMs: 1200, resourceGroup: "verdict", run: () => ({ ok: true, detail: "ok" }) },
    ];
    const shards = shardJobs(jobs, 5_000);
    const results = shards.map(runShard);
    const passedIds = new Set(results.flatMap((r) => r.results.filter((x) => x.ok).map((x) => x.id)));
    const incremental = incrementalJobs(jobs, { passedIds });
    return {
      shardCount: shards.length,
      shardLoad: shards.map((s) => `${s.index}#${s.jobs.length}项/${s.estMs}ms`).join(" "),
      pass: results.flatMap((r) => r.results).filter((x) => x.ok).length,
      fail: results.flatMap((r) => r.results).filter((x) => !x.ok).map((x) => x.id),
      crashed: results.flatMap((r) => r.crashedJobs),
      incremental: incremental.map((j) => j.id),
    };
  }, []);
  return (
    <Card title={t("runnerTitle")}>
      <Row label={`${t("runnerShards")} ${demo.shardCount}（片宽预算 5s · LPT 贪心均衡）`} sub={demo.shardLoad}>
        <span />
      </Row>
      <Row label={`执行: 绿 ${demo.pass} / 红 ${demo.fail.length}`} sub={`红项 ${demo.fail.join(",") || "无"} · 片内异常隔离 ${demo.crashed.join(",") || "无"}（异常按项捕获——片不死）`}>
        <span style={{ color: demo.fail.length === 0 ? "var(--p-success)" : "var(--p-warn)" }}>{demo.fail.length === 0 ? "全绿" : "有红项"}</span>
      </Row>
      <Row label={t("runnerIncremental")} sub={`上轮绿项跳过 → 只重跑: ${demo.incremental.join(", ") || "无（全绿即零重跑）"}`}>
        <span />
      </Row>
    </Card>
  );
}
