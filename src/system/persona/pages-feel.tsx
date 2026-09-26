/**
 * E 域页组③：手感（F157 声音混合器 / F158 开始菜单预设 / F159 字体安全档 /
 * F160 动效强度预演）。
 */
import { useEffect, useRef, useState } from "react";
import {
  SOUND_EVENTS, loadSoundMixerConfig, saveSoundMixerConfig, clampSlider,
  effectiveVolume, SLIDER_STEP,
} from "./soundmix";
import {
  loadStartPresetConfig, saveStartPresetConfig, applyPreset, saveAsCustom,
  defaultStartLayout, fullscreenFitWarning, elderTouchOk,
} from "./startpresets";
import { loadMotionTier, saveMotionTier, motionPlan, scaledDuration, DEMO_SCENES, LOOP_PAUSE_MS } from "./motiontier";
import { resolveEventSound, volumeRamp, rampAt } from "./audio-engine";
import { startMenuGeometry, presetThumbSpec } from "./layout-engine";
import { FrameTimeSampler, animationFallback, progressNumberSpec, progressNumberText, previewHonestyNote } from "./motion-monitor";
import { SynthLabCard } from "./pages-lab";
import { MotionCurveCard } from "./pages-lab3";
import { Card, PageHeader, Row, Toggle, Slider, PButton, Notice, useT, usePersonaSection } from "./ui";// ---------- F157 声音混合器 ----------

export function SoundPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("sound");
  const cfg = loadSoundMixerConfig();

  return (
    <div>
      <PageHeader title={t("soundTitle")} hint="六事件六档位独立调节；感知对数曲线（50% 滑杆=半响——线性杆骗耳朵）；2% 步进；试听走独立一次性流。" />
      {cfg.masterMute ? <Notice tone="warn">{t("mutedGlobally")}</Notice> : null}
      <Card>
        <Row label={t("masterMute")} sub="总闸 100% 压制（含系统级提示音）">
          <Toggle checked={cfg.masterMute} onChange={(v) => saveSoundMixerConfig({ ...cfg, masterMute: v })} ariaLabel={t("masterMute")} />
        </Row>
        {SOUND_EVENTS.map((e) => (
          <Row key={e.id} label={e.zh} sub={`${e.id} · 实际响度 ${Math.round(effectiveVolume(cfg, e.id) * 100)}%`}>
            <Slider
              value={cfg.events[e.id]?.slider ?? 1}
              min={0} max={1} step={SLIDER_STEP}
              ariaLabel={e.zh}
              format={(v) => `${Math.round(v * 100)}%`}
              onChange={(v) => saveSoundMixerConfig({ ...cfg, events: { ...cfg.events, [e.id]: { schemeId: cfg.events[e.id]?.schemeId ?? null, slider: clampSlider(v) } } })}
            />
            <PButton disabled={cfg.masterMute} onClick={() => tryEventPreview(e.id)}>{t("preview")}</PButton>
          </Row>
        ))}
      </Card>
      <SynthLabCard />
      <SoundEngineCard muted={cfg.masterMute} />
    </div>
  );
}

/**
 * 音频引擎面板（audio-engine 接线）：事件→声音解析链（方案资产优先、合成兜底）
 * + 六事件合成特征表 + 音量过渡曲线（防爆音的 80ms ramp）。
 */
function SoundEngineCard(props: { muted: boolean }): React.ReactNode {
  const cfg = loadSoundMixerConfig();
  const [rampFrom, setRampFrom] = useState(0.2);
  const ramp = volumeRamp(rampFrom, 0.9);

  return (
    <Card title="解析链与合成特征（零素材兜底 · 试听延迟 <200ms）">
      {SOUND_EVENTS.map((e) => {
        const r = resolveEventSound(cfg, e.id);
        return (
          <Row
            key={e.id}
            label={e.zh}
            sub={r ? `${r.path} · 合成 ${r.synth.wave} ${r.synth.freq}Hz ${r.synth.durationMs}ms${r.synth.dual ? " 双音" : ""} · 响度 ${Math.round(r.volume * 100)}%` : "未知事件（已拒绝）"}
          >
            <span />
          </Row>
        );
      })}
      <Row label="音量过渡（80ms 防爆音 ramp）" sub={ramp ? `20%→90%：中点实际 ${Math.round(rampAt(ramp, 0.5) * 100)}%（线性过渡——听觉平滑）` : "差值 <2% 不过渡（无意义微动被忽略）"}>
        <Slider value={rampFrom} min={0} max={1} step={0.05} ariaLabel="过渡起点音量" format={(v) => `${Math.round(v * 100)}%`} onChange={setRampFrom} />
      </Row>
      {props.muted ? <Notice tone="warn">总闸开启——试听与播放同哑（总闸语义一致，诚实）。</Notice> : null}
    </Card>
  );
}

function tryEventPreview(eventId: string): void {
  // 试听走 F026 播放面；设备缺失时按钮灰置（总闸开时此钮已禁用——语义一致）。
  document.dispatchEvent(new CustomEvent("persona:sound-preview", { detail: { eventId } }));
}

// ---------- F158 开始菜单预设 ----------

export function StartMenuPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("startmenu");
  const cfg = loadStartPresetConfig();
  const [msg, setMsg] = useState<string | null>(null);
  const installed = new Set<string>(["common-1", "common-2", "common-3", "common-4", "common-5", "common-6", "common-7", "common-8"]);
  const screenH = typeof window !== "undefined" ? window.innerHeight : 1080;

  function switchTo(id: string): void {
    const preset = cfg.presets.find((p) => p.id === id);
    if (!preset) return;
    const { layout, skipped } = applyPreset(preset, installed);
    saveStartPresetConfig({ ...cfg, activeId: id });
    setMsg([
      `已切到「${preset.name}」——固定 ${layout.pinned.length} 项、全屏 ${layout.fullscreen ? "开" : "关"}、大图标 ${layout.largeIcons ? "开" : "关"}（200ms F124 曲线重排）`,
      skipped.length > 0 ? `已跳过卸载项: ${skipped.join(" ")}` : null,
      layout.fullscreen ? fullscreenFitWarning(screenH) : null,
    ].filter(Boolean).join("　"));
  }

  // 布局几何（layout-engine 接线）：当前激活预设的确定性几何 + 各预设缩略规格。
  const activePreset = cfg.presets.find((p) => p.id === cfg.activeId);
  const activeLayout = activePreset ? applyPreset(activePreset, installed).layout : defaultStartLayout();
  const screenW = typeof window !== "undefined" ? window.innerWidth : 1920;
  const geometry = startMenuGeometry(activeLayout, screenW, screenH);
  const thumbs = cfg.presets.map((p) => presetThumbSpec(p, screenW, screenH));

  return (
    <div>
      <PageHeader title={t("startTitle")} hint="预设=差异集存储（只记与默认的差异——官方更新不冲掉用户改动）；切换不丢最近使用数据。" />
      {msg ? <Notice tone="info">{msg}</Notice> : null}
      <Card>
        {cfg.presets.map((p) => (
          <Row key={p.id} label={`${p.name}${p.official ? "" : "（自定义）"}`} sub={`固定 ${p.diff.pinned?.length ?? "默认"} · 全屏 ${p.diff.fullscreen ?? false} · 大图标 ${p.diff.largeIcons ?? false}`}>
            <PButton kind={cfg.activeId === p.id ? "primary" : "ghost"} onClick={() => switchTo(p.id)}>{cfg.activeId === p.id ? "当前" : "切换"}</PButton>
          </Row>
        ))}
        <Row label={t("saveCustom")} sub="与官方同名自动加「（自定义）」">
          <PButton onClick={() => {
            const layout = defaultStartLayout();
            const { config, preset } = saveAsCustom(cfg, "我的布局", layout);
            saveStartPresetConfig(config);
            setMsg(`已保存「${preset.name}」`);
          }}>{t("saveCustom")}</PButton>
        </Row>
        <Row label={t("elderMode")} sub={`大图标档格子 ≥56px 达标线：${elderTouchOk(56) ? "通过" : "未过"}`}>
          <span />
        </Row>
      </Card>
      <Card title="布局几何（layout-engine 实算 · 预设卡缩略=真缩小版）">
        <Row
          label={`当前几何 · ${geometry.width}×${geometry.height}px · 格子 ${geometry.tilePx}px × ${geometry.tileColumns} 列`}
          sub={`固定区 ${geometry.pinnedRows} 行 · 最近区 ${geometry.showRecent ? "显" : "隐"} · 推荐区 ${geometry.showRecommended ? "显" : "隐"} · 长辈达标 ${geometry.elderOk ? "✓" : "✗"}`}
        >
          <span />
        </Row>
        {thumbs.map((th, idx) => {
          const p = cfg.presets[idx];
          return (
            <Row
              key={p?.id ?? idx}
              label={`${p?.name ?? "?"} 缩略 ×${th.scale.toFixed(2)}`}
              sub={`${th.geometry.width}×${th.geometry.height}px · 格子 ${th.geometry.tilePx}px · ${th.geometry.tileColumns} 列 · 长辈达标 ${th.geometry.elderOk ? "✓" : "✗"}`}
            >
              <span />
            </Row>
          );
        })}
      </Card>
    </div>
  );
}

// ---------- F160 动效强度预演（含三联并排真渲染） ----------

export function MotionPage(): React.ReactNode {
  const t = useT();
  usePersonaSection("motion");
  const tier = loadMotionTier();
  const [cycle, setCycle] = useState(0);
  const rafRef = useRef(0);

  // 三联共用同一时间轴驱动（对齐比较才公平——主册设计细节）。
  useEffect(() => {
    let cancelled = false;
    function loop(): void {
      if (cancelled) return;
      setCycle((c) => c + 1);
      rafRef.current = window.setTimeout(loop, LOOP_PAUSE_MS + 600);
    }
    rafRef.current = window.setTimeout(loop, LOOP_PAUSE_MS + 600);
    return () => { cancelled = true; window.clearTimeout(rafRef.current); };
  }, []);

  const tiers = ["full", "reduced", "off"] as const;

  return (
    <div>
      <PageHeader title={t("motionTitle")} hint="三档并排预演：完整=全动画 / 减弱=时长 60% 简化轨迹 / 关闭=瞬显——选择建立在看见之后。" />
      <Notice tone="info">{t("a11yNote")}</Notice>
      <Card>
        <Row label="当前档位" sub="应用后全局时长缩放即时生效">
          {tiers.map((tr) => (
            <PButton key={tr} kind={tier === tr ? "primary" : "ghost"} onClick={() => saveMotionTier(tr)}>
              {tr === "full" ? t("tierFull") : tr === "reduced" ? t("tierReduced") : t("tierOff")}
            </PButton>
          ))}
        </Row>
      </Card>
      <Card title="预演三联（真渲染·同轴驱动）">
        <div style={{ display: "grid", gridTemplateColumns: `repeat(${tiers.length}, 1fr)`, gap: 12 }}>
          {tiers.map((tr) => (
            <div key={tr} style={{ border: tier === tr ? "2px solid var(--p-accent, #6e7fd4)" : "1px solid var(--p-border-subtle, rgba(140,140,160,0.14))", borderRadius: 10, padding: 10, background: "#80808814" }}>
              <b style={{ fontSize: 12 }}>{tr === "full" ? t("tierFull") : tr === "reduced" ? t("tierReduced") : t("tierOff")}</b>
              {DEMO_SCENES.map((s) => {
                const plan = motionPlan(s.baseMs, s.basePx, tr);
                return (
                  <div key={s.id} style={{ marginTop: 10 }}>
                    <small style={{ opacity: 0.7 }}>{s.zh} · {plan.durationMs === 0 ? "瞬显" : `${plan.durationMs}ms（基线 ${scaledDuration(s.baseMs, "full")}ms）`}</small>
                    <DemoBar key={`${tr}-${s.id}-${cycle}`} plan={plan} label={s.id} />
                  </div>
                );
              })}
            </div>
          ))}
        </div>
        <Row label="重播" sub="播完停 1s 循环（呼吸间隙）">
          <PButton onClick={() => setCycle((c) => c + 1)}>{t("replay")}</PButton>
        </Row>
      </Card>
      <MotionPerfCard />
      <MotionCurveCard />
    </div>
  );
}

/**
 * 性能监控面板（motion-monitor 接线）：帧时采样 → 80fps 预算判定 → 降级执法
 * （必达动画转 WP-207 进度数字 / 普通动画降直线）+ 预演诚实提示。
 */
function MotionPerfCard(): React.ReactNode {
  const samplerRef = useRef(new FrameTimeSampler());
  const [, bump] = useState(0);
  const v = samplerRef.current.verdict();
  const keepVerdict = animationFallback({ durationMs: 200, missionCritical: false }, samplerRef.current);
  const missionVerdict = animationFallback({ durationMs: 1200, missionCritical: true }, samplerRef.current);
  const honesty = previewHonestyNote(samplerRef.current);
  const progressText = progressNumberText(progressNumberSpec(0.42, 30));

  function inject(kind: "normal" | "jank"): void {
    const s = samplerRef.current;
    if (kind === "normal") { for (let i = 0; i < 60; i++) s.record(11 + (i % 3)); }
    else { for (let i = 0; i < 30; i++) s.record(24 + (i % 5)); }
    bump((n) => n + 1);
  }

  return (
    <Card title="性能监控（F124 联动 · 保帧率不保花活）">
      <Row label={`帧时 P95 ${v.p95.toFixed(1)}ms / P99 ${v.p99.toFixed(1)}ms · 样本 ${v.samples}`} sub={`80fps 预算（≤12.5ms）: ${v.within80fps ? "达标" : "超标"} · 60fps 底线（≤16.6ms）: ${v.within60fps ? "达标" : "超标"}`}>
        <span style={{ display: "inline-flex", gap: 6 }}>
          <PButton onClick={() => inject("normal")}>注入 60 正常帧</PButton>
          <PButton onClick={() => inject("jank")}>注入 30 掉帧</PButton>
        </span>
      </Row>
      <Row label="降级执法（注入后判定）" sub={`普通动画: ${keepVerdict === "keep" ? "保留全动画" : keepVerdict === "linear" ? "降直线（位移不丢）" : "转数字"} · 必达动画（进度环）: ${missionVerdict === "progress-number" ? `WP-207 转显性数字——${progressText}` : "保留"}`}>
        <span />
      </Row>
      {honesty ? <Notice tone="warn">{honesty}</Notice> : <Notice tone="info">预演诚实检查：掉帧时才提示性能受限——不无病呻吟。</Notice>}
    </Card>
  );
}

function DemoBar(props: { plan: ReturnType<typeof motionPlan>; label: string }): React.ReactNode {
  const { plan } = props;
  const [run, setRun] = useState(0);
  useEffect(() => {
    setRun((r) => r + 1);
  }, [props.plan]);
  if (plan.durationMs === 0) {
    return <div style={{ marginTop: 4, height: 14, borderRadius: 7, background: "var(--p-accent, #6e7fd4)", width: "100%" }} />;
  }
  return (
    <div style={{ marginTop: 4, height: 14, borderRadius: 7, background: "var(--p-border-regular, rgba(140,140,160,0.2))", overflow: "hidden" }}>
      <div
        key={run}
        style={{
          height: "100%", width: "40%", borderRadius: 7,
          background: "var(--p-accent, #6e7fd4)",
          animation: `persona-demo-slide ${plan.durationMs}ms var(--p-ease-enter, cubic-bezier(0.16,1,0.3,1)) forwards`,
        }}
      />
      <style>{`@keyframes persona-demo-slide { from { transform: translateX(-100%); opacity: ${plan.opacityFrom}; } to { transform: translateX(150%); opacity: 1; } }`}</style>
    </div>
  );
}
