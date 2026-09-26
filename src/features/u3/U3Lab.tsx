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
  walkDictEntries, dictWalkVerdict, U3_DICT_ENTRIES,
  bridgeVerdict, KERNEL_CHECK_COUNTS,
} from "./engines";
import {
  V7_ENGINE_SELFCHECKS, v7EnginesSelfCheck,
  pendingLedger, recordWalk, blockedWorkorder, fiveCheck,
  MANUAL_WALK_QUERIES, DPI_SCALE_TIERS,
} from "./labapi";
import {
  V8_ENGINE_SELFCHECKS, v8EnginesSelfCheck,
  paneRows, auditDualForm, crossVerifySources, NotifChainLog,
  fiveCheckStructural, buildWallMatrix, type NotifChain,
} from "./labapi";
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

/* ------------------------------ v6 词典走查区 ------------------------------ */

/** U3Lab v6 交互词典走查实况（dictwalk 引擎活体驱动——红项显性不冒领）。 */
export function U3DictWalkSection(): React.ReactElement {
  const [lang] = useState<"zh" | "en">("zh");
  const [verdicts, setVerdicts] = useState<ReturnType<typeof dictWalkVerdict> | null>(null);
  const rows = verdicts === null ? null : walkDictEntries();

  return (
    <SectionCard title={u3Label("dictGroup", lang, U3_LAB_LABELS)} f="十章·一致性·v6">
      <div className="u3-lab-intro">
        U3 领地 {U3_DICT_ENTRIES.length} 个浮层/面板逐条走查：四条关闭出路（点外部/Esc/再点触发钮/失焦）+ 焦点归还链 + 埋点域——系统层锁屏 PIN 豁免 outside-click，其余缺一即红。
      </div>
      <Row fno="十章" name="浮层出路账本机检" desc="新增浮层必须先入 dictwalk 账本再写码——出路清单是需求不是优化">
        <button
          type="button"
          className="j1x-btn"
          onClick={() => setVerdicts(dictWalkVerdict(walkDictEntries(), U3_DICT_ENTRIES))}
        >执行走查</button>
        {verdicts && (
          <span className={`u3-badge ${verdicts.every((v) => v.pass) ? "ok" : "warn"}`}>
            {verdicts.filter((v) => v.pass).length}/{verdicts.length} {verdicts.every((v) => v.pass) ? u3Label("allGreen", lang, U3_LAB_LABELS) : u3Label("hasRed", lang, U3_LAB_LABELS)}
          </span>
        )}
      </Row>
      {verdicts && rows && verdicts.map((v, i) => (
        <Row key={v.id} fno={U3_DICT_ENTRIES[i]!.layer} name={v.id} desc={v.reason}>
          <span className={`u3-badge ${v.pass ? "ok" : "warn"}`}>{v.pass ? "✓" : "✗"}</span>
          <span className="u3-stat">出路 {rows[i]!.dismissalsOk ? "齐" : "缺"} · 焦点 {rows[i]!.focusOk ? "还" : "断"}</span>
        </Row>
      ))}
    </SectionCard>
  );
}

/* ------------------------------ v6 内核桥对账区 ------------------------------ */

/** U3Lab v6 内核域桥接对账实况（kernelbridge 引擎活体驱动）。 */
export function U3KernelBridgeSection(): React.ReactElement {
  const [lang] = useState<"zh" | "en">("zh");
  const [v, setV] = useState<ReturnType<typeof bridgeVerdict> | null>(null);

  return (
    <SectionCard title={u3Label("bridgeGroup", lang, U3_LAB_LABELS)} f="F501-F550·内核桥·v6">
      <div className="u3-lab-intro">
        内核 ustar3 十域镜像账本 ↔ 前端承接面对账：50 编号连续性、检查点计数溯源（共 {Object.values(KERNEL_CHECK_COUNTS).reduce((a, b) => a + b, 0)} 条）、差异显性化——红绿随闸门以 cargo test 为准，此处钉住结构面。
      </div>
      <Row fno="v6" name="两侧承接对账" desc="每编号内核有 run_fXXX_checks、前端有承接域——缺一侧即差异显性">
        <button type="button" className="j1x-btn" onClick={() => setV(bridgeVerdict())}>执行对账</button>
        {v && (
          <span className={`u3-badge ${v.total50 && v.diffs.length === 0 ? "ok" : "warn"}`}>
            50 编号 {v.total50 ? "连续" : "断"} · 差异 {v.diffs.length}
          </span>
        )}
      </Row>
      {v && (
        <Row fno="v6" name="内核十域检查点计数" desc="grep 实测口径誊写——内核深化后手工同步此表">
          {Object.entries(KERNEL_CHECK_COUNTS).map(([file, n]) => (
            <span key={file} className="u3-stat">{file.replace(".rs", "")} <b>{n}</b></span>
          ))}
        </Row>
      )}
    </SectionCard>
  );
}

/* ------------------------------ v7 走查预演工位区 ------------------------------ */

/** U3Lab v7 走查预演工位（walkrehearse 引擎活体驱动——pending 显性可入账可出单）。 */
export function U3WalkRehearseSection(): React.ReactElement {
  const [ledger, setLedger] = useState(() => pendingLedger(["deskicons", "copyops", "clockcal"]));
  const [workorder, setWorkorder] = useState<ReturnType<typeof blockedWorkorder> | null>(null);
  const [v7run, setV7run] = useState<ReturnType<typeof v7EnginesSelfCheck> | null>(null);
  const [benchMsg, setBenchMsg] = useState("工位待命——脚本在下、台账在中、工单出口在下");
  const done = ledger.filter((r) => r.result !== "pending").length;
  const q4 = ledger.filter((r) => r.queryNo === 4);
  // 收工五勾（结构事实样张——真机走查时以实际面板结构断言）
  const five = fiveCheck({ categoryPage: true, searchReachable: true, inlineAdjustable: true, descSentences: true, pathChainRegistered: true });

  const bookOne = useCallback(() => {
    const { ledger: next, changed } = recordWalk(ledger, 4, "deskicons", 100, "pass", "预演样张：100% 档走查绿");
    if (changed) {
      setLedger(next);
      setBenchMsg("查4·deskicons·100% 档已入账 pass（已入账不可改判）");
    } else {
      setBenchMsg("该槽已入账——重复入账被拒（防洗账）");
    }
  }, [ledger]);

  return (
    <SectionCard title="走查预演工位" f="十二查·manual-walk·v7">
      <div className="u3-lab-intro">
        十二查 manual-walk 类查项（{MANUAL_WALK_QUERIES.join("/")}）的预演工位：脚本生成（做什么/看什么/什么算过）→ 显性 pending 台账（{DPI_SCALE_TIERS.join("%/")}% 四档拆槽）→ 执行入账 → 工单导出（卡在哪/需要什么/谁能解）。不冒领全绿。
      </div>
      <Row fno="v7" name="v7 引擎群自检" desc="deskmenu/copyqueue/clockpanel/walkrehearse 四引擎同步执行——红项点名">
        <button type="button" className="j1x-btn" onClick={() => setV7run(v7EnginesSelfCheck())}>执行 v7 总自检</button>
        {v7run && (
          <span className={`u3-badge ${v7run.every((c) => c.pass) ? "ok" : "warn"}`}>
            {v7run.filter((c) => c.pass).length}/{v7run.length} {v7run.every((c) => c.pass) ? u3Label("allGreen", "zh", U3_LAB_LABELS) : u3Label("hasRed", "zh", U3_LAB_LABELS)}
          </span>
        )}
        {v7run && V7_ENGINE_SELFCHECKS.map((e) => {
          const n = v7run.filter((c) => c.name.startsWith(`[${e.engine}]`)).length;
          return <span key={e.engine} className="u3-stat">{u3Label(e.engine, "zh", U3_ENGINE_LABELS)} <b>{n}</b> 条</span>;
        })}
      </Row>
      <Row fno="查4" name="4K 四档走查台账（样张：3 域）" desc={`${DPI_SCALE_TIERS.join("%/")}% 每档一槽——执行入账后 pending → pass/fail，已入账不可改判（防洗账）`}>
        <button type="button" className="j1x-btn" onClick={bookOne}>入账 1 条（deskicons·100%）</button>
        <span className="u3-stat">已入账 <b>{done}</b> / {ledger.length} · 查4 剩 {q4.filter((r) => r.result === "pending").length}/{q4.length} 槽 pending</span>
        <span className="u3-stat" role="status">{benchMsg}</span>
      </Row>
      <Row fno="查12" name="收工五勾机检" desc="分类页/搜索/就地可调/说明句/路径链——从结构事实断言，缺勾点名">
        <span className={`u3-badge ${five.pass ? "ok" : "warn"}`}>{five.pass ? "✓ 五勾全绿" : `✗ 缺 ${five.missing.join("/")}`}</span>
      </Row>
      <Row fno="纪律④" name="阻塞工单导出" desc="多 AI 并行纪律④：卡在哪/需要什么/谁能解——卡住的永远是任务，不是人">
        <button type="button" className="j1x-btn" onClick={() => setWorkorder(blockedWorkorder(ledger))}>导出工单</button>
        {workorder && <span className="u3-stat">待走查 <b>{workorder.total}</b> 项</span>}
      </Row>
      {workorder && workorder.items.length > 0 && (
        <div className="u3-rehearse-wo" role="log" aria-label="阻塞工单">
          {workorder.items.slice(0, 4).map((b) => (
            <div key={`${b.domain}-${b.queryNo}-${b.need}`} className="u3-rehearse-wo-row">
              [{b.domain}·查{b.queryNo}] {b.what} → 需要：{b.need}
            </div>
          ))}
          {workorder.items.length > 4 && <div className="u3-rehearse-wo-row">…共 {workorder.total} 条（台账全量可导）</div>}
        </div>
      )}
    </SectionCard>
  );
}

/* ------------------------------ v8 批次八工单区 ------------------------------ */

/** U3Lab v8 批次八工单区（五引擎活体：同源审计/互证桥/三链路/五勾真结构/壁纸矩阵）。 */
export function U3V8Section(): React.ReactElement {
  const [v8run, setV8run] = useState<ReturnType<typeof v8EnginesSelfCheck> | null>(null);
  const [dual, setDual] = useState<ReturnType<typeof auditDualForm> | null>(null);
  const [verify, setVerify] = useState<ReturnType<typeof crossVerifySources> | null>(null);
  const [chainLog] = useState(() => new NotifChainLog());
  const [chainCount, setChainCount] = useState(0);
  const [five, setFive] = useState<ReturnType<typeof fiveCheckStructural> | null>(null);
  const [matrix, setMatrix] = useState<ReturnType<typeof buildWallMatrix> | null>(null);
  const [lastChain, setLastChain] = useState<NotifChain | null>(null);

  const openChain = useCallback(() => {
    const c = chainLog.open(false, Date.now() % 100000);
    chainLog.node(c.chainId, { layer: "banner", action: "show", atMs: 1, latencyMs: 12, exit: null });
    chainLog.node(c.chainId, { layer: "banner", action: "click", atMs: 2, latencyMs: 8, exit: "user-click" });
    setLastChain(c);
    setChainCount(chainLog.size());
  }, [chainLog]);

  const demoFacts = {
    name: "季度汇报.pptx", kind: "file" as const, openWith: "简报", location: "D:\\work",
    sizeBytes: 3 * 1024 * 1024, createdAt: 0, modifiedAt: 0, accessedAt: 0,
    readOnly: false, hidden: false, protectedReason: null, contains: null,
  };

  return (
    <SectionCard title="批次八工单区" f="F091/F078/F516/F501·v8">
      <div className="u3-lab-intro">
        批次八工单五件活体：①F091 详情窗格双形制同源机检（窗格字段全部由 F264 propsGeneral 派生）；②F078 日历飞出 × F549/F560 三方数据源互证桥（节假日锚点单一事实源收拢）；③通知中心→横幅→锁屏三链路体验日志贯通（关联 id 串线+孤儿显性）；④收工五勾从样张转真结构断言（注册表↔事实表↔store 三方对账）；⑤F501 五档亮度×三纹理×三区域壁纸采样矩阵（45 格真实扫描）。
      </div>
      <Row fno="v8" name="v8 引擎群自检" desc="exppane/calsync/notifchain/fivecheck/wallmatrix 五引擎同步执行——红项点名">
        <button type="button" className="j1x-btn" onClick={() => setV8run(v8EnginesSelfCheck())}>执行 v8 总自检</button>
        {v8run && (
          <span className={`u3-badge ${v8run.every((c) => c.pass) ? "ok" : "warn"}`}>
            {v8run.filter((c) => c.pass).length}/{v8run.length} {v8run.every((c) => c.pass) ? u3Label("allGreen", "zh", U3_LAB_LABELS) : u3Label("hasRed", "zh", U3_LAB_LABELS)}
          </span>
        )}
        {v8run && V8_ENGINE_SELFCHECKS.map((e) => {
          const n = v8run.filter((c) => c.name.startsWith(`[${e.engine}]`)).length;
          return <span key={e.engine} className="u3-stat">{u3Label(e.engine, "zh", U3_ENGINE_LABELS)} <b>{n}</b> 条</span>;
        })}
      </Row>
      <Row fno="工单①" name="F091 双形制同源机检" desc="窗格行全部由 propsGeneral 派生——mismatches 恒空当且仅当零旁路字段">
        <button type="button" className="j1x-btn" onClick={() => setDual(auditDualForm(demoFacts))}>跑同源审计</button>
        {dual && <span className={`u3-badge ${dual.sameSource ? "ok" : "warn"}`}>{dual.sameSource ? "✓ 同源零旁路" : `✗ ${dual.mismatches.length} 字段漂移`}</span>}
        {dual && <span className="u3-stat">窗格行 <b>{paneRows(demoFacts).length}</b> 条（含安全提示/包含行自动增删）</span>}
      </Row>
      <Row fno="工单②" name="F078 数据源互证桥" desc="groupA.ts × clockpanel × CNY_ANCHORS 三方逐锚比对——漂移点名（版本 v1 冻结出口）">
        <button type="button" className="j1x-btn" onClick={() => setVerify(crossVerifySources())}>跑三方互证</button>
        {verify && <span className={`u3-badge ${verify.ok ? "ok" : "warn"}`}>{verify.ok ? `✓ 三方 ${verify.counts.canonical} 锚零漂移` : `✗ ${verify.drifts.length} 处漂移`}</span>}
      </Row>
      <Row fno="工单③" name="三链路通知日志" desc="中心入链 → 横幅 show → click 关——关联 id 串线、孤儿事件显性拒绝、结论随链推导">
        <button type="button" className="j1x-btn" onClick={openChain}>开一条完整链路</button>
        <span className="u3-stat">链数 <b>{chainCount}</b>{lastChain ? ` · 末链 ${lastChain.chainId} 结论 ${lastChain.finalVerdict}` : ""}</span>
      </Row>
      <Row fno="工单④" name="收工五勾真结构" desc="U3_SECTIONS 注册表 ↔ 事实表双向对账 + store 交叉验证假可调——样张时代结束">
        <button type="button" className="j1x-btn" onClick={() => setFive(fiveCheckStructural())}>跑结构断言</button>
        {five && <span className={`u3-badge ${five.allGreen ? "ok" : "warn"}`}>{five.allGreen ? `✓ ${five.greenGroups} 组五勾全绿` : `✗ 漂移 ${five.missingFacts.length + five.unregisteredGroups.length} 组`}</span>}
      </Row>
      <Row fno="工单⑤" name="F501 壁纸采样矩阵" desc="五档亮度 × 三纹理 × 三区域 = 45 格真实像素扫描——滞回带防字色闪烁、F297 压暗联动">
        <button type="button" className="j1x-btn" onClick={() => setMatrix(buildWallMatrix())}>扫全矩阵</button>
        {matrix && <span className={`u3-badge ${matrix.every((m) => m.consistent) ? "ok" : "warn"}`}>{matrix.filter((m) => m.consistent).length}/{matrix.length} 格方向自洽</span>}
      </Row>
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
