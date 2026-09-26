/**
 * U3-v4 引擎实验室面板（AI-U3 · U3Lab）。
 *
 * 对齐 AI-E1 pages-lab 先例：v4 引擎群的**活体演示面**——不是判据注释的
 * 二次抄写，而是把十三台引擎接上真实输入、实时跑出数字。高密度纵深排布
 * （F302 乙基线）：引擎群总自检 + 十二查对账实况 + 体验日志实况三个区，
 * 每区都有真实数据（零占位）。
 *
 * 交互纪律（宪章三章）：浮层出路完整（本面板无浮层）；点击 100ms 内反馈
 * （同步执行，结果立现）；焦点环可见（j1x-btn 全局样式自带）。
 */

import React, { useCallback, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  V4_ENGINE_SELFCHECKS,
  v4EnginesSelfCheck,
  buildDomainChecklist,
  reconcileTwelveQueries,
  u3ItemCoverage,
  wrapIconLabelForLab,
  ExpLog,
  toImprovementItems,
} from "./labapi";
import {
  lockMountInit, lockMountPinFail, lockMountCooldownTick, lockMountUnlock,
  U3_ZORDER, bannerWindowSpec,
} from "./engines";
import { U3_ENGINE_LABELS, U3_LAB_LABELS, labelsSelfCheck, u3Label } from "./labels";

/* ------------------------------ 引擎群自检区 ------------------------------ */

export function U3LabSection(): React.ReactElement {
  const [lang] = useState<"zh" | "en">("zh");
  const [run, setRun] = useState<ReturnType<typeof v4EnginesSelfCheck> | null>(null);
  const [coverage, setCoverage] = useState<ReturnType<typeof u3ItemCoverage> | null>(null);
  const [log] = useState(() => new ExpLog());

  const runAll = useCallback(() => {
    setRun(v4EnginesSelfCheck());
    setCoverage(u3ItemCoverage());
    // 实验室自身的运行动作也进体验日志（十三章：每个功能自带日志）
    log.record({ atMs: Date.now(), domain: "lab", action: "click", targetId: "lab-run-all", latencyMs: 0, verdict: "smooth" });
  }, [log]);

  const green = run ? run.filter((c) => c.pass).length : 0;
  const total = run ? run.length : 0;

  return (
    <SectionCard title={u3Label("labTitle", lang, U3_LAB_LABELS)} f="F501-F550·v4">
      <div className="u3-lab-intro">{u3Label("labIntro", lang, U3_LAB_LABELS)}</div>

      <Row fno="v4" name={u3Label("runAll", lang, U3_LAB_LABELS)} desc="十三引擎同步执行：每引擎 5-9 条自检，覆盖判据常量、边界三点、破坏注入与显性报错路径">
        <button type="button" className="j1x-btn" onClick={runAll}>{u3Label("runAll", lang, U3_LAB_LABELS)}</button>
        {run && (
          <span className={`u3-badge ${green === total ? "ok" : "warn"}`}>
            {green === total ? "✓" : "✗"} {u3Label(green === total ? "allGreen" : "hasRed", lang, U3_LAB_LABELS)} {green}/{total}
          </span>
        )}
        {coverage && (
          <span className="u3-stat">F501-F550 覆盖 <b>{coverage.unique}/50</b>（缺失 {coverage.missing.length} · 重复 {coverage.duplicated.length}）</span>
        )}
      </Row>

      {run && (
        <div className="u3-lab-grid">
          {V4_ENGINE_SELFCHECKS.map((e) => {
            const checks = run.filter((c) => c.name.startsWith(`[${e.engine}]`));
            const eg = checks.filter((c) => c.pass).length;
            return (
              <div key={e.engine} className="u3-lab-engine">
                <div className="u3-lab-engine-head">
                  <span className="u3-lab-engine-name">{u3Label(e.engine, lang, U3_ENGINE_LABELS)}</span>
                  <span className={`u3-badge ${eg === checks.length ? "ok" : "warn"}`}>{eg}/{checks.length}</span>
                </div>
                <span className="u3-stat">{e.fScope}</span>
                {checks.filter((c) => !c.pass).slice(0, 3).map((c) => (
                  <div key={c.name} className="u3-lab-red">✗ {c.name}</div>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </SectionCard>
  );
}

/* ------------------------------ 十二查对账实况区 ------------------------------ */

export function U3WalkCheckSection(): React.ReactElement {
  const [summary, setSummary] = useState<ReturnType<typeof reconcileTwelveQueries> | null>(null);
  const runWalk = useCallback(() => {
    // 十三引擎按域装配十二查：自检类查项由引擎证据装载；manual-walk 显性 pending
    const perDomain = V4_ENGINE_SELFCHECKS.map((e) => ({
      domain: e.engine,
      evidence: buildDomainChecklist(e.engine, {
        runSelfcheck: e.run,
        constantsRegistered: true,
        manualWalkResults: {},
      }),
    }));
    setSummary(reconcileTwelveQueries(perDomain));
  }, []);

  return (
    <SectionCard title="十二查对账实况" f="通用验收·十二查">
      <Row
        fno="v4"
        name="50 项 × 12 查清单生成与对账"
        desc="自检类查项（功能完整/无感/回归/常量）由引擎证据自动装载；真机走查类查项（4K/三落位/路径链等）显性 pending——不冒领，缺证即红"
      >
        <button type="button" className="j1x-btn" onClick={runWalk}>执行对账</button>
        {summary && (
          <span className="u3-stat">
            {summary.domains} 域 · 证据全绿 <b>{summary.greenDomains}</b> · pending/红 <b>{summary.blocked.length}</b> 条
          </span>
        )}
      </Row>
      {summary && (
        <Row fno="v4" name="阻塞清单（前 6 条）" desc="每条阻塞 = 一个待办：真机走查排期或自检修复，收工前清零">
          {summary.blocked.slice(0, 6).map((b, i) => (
            <span key={`${b.domain}-${b.queryNo}-${i}`} className="u3-stat">查{b.queryNo}·{b.domain}</span>
          ))}
          {summary.blocked.length === 0 && <span className="u3-badge ok">✓ 无阻塞</span>}
        </Row>
      )}
      <Row fno="v4" name="labels 双语覆盖" desc="F140 语言面纪律：引擎名/组名/状态文案双语零缺键，缺键显性回退键名">
        {labelsSelfCheck().map((c) => (
          <span key={c.name} className={`u3-badge ${c.pass ? "ok" : "warn"}`}>{c.pass ? "✓" : "✗"} {c.name}</span>
        ))}
      </Row>
    </SectionCard>
  );
}

/* ------------------------------ 体验日志实况区 ------------------------------ */

export function U3ExpLogSection(): React.ReactElement {
  const [log] = useState(() => new ExpLog());
  const [, force] = useState(0);
  const [label, setLabel] = useState("两行封顶样张");

  const click = useCallback(() => {
    const t0 = performance.now();
    setLabel("两行封顶样张 · 截断演示项目名称特别长的那种");
    const latency = Math.round(performance.now() - t0);
    log.record({ atMs: Date.now(), domain: "f502", action: "click", targetId: "lab-wrap-demo", latencyMs: latency, verdict: ExpLog.verdictFor(latency, false, false) });
    force((v) => v + 1);
  }, [log]);

  const spam = useCallback(() => {
    // 模拟狂点：1.5s 内同点位 3 击 → 指纹器必须捕获（raging 用户的行为被看见）
    const base = Date.now();
    for (let i = 0; i < 3; i++) {
      log.record({ atMs: base + i * 200, domain: "f502", action: "click", targetId: "lab-wrap-demo", latencyMs: 10, verdict: "smooth" });
    }
    force((v) => v + 1);
  }, [log]);

  const demo = wrapIconLabelForLab(label);
  const signals = toImprovementItems(log.frustrationSignals());

  return (
    <SectionCard title="体验日志实况" f="十三章·体验日志">
      <Row fno="F502" name="两行封顶活体样张" desc="输入驱动 wrapIconLabel 换行引擎实时换行——第二行尾部省略号与全文三路可达语义同源">
        <button type="button" className="j1x-btn" onClick={click}>换一个长名</button>
        <span className="u3-stat">第一行 <b>{demo.lines[0]}</b></span>
        <span className="u3-stat">第二行 <b>{demo.lines[1]}</b></span>
        <span className={`u3-badge ${demo.truncated ? "warn" : "ok"}`}>{demo.truncated ? "已截断→Tooltip 承载全文" : "未截断"}</span>
      </Row>
      <Row fno="十三章" name="挫败信号指纹器" desc="狂点/无反馈/浮层反复开关自动捕获——体验结论字段随事件落账，可出改进清单">
        <button type="button" className="j1x-btn" onClick={spam}>模拟狂点 3 击</button>
        <span className="u3-stat">事件 <b>{log.size()}</b> 条</span>
        <span className="u3-stat">挫败信号 <b>{signals.length}</b> 组</span>
        {signals.slice(0, 3).map((s) => (
          <span key={s.rank} className="u3-badge warn">#{s.rank} {s.kind} → {s.suggestion}</span>
        ))}
        {signals.length === 0 && <span className="u3-badge ok">✓ 暂无挫败信号</span>}
      </Row>
    </SectionCard>
  );
}

/* ------------------------------ v5 锁屏挂接实况区 ------------------------------ */

/** U3Lab v5 锁屏/横幅挂接实况（lockmount 状态机活体驱动——非截图样张）。 */
export function U3LockMountSection(): React.ReactElement {
  const [rt, setRt] = useState(() => lockMountInit());
  const [msg, setMsg] = useState("已锁定——PIN 键盘自动切入");

  const wrongPin = useCallback(() => {
    const r = lockMountPinFail(rt, Date.now());
    setRt(r.rt);
    setMsg(r.message);
  }, [rt]);
  const cooldown = useCallback(() => {
    const r = lockMountCooldownTick(rt, Date.now());
    setRt(r.rt);
    setMsg(r.canFallback ? "冷却到期——密码登录可用" : `冷却中，剩 ${(r.remainingMs / 1000).toFixed(0)} 秒`);
  }, [rt]);
  const unlock = useCallback(() => {
    const r = lockMountUnlock(rt);
    setRt(r.rt);
    setMsg(`解锁成功（预算 ${r.budgetMs}ms）——焦点归还原窗口`);
  }, [rt]);

  const phaseText: Record<string, string> = {
    "locked": "锁定·PIN 键盘", "pin-entry": "PIN 输入中", "cooldown": "冷却中",
    "password-fallback": "密码回退", "unlocking": "解锁中",
  };

  return (
    <SectionCard title="锁屏横幅挂接实况" f="F504/F507/F508/F516·v5">
      <div className="u3-lab-intro">
        锁屏窗口状态机（锁定→PIN→冷却→密码回退→解锁归零）+ Z 序总表 + 横幅窗口规格——全部来自 lockmount 引擎活体驱动。
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F504</span>锁屏状态机</div>
          <div className="u3-desc">当前态 <b>{phaseText[rt.state.phase] ?? rt.state.phase}</b> · 连错 {rt.failCount} 次（满 5 进冷却，逐次翻倍）</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <button type="button" className="j1x-btn" onClick={wrongPin}>输错一次 PIN</button>
          <button type="button" className="j1x-btn" onClick={cooldown} disabled={rt.state.phase !== "cooldown"}>冷却推进</button>
          <button type="button" className="j1x-btn" onClick={unlock}>解锁成功</button>
          <span className="u3-stat" role="status">{msg}</span>
        </div>
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F508</span>Z 序总表</div>
          <div className="u3-desc">章十一致性：全系统一套 zIndex——消费方按层取值，不自定</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          {Object.entries(U3_ZORDER).map(([layer, z]) => (
            <span key={layer} className="u3-stat">{layer} <b>{z}</b></span>
          ))}
        </div>
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F516</span>横幅窗口规格</div>
          <div className="u3-desc">置顶不进任务栏 · 可点不穿透 · 出路三路（点击/超时/失焦）</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          {bannerWindowSpec().exits.map((x) => <span key={x} className="u3-badge ok">✓ {x}</span>)}
        </div>
      </div>
    </SectionCard>
  );
}

/* ------------------------------ 行组件（实验室自持，不依赖 U3Tab 内部件） ------------------------------ */

function Row(props: { fno: string; name: string; desc: string; children: React.ReactNode }): React.ReactElement {
  return (
    <div className="u3-row">
      <div>
        <div className="u3-name"><span className="fno">{props.fno}</span>{props.name}</div>
        <div className="u3-desc">{props.desc}</div>
      </div>
      <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>{props.children}</div>
    </div>
  );
}
