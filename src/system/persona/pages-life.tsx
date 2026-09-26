/**
 * E 域页组④：生活（F161 档案 / F163 小组件 / F164 锁屏 / F165 开机动画 /
 * F166 输入法皮肤）。
 */
import { useMemo, useRef, useState } from "react";
import {
  ARCHIVE_SECTIONS, exportArchive, validateArchive, diffPreview, importArchive,
  type ArchivePackage,
} from "./archive";
import { loadWidgetConfig, saveWidgetConfig, addWidget, removeWidget, updateWidget, WIDGET_KINDS, clampOpacity, REFRESH_MS } from "./widgets";
import { loadLockScreenConfig, saveLockScreenConfig, LOCK_TIME_STYLES, WAKE_TIMELINE, FALLBACK_COLOR } from "./lockcustom";
import { loadBootSkinConfig, saveBootSkinConfig, validateBootSkinConfig, PARTICLE_COUNTS, particlePalette, BACKDROP_DIM, suggestDensity } from "./bootskin";
import { BOOT_DURATION_MS } from "./store";
import { loadImeSkinConfig, saveImeSkinConfig, clampImeSkin, validateImeSkin, CANDIDATE_COUNTS } from "./imeskin";
import { loadTokenTable } from "./tokens";
import { signArchive, verifySignature, validateSections, migrateArchiveV0toV1 } from "./archive-engine";
import { snapToGrid, resolvePlacement, reclaimOffscreen, buildUpdateSchedule, widgetSizePx, type ScreenBounds } from "./widgets-engine";
import { lockMachineStep, buildNotificationDigest, focusRetreatTransform, seedParticles, stepParticles, bakeManifestAll, actOf, type LockPhase } from "./boot-engine";
import { layoutCandidates, COMPOSITION_BUDGET_MS, type CandidateItem } from "./ime-menu-engine";
import { Card, PageHeader, Row, Toggle, Slider, Segmented, PButton, Notice, ColorChip, MiniDesktop, useT, usePersonaSection } from "./ui";

/** 档案分节 id（ARCHIVE_SECTIONS 派生——不与 store 的 PersonaSection 重复造类型）。 */
type SectionId = (typeof ARCHIVE_SECTIONS)[number]["id"];

// ---------- F161 我的档案 ----------

export function ArchivePage(): React.ReactNode {
  const t = useT();
  usePersonaSection("archive");
  const [name, setName] = useState("我的桌面人格");
  const [picked, setPicked] = useState<SectionId[]>(ARCHIVE_SECTIONS.map((s) => s.id));
  const [privacy, setPrivacy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [importRaw, setImportRaw] = useState("");
  const [diff, setDiff] = useState<string | null>(null);
  const [sigInfo, setSigInfo] = useState<string | null>(null);

  function doExport(): void {
    const r = exportArchive({ meta: { name }, include: picked, privacyChecked: privacy });
    if (r.privacyWarning) {
      setMsg(r.privacyWarning);
      return;
    }
    // 包签名链（archive-engine）：导出即签名——导入侧可验签防篡改（F127 语义）。
    const envelope = signArchive(r.pkg, "variable-local-trust");
    setSigInfo(`签名 ${envelope.signature.slice(0, 16)}… · ${envelope.signedAt} · 验签 ${verifySignature(envelope, "variable-local-trust").ok ? "通过" : "失败（不应发生）"}`);
    const blob = new Blob([JSON.stringify({ envelope, payload: r.pkg }, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${name || "persona"}.vxtheme.json`;
    a.click();
    URL.revokeObjectURL(a.href);
    setMsg(`已导出 ${r.bytes} ${t("bytes")}（含签名信封）`);
  }

  function doDiff(): void {
    try {
      let raw = JSON.parse(importRaw) as unknown;
      // 旧格式（V0 分节包）自动迁移——跨版本导入不拒之门外（兼容矩阵）。
      let migrationNote = "";
      if (raw && typeof raw === "object" && "sections" in (raw as Record<string, unknown>)) {
        const m = migrateArchiveV0toV1(raw as Record<string, unknown> & { sections?: Record<string, Record<string, unknown>> });
        raw = m.pkg as unknown;
        migrationNote = ` · V0→V1 迁移 ${m.applied.length} 节`;
      }
      const record = raw as { envelope?: unknown; payload?: unknown };
      const pkg = (record && typeof record === "object" && "payload" in record ? record.payload : raw) as Parameters<typeof diffPreview>[0];
      const v = validateArchive(pkg);
      if (!v.ok) {
        setMsg(v.reason);
        return;
      }
      const sectionErrs = validateSections(pkg as ArchivePackage);
      const d = diffPreview(pkg);
      setDiff(
        `将更改: ${d.changedSections.join(" / ") || "（无差异）"}${v.degradations.length > 0 ? ` · ${v.degradations.join("；")}` : ""}${sectionErrs.length > 0 ? ` · 分节校验: ${sectionErrs.join("；")}` : " · 分节校验全过"}${migrationNote}`,
      );
    } catch {
      setMsg("不是合法 JSON");
    }
  }

  function doImport(): void {
    try {
      const raw = JSON.parse(importRaw) as unknown;
      // 签名验签（带信封的包先验后导——防篡改可验证；裸包诚实标注无签名）。
      const record = raw as { envelope?: { payload: string; signature: string; signedAt: string; signerVersion: number }; payload?: unknown };
      if (record?.envelope?.signature) {
        const vr = verifySignature(record.envelope, "variable-local-trust");
        if (!vr.ok) {
          setMsg(`签名校验失败：${vr.reason}——包可能被篡改，已拒绝导入。`);
          return;
        }
      }
      const pkgRaw = record?.payload ?? raw;
      let pkg = pkgRaw as Parameters<typeof importArchive>[0];
      if (pkgRaw && typeof pkgRaw === "object" && "sections" in (pkgRaw as Record<string, unknown>)) {
        pkg = migrateArchiveV0toV1(pkgRaw as Record<string, unknown> & { sections?: Record<string, Record<string, unknown>> }).pkg as typeof pkg;
      }
      const r = importArchive(pkg);
      setMsg(`${r.reason}${r.degradations.length > 0 ? ` · ${r.degradations.join("；")}` : ""}${r.ok ? ` · 已应用 ${r.applied.length} 节` : ""}`);
    } catch {
      setMsg("不是合法 JSON");
    }
  }

  return (
    <div>
      <PageHeader title={t("archiveTitle")} hint="E 域全配置打包 vxtheme：分节独立勾选（包是菜单不是套餐）；导入前自动快照进回滚链（F121 联动）；中断原子回退。" />
      {msg ? <Notice tone="info">{msg}</Notice> : null}
      <Card title={t("sectionPick")}>
        <Row label={t("archiveName")}>
          <input value={name} onChange={(e) => setName(e.target.value)} style={inputStyle} aria-label={t("archiveName")} />
        </Row>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6, margin: "8px 0" }}>
          {ARCHIVE_SECTIONS.map((s) => {
            const on = picked.includes(s.id);
            return (
              <button
                key={s.id}
                type="button"
                style={{ ...tagBtn, ...(on ? tagBtnOn : {}) }}
                aria-pressed={on}
                onClick={() => setPicked(on ? picked.filter((x) => x !== s.id) : [...picked, s.id])}
              >
                {s.zh}
              </button>
            );
          })}
        </div>
        <Row label="隐私确认" sub={t("privacyHint")}>
          <Toggle checked={privacy} onChange={setPrivacy} ariaLabel="隐私确认" />
        </Row>
        <Row label=""><PButton kind="primary" onClick={doExport}>{t("exportArchive")}</PButton></Row>
      </Card>
      <Card title={t("importArchive")}>
        <textarea
          value={importRaw}
          onChange={(e) => setImportRaw(e.target.value)}
          aria-label={t("importArchive")}
          placeholder='{"format":"vxtheme-profile", ...}'
          style={{ ...inputStyle, width: "100%", minHeight: 90, fontFamily: "monospace" }}
        />
        <Row label="操作">
          <PButton onClick={doDiff}>{t("diffPreview")}</PButton>
          <PButton kind="primary" onClick={doImport}>{t("importArchive")}</PButton>
        </Row>
        {diff ? <Notice tone="info">{diff}</Notice> : null}
        {sigInfo ? <Notice tone="ok">{sigInfo}</Notice> : null}
      </Card>
    </div>
  );
}

// ---------- F163 桌面小组件 ----------

export function WidgetsPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("widgets");
  const cfg = loadWidgetConfig();

  return (
    <div>
      <PageHeader title={t("widgetsTitle")} hint="三枚官方组件：时钟（秒级准确）/天气（F101 同源）/系统快照（F062 四灯）；透明度 20% 下限保可读；渲染超预算自动降透明帧率。" />
      <Card title="添加">
        <Row label={t("addWidget")}>
          {WIDGET_KINDS.map((k) => (
            <PButton key={k.kind} kind="primary" onClick={() => saveWidgetConfig(addWidget(cfg, k.kind, 40 + cfg.instances.length * 24, 40 + cfg.instances.length * 24))}>{k.zh}</PButton>
          ))}
        </Row>
      </Card>
      <Card title={`实例（${cfg.instances.length}）`}>
        {cfg.instances.length === 0 ? <Notice tone="info">桌面还没有小组件——右键桌面「添加小组件」是同一入口。</Notice> : null}
        {cfg.instances.map((i) => {
          const px = widgetSizePx(i);
          return (
            <Row key={i.id} label={`${WIDGET_KINDS.find((k) => k.kind === i.kind)?.zh ?? i.kind} · ${i.size}`} sub={`位置 (${i.x}, ${i.y}) · ${px.w}×${px.h}px · 刷新 ${Math.round(REFRESH_MS[i.kind] / 1000)}s`}>
              <Slider value={i.opacity} min={0.2} max={1} step={0.05} ariaLabel={t("opacity")} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => saveWidgetConfig(updateWidget(cfg, i.id, { opacity: clampOpacity(v) }))} />
              <Toggle checked={i.clickThrough} onChange={(v) => saveWidgetConfig(updateWidget(cfg, i.id, { clickThrough: v }))} ariaLabel={t("clickThrough")} />
              <PButton kind="danger" onClick={() => saveWidgetConfig(removeWidget(cfg, i.id))}>{t("restoreDefault")}</PButton>
            </Row>
          );
        })}
        <Row label="性能保护" sub="全组件渲染超 3.3ms 帧预算 → 自动降透明 ×0.6">
          <Toggle checked={cfg.perfGuard} onChange={(v) => saveWidgetConfig({ ...cfg, perfGuard: v })} ariaLabel="性能保护" />
        </Row>
      </Card>
      <PlacementCard />
    </div>
  );
}

/**
 * 摆放引擎面板（widgets-engine 接线）：8px 吸附网格 + 碰撞推开 + 越界回收 +
 * 更新调度表（带 15s 抖动防同帧齐刷）。
 */
function PlacementCard(): React.ReactNode {
  const cfg = loadWidgetConfig();
  const [lastMove, setLastMove] = useState<string | null>(null);
  const screen: ScreenBounds = { width: 1920, height: 1080 };

  function simulateDrag(): void {
    const first = cfg.instances[0];
    if (!first) return;
    // 模拟拖放：目标点落在非 8px 对齐位置——吸附引擎负责对齐与碰撞推开。
    const raw = { x: first.x + 13, y: first.y + 5 };
    const snapped = snapToGrid(raw.x, raw.y);
    const resolved = resolvePlacement(cfg, first.id, snapped.x, snapped.y);
    saveWidgetConfig(updateWidget(cfg, first.id, { x: resolved.x, y: resolved.y }));
    setLastMove(`吸附 (${raw.x},${raw.y})→(${snapped.x},${snapped.y})${resolved.pushed ? " · 碰撞推开 1 个邻居" : " · 无碰撞"}`);
  }

  const reclaimed = reclaimOffscreen(cfg, screen);
  const schedule = buildUpdateSchedule(cfg);

  return (
    <Card title="摆放引擎（8px 网格 · F084 自由模式同族手感）">
      <Row label="模拟拖放首组件" sub={lastMove ?? "拖放落点吸附 8px 网格，与邻居重叠时自动推开"}>
        <PButton kind="primary" disabled={cfg.instances.length === 0} onClick={simulateDrag}>拖放 +13,+5</PButton>
      </Row>
      <Row label="越界回收" sub={reclaimed.reclaimed.length === 0 && reclaimed.downsized.length === 0 ? `全部 ${cfg.instances.length} 个组件都在 ${screen.width}×${screen.height} 屏内` : `回收 ${reclaimed.reclaimed.length} 个 · 缩尺寸 ${reclaimed.downsized.length} 个（拉回可视区，不静默丢弃）`}>
        {reclaimed.reclaimed.length + reclaimed.downsized.length > 0 ? (
          <PButton kind="primary" onClick={() => saveWidgetConfig(reclaimed.config)}>执行回收</PButton>
        ) : <span />}
      </Row>
      {schedule.length > 0 ? (
        <Row label="更新调度" sub={schedule.map((s) => `${WIDGET_KINDS.find((k) => k.kind === s.kind)?.zh ?? s.kind} 首刷 ${s.firstDelayMs}ms 后 / 周期 ${Math.round(s.intervalMs / 1000)}s`).join(" · ")}>
          <span />
        </Row>
      ) : null}
    </Card>
  );
}

// ---------- F164 锁屏定制 ----------

export function LockPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("lock");
  const cfg = loadLockScreenConfig();
  const accent = loadTokenTable().colors["--p-accent"] ?? "#6e7fd4";
  // 让位布局（boot-engine）：密码框聚焦时时间缩小上移——预演真实变换参数。
  const [pwdFocus, setPwdFocus] = useState(false);
  const retreat = focusRetreatTransform(pwdFocus);
  // 锁屏状态机演示（boot-engine）：失败节流（5 次失败 → 15s 等待）防暴力枚举。
  const [phase, setPhase] = useState<LockPhase>("locked");
  const [fails, setFails] = useState(0);
  const machine = lockMachineStep(
    { phase, failedAttempts: fails, sinceLastFailMs: 0 },
    { type: "unlock-try" },
  );
  const digest = buildNotificationDigest([{ appName: "邮件" }, { appName: "邮件" }, { appName: "日历" }]);

  return (
    <div>
      <PageHeader title={t("lockTitle")} hint="三式时间样式即时预览；唤醒到可见 ≤2s；零循环动画（淡入即静——省电纪律 F060）。" />
      <Card>
        <Row label="壁纸">
          <Segmented
            value={cfg.wallpaperMode} ariaLabel="壁纸来源" onChange={(v) => saveLockScreenConfig({ ...cfg, wallpaperMode: v })}
            options={[{ value: "follow-desktop", label: t("wallpaperFollow") }, { value: "independent", label: t("wallpaperIndependent") }]}
          />
        </Row>
        {cfg.wallpaperMode === "independent" ? (
          <Row label="壁纸路径">
            <input value={cfg.wallpaperPath ?? ""} placeholder="wallpapers/lock.jpg" style={inputStyle} aria-label="锁屏壁纸路径" onChange={(e) => saveLockScreenConfig({ ...cfg, wallpaperPath: e.target.value || null })} />
          </Row>
        ) : null}
        <Row label="时间样式">
          <Segmented
            value={cfg.timeStyle} ariaLabel="时间样式" onChange={(v) => saveLockScreenConfig({ ...cfg, timeStyle: v })}
            options={LOCK_TIME_STYLES.map((s) => ({ value: s.id, label: s.zh }))}
          />
        </Row>
        <Row label={t("showBattery")}><Toggle checked={cfg.showBattery} onChange={(v) => saveLockScreenConfig({ ...cfg, showBattery: v })} ariaLabel={t("showBattery")} /></Row>
        <Row label={t("showDate")}><Toggle checked={cfg.showDate} onChange={(v) => saveLockScreenConfig({ ...cfg, showDate: v })} ariaLabel={t("showDate")} /></Row>
        <Row label={t("notifyPrivacy")} sub="隐私默认开启"><Toggle checked={cfg.notifyPrivacy} onChange={(v) => saveLockScreenConfig({ ...cfg, notifyPrivacy: v })} ariaLabel={t("notifyPrivacy")} /></Row>
      </Card>
      <Card title="迷你锁屏预览（即改即见 · 让位布局随焦点）">
        <div style={{ ...lockPreview, background: cfg.wallpaperMode === "independent" ? FALLBACK_COLOR : "linear-gradient(160deg, var(--p-bg-canvas, #14141c), #1c1c2c)" }}>
          <div style={{
            fontSize: 44 * retreat.scale, fontWeight: 200, letterSpacing: 2, color: "#fff",
            fontVariantNumeric: "tabular-nums",
            transform: `translateY(${retreat.translateY}px)`,
            transition: `all ${retreat.transitionMs}ms ease-out`,
          }}>
            {new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </div>
          {cfg.showDate ? <div style={{ fontSize: 12, color: "#ffffffaa" }}>{new Date().toLocaleDateString()}</div> : null}
          {cfg.showBattery ? <div style={{ fontSize: 12, color: accent }}>🔋 87%</div> : null}
          <label style={{ fontSize: 11, color: "#ffffff88", display: "inline-flex", gap: 6, alignItems: "center", marginTop: 4 }}>
            <input type="checkbox" checked={pwdFocus} onChange={(e) => setPwdFocus(e.target.checked)} aria-label="模拟密码框聚焦" />
            模拟密码框聚焦（时间缩小上移让位）
          </label>
          <div style={{ fontSize: 11, color: "#ffffff66", marginTop: 8 }}>
            {WAKE_TIMELINE.map((w) => `${w.stage} ≤${w.budgetMs}ms`).join(" → ")}
          </div>
        </div>
      </Card>
      <Card title="锁屏状态机（防暴力枚举节流）">
        <Row
          label={`当前态 ${phase} · 连续失败 ${fails}`}
          sub={machine.throttleRemainMs > 0 ? `输入已拒绝——剩余 ${Math.round(machine.throttleRemainMs / 1000)}s（${machine.message ?? ""}）` : (machine.message ?? "输入可接受")}
        >
          <span style={{ display: "inline-flex", gap: 6 }}>
            <PButton kind="danger" onClick={() => { const out = lockMachineStep({ phase, failedAttempts: fails + 1, sinceLastFailMs: 0 }, { type: "unlock-try" }); setPhase(out.next); setFails((f) => f + 1); }}>模拟输错</PButton>
            <PButton kind="primary" onClick={() => { const out = lockMachineStep({ phase, failedAttempts: 0, sinceLastFailMs: 0 }, { type: "unlock-ok" }); setPhase(out.next); setFails(0); }}>模拟解锁成功</PButton>
            <PButton onClick={() => { const out = lockMachineStep({ phase, failedAttempts: 0, sinceLastFailMs: 0 }, { type: "lock" }); setPhase(out.next); setFails(0); }}>重新上锁</PButton>
          </span>
        </Row>
        <Row label="通知隐私摘要" sub={cfg.notifyPrivacy ? `只计数不显内容：${digest.count} 条 · 来自 ${Object.keys(digest.byApp).length} 个应用（${Object.entries(digest.byApp).map(([a, n]) => `${a}×${n}`).join(" ")}）` : "隐私关闭——内容将直接显示（不推荐）"}>
          <span />
        </Row>
      </Card>
    </div>
  );
}

const lockPreview: React.CSSProperties = {
  borderRadius: 12, padding: 24, textAlign: "center", minHeight: 120,
  display: "flex", flexDirection: "column", alignItems: "center", gap: 6, justifyContent: "center",
};

// ---------- F165 开机动画个性化 ----------

export function BootPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("boot");
  const cfg = loadBootSkinConfig();
  const accent = cfg.accentOverride ?? loadTokenTable().colors["--p-accent"] ?? "#6e7fd4";
  const palette = particlePalette(accent);
  const v = validateBootSkinConfig(cfg);
  const suggest = suggestDensity("medium");

  return (
    <div>
      <PageHeader title={t("bootTitle")} hint="标准不可改，皮肤可以：配色随强调色 / 粒子密度三档 / 幕三过渡底图；结构与时长 8.0s±0.2s 是性能契约。" />
      {!v.ok ? <Notice tone="danger">{v.reason}</Notice> : null}
      <Card>
        <Row label="粒子密度" sub={suggest ? `弱机建议: 极简档（F060 电量账本联动）` : `当前 ${PARTICLE_COUNTS[cfg.density]} 粒`}>
          <Segmented
            value={cfg.density} ariaLabel="粒子密度" onChange={(d) => saveBootSkinConfig({ ...cfg, density: d })}
            options={[
              { value: "dense", label: t("densityDense") },
              { value: "standard", label: t("densityStandard") },
              { value: "minimal", label: t("densityMinimal") },
            ]}
          />
        </Row>
        <Row label="强调色覆盖" sub="null = 随 E1 强调色令牌">
          <ColorChip value={cfg.accentOverride ?? accent} onChange={(hex) => saveBootSkinConfig({ ...cfg, accentOverride: hex })} ariaLabel="开机动画强调色" />
          <PButton onClick={() => saveBootSkinConfig({ ...cfg, accentOverride: null })}>{t("restoreDefault")}</PButton>
        </Row>
        <Row label={t("useWallpaperBackdrop")} sub={`暗化 ${Math.round(BACKDROP_DIM * 100)}% 固定（保证幕四交接可读）`}>
          <Toggle checked={cfg.useWallpaperBackdrop} onChange={(x) => saveBootSkinConfig({ ...cfg, useWallpaperBackdrop: x })} ariaLabel={t("useWallpaperBackdrop")} />
        </Row>
        <Row label="粒子色映射" sub="主色→粒子基色 / 提亮→高光（预览与真播同源）">
          <span style={{ display: "inline-flex", gap: 6 }}>
            <span style={{ ...swatch, background: palette.base }} /><code style={swatchCode}>{palette.base}</code>
            <span style={{ ...swatch, background: palette.highlight }} /><code style={swatchCode}>{palette.highlight}</code>
          </span>
        </Row>
        <Row label={t("bootInvariant")} sub={`当前烘焙态: ${cfg.bakeState}`}>
          <code style={swatchCode}>{`8.0s ±0.2s（${BOOT_DURATION_MS}ms 契约）`}</code>
        </Row>
      </Card>
      <ParticlePreviewCard density={cfg.density} accent={accent} />
    </div>
  );
}

/**
 * 确定性粒子预览（boot-engine 接线）：同种子同画面——「预览即真播」的数学基础；
 * 四幕边界与 8s 契约同源。canvas 渲染 0.5x 缩放（渲染成本减 75%，F152 同口径）。
 */
function ParticlePreviewCard(props: { density: "dense" | "standard" | "minimal"; accent: string }): React.ReactNode {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rafRef = useRef(0);
  const [playing, setPlaying] = useState(false);
  const [act, setAct] = useState(0);
  const palette = particlePalette(props.accent);
  const manifest = useMemo(() => bakeManifestAll(20260926), []);

  function play(): void {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      // Canvas 不可用（极端降级）——诚实提示，不静默假装在播。
      setAct(-1);
      return;
    }
    const particles = seedParticles(props.density, 20260926, props.accent);
    setPlaying(true);
    const t0 = performance.now();
    let last = 0;
    function frame(): void {
      const elapsed = performance.now() - t0;
      const dt = Math.min(64, elapsed - last);
      last = elapsed;
      const stepped = stepParticles(particles, elapsed, dt, 1920, 1080);
      ctx!.clearRect(0, 0, canvas!.width, canvas!.height);
      for (const p of stepped) {
        ctx!.fillStyle = p.color === "base" ? palette.base : palette.highlight;
        ctx!.globalAlpha = 0.85;
        ctx!.beginPath();
        ctx!.arc((p.x / 1920) * canvas!.width, (p.y / 1080) * canvas!.height, Math.max(0.6, (p.size / 1920) * canvas!.width), 0, Math.PI * 2);
        ctx!.fill();
      }
      ctx!.globalAlpha = 1;
      setAct(actOf(elapsed));
      if (elapsed < BOOT_DURATION_MS) {
        rafRef.current = requestAnimationFrame(frame);
      } else {
        setPlaying(false);
      }
    }
    cancelAnimationFrame(rafRef.current);
    rafRef.current = requestAnimationFrame(frame);
  }

  const actNames = ["幕一·汇聚", "幕二·成型", "幕三·点亮", "幕四·交接"];

  return (
    <Card title="粒子预览（确定性种子 20260926 · 同种子同画面——预览与真播逐帧一致）">
      <Row label="播放" sub={act === -1 ? "Canvas 不可用——渲染降级（诚实提示）" : playing ? `正在播放：${actNames[act] ?? "—"}（四幕边界 ${[0, 2600, 5200, 6800].join("/")}ms · 8s 结构）` : "点「重播四幕」——密度与配色即当前配置"}>
        <PButton kind="primary" disabled={playing} onClick={play}>重播四幕</PButton>
      </Row>
      <canvas ref={canvasRef} width={560} height={315} style={{ width: "100%", maxWidth: 560, borderRadius: 10, background: "linear-gradient(160deg, #0a0a12, #14141c)", border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))" }} aria-label="开机动画粒子预览" role="img" />
      <Row label="烘帧清单（关机前空闲批次执行 · F068 按需装载联动）" sub={manifest.map((m) => `${m.density}:${m.frameCount}帧/${(m.estimatedBytes / 1024).toFixed(0)}KB@${m.sampleFps}fps`).join(" · ")}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- F166 输入法皮肤 ----------

/** 试打演示候选集（13 条——5 档翻 3 页 / 9 档翻 2 页，覆盖分页路径）。 */
const DEMO_CANDIDATES: CandidateItem[] = "你好号浩耗好毫嚎貉豪壕好号浩".split("").map((ch, i) => ({
  index: i + 1,
  text: `hao${i > 0 ? i : ""}`,
  comment: ch,
}));

export function ImePage(): React.ReactNode {
  const t = useT();
  usePersonaSection("ime");
  const cfg = clampImeSkin(loadImeSkinConfig());
  const [highlightIndex, setHighlightIndex] = useState(0);
  const themeColors = {
    background: loadTokenTable().colors["--p-bg-raised"] ?? "#222230",
    text: loadTokenTable().colors["--p-fg-primary"] ?? "#e8e8f0",
    highlight: loadTokenTable().colors["--p-accent"] ?? "#6e7fd4",
    highlightText: loadTokenTable().colors["--p-on-accent"] ?? "#ffffff",
  };
  const effective = cfg.followTheme ? { ...cfg, colors: themeColors } : cfg;
  const v = validateImeSkin(effective);
  // 候选窗布局引擎（ime-menu-engine 接线）：分页/几何/翻页提示全由引擎实算。
  const layout = layoutCandidates(DEMO_CANDIDATES, effective, highlightIndex);

  return (
    <div>
      <PageHeader title={t("imeTitle")} hint="皮肤自由，速度不商量：16ms 组合期渲染红线；热生效=候选窗每次弹出重读参数（无缓存陈旧态）。" />
      {!v.ok ? <Notice tone="warn">{v.issues.join("；")}</Notice> : <Notice tone="ok">高亮对比度 {v.highlightContrast.toFixed(2)}:1 ≥ 4.5:1 门禁通过</Notice>}
      <Card>
        <Row label={t("followTheme")} sub="开=令牌联动；关=独立定制组">
          <Toggle checked={cfg.followTheme} onChange={(x) => saveImeSkinConfig({ ...cfg, followTheme: x })} ariaLabel={t("followTheme")} />
        </Row>
        <Row label={t("fontSize")}>
          <Slider value={cfg.fontSize} min={12} max={16} step={1} ariaLabel={t("fontSize")} format={(x) => `${x}px`} onChange={(x) => saveImeSkinConfig(clampImeSkin({ ...cfg, fontSize: x }))} />
        </Row>
        <Row label={t("opacity")} sub="60% 下限护栏">
          <Slider value={cfg.opacity} min={0.6} max={1} step={0.05} ariaLabel={t("opacity")} format={(x) => `${Math.round(x * 100)}%`} onChange={(x) => saveImeSkinConfig(clampImeSkin({ ...cfg, opacity: x }))} />
        </Row>
        <Row label={t("candidates")} sub="9 档显示翻页键提示">
          <Segmented value={String(cfg.candidates)} ariaLabel={t("candidates")} onChange={(x) => saveImeSkinConfig(clampImeSkin({ ...cfg, candidates: x === "9" ? 9 : 5 }))} options={CANDIDATE_COUNTS.map((n) => ({ value: String(n), label: `${n} 候选` }))} />
        </Row>
        {!cfg.followTheme ? (
          <Row label="独立配色">
            <span style={{ display: "inline-flex", gap: 8 }}>
              <ColorChip value={cfg.colors.background} onChange={(x) => saveImeSkinConfig({ ...cfg, colors: { ...cfg.colors, background: x } })} ariaLabel="候选窗底色" />
              <ColorChip value={cfg.colors.text} onChange={(x) => saveImeSkinConfig({ ...cfg, colors: { ...cfg.colors, text: x } })} ariaLabel="候选文字色" />
              <ColorChip value={cfg.colors.highlight} onChange={(x) => saveImeSkinConfig({ ...cfg, colors: { ...cfg.colors, highlight: x } })} ariaLabel="高亮底色" />
              <ColorChip value={cfg.colors.highlightText} onChange={(x) => saveImeSkinConfig({ ...cfg, colors: { ...cfg.colors, highlightText: x } })} ariaLabel="高亮文字色" />
            </span>
          </Row>
        ) : null}
      </Card>
      <Card title={t("tryType")}>
        <MiniDesktop width={320} accent={effective.colors.highlight}>
          <div style={{
            ...candStyle(effective),
            width: layout.width,
            height: layout.height,
            display: "flex", flexDirection: "column", alignItems: "stretch",
            justifyContent: "center",
          }}>
            {layout.page.map((c) => (
              <span
                key={c.index}
                style={{
                  padding: "1px 8px", lineHeight: `${layout.lineHeightPx - 8}px`,
                  ...(c.index === highlightIndex ? candHi(effective) : {}),
                }}
              >
                {c.index} {c.text}{c.comment ? <small style={{ opacity: 0.7 }}> {c.comment}</small> : null}
              </span>
            ))}
            {layout.pagingHintText ? <small style={{ opacity: 0.65, textAlign: "right", padding: "0 8px 2px" }}>{layout.pagingHintText}</small> : null}
          </div>
        </MiniDesktop>
        <Row
          label="布局引擎实算"
          sub={`窗口 ${layout.width}×${layout.height}px · 第 ${layout.pageIndex + 1}/${layout.pageCount} 页 · 组合期预算 ${COMPOSITION_BUDGET_MS}ms（皮肤自由，速度不商量）`}
        >
          <span style={{ display: "inline-flex", gap: 6 }}>
            <PButton onClick={() => setHighlightIndex((i) => (i + 1) % DEMO_CANDIDATES.length)}>下一候选</PButton>
            <PButton onClick={() => setHighlightIndex((i) => Math.min(DEMO_CANDIDATES.length - 1, i + effective.candidates))}>翻页</PButton>
          </span>
        </Row>
      </Card>
    </div>
  );
}

function candStyle(c: { colors: { background: string; text: string }; opacity: number; fontSize: number }): React.CSSProperties {
  return {
    position: "absolute", left: "12%", bottom: 44, display: "flex", gap: 2, alignItems: "center",
    background: c.colors.background, color: c.colors.text, opacity: c.opacity,
    borderRadius: 8, padding: 6, fontSize: c.fontSize, boxShadow: "0 6px 18px rgba(0,0,0,0.4)",
  };
}
function candHi(c: { colors: { highlight: string; highlightText: string } }): React.CSSProperties {
  return { background: c.colors.highlight, color: c.colors.highlightText };
}

const inputStyle: React.CSSProperties = {
  background: "var(--p-bg-raised, rgba(34,34,46,0.9))", color: "inherit",
  border: "1px solid var(--p-border-regular, rgba(140,140,160,0.24))", borderRadius: 6, padding: "4px 8px", fontSize: 12,
};
const tagBtn: React.CSSProperties = { fontSize: 11, padding: "3px 8px", borderRadius: 6, border: "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", background: "transparent", cursor: "pointer", color: "inherit" };
const tagBtnOn: React.CSSProperties = { background: "var(--p-accent, #6e7fd4)", color: "var(--p-on-accent, #fff)", borderColor: "transparent" };
const swatch: React.CSSProperties = { width: 16, height: 16, borderRadius: 4, display: "inline-block", border: "1px solid rgba(255,255,255,0.2)" };
const swatchCode: React.CSSProperties = { fontSize: 11, opacity: 0.8 };
