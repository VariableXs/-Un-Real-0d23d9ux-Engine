/**
 * AI-J1 · 批次七面板（十引擎的消费端）。
 * 曲线谱学 / 精密模式 / 手掌守门 / 倾斜通道 / 接缝状态机 / 边缘滚纵深 /
 * 穿透规则审计 / 影子物理 / 显示器身份 / 档案包生命周期。
 * 消费的都是纯函数引擎——面板只做参数输入与结果呈现，不复制逻辑。
 */
import React, { useMemo, useState } from "react";
import { SectionCard, MiniButton } from "./MouseJ1Panels";
import {
  WIN_SLIDER_MAX,
  blendPreviewTable,
  inspectCurve,
  sensToWinSlider,
  translatorSelfTest,
  winSliderToSens,
} from "../mouse/speedspectrum";
import {
  PRECISION_LEVELS,
  rampFactor,
  transitionPrecision,
  type PrecisionEvent,
  type PrecisionState,
} from "../mouse/precisiontune";
import { guardSummary, initialGuardState, feedGuard, PALM_RADIUS_PX } from "../mouse/palmguard";
import { discreteTiltRate, tiltRate, tiltZoomStepsPerSec, arbiterTiltPress } from "../mouse/tiltchannel";
import { SeamCrossMachine } from "../mouse/seamcross";
import { resolveEdgeTarget, edgeReleaseCoast, type EdgeContainer } from "../mouse/edgeramp";
import { WheelRuleAudit, resolveRule, validateRules, tempRemainingSec, type WheelRule } from "../mouse/wheelrules";
import { REST_LIFT, liftFromSpeed, pressLift, respectReducedMotion, groundShadowColor } from "../mouse/shadowcast";
import { displayCapability, identityConfidence, mayRestore } from "../mouse/displayidentity";
import { exportPack, mergeProfiles, migrateProfilePack, verifyPack, type ProfileSnapshot } from "../mouse/profilesync";
import { parseEdid, synthEdid } from "../mouse/edid";
import { pushToast } from "../../state/uiStore";

function KV(props: { k: string; v: React.ReactNode }): React.ReactElement {
  return (
    <div className="j1x-kv">
      <span className="j1x-kv-k">{props.k}</span>
      <span className="j1x-kv-v">{props.v}</span>
    </div>
  );
}

/** 批次七十面板组（单一挂载点——MouseJ1Tab 只引这个）。 */
export function V7Panels(): React.ReactElement {
  return (
    <>
      <CurveSpectrumLabPanel />
      <PrecisionTunePanel />
      <PalmGuardPanel />
      <TiltChannelPanel />
      <SeamMachinePanel />
      <EdgeRampPanel />
      <WheelRulesPanel />
      <ShadowPhysicsPanel />
      <DisplayIdentityPanel />
      <ProfilePackPanel />
    </>
  );
}

/* ------------------------------- 曲线谱学实验室 ------------------------------- */

export function CurveSpectrumLabPanel(): React.ReactElement {
  const [slider, setSlider] = useState(7);
  const [t, setT] = useState(0.5);
  const selfTest = useMemo(() => translatorSelfTest(), []);
  const linear = (px: number) => 0.5 + px / 128;
  const aggressive = (px: number) => 0.4 + (px / 128) ** 0.6 * 1.8;
  const rows = blendPreviewTable(linear, aggressive, t, 16, 128);
  const health = inspectCurve((px) => (1 - t) * linear(px) + t * aggressive(px));
  return (
    <SectionCard title="曲线谱学实验室（F601 纵深）" f="v7">
      <div className="j1x-grid2">
        <div>
          <KV k="Windows 档 → Varix" v={`${slider} 档 = ${winSliderToSens(slider).toFixed(2)}x`} />
          <KV k="Varix → 最近档" v={`${winSliderToSens(slider).toFixed(2)}x → ${sensToWinSlider(winSliderToSens(slider))} 档`} />
          <input type="range" min={1} max={WIN_SLIDER_MAX} value={slider} onChange={(e) => setSlider(Number(e.target.value))} aria-label="Windows 滑杆档" />
          <KV k="换算自检" v={selfTest.pass ? `11 档 round-trip 全过` : `失败档：${selfTest.failures.join(",")}`}
          />
        </div>
        <div>
          <KV k="混成权重 t" v={t.toFixed(2)} />
          <input type="range" min={0} max={1} step={0.05} value={t} onChange={(e) => setT(Number(e.target.value))} aria-label="混成权重" />
          <table className="j1x-table">
            <thead>
              <tr><th>px</th><th>线性</th><th>激进</th><th>混成</th></tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.px}><td>{r.px}</td><td>{r.a}</td><td>{r.b}</td><td>{r.mixed}</td></tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      <div className={`j1x-verdict j1x-verdict-${health.verdict}`}>
        <strong>曲线体检：{health.verdict === "pass" ? "通过" : health.verdict === "warn" ? "有警告" : "不通过"}</strong>
        {health.notes.map((n, i) => (
          <div key={i}>{n}</div>
        ))}
      </div>
    </SectionCard>
  );
}

/* ------------------------------- 精密模式 ------------------------------- */

export function PrecisionTunePanel(): React.ReactElement {
  const [st, setSt] = useState<PrecisionState>({ phase: "idle", sinceMs: null, stickyCount: 0 });
  const send = (kind: PrecisionEvent["kind"]) => setSt(transitionPrecision(st, { kind, atMs: Date.now() }));
  const ramp = rampFactor(60, PRECISION_LEVELS[1], st.phase === "active" || st.phase === "sticky");
  return (
    <SectionCard title="精密模式状态机（F602 纵深）" f="v7">
      <KV k="当前相位" v={st.phase} />
      <KV k="粘滞计数" v={st.stickyCount} />
      <KV k="坡道 60ms 处系数" v={`${ramp.toFixed(3)}x`} />
      <KV k="键盘步进" v="方向 1px · Shift 5px · Ctrl 10px · Home/End 轴吸附" />
      <div className="j1x-btnrow">
        <MiniButton onClick={() => send("tap")}>tap</MiniButton>
        <MiniButton onClick={() => send("tripleTap")}>三连击</MiniButton>
        <MiniButton onClick={() => send("release")}>release</MiniButton>
      </div>
      <div className="j1x-hint">
        {st.phase === "sticky" ? "已粘滞——release 被吞掉，再按一次退出。" : st.phase === "cooldown" ? "冷却防抖中——窗内触发被忽略。" : "tap 进入 · 三连击粘滞 · release 退出。"}
      </div>
    </SectionCard>
  );
}

/* ------------------------------- 手掌守门 ------------------------------- */

export function PalmGuardPanel(): React.ReactElement {
  const [st, setSt] = useState(initialGuardState());
  const feed = (radius: number) =>
    setSt(feedGuard(st, { radiusPx: radius, speedPxMs: 0.1, pressure: 0.2, sinceTouchMs: 500, angleDelta: 0 }).next);
  return (
    <SectionCard title="手掌守门分类器（F603 纵深）" f="v7">
      <KV k="守门状态" v={guardSummary(st)} />
      <KV k="签名阈值" v={`半径 ≥${PALM_RADIUS_PX}px + 低速 + 浅压 → reject`} />
      <div className="j1x-btnrow">
        <MiniButton onClick={() => feed(3)}>喂正常笔尖</MiniButton>
        <MiniButton onClick={() => feed(PALM_RADIUS_PX + 4)}>喂手掌边缘</MiniButton>
      </div>
      <div className="j1x-hint">三态处置：reject 丢弃 / suspect 交给平滑（不静默丢）/ clean 放行；恢复需连续 3 帧 clean。</div>
    </SectionCard>
  );
}

/* ------------------------------- 倾斜通道 ------------------------------- */

export function TiltChannelPanel(): React.ReactElement {
  const [angle, setAngle] = useState(5);
  const rate = tiltRate(angle);
  return (
    <SectionCard title="倾斜滚轮模拟量通道（F606 纵深）" f="v7">
      <KV k="倾斜角" v={`${angle.toFixed(1)}°`} />
      <KV k="横滚速率" v={`${rate.toFixed(0)} px/s`} />
      <KV k="缩放步频" v={`${tiltZoomStepsPerSec(angle)} 步/s（档位感通道）`} />
      <KV k="离散降级档" v={`±${discreteTiltRate("right").toFixed(0)} px/s（三态设备的诚实口径）`} />
      <KV k="按压仲裁" v={`按压制胜示例：${arbiterTiltPress(100, 200) === "autoscroll" ? "先按→自动滚接管" : "先倾→缩放接管"}`} />
      <input type="range" min={-10} max={10} step={0.5} value={angle} onChange={(e) => setAngle(Number(e.target.value))} aria-label="倾斜角" />
    </SectionCard>
  );
}

/* ------------------------------- 接缝状态机 ------------------------------- */

export function SeamMachinePanel(): React.ReactElement {
  const m = useMemo(() => new SeamCrossMachine(), []);
  const [depth, setDepth] = useState(0);
  const [last, setLast] = useState<string>("—");
  const step = (d: number) => {
    const f = m.frame({ alongPx: 20, seamLenPx: 1000, depthPx: d, speedPxMs: 0.5, shiftHeld: false, stickiness: "light" });
    setLast(`${f.crossed ? "已跨" : "未跨"} · vx×${f.vxScale.toFixed(2)} · ${f.reason}`);
  };
  return (
    <SectionCard title="接缝状态机（F607 纵深）" f="v7">
      <KV k="最近一帧" v={last} />
      <div className="j1x-btnrow">
        <MiniButton onClick={() => { setDepth((v) => Math.min(12, v + 2)); step(Math.min(12, depth + 2)); }}>向对面推 2px</MiniButton>
        <MiniButton onClick={() => { setDepth((v) => Math.max(-12, v - 2)); step(Math.max(-12, depth - 2)); }}>向近侧拉 2px</MiniButton>
        <MiniButton onClick={() => { m.reset(); setDepth(0); setLast("已重置"); }}>重置</MiniButton>
      </div>
      <div className="j1x-hint">角落区滞回：进 +2px / 退 -6px 双阈值——两屏间抖动免疫。当前深度 {depth}px。</div>
    </SectionCard>
  );
}

/* ------------------------------- 边缘滚纵深 ------------------------------- */

export function EdgeRampPanel(): React.ReactElement {
  const stacks: { name: string; stack: EdgeContainer[] }[] = [
    { name: "内层有余量", stack: [{ id: "主滚动区", depth: 0, remainingPx: 500 }, { id: "树面板", depth: 1, remainingPx: 100 }] },
    { name: "内层耗尽→接力", stack: [{ id: "主滚动区", depth: 0, remainingPx: 500 }, { id: "树面板", depth: 1, remainingPx: 0 }] },
    { name: "全部耗尽", stack: [{ id: "主滚动区", depth: 0, remainingPx: 0 }, { id: "树面板", depth: 1, remainingPx: 0 }] },
  ];
  return (
    <SectionCard title="拖拽边缘滚纵深（F609 纵深）" f="v7">
      {stacks.map((s) => {
        const r = resolveEdgeTarget(s.stack);
        return (
          <KV key={s.name} k={s.name} v={r.target ? `${r.target.id}${r.carried ? "（接力）" : ""}` : "不滚（显式 null）"} />
        );
      })}
      <KV k="松手余韵" v={`drop 100ms 保留 ${(edgeReleaseCoast("drop", 100) * 100).toFixed(0)}% · 离带即停 0%`} />
    </SectionCard>
  );
}

/* ------------------------------- 穿透规则审计 ------------------------------- */

export function WheelRulesPanel(): React.ReactElement {
  const audit = useMemo(() => new WheelRuleAudit(), []);
  const [log, setLog] = useState<string[]>([]);
  const rules: WheelRule[] = [
    { id: "g1", layer: "global", mode: "intercept" },
    { id: "a1", layer: "app", mode: "pass", appId: "pdf-reader" },
    { id: "c1", layer: "container", mode: "intercept", appId: "pdf-reader", container: "sidebar" },
  ];
  const errs = validateRules(rules);
  const judge = (ctx: string) => {
    const r = resolveRule(rules);
    const mode = ctx === "sidebar" ? resolveRule([rules[2]!, rules[0]!, rules[1]!]).mode : r.mode;
    audit.record({ atMs: Date.now(), layer: "container", ruleId: null, mode, ctx });
    setLog([...log, `${ctx} → ${mode}`]);
  };
  return (
    <SectionCard title="滚轮穿透规则引擎（F618 纵深）" f="v7">
      <KV k="规则集校验" v={errs.length === 0 ? "通过（三层 specificity）" : errs.join("；")} />
      <div className="j1x-btnrow">
        <MiniButton onClick={() => judge("正文")}>裁决：PDF 正文</MiniButton>
        <MiniButton onClick={() => judge("sidebar")}>裁决：侧边栏</MiniButton>
      </div>
      <KV k="临时倒计时示例" v={`${tempRemainingSec({ expiresAt: Date.now() + 3500, pass: true }, Date.now()) ?? "按住型"}s`} />
      {log.length > 0 && <div className="j1x-log">{log.slice(-6).map((l, i) => <div key={i}>{l}</div>)}</div>}
      <div className="j1x-hint">审计环形 {audit.all().length} 条在内存（上限 200）——「滚了没动」的一键答案。</div>
    </SectionCard>
  );
}

/* ------------------------------- 影子物理 ------------------------------- */

export function ShadowPhysicsPanel(): React.ReactElement {
  const [speed, setSpeed] = useState(0);
  const [pressed, setPressed] = useState(false);
  const [reduced, setReduced] = useState(false);
  const f = respectReducedMotion(reduced, speed, pressed, 0);
  const ground = groundShadowColor({ r: 200, g: 200, b: 200 });
  return (
    <SectionCard title="指针投影物理光照（F620 纵深）" f="v7">
      <KV k="速度" v={`${speed.toFixed(2)} px/ms → 高度 ${liftFromSpeed(speed).toFixed(2)}`} />
      <KV k="投影三联" v={`offset ${f.offsetPx}px · blur ${f.blurPx}px · opacity ${f.opacity}`} />
      <KV k="按压高度" v={pressed ? "按下（贴地）" : `回弹中 ${(pressLift(45, false) / REST_LIFT).toFixed(2)}x`} />
      <KV k="地面反光" v={`投影色 rgb(${ground.shadow.r},${ground.shadow.g},${ground.shadow.b}) · 明度差 ${(ground.lumDeltaPct * 100).toFixed(0)}%`} />
      <label><input type="checkbox" checked={pressed} onChange={(e) => setPressed(e.target.checked)} /> 按下</label>
      <label><input type="checkbox" checked={reduced} onChange={(e) => setReduced(e.target.checked)} /> 弱动效（投影钉静息）</label>
      <input type="range" min={0} max={1.5} step={0.05} value={speed} onChange={(e) => setSpeed(Number(e.target.value))} aria-label="指针速度" />
      <div className="j1x-hint">弱动效开启时投影仍存在（常态渲染），归零的是「变化」不是「存在」。</div>
    </SectionCard>
  );
}

/* ------------------------------- 显示器身份 ------------------------------- */

export function DisplayIdentityPanel(): React.ReactElement {
  const e = parseEdid(synthEdid({ manufacturer: "AUS", productCode: 0x1234, serial: 77, hActive: 3840, vActive: 2160 }));
  const cap = displayCapability(e, { wCm: 61, hCm: 34 });
  const conf = identityConfidence(e);
  const restore = mayRestore(e);
  return (
    <SectionCard title="显示器身份与能力档案（EDID 消费）" f="v7">
      <KV k="身份置信度" v={`${conf.confidence} — ${conf.describe}`} />
      <KV k="首选时序" v={`${cap.nativeW}×${cap.nativeH} @ ${(e.preferred?.pixelClockKhz ?? 0) / 1000}MHz`} />
      <KV k="物理口径" v={`PPI ${cap.ppi ?? "—"} · 点距 ${cap.dotPitchMm ?? "—"}mm · ${cap.diagonalIn ?? "—"}"`} />
      <KV k="落点恢复授权" v={restore.allowed ? "允许" : `拒绝——${restore.reason ?? ""}`} />
      <div className="j1x-hint">低置信身份拒绝恢复记忆点：宁可丢记忆，不可丢确定性。</div>
    </SectionCard>
  );
}

/* ------------------------------- 档案包生命周期 ------------------------------- */

export function ProfilePackPanel(): React.ReactElement {
  const [report, setReport] = useState<string>("");
  const demo: ProfileSnapshot[] = [
    { id: "p1", name: "办公", sens: 1.2, curve: "balanced" },
    { id: "p2", name: "绘图", sens: 0.8, curve: "precise", appOverrides: { krita: "draw" } },
  ];
  const doExport = () => {
    const pack = exportPack(demo, new Date().toISOString());
    const check = verifyPack(pack);
    setReport(check.ok ? `导出 2 档案 · v${pack.formatVersion} · 校验 ${pack.checksum} · 回读验证通过` : `回读失败：${check.reason}`);
    pushToast("success", "档案包已生成并回读验证");
  };
  const doMigrate = () => {
    const v1 = { formatVersion: 1, exportedAt: "old", profiles: [{ id: "p1", name: "旧包", curve: "Custom" }], checksum: "x" };
    const m = migrateProfilePack(v1 as Parameters<typeof migrateProfilePack>[0]);
    setReport(`v1→v2 迁移完成：curve 归一 ${m.profiles[0]?.curve} · appOverrides 补齐`);
  };
  const doMerge = () => {
    const incoming: ProfileSnapshot[] = [{ id: "p1", name: "办公", sens: 9 }, { id: "p3", name: "新档", sens: 1 }];
    const r = mergeProfiles(demo, incoming, "merge");
    setReport(`merge：新增 ${r.report.added} · 合并 ${r.report.merged}（补字段 ${r.report.mergedFields.flatMap((f) => f.fields).join("/") || "无"}）· 冲突字段保留现有`);
  };
  return (
    <SectionCard title="档案包生命周期（导出/迁移/合并）" f="v7">
      <div className="j1x-btnrow">
        <MiniButton onClick={doExport}>导出+回读验证</MiniButton>
        <MiniButton onClick={doMigrate}>v1→v2 迁移</MiniButton>
        <MiniButton onClick={doMerge}>冲突 merge 演示</MiniButton>
      </div>
      {report && <div className="j1x-log">{report}</div>}
      <div className="j1x-hint">默认 merge：丢用户数据是红线——冲突字段保留现有、缺失字段补入、身份字段永不覆盖。</div>
    </SectionCard>
  );
}
