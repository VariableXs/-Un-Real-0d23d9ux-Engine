/**
 * E 域页组②：资产（F154 壁纸每日一换 / F155 图标包热更换 / F156 指针编辑器）。
 */
import { useEffect, useMemo, useState } from "react";
import {
  loadDailyWallConfig, saveDailyWallConfig, pickDailyWallpaper, commitDailyPick,
  pinToday, shouldRotateNow, nextRotateAt, RESOLUTION_MATRIX, parseHHMM,
} from "./dailywall";
import {
  loadIconPackState, saveIconPackState, switchIconPack, rollbackIconPack, validateIconPack,
  coverageStats, resolveIcon, ICON_CATEGORIES, SWITCH_BUDGET_MS,
} from "./iconswap";
import {
  POINTER_ROLES, spritePlan, validateScheme, clampFps, clampFrames,
  PREVIEW_BACKDROPS, HOTSPOT_ZOOM, SIZE_LIMIT_PX,
  loadPointerScheme, savePointerScheme, type PointerRoleAsset,
} from "./pointer";
import {
  scheduleDownloads, inIdleWindow, planPreload, pickForMonitors,
  DEFAULT_QUEUE_CONFIG, type DownloadTask, type MonitorPool,
} from "./wallpaper-engine";
import { iconInvalidationBus, auditSvgAsset } from "./icon-engine";
import { renderPlan, evaluateCurAniCompatibility } from "./pointer-engine";
import { AtlasLabCard, CursorPhysicsCard } from "./pages-lab";
import { Card, PageHeader, Row, Toggle, Slider, Segmented, PButton, Notice, useT, usePersonaSection } from "./ui";

// ---------- F154 壁纸每日一换 ----------

export function DailyWallPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("wallpaper");
  const cfg = loadDailyWallConfig();
  const [lastMsg, setLastMsg] = useState<string | null>(null);

  // 池展开（演示池：真实 IO 由 wallpaper 域注入——本页绑定配置与纯逻辑引擎）。
  const poolItems = useMemo(() => {
    const out: Record<string, string[]> = {};
    for (const p of cfg.pools) out[p.id] = p.sources.length > 0 ? p.sources : [];
    return out;
  }, [cfg.pools]);

  function rotate(now = Date.now()): void {
    const r = pickDailyWallpaper(cfg, { now, poolItems });
    if (r.wallpaperId) {
      saveDailyWallConfig(commitDailyPick(cfg, r.wallpaperId, now));
    }
    setLastMsg(r.reason);
  }

  return (
    <div>
      <PageHeader title={`${t("navWallpaper")}（F154）`} hint="本地池/官方池每日轮换：500ms 交叉淡入无撕裂；构图保护自适应裁切（主体不裁头）；近 7 天排除表防短期重复。" />
      {lastMsg ? <Notice tone="info">{lastMsg}</Notice> : null}
      <Card>
        <Row label={t("enabled")}>
          <Toggle checked={cfg.enabled} onChange={(v) => saveDailyWallConfig({ ...cfg, enabled: v })} ariaLabel={t("dailyWallTitle")} />
        </Row>
        {cfg.pools.map((p) => (
          <Row key={p.id} label={t("pool")} sub={`${p.kind} · ${p.sources.length} 源`}>
            <Toggle checked={p.enabled} onChange={(v) => saveDailyWallConfig({ ...cfg, pools: cfg.pools.map((x) => (x.id === p.id ? { ...x, enabled: v } : x)) })} ariaLabel={p.id} />
          </Row>
        ))}
        <Row label={t("changeAt")} sub="HH:MM">
          <input type="time" value={cfg.changeAt} aria-label={t("changeAt")} style={sel} onChange={(e) => { if (parseHHMM(e.target.value) !== null) saveDailyWallConfig({ ...cfg, changeAt: e.target.value }); }} />
        </Row>
        <Row label="到点判定" sub={shouldRotateNow(cfg, Date.now()) ? "已到点待轮换" : `下次 ${nextRotateAt(cfg, Date.now()) ? new Date(nextRotateAt(cfg, Date.now()) ?? 0).toLocaleString() : "—"}`}>
          <PButton kind="primary" onClick={() => rotate()}>{t("rotateNow")}</PButton>
        </Row>
        <Row label={t("pinToday")} sub={cfg.pinnedDate ? `已固定至 ${cfg.pinnedDate}` : "次日自动解除"}>
          <PButton onClick={() => saveDailyWallConfig(pinToday(cfg, Date.now()))}>{t("pinToday")}</PButton>
        </Row>
        <Row label={t("recentExclude")} sub={cfg.recent.join(" ") || "—"}>
          <span />
        </Row>
        <Row label={t("history")} sub={`${Object.keys(cfg.history).length} 天`}>
          <span />
        </Row>
      </Card>
      <Card title="构图保护 · 20 档分辨率矩阵">
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {RESOLUTION_MATRIX.map((r) => (
            <span key={r.label} style={tagStyle}>{r.label} {r.w}×{r.h}</span>
          ))}
        </div>
      </Card>
      <WallpaperPipelineCard nextRotateAt={nextRotateAt(cfg, Date.now())} />
    </div>
  );
}

/**
 * 官方池下载管线面板（wallpaper-engine 接线）：空闲窗口调度 + 次日预载计划 +
 * 多屏独立池。下载只写白名单缓存目录、校验失败即弃（数据安全红线）。
 */
function WallpaperPipelineCard(props: { nextRotateAt: number | null }): React.ReactNode {
  const now = Date.now();
  const inWindow = inIdleWindow(now, DEFAULT_QUEUE_CONFIG.idleWindow);
  const [queue, setQueue] = useState<DownloadTask[] | null>(null);
  const preload = useMemo(
    () => planPreload(props.nextRotateAt ?? now, DEFAULT_QUEUE_CONFIG.idleWindow, now, "w-preload-demo"),
    [props.nextRotateAt, now],
  );

  function enqueueDemo(): void {
    const tasks: DownloadTask[] = ["官方池·晨雾", "官方池·夜航", "官方池·山谷"].map((id) => ({
      wallpaperId: id, url: "pipeline://demo", maxBytes: 24 * 1024 * 1024,
      checksum: "pending", state: "queued", attempts: 0, bytes: 0,
    }));
    setQueue(scheduleDownloads(tasks, DEFAULT_QUEUE_CONFIG, now));
  }

  const pools: MonitorPool[] = [
    { monitorId: "主屏", poolIds: ["本地池"], followGlobal: true },
    { monitorId: "副屏", poolIds: ["官方池"], followGlobal: false },
  ];
  const perMonitor = pickForMonitors(pools, "全局抽取 w-main", { side: () => 0 });

  return (
    <Card title="下载管线（官方池 · 空闲窗口 02:00–05:00 · F049 零等待预载）">
      <Row label="当前窗口状态" sub={inWindow ? "空闲窗口内——下载可启动（并发 2）" : "窗口外——任务保持排队（前台零争抢）"}>
        <span />
      </Row>
      <Row label="预载计划" sub={preload ? `${preload.wallpaperId} → ${new Date(preload.preloadAt).toLocaleString()} 解码入缓存（换的时刻零等待）` : "无待预载项"}>
        <span />
      </Row>
      <Row label="排队演示" sub={queue ? queue.map((q) => `${q.wallpaperId}:${q.state}`).join(" · ") : "未排队"}>
        <PButton onClick={enqueueDemo}>{queue ? "重新调度" : "排队 3 任务"}</PButton>
      </Row>
      <Row label="多屏独立池（前瞻接口）" sub={Object.entries(perMonitor).map(([m, w]) => `${m}←${w}`).join(" · ")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F155 图标包热更换 ----------

export function IconPackPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("icons");
  const state = loadIconPackState();
  const stats = coverageStats(state.current);
  const [msg, setMsg] = useState<string | null>(null);
  // 失效广播事件流（订阅制不轮询——各面监听重取的可见化）。
  const [busLog, setBusLog] = useState<string[]>([]);
  useEffect(() => {
    const off = iconInvalidationBus.subscribe((e) => {
      setBusLog((prev) => [`${new Date(e.at).toLocaleTimeString()} · ${e.reason}${e.packId ? ` · ${e.packId}@${e.packVersion}` : " · 官方默认"}`, ...prev].slice(0, 5));
    });
    return off;
  }, []);
  // SVG 资产安全审计演示（F133 规范子集——外部输入全清洗）。
  const svgAudit = useMemo(() => ({
    clean: auditSvgAsset('<svg viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/></svg>'),
    evil: auditSvgAsset('<svg><script>alert(1)</script></svg>'),
  }), []);

  function broadcast(reason: "switch" | "rollback" | "uninstall", packId: string | null, packVersion: string | null): void {
    iconInvalidationBus.broadcast({ reason, packId, packVersion, at: Date.now() });
  }

  function installSample(): void {
    const t0 = performance.now();
    const pack = {
      id: "community-amber",
      name: "社区·琥珀",
      version: "1.0.0",
      checksum: "sample-checksum",
      coverage: Object.fromEntries(ICON_CATEGORIES.slice(0, 31).map((c) => [c, `amber:${c}`])),
    };
    const v = validateIconPack(pack);
    const r = switchIconPack(state, v.ok ? pack : null, v, Math.round(performance.now() - t0));
    if (r.ok) {
      broadcast("switch", pack.id, pack.version);
      setMsg(`换装成功 · ${Math.round(r.elapsedMs)}ms（预算 ${SWITCH_BUDGET_MS}ms）· 覆盖 ${coverageStats(pack).ratioLabel} · 已广播失效（订阅面自动重取）`);
    } else {
      setMsg(`换装失败已整体回退：${r.reason}`);
    }
  }

  function rollback(): void {
    const r = rollbackIconPack(state);
    if (r) {
      saveIconPackState(r.next);
      broadcast("rollback", r.restored?.id ?? null, r.restored?.version ?? null);
      setMsg(`已回退到「${r.restored?.name ?? "官方默认包"}」· 已广播失效`);
    } else {
      setMsg("回退栈已空（最多连退三次）");
    }
  }

  return (
    <div>
      <PageHeader title={`${t("navIcons")}（F155）`} hint="整包原子换指针+广播失效（订阅制不轮询）；失败整体回退不留半套；回退栈深 3 层。" />
      {msg ? <Notice tone="info">{msg}</Notice> : null}
      <Card title={t("coverageLabel")}>
        <Row label={state.current ? state.current.name : "官方默认包"} sub={`${stats.ratioLabel}${stats.missing.length > 0 ? ` · 缺失回退官方项: ${stats.missing.slice(0, 4).join(" ")}${stats.missing.length > 4 ? "…" : ""}` : " · 全覆盖"}`}>
          <span />
        </Row>
        <Row label="操作" sub="运行中应用窗口图标下次刷新生效（诚实边界）">
          <PButton kind="primary" onClick={installSample}>{t("installPack")}</PButton>
          <PButton onClick={rollback}>{t("rollbackPack")}（{state.rollback.length}/3）</PButton>
          <PButton kind="danger" onClick={() => { switchIconPack(state, null, { ok: true, reason: "" }, 0); broadcast("uninstall", null, null); setMsg(`${t("uninstallPack")} · 已广播失效`); }}>{t("uninstallPack")}</PButton>
        </Row>
      </Card>
      <Card title="失效广播事件流（订阅制不轮询）">
        {busLog.length === 0 ? <Notice tone="info">尚无广播——换装/回退/卸载会在此留下事件。</Notice> : null}
        {busLog.map((line, i) => <Row key={`${line}-${i}`} label={line} sub="订阅方收到后按需重取（缓存键含包版本——旧包缓存不误命中）"><span /></Row>)}
      </Card>
      <Card title="SVG 资产安全审计（F133 规范子集 · 外部输入全清洗）">
        <Row label="正常资产" sub={svgAudit.clean.ok ? "通过（无脚本注入向量）" : `拒绝: ${svgAudit.clean.issues.join("；")}`}>
          <span />
        </Row>
        <Row label="恶意样本（script 注入）" sub={svgAudit.evil.ok ? "漏放——必须修" : `已拒绝: ${svgAudit.evil.issues.join("；")}`}>
          <span />
        </Row>
      </Card>
      <Card title="解析优先级（当前包 &gt; 官方 &gt; 默认）">
        {ICON_CATEGORIES.slice(0, 8).map((c) => {
          const r = resolveIcon(state.current, {}, c);
          return <Row key={c} label={c} sub={`来源: ${r.source}`}><span /></Row>;
        })}
      </Card>
      <AtlasLabCard />
    </div>
  );
}

// ---------- F156 指针编辑器 ----------

export function PointerPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("pointer");
  const scheme = loadPointerScheme();
  const [role, setRole] = useState("arrow");
  const [bg, setBg] = useState(PREVIEW_BACKDROPS[0]?.color ?? "#1c1c26");
  const asset = scheme.roles[role];
  const issues = validateScheme(scheme);
  const plan = asset ? spritePlan(asset, scheme.scale) : null;

  function update(patch: Partial<PointerRoleAsset>): void {
    if (!asset) return;
    const next = { ...scheme, roles: { ...scheme.roles, [role]: { ...asset, ...patch } } };
    savePointerScheme(next);
  }

  return (
    <div>
      <PageHeader title={t("pointerTitle")} hint="15 枚标准指针全可编辑：热点像素级标定（1px 对拍）、0.5x-2x 缩放、双倍率 sprite 自动生成（4K 放大验证）。" />
      <Card title="指针清单（15）">
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {POINTER_ROLES.map((r) => (
            <button key={r.id} type="button" style={{ ...tagBtnStyle, ...(role === r.id ? tagBtnOnStyle : {}) }} onClick={() => setRole(r.id)}>
              {r.zh}{r.animated ? " ⏳" : ""}
            </button>
          ))}
        </div>
      </Card>
      {asset ? (
        <Card title={`${asset.role}`}>
          <Row label={t("hotspot")} sub={asset.hotspot ? `(${asset.hotspot.x}, ${asset.hotspot.y}) · 1px 级` : t("hotspotUnset")}>
            <PButton onClick={() => update({ hotspot: { x: 2, y: 2 } })}>设为 (2,2)</PButton>
            <PButton onClick={() => update({ hotspot: { x: 4, y: 3 } })}>设为 (4,3)</PButton>
            <PButton onClick={() => update({ hotspot: null })}>清除</PButton>
          </Row>
          <Row label={t("scale")} sub={`基准 ${asset.size}px × ${scheme.scale} = ${plan?.size1x ?? 0}px（1x）/ ${plan?.size2x ?? 0}px（2x）`}>
            <Slider
              value={scheme.scale} min={0.5} max={2} step={0.05}
              ariaLabel={t("scale")}
              format={(v) => `${v.toFixed(2)}x`}
              onChange={(v) => savePointerScheme({ ...scheme, scale: v })}
            />
          </Row>
          <Row label="动效开关" sub={`帧率 ${clampFps(asset.fps)}fps · 帧数 ${asset.frames.length}/16`}>
            <Toggle checked={asset.animated} onChange={(v) => update({ animated: v })} ariaLabel="动效开关" />
          </Row>
          {plan && plan.size2x > SIZE_LIMIT_PX ? <Notice tone="warn">缩放后超过 {SIZE_LIMIT_PX}px——可能遮挡内容</Notice> : null}
          <Row label={t("preview")} sub="预览底三色切换（深/浅/中灰可见性检查）">
            <Segmented
              value={bg} ariaLabel="预览底色" onChange={setBg}
              options={PREVIEW_BACKDROPS.map((b) => ({ value: b.color, label: b.id }))}
            />
          </Row>
          <div style={{ background: bg, borderRadius: 8, height: 96, display: "grid", placeItems: "center", position: "relative", overflow: "hidden" }}>
            {/* 热点十字标定（8x 放大精度视图） */}
            <div style={{ position: "relative", width: 32, height: 32, transform: `scale(${HOTSPOT_ZOOM / 2})` }}>
              <div style={{ position: "absolute", left: "50%", top: 0, bottom: 0, width: 1, background: "rgba(255,80,80,0.7)" }} />
              <div style={{ position: "absolute", top: "50%", left: 0, right: 0, height: 1, background: "rgba(255,80,80,0.7)" }} />
              <div style={{ position: "absolute", inset: 6, background: "var(--p-accent, #6e7fd4)", clipPath: "polygon(0 0, 100% 60%, 55% 62%, 35% 100%)" }} />
            </div>
          </div>
        </Card>
      ) : null}
      {asset ? (
        <Card title="渲染规划（DPR 感知 · 4K 管线——高分屏放大不糊）">
          {[1, 2].map((dpr) => {
            const rp = renderPlan(asset, scheme.scale, dpr);
            return (
              <Row
                key={dpr}
                label={`DPR ${dpr} · ${rp.cssSize}px CSS → ${rp.physicalSize}px 物理`}
                sub={`选 ${rp.spriteScale}x sprite（≥物理尺寸的最小档）· 热点物理坐标 (${rp.hotspotPhysical.x}, ${rp.hotspotPhysical.y})`}
              >
                <span />
              </Row>
            );
          })}
          <Row label=".cur / .ani 导入评估（Windows 生态互通）" sub=".cur 静态单帧可映射 · .ani 动效帧率取自 RIFF（超 60 降采样）">
            <span />
          </Row>
          {([".cur", ".ani"] as const).map((kind) => {
            const rep = evaluateCurAniCompatibility(kind);
            return (
              <Row key={kind} label={kind} sub={rep.supported ? `可导入 · 限制: ${rep.limitations[0]}${rep.limitations.length > 1 ? ` 等 ${rep.limitations.length} 条` : ""}` : "不支持"}>
                <span />
              </Row>
            );
          })}
        </Card>
      ) : null}
      <Card title="方案校验">
        {issues.length === 0 ? <Notice tone="ok">15/15 枚无异常</Notice> : null}
        {issues.slice(0, 6).map((i, idx) => (
          <Notice key={`${i.role}-${i.kind}-${idx}`} tone={i.level === "error" ? "danger" : "warn"}>{i.role}: {i.message}</Notice>
        ))}
        <Row label={t("exportScheme")} sub="方案存 E4 面（vxtheme 兼容）；导出走 F127 打包">
          <PButton kind="primary" onClick={() => {
            const next = { ...scheme, roles: Object.fromEntries(Object.entries(scheme.roles).map(([k, v]) => [k, { ...v, frames: clampFrames(v.frames), fps: clampFps(v.fps) }])) };
            savePointerScheme(next);
            const blob = new Blob([JSON.stringify(next, null, 2)], { type: "application/json" });
            const a = document.createElement("a");
            a.href = URL.createObjectURL(blob);
            a.download = "pointer-scheme.vxpointer.json";
            a.click();
            URL.revokeObjectURL(a.href);
          }}>{t("exportScheme")}</PButton>
        </Row>
      </Card>
      <CursorPhysicsCard />
    </div>
  );
}

const sel: React.CSSProperties = {
  background: "var(--p-bg-raised, rgba(34,34,46,0.9))", color: "inherit",
  border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", borderRadius: 6, padding: "4px 8px", fontSize: 12,
};
const tagStyle: React.CSSProperties = { fontSize: 11, padding: "3px 8px", borderRadius: 6, border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", background: "var(--p-accent-soft, rgba(110,127,212,0.12))" };
const tagBtnStyle: React.CSSProperties = { ...tagStyle, cursor: "pointer", color: "inherit", background: "transparent" };
const tagBtnOnStyle: React.CSSProperties = { background: "var(--p-accent, #6e7fd4)", color: "var(--p-on-accent, #fff)", borderColor: "transparent" };
