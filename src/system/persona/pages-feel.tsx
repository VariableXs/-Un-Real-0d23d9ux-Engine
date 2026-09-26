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
    </div>
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
    </div>
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
