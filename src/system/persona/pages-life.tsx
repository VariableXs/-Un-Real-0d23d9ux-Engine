/**
 * E 域页组④：生活（F161 档案 / F163 小组件 / F164 锁屏 / F165 开机动画 /
 * F166 输入法皮肤）。
 */
import { useState } from "react";
import {
  ARCHIVE_SECTIONS, exportArchive, validateArchive, diffPreview, importArchive,
} from "./archive";
import { loadWidgetConfig, saveWidgetConfig, addWidget, removeWidget, updateWidget, WIDGET_KINDS, clampOpacity, REFRESH_MS } from "./widgets";
import { loadLockScreenConfig, saveLockScreenConfig, LOCK_TIME_STYLES, WAKE_TIMELINE, FALLBACK_COLOR } from "./lockcustom";
import { loadBootSkinConfig, saveBootSkinConfig, validateBootSkinConfig, PARTICLE_COUNTS, particlePalette, BACKDROP_DIM, suggestDensity } from "./bootskin";
import { BOOT_DURATION_MS } from "./store";
import { loadImeSkinConfig, saveImeSkinConfig, clampImeSkin, validateImeSkin, CANDIDATE_COUNTS } from "./imeskin";
import { loadTokenTable } from "./tokens";
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

  function doExport(): void {
    const r = exportArchive({ meta: { name }, include: picked, privacyChecked: privacy });
    if (r.privacyWarning) {
      setMsg(r.privacyWarning);
      return;
    }
    const blob = new Blob([JSON.stringify(r.pkg, null, 2)], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${name || "persona"}.vxtheme.json`;
    a.click();
    URL.revokeObjectURL(a.href);
    setMsg(`已导出 ${r.bytes} ${t("bytes")}`);
  }

  function doDiff(): void {
    try {
      const raw = JSON.parse(importRaw) as unknown;
      const v = validateArchive(raw);
      if (!v.ok) {
        setMsg(v.reason);
        return;
      }
      const d = diffPreview(raw as Parameters<typeof diffPreview>[0]);
      setDiff(`将更改: ${d.changedSections.join(" / ") || "（无差异）"}${v.degradations.length > 0 ? ` · ${v.degradations.join("；")}` : ""}`);
    } catch {
      setMsg("不是合法 JSON");
    }
  }

  function doImport(): void {
    try {
      const raw = JSON.parse(importRaw) as unknown;
      const r = importArchive(raw);
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
        {cfg.instances.map((i) => (
          <Row key={i.id} label={`${WIDGET_KINDS.find((k) => k.kind === i.kind)?.zh ?? i.kind} · ${i.size}`} sub={`位置 (${i.x}, ${i.y}) · 刷新 ${Math.round(REFRESH_MS[i.kind] / 1000)}s`}>
            <Slider value={i.opacity} min={0.2} max={1} step={0.05} ariaLabel={t("opacity")} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => saveWidgetConfig(updateWidget(cfg, i.id, { opacity: clampOpacity(v) }))} />
            <Toggle checked={i.clickThrough} onChange={(v) => saveWidgetConfig(updateWidget(cfg, i.id, { clickThrough: v }))} ariaLabel={t("clickThrough")} />
            <PButton kind="danger" onClick={() => saveWidgetConfig(removeWidget(cfg, i.id))}>{t("restoreDefault")}</PButton>
          </Row>
        ))}
        <Row label="性能保护" sub="全组件渲染超 3.3ms 帧预算 → 自动降透明 ×0.6">
          <Toggle checked={cfg.perfGuard} onChange={(v) => saveWidgetConfig({ ...cfg, perfGuard: v })} ariaLabel="性能保护" />
        </Row>
      </Card>
    </div>
  );
}

// ---------- F164 锁屏定制 ----------

export function LockPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("lock");
  const cfg = loadLockScreenConfig();
  const accent = loadTokenTable().colors["--p-accent"] ?? "#6e7fd4";

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
      <Card title="迷你锁屏预览（即改即见）">
        <div style={{ ...lockPreview, background: cfg.wallpaperMode === "independent" ? FALLBACK_COLOR : "linear-gradient(160deg, var(--p-bg-canvas, #14141c), #1c1c2c)" }}>
          <div style={{ fontSize: 44, fontWeight: 200, letterSpacing: 2, color: "#fff", fontVariantNumeric: "tabular-nums" }}>
            {new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </div>
          {cfg.showDate ? <div style={{ fontSize: 12, color: "#ffffffaa" }}>{new Date().toLocaleDateString()}</div> : null}
          {cfg.showBattery ? <div style={{ fontSize: 12, color: accent }}>🔋 87%</div> : null}
          <div style={{ fontSize: 11, color: "#ffffff66", marginTop: 8 }}>
            {WAKE_TIMELINE.map((w) => `${w.stage} ≤${w.budgetMs}ms`).join(" → ")}
          </div>
        </div>
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
    </div>
  );
}

// ---------- F166 输入法皮肤 ----------

export function ImePage(): React.ReactNode {
  const t = useT();
  usePersonaSection("ime");
  const cfg = clampImeSkin(loadImeSkinConfig());
  const themeColors = {
    background: loadTokenTable().colors["--p-bg-raised"] ?? "#222230",
    text: loadTokenTable().colors["--p-fg-primary"] ?? "#e8e8f0",
    highlight: loadTokenTable().colors["--p-accent"] ?? "#6e7fd4",
    highlightText: loadTokenTable().colors["--p-on-accent"] ?? "#ffffff",
  };
  const effective = cfg.followTheme ? { ...cfg, colors: themeColors } : cfg;
  const v = validateImeSkin(effective);

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
          <div style={candStyle(effective)}>
            <span style={{ ...candHi(effective), padding: "2px 8px", borderRadius: 4 }}>1 你好</span>
            <span style={{ padding: "2px 8px" }}>2 好</span>
            <span style={{ padding: "2px 8px" }}>3 号</span>
            <span style={{ padding: "2px 8px" }}>4 浩</span>
            <span style={{ padding: "2px 8px" }}>5 耗</span>
          </div>
        </MiniDesktop>
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
