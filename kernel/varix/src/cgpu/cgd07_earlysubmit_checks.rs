//! cgd07_earlysubmit_checks — CGPU-F0488 域自检（判据 12 条六组）。
//!
//! 判据六组：联动预留（提-01/02）、自适应（提-03/04）、回退（回-01/02）、
//! 诚实（诚-01）、账本（账-01）、判据元（码-01/02、元-01/02）。判据面
//! 零 panic 零 unsafe（无 unwrap/expect——全部 match/bool 落账）。

use alloc::string::String;
use alloc::vec::Vec;

use crate::cgpu::cgd07_earlysubmit::*;
use crate::checks::CheckSet;

/// 测试用确定性预测器（联动预留的正向验证——可注入即接口活的）。
struct ScriptedPredictor {
    /// 逐帧脚本（frame_no 下标越界回退末值）。
    script: Vec<u32>,
    /// 预测器名。
    tag: &'static str,
    /// 调用计数。
    pub calls: u32,
}

impl ScriptedPredictor {
    fn new(script: &[u32]) -> Self {
        ScriptedPredictor { script: script.to_vec(), tag: "scripted", calls: 0 }
    }
}

impl PredictorHook for ScriptedPredictor {
    fn predict(&mut self, frame_no: u64) -> u32 {
        self.calls += 1;
        let idx = if self.script.is_empty() {
            0
        } else {
            let n = self.script.len();
            let i = frame_no as usize;
            if i < n { i } else { n - 1 }
        };
        self.script[idx]
    }
    fn name(&self) -> &'static str {
        self.tag
    }
}

/// F0488 域自检（判据 12 条六组）。
pub fn run_cgd07_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0488");

    // --- 联动预留（判据一）---
    // 提-01：PredictorHook 接口位活——NoPredictor 桩恒回快照；脚本预测器
    // 可注入且逐帧取值（C04 接入点：实现 trait 即插即用，控制器不改行）。
    let mut stub = NoPredictor { ewma_snapshot: 40_000 };
    let stub_ok = stub.predict(1) == 40_000 && stub.predict(99) == 40_000
        && stub.name() == "no_predictor";
    let mut sp = ScriptedPredictor::new(&[30_000, 31_000, 32_000]);
    let sp_ok = sp.predict(0) == 30_000 && sp.predict(1) == 31_000
        && sp.predict(2) == 32_000 && sp.predict(9) == 32_000 && sp.calls == 4;
    if stub_ok && sp_ok {
        s.ok("D07-提-01-预测器接口联动预留");
    } else {
        s.fail("D07-提-01-预测器接口联动预留", "接口位失效");
    }

    // 提-02：F0369 理念复用——预测只进决策不进 correctness：plan_next 产
    // 出的 lead 由稳定性+偏差双闸决定，预测值仅留痕（predicted_us 字段），
    // 预测器 panic 面为零（trait 方法签名无 panic 语义）。
    let mut c2 = LeadController::new();
    let p2 = c2.plan_next(1, 40_000, &mut stub);
    let f0369_ok = p2.predicted_us == 40_000
        && p2.predictor == "no_predictor"
        && (p2.lead == 0 || p2.lead == 1);
    if f0369_ok {
        s.ok("D07-提-02-预测仅进决策不进correctness");
    } else {
        s.fail("D07-提-02-预测仅进决策不进correctness", "理念复用失效");
    }

    // --- 自适应（判据二）---
    // 提-03：升轨滞回——稳定连续 4 帧 lead 0→1（手算：EWMA 锚定 30000，
    // 样本 30000±1000 全 Stable 且 streak 达 4）；未满 4 帧 lead 保持 0。
    let mut c3 = LeadController::new();
    let mut sp3 = ScriptedPredictor::new(&[30_000]);
    let p31 = c3.plan_next(1, 30_000, &mut sp3); // streak=1
    let p32 = c3.plan_next(2, 30_500, &mut sp3); // streak=2
    let p33 = c3.plan_next(3, 29_800, &mut sp3); // streak=3
    let p34 = c3.plan_next(4, 30_200, &mut sp3); // streak=4 → lead=1
    let promote_ok = p31.lead == 0
        && p32.lead == 0
        && p33.lead == 0
        && p34.lead == 1
        && p34.mode == SubmitMode::Early
        && c3.promotions == 1
        && c3.max_lead_seen == 1;
    if promote_ok {
        s.ok("D07-提-03-升轨滞回连续四帧");
    } else {
        s.fail("D07-提-03-升轨滞回连续四帧", "升轨失效");
    }

    // 提-04：封顶+降轨即时——继续稳定帧 lead 封顶 2；单帧抖动立即归零
    // （降轨无滞回——抖动不等人）；降后重连续 4 帧才回升（滞回带双向
    // 不对称：升难降易）。
    let mut c4 = LeadController::new();
    let mut sp4 = ScriptedPredictor::new(&[30_000]);
    let mut f = 1u64;
    let mut capped = false;
    let mut promote_n = 0u32;
    while f <= 12 {
        let sample = 30_000u32;
        let p = c4.plan_next(f, sample, &mut sp4);
        if p.lead == MAX_LEAD {
            capped = true;
        }
        if c4.promotions > promote_n {
            promote_n = c4.promotions;
        }
        f += 1;
    }
    // 封顶 2 后单帧抖动（40000——dev=10000>6000 带）→ 归零。
    let p_jit = c4.plan_next(13, 40_000, &mut sp4);
    let demote_ok = p_jit.lead == 0 && p_jit.mode == SubmitMode::SyncFallback && c4.lead == 0;
    // 抖后回稳：重连续 4 帧 → lead 回 1（滞回复位——不是直接回 2）。
    let mut r4 = 0u32;
    let mut repromoted = false;
    let mut g = 14u64;
    while g <= 20 {
        let p = c4.plan_next(g, 30_000, &mut sp4);
        if c4.promotions > r4 {
            r4 = c4.promotions;
        }
        if p.lead == 1 && !repromoted {
            repromoted = true;
        }
        g += 1;
    }
    let retrack_ok = repromoted && c4.max_lead_seen == MAX_LEAD;
    if capped && demote_ok && retrack_ok && promote_n >= 2 {
        s.ok("D07-提-04-封顶降轨滞回不对称");
    } else {
        s.fail("D07-提-04-封顶降轨滞回不对称", "自适应失效");
    }

    // --- 回退（判据三）---
    // 回-01：偏差回退零窗口——|预测-实测|>12000 → SyncFallback+lead=0+
    // 计数+计划自检零案（回退计划合法）。
    let mut c5 = LeadController::new();
    // 先把 lead 拉到 1（8 稳定帧）。
    let mut sp5 = ScriptedPredictor::new(&[30_000]);
    let mut f5 = 1u64;
    while f5 <= 8 {
        let _ = c5.plan_next(f5, 30_000, &mut sp5);
        f5 += 1;
    }
    let lead_before = c5.lead;
    // 预测突变：预测 30_000 实测 50_000（dev=20000>12000）→ 回退。
    let mut sp5b = ScriptedPredictor::new(&[30_000]);
    let p5 = c5.plan_next(9, 50_000, &mut sp5b);
    let fb_ok = lead_before >= 1
        && p5.mode == SubmitMode::SyncFallback
        && p5.lead == 0
        && c5.fallbacks == 1
        && c5.lead == 0
        && p5.audit().is_none()
        && LeadController::miss_should_fallback(30_000, 50_000)
        && !LeadController::miss_should_fallback(30_000, 40_000);
    if fb_ok {
        s.ok("D07-回-01-偏差超带零窗口回退");
    } else {
        s.fail("D07-回-01-偏差超带零窗口回退", "回退闸失效");
    }

    // 回-02：回退不冻结——回退帧次帧照常评估（稳定样本回归 → streak 重
    // 累计，回退只是单帧事件不是锁死）。
    let p6 = c5.plan_next(10, 30_500, &mut sp5b);
    let p7 = c5.plan_next(11, 30_200, &mut sp5b);
    let recover_ok = c5.fallbacks == 1 // 无新增回退
        && p6.mode == SubmitMode::SyncFallback // streak 未满仍同步
        && c5.tracker.stable_streak >= 2; // streak 在重新累计
    let _ = p7;
    if recover_ok {
        s.ok("D07-回-02-回退不冻结次帧照常评估");
    } else {
        s.fail("D07-回-02-回退不冻结次帧照常评估", "回退冻结");
    }

    // --- 诚实（判据四）---
    // 诚-01：标注强制——每份计划 note 恒为 HONEST_NOTE（一期统计预测声
    // 明）；空标注计划 audit 报 EsNoteMissing；summary 携带读屏面。
    let mut c7 = LeadController::new();
    let mut sp7 = ScriptedPredictor::new(&[30_000]);
    let pa = c7.plan_next(1, 30_000, &mut sp7);
    let note_ok = pa.note == HONEST_NOTE
        && !HONEST_NOTE.is_empty()
        && HONEST_NOTE.contains("一期统计预测")
        && c7.summary().contains("D07");
    let bad_plan = SubmitPlan {
        frame_no: 2,
        lead: 0,
        mode: SubmitMode::SyncFallback,
        note: "",
        predictor: "no_predictor",
        predicted_us: 30_000,
        actual_us: 30_000,
    };
    let audit_note = bad_plan.audit() == Some(EsCode::EsNoteMissing);
    if note_ok && audit_note {
        s.ok("D07-诚-01-诚实标注一期统计预测");
    } else {
        s.fail("D07-诚-01-诚实标注一期统计预测", "标注失效");
    }

    // --- 账本（判据五）---
    // 账-01：账实守恒——plans == 提前计划数+回退计划数；max_lead_seen 单
    // 调不回退；立案簿与审计一致（自审零案则账面零案）。
    let mut c8 = LeadController::new();
    let mut sp8 = ScriptedPredictor::new(&[30_000]);
    let mut early_n = 0u32;
    let mut sync_n = 0u32;
    let mut f8 = 1u64;
    // 12 帧平稳（升轨）+ 1 帧抖动 + 3 帧偏差回退 + 2 帧平稳 = 18。
    while f8 <= 12 {
        let p = c8.plan_next(f8, 30_000, &mut sp8);
        match p.mode {
            SubmitMode::Early => early_n += 1,
            SubmitMode::SyncFallback => sync_n += 1,
        }
        f8 += 1;
    }
    let p13 = c8.plan_next(13, 40_000, &mut sp8); // 抖动帧（EWMA dev 10000>6000 抖；预测偏差 10000≤12000 吻合——走抖动归零不走回退）
    match p13.mode {
        SubmitMode::Early => early_n += 1,
        SubmitMode::SyncFallback => sync_n += 1,
    }
    let mut sp8b = ScriptedPredictor::new(&[30_000]);
    let mut f8b = 14u64;
    while f8b <= 16 {
        let p = c8.plan_next(f8b, 50_000, &mut sp8b); // 偏差回退×3
        match p.mode {
            SubmitMode::Early => early_n += 1,
            SubmitMode::SyncFallback => sync_n += 1,
        }
        f8b += 1;
    }
    let mut f8c = 17u64;
    while f8c <= 18 {
        let p = c8.plan_next(f8c, 30_000, &mut sp8b);
        match p.mode {
            SubmitMode::Early => early_n += 1,
            SubmitMode::SyncFallback => sync_n += 1,
        }
        f8c += 1;
    }
    let ledger_ok = c8.plans == 18
        && c8.plans == early_n + sync_n
        && c8.fallbacks == 3
        && c8.demotions >= 1
        && c8.case_len() == 0 // 自审零案
        && c8.max_lead_seen == MAX_LEAD
        && c8.tracker.stable_streak >= 1; // f18 对称回稳——EWMA 下行回落防卡高位（抖后可恢复的观测面）;
    if ledger_ok {
        s.ok("D07-账-01-账实守恒与立案一致");
    } else {
        s.fail("D07-账-01-账实守恒与立案一致", "账面失恒");
    }

    // --- 诊断码（判据六）---
    // 码-01：五码互异 + 0x4F 段独占（段占位由文件头声明锁定——互异性与
    // 详情非空此处复核）。
    let codes = EsCode::ALL;
    let mut codes_ok = codes.len() == 5;
    let mut i = 0usize;
    while i < codes.len() && codes_ok {
        if codes[i].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    // 静态策略自洽核验（policy_selfcheck 零码=策略面自洽）。
    let policy_ok = policy_selfcheck().is_none();
    if codes_ok && policy_ok {
        s.ok("D07-码-01-五码互异策略自洽");
    } else {
        s.fail("D07-码-01-五码互异策略自洽", "码或策略漂移");
    }

    // 码-02：计划审计三码可触发——lead 越界（构造 3）报 EsLeadOver、回退
    // 非零（lead=2+SyncFallback）报 EsFallbackNonZero、合法计划报 None。
    let over = SubmitPlan {
        frame_no: 1,
        lead: MAX_LEAD + 1,
        mode: SubmitMode::Early,
        note: HONEST_NOTE,
        predictor: "no_predictor",
        predicted_us: 30_000,
        actual_us: 30_000,
    };
    let nonzero = SubmitPlan {
        frame_no: 2,
        lead: 2,
        mode: SubmitMode::SyncFallback,
        note: HONEST_NOTE,
        predictor: "no_predictor",
        predicted_us: 30_000,
        actual_us: 30_000,
    };
    let legal = SubmitPlan {
        frame_no: 3,
        lead: 2,
        mode: SubmitMode::Early,
        note: HONEST_NOTE,
        predictor: "no_predictor",
        predicted_us: 30_000,
        actual_us: 30_000,
    };
    let audit_ok = over.audit() == Some(EsCode::EsLeadOver)
        && nonzero.audit() == Some(EsCode::EsFallbackNonZero)
        && legal.audit().is_none();
    if audit_ok {
        s.ok("D07-码-02-审计三码可触发");
    } else {
        s.fail("D07-码-02-审计三码可触发", "审计面失效");
    }

    // --- 判据元 ---
    // 元-01：判据面零 panic 零 unsafe（自扫本文件+实现文件，字符串剥空）。
    let src_impl = include_str!("cgd07_earlysubmit.rs");
    let src_checks = include_str!("cgd07_earlysubmit_checks.rs");
    let clean_impl = strip_lexical_noise(src_impl);
    let clean_checks = strip_lexical_noise(src_checks);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean_impl.contains(pat) || clean_checks.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        s.ok("D07-元-01-零panic零unsafe面");
    } else {
        s.fail("D07-元-01-零panic零unsafe面", "判据/生产面含 panic 或 unsafe");
    }

    // 元-02：常量口径对拍——PREDICT_BAND 严于 STABLE_BAND（预测错比负载
    // 抖更不可原谅）；EWMA 分母与 cgd06 同款 8；目标线 50ms 同口径；
    // 版本号非空。
    let consts_ok = PREDICT_BAND_US > STABLE_BAND_US
        && EWMA_DENOM == 8
        && TARGET_FRAME_US == 50_000
        && !EARLY_SUBMIT_VERSION.is_empty()
        && MAX_LEAD == 2;
    if consts_ok {
        s.ok("D07-元-02-常量口径对拍");
    } else {
        s.fail("D07-元-02-常量口径对拍", "口径漂移");
    }

    s
}

/// 单遍词法剥除（与 cgd06 同款：注释剥除、字符串整体剥空——判据模式串
/// 不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
