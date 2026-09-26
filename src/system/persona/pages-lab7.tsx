/**
 * E 域页组⑫ · 深化实验室七（批次十收官）：导览/预设库/键鼠对等面板。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton, Notice, useT } from "./ui";
import { buildTour, TourRunner, PresetLibrary, auditParity, E_ACTION_PAIRS } from "./tour-preset-parity";

// ---------- 导览面板 ----------

const FIRST_MIN_TOUR = buildTour("e-first-min", "五分钟上手", [
  ["tokens", "tokens-accent", "改一个强调色——全桌即时生效", "click-target"],
  ["preview", "preview-diff", "看差异预览——改了什么一目了然", "next-button"],
  ["archive", "archive-export", "导出档案——个性随时可迁移", "next-button"],
]);

export function TourCard(): React.ReactNode {
  const t = useT();
  const [runner, setRunner] = useState(() => new TourRunner(FIRST_MIN_TOUR));
  const step = runner.current;
  return (
    <Card title={t("tourTitle")}>
      <Row label={t("tourProgress") + ` ${runner.progress()}%`} sub={step ? `${step.page}/${step.targetId}: ${step.note}（触发: ${step.advance}）` : "导览完成——跳过可找回（first-run 接线）"}>
        {step ? <PButton onClick={() => setRunner(new TourRunner(FIRST_MIN_TOUR))}>{t("tourSkip")}</PButton> : <span />}
      </Row>
      <Notice tone="info">五分钟契约：步数 ≤10（每步 ≤30s）· Esc 整体跳过 · 跳过的引导在帮助中心可找回（十一章三问全答）。</Notice>
    </Card>
  );
}

// ---------- 预设库面板 ----------

export function PresetLibraryCard(): React.ReactNode {
  const t = useT();
  const demo = useMemo(() => {
    const lib = new PresetLibrary();
    lib.registerOfficial("p-dark-pro", "深色专业", { accent: "#6e7fd4", density: "standard" });
    const saveOnOfficial = lib.saveUser("p-dark-pro", "盗版官方", { accent: "#ff0000" });
    lib.saveUser("p-mine", "我的配置", { accent: "#22aa88", density: "dense" });
    const delOfficial = lib.deleteUser("p-dark-pro");
    const delMine = lib.deleteUser("p-mine");
    const preset = lib.get("p-dark-pro")!;
    const diff = lib.diff(preset, { accent: "#ff8800", density: "standard" });
    return { count: lib.all.length, saveOnOfficial, delOfficial: delOfficial.reason, delMine: delMine.ok, diff };
  }, []);
  return (
    <Card title={t("presetTitle")}>
      <Row label={`${t("presetOfficial")}（${demo.count} 条在册）`} sub={`覆盖官方预设: ${demo.saveOnOfficial.reason} · 删官方: ${demo.delOfficial} · 删用户: ${demo.delMine ? "成功" : "失败"}`}>
        <span />
      </Row>
      <Row label={t("presetDiff")} sub={demo.diff.map((d) => `${d.path}: ${JSON.stringify(d.from)}→${JSON.stringify(d.to)}`).join(" · ") + "（应用前先看差异——不是盲应用）"}>
        <span />
      </Row>
      <Notice tone="info">三铁律"预设+微调"：官方预设保证出厂可回退、用户预设自由增删、应用带差异预览与撤销。</Notice>
    </Card>
  );
}

// ---------- 键鼠对等面板 ----------

export function ParityCard(): React.ReactNode {
  const t = useT();
  const audit = useMemo(() => auditParity(E_ACTION_PAIRS), []);
  const withGap = useMemo(() => auditParity([...E_ACTION_PAIRS, { action: "拖拽组件", mouse: "拖到目标位", keyboard: null }]), []);
  return (
    <Card title={t("parityTitle")}>
      <Row label={`核心动作 ${audit.total} 项`} sub={audit.ok ? "全部有键盘等价——键盘用户与鼠标用户能力对等（四章红线）" : `缺等价: ${audit.missing.map((m) => m.action).join(",")}`}>
        <span style={{ color: audit.ok ? "var(--p-success)" : "var(--p-danger)" }}>{audit.ok ? "对等" : "有缺口"}</span>
      </Row>
      <Row label="缺口的样貌（负例演示）" sub={withGap.missing.map((m) => `${m.action}: ${t("parityMissing")}`).join(" · ")}>
        <span />
      </Row>
      {E_ACTION_PAIRS.slice(0, 3).map((p) => (
        <Row key={p.action} label={p.action} sub={`鼠标: ${p.mouse} ｜ 键盘: ${p.keyboard}`}>
          <span />
        </Row>
      ))}
    </Card>
  );
}
