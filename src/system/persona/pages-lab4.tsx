/**
 * E 域页组⑨ · 深化实验室四（批次七）：状态块/导出格式/UX 词典面板。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton } from "./ui";
import { EmptyStateBlock, ErrorTriadBlock, confirmDialogModel, auditStateCoverage, STATE_PAGES } from "./state-blocks";
import { EXPORT_FORMATS, auditFormatRegistry, recognizeFormat, migrationNotes } from "./export-formats";
import { UX_DICTIONARY, auditDictionary, FirstRunLedger, honestProgress, cancelTransition, pushUndo, UNDO_STACK_MAX } from "./ux-dictionary";

// ---------- 状态块演示（二十页覆盖审计） ----------

export function StateBlocksCard(): React.ReactNode {
  const [demoPage, setDemoPage] = useState<string>("archive");
  const coverage = useMemo(() => auditStateCoverage(), []);
  const model = confirmDialogModel(demoPage);
  return (
    <Card title="状态块渲染层（十二查 8/9/11 接线）">
      <Row label={`二十页目录覆盖：${coverage.total} 页`} sub={coverage.missingCatalog.length === 0 ? "空态/错误态/确认态三态双语齐备（漏页即红）" : `缺目录: ${coverage.missingCatalog.join(",")}`}>
        <span style={{ color: coverage.missingCatalog.length === 0 ? "var(--p-success)" : "var(--p-danger)" }}>{coverage.missingCatalog.length === 0 ? "全覆盖" : "有缺口"}</span>
      </Row>
      <Row label="页面切换" sub="EmptyStateBlock / ErrorTriadBlock / 确认模型随目录即时换装">
        <span style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
          {STATE_PAGES.slice(0, 8).map((p) => (
            <PButton key={p} kind={demoPage === p ? "primary" : "ghost"} onClick={() => setDemoPage(p)}>{p}</PButton>
          ))}
        </span>
      </Row>
      <EmptyStateBlock page={demoPage} />
      <ErrorTriadBlock page={demoPage} technical="demo: QuotaExceededError（默认折叠——技术细节不上首行）" />
      {model ? (
        <Card title={`确认对话模型 · ${model.title}`}>
          {model.rows.map((r) => (
            <Row key={r.label} label={`${r.label}: ${r.text}`} sub="">
              <span />
            </Row>
          ))}
          <Row label={`按钮序（词典 D-CONFIRM-01）`} sub={model.buttons.map((b) => `${b.position}: ${b.text}（${b.kind}）`).join(" · ") + "——取消永远在安全侧"}>
            <span />
          </Row>
        </Card>
      ) : null}
    </Card>
  );
}

// ---------- 导出格式注册表面板 ----------

export function ExportFormatsCard(): React.ReactNode {
  const [probe, setProbe] = useState<string>('{"format":"vxkeymap","version":1,"overrides":{}}');
  const audit = useMemo(() => auditFormatRegistry(), []);
  const probeResult = useMemo(() => {
    try {
      return recognizeFormat(JSON.parse(probe));
    } catch {
      return { format: null, advice: "不是合法 JSON——门房直接拒（不猜）" };
    }
  }, [probe]);
  return (
    <Card title="导出格式注册表（十四章开放性）">
      <Row label={`注册 ${EXPORT_FORMATS.length} 格式（active ${audit.activeCount}）`} sub={audit.ok ? "机检全绿：active 有 schema 约束、deprecated 有去向（废弃走流程）" : audit.issues.join("；")}>
        <span style={{ color: audit.ok ? "var(--p-success)" : "var(--p-danger)" }}>{audit.ok ? "红线达标" : "有违例"}</span>
      </Row>
      {EXPORT_FORMATS.slice(0, 5).map((f) => (
        <Row key={f.id} label={`${f.id} v${f.version}（${f.status}）`} sub={`${f.purpose} · 兼容: ${f.compatibility}`}>
          <span />
        </Row>
      ))}
      <Row label="格式识别（导入门房）" sub={probeResult.advice + (probeResult.format ? ` · 迁移说明: ${migrationNotes(probeResult.format.id)[2] ?? migrationNotes(probeResult.format.id)[1]}` : "")}>
        <input value={probe} onChange={(e) => setProbe(e.target.value)} style={{ width: 220, fontSize: 11 }} aria-label="格式探测输入" />
      </Row>
    </Card>
  );
}

// ---------- UX 词典面板 ----------

export function UxDictionaryCard(): React.ReactNode {
  const demo = useMemo(() => {
    const dict = auditDictionary();
    // first-run：只出现一次/可跳过/可找回。
    const fr = new FirstRunLedger();
    fr.register("accent-tip", "改一个颜色即可全桌生效");
    const show1 = fr.shouldShow("accent-tip");
    fr.markSeen("accent-tip");
    const show2 = fr.shouldShow("accent-tip");
    fr.register("skip-demo", "可跳过的引导");
    fr.skip("skip-demo");
    const findable = fr.findable().length;
    // 诚实进度：正常 / 样本不足 / 卡住三态。
    const now = Date.now();
    const flowing = honestProgress([{ at: now - 8000, done: 10 }, { at: now - 4000, done: 30 }, { at: now, done: 50 }], 100, now);
    const insufficient = honestProgress([{ at: now, done: 1 }], 100, now);
    const stalled = honestProgress([{ at: now - 40000, done: 10 }, { at: now - 20000, done: 10 }, { at: now, done: 10 }], 100, now);
    // 取消状态机 running → cancelling → cancelled。
    const seq = ["running", cancelTransition("running", "cancel"), cancelTransition("cancelling", "cancel-ack")];
    // undo 归并：同类 2s 内并一步 + 栈深封顶。
    let stack = pushUndo([], { kind: "slider", ops: 1, at: 0 });
    stack = pushUndo(stack, { kind: "slider", ops: 1, at: 1000 });
    stack = pushUndo(stack, { kind: "slider", ops: 1, at: 1500 });
    stack = pushUndo(stack, { kind: "color", ops: 1, at: 3000 });
    return {
      dict, show1, show2, findable,
      flowing, insufficient, stalled, seq,
      undo: stack.map((g) => `${g.kind}×${g.ops}`).join(" · "),
      undoMax: UNDO_STACK_MAX,
      corePath: new FirstRunLedger().corePath().join(" → "),
    };
  }, []);
  return (
    <Card title="UX 词典与状态模型（十/十一/八/九章）">
      <Row label={`交互词典 ${UX_DICTIONARY.length} 条规则`} sub={demo.dict.ok ? "机检零违例（违例登记即红——规则即数据）" : demo.dict.findings.map((f) => f.ruleId).join(",")}>
        <span style={{ color: demo.dict.ok ? "var(--p-success)" : "var(--p-danger)" }}>{demo.dict.ok ? "一致" : "有违例"}</span>
      </Row>
      <Row label="first-run 三态（只一次/可跳过/可找回）" sub={`首见 ${demo.show1} → 已见 ${demo.show2} · 跳过可找回 ${demo.findable} 条 · 新手核心路径: ${demo.corePath}`}>
        <span />
      </Row>
      <Row label="诚实进度三态" sub={`正常: ${demo.flowing.message} · 样本不足: ${demo.insufficient.message} · 卡住: ${demo.stalled.message}`}>
        <span />
      </Row>
      <Row label="可取消状态机" sub={demo.seq.join(" → ") + "（cancel-failed 显性化——取消失败不是假装取消成功）"}>
        <span />
      </Row>
      <Row label={`undo 归并（同类 2s 内并步 · 栈深 ${demo.undoMax}）`} sub={demo.undo}>
        <span />
      </Row>
    </Card>
  );
}
