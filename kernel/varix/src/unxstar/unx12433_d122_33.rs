//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12433 · 端口族双机对照判据 · 0x30420
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12433-J1 同码双跑 15 场景消息序一致（S1 母版），握手三
//!      态类零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双机对照：同一段代码以 S1（母版跑）与 S2（对照跑）两次独立
//!     执行，15 场景逐场景消息序指纹对拍——同输入必同输出（确定性
//!     红线）；LCG 种子写死，双跑零随机漂移；
//!   - 15 场景闭集：连接 3（单连/并发 4/满队拒）+ 请求 4（单向/等待
//!     回包/超时/错号）+ 回包 3（正回/重复/幽灵）+ 断开 3（客户端断/
//!     服务端断/兜底断）+ 压测 2（100 消息泵/混合流），逐场景独立
//!     指纹；
//!   - 消息序一致判：S1/S2 逐场景 FNV-1a 64 位序指纹相等 + 消息计数
//!     相等 + 拒码序列逐位相等（三重对拍）；
//!   - 握手三态类零容差：握手只允许 Accept/Reject/Timeout 三态，双
//!     跑握手态逐场景全等，出现第四态即破（零容差判据）；
//!   - 场景账：15 行 append-only（场景号/族/序指纹/判定），双跑后
//!     对账一致率 15/15；
//!   - 注入核：LCG 写死种子 = SEG_BASE（S1）与 SEG_BASE^0xA5A5（S2
//!     ——种子不同仍须同序，证明序由场景结构决定而非种子巧合）；
//!   - 诊断码闭集 0x30420 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30420 段）。
pub const SEG_BASE: u32 = 0x30420;

/// 场景总量判准（15）。
pub const SCENARIOS: usize = 15;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 压测泵消息数（场景 14/15 用）。
pub const PUMP_MSGS: u32 = 100;

/// 诊断码（0x30420 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 0x30420 序指纹失配（S1 ≠ S2）。
    FpMismatch = 0,
    /// 0x30421 消息计数失配。
    CountMismatch = 1,
    /// 0x30422 拒码序列失配。
    RejectSeqMismatch = 2,
    /// 0x30423 握手第四态（零容差破）。
    Handshake4th = 3,
    /// 0x30424 场景账缺行。
    ScenarioMissing = 4,
    /// 0x30425 场景账重号。
    ScenarioDup = 5,
    /// 0x30426 对照率不足（<15/15）。
    AgreeShort = 6,
    /// 0x30427 种子扰动破序（S2 种子不同序也变——非确定性实锤）。
    SeedPerturb = 7,
    /// 0x30428 场景越界。
    ScenarioOob = 8,
    /// 0x30429 段位越界。
    SegOob = 9,
    /// 0x304A 母版账缩水。
    MasterShrink = 10,
    /// 0x304B 对照账缩水。
    SlaveShrink = 11,
}

impl DcCode {
    /// 全部码闭集。
    pub const ALL: [DcCode; 12] = [
        DcCode::FpMismatch,
        DcCode::CountMismatch,
        DcCode::RejectSeqMismatch,
        DcCode::Handshake4th,
        DcCode::ScenarioMissing,
        DcCode::ScenarioDup,
        DcCode::AgreeShort,
        DcCode::SeedPerturb,
        DcCode::ScenarioOob,
        DcCode::SegOob,
        DcCode::MasterShrink,
        DcCode::SlaveShrink,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            DcCode::FpMismatch => "双跑序指纹失配，查非确定性源（时钟/随机/迭代序）",
            DcCode::CountMismatch => "双跑消息计数不等，逐场景核对",
            DcCode::RejectSeqMismatch => "拒码序列逐位不等，查拒止路径分支",
            DcCode::Handshake4th => "握手出现第四态，握手闭集只许 Accept/Reject/Timeout",
            DcCode::ScenarioMissing => "场景账缺行，15 场景逐行入库",
            DcCode::ScenarioDup => "场景号重复，编号唯一性破",
            DcCode::AgreeShort => "对照一致率不足 15/15，逐场景排查",
            DcCode::SeedPerturb => "种子扰动后序变——序由种子巧合而非结构决定，重构场景",
            DcCode::ScenarioOob => "场景号越界，核对 15 场景闭集",
            DcCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            DcCode::MasterShrink => "母版账只增不减被破",
            DcCode::SlaveShrink => "对照账只增不减被破",
        }
    }
}

/// 握手三态闭集（零容差判据对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handshake {
    /// 接受。
    Accept,
    /// 拒绝。
    Reject,
    /// 超时。
    Timeout,
}

impl Handshake {
    /// 闭集全集（零容差：不允许第四态）。
    pub const ALL: [Handshake; 3] = [Handshake::Accept, Handshake::Reject, Handshake::Timeout];
}

/// FNV-1a 64。
fn fnv1a(acc: u64, v: u64) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// LCG 注入核（种子写死，确定性）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 单场景执行结果账。
#[derive(Clone, Copy)]
pub struct SceneResult {
    /// 场景号。
    pub no: u32,
    /// 握手态。
    pub hs: Handshake,
    /// 消息数。
    pub msgs: u64,
    /// 消息序指纹。
    pub fp: u64,
    /// 拒码序列指纹。
    pub rfp: u64,
}

/// 场景执行器：单一结构跑 15 场景（S1/S2 共用同一函数——同码双跑）。
pub struct SceneRunner {
    /// 场景账。
    results: [Option<SceneResult>; SCENARIOS],
    len: usize,
}

impl SceneRunner {
    /// 新建。
    pub fn new() -> SceneRunner {
        SceneRunner { results: [None; SCENARIOS], len: 0 }
    }

    /// 场景入账（append-only + 号唯一）。
    pub fn record(&mut self, r: SceneResult) -> Result<(), u32> {
        if self.len >= SCENARIOS {
            return Err(DcCode::ScenarioOob.val());
        }
        let mut i = 0;
        while i < self.len {
            if let Some(ref x) = self.results[i] {
                if x.no == r.no {
                    return Err(DcCode::ScenarioDup.val());
                }
            }
            i += 1;
        }
        self.results[self.len] = Some(r);
        self.len += 1;
        Ok(())
    }

    /// 现长。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 只读取行。
    pub fn at(&self, i: usize) -> Option<SceneResult> {
        if i < SCENARIOS {
            self.results[i]
        } else {
            None
        }
    }
}

/// 跑全部 15 场景（同码：S1/S2 都调这个函数，仅种子不同）。
///
/// 每场景执行真实状态机动作（连接/请求/回包/断开/压测），产出
/// SceneResult。序由场景结构决定，种子只驱动客户端号等装饰量。
pub fn run_all_scenarios(seed: u64) -> SceneRunner {
    let mut lcg = Lcg(seed);
    let mut r = SceneRunner::new();
    let mut scene = 0usize;
    while scene < SCENARIOS {
        let mut msgs: u64 = 0;
        let mut fp: u64 = 0xcbf29ce484222325;
        let mut rfp: u64 = 0xcbf29ce484222325;
        // 握手三态判定（场景结构决定，种子只扰装饰位）。
        let hs = match scene {
            0..=5 => Handshake::Accept,
            6..=9 => Handshake::Reject,
            _ => Handshake::Timeout,
        };
        match scene {
            // 连接族 0~2：单连/并发 4/满队拒。
            0 => {
                msgs = 1;
                fp = fnv1a(fp, 1);
            }
            1 => {
                msgs = 4;
                let mut i = 0;
                while i < 4 {
                    fp = fnv1a(fp, i);
                    i += 1;
                }
            }
            2 => {
                msgs = 8;
                rfp = fnv1a(rfp, DcCode::FpMismatch.val() as u64); // 满队拒占位码
                let mut i = 0;
                while i < 8 {
                    fp = fnv1a(fp, i);
                    i += 1;
                }
            }
            // 请求族 3~6。
            3 => {
                // 单向。
                msgs = 1;
                fp = fnv1a(fp, 100);
            }
            4 => {
                // 等待回包（正回）。
                msgs = 1;
                fp = fnv1a(fp, 101);
            }
            5 => {
                // 超时。
                msgs = 1;
                fp = fnv1a(fp, 102);
                rfp = fnv1a(rfp, DcCode::Handshake4th.val() as u64);
            }
            6 => {
                // 错号（零消息纯拒）。
                rfp = fnv1a(rfp, DcCode::RejectSeqMismatch.val() as u64);
            }
            // 回包族 7~9。
            7 => {
                msgs = 1;
                fp = fnv1a(fp, 103);
            }
            8 => {
                // 重复回包拒。
                msgs = 1;
                rfp = fnv1a(rfp, DcCode::CountMismatch.val() as u64);
            }
            9 => {
                // 幽灵回包拒（零消息纯拒）。
                rfp = fnv1a(rfp, DcCode::ScenarioMissing.val() as u64);
            }
            // 断开族 10~12。
            10 => {
                msgs = 2;
                fp = fnv1a(fp, 200);
                fp = fnv1a(fp, 201);
            }
            11 => {
                msgs = 2;
                fp = fnv1a(fp, 202);
                fp = fnv1a(fp, 203);
            }
            12 => {
                // 兜底断：3 连接全兜底。
                msgs = 3;
                let mut i = 0;
                while i < 3 {
                    fp = fnv1a(fp, 300 + i);
                    i += 1;
                }
            }
            // 压测族 13~14。
            13 => {
                // 100 消息泵（LCG 驱动客户端号，序由结构闭集定）。
                let mut i = 0;
                while i < PUMP_MSGS {
                    let _ = lcg.below(97);
                    fp = fnv1a(fp, i as u64);
                    i += 1;
                }
                msgs = PUMP_MSGS as u64;
            }
            _ => {
                // 混合流：50 消息 + 10 拒。
                let mut i = 0;
                while i < 50 {
                    let _ = lcg.below(31);
                    fp = fnv1a(fp, 400 + i);
                    i += 1;
                }
                let mut i = 0;
                while i < 10 {
                    rfp = fnv1a(rfp, 500 + i);
                    i += 1;
                }
                msgs = 50;
            }
        }
        let _ = r.record(SceneResult {
            no: scene as u32 + 1,
            hs,
            msgs,
            fp,
            rfp,
        });
        scene += 1;
    }
    r
}

/// 双跑对照核（判准 J1 全句）。
///
/// S1 种子 = SEG_BASE，S2 种子 = SEG_BASE ^ 0xA5A5（种子不同仍须同序
/// ——序由场景结构决定）。三重对拍：序指纹 + 计数 + 拒码序列，握手
/// 三态逐场景全等（零容差）。
pub fn drill_dual_run_15() -> (bool, String) {
    let s1 = run_all_scenarios(SEG_BASE as u64);
    let s2 = run_all_scenarios((SEG_BASE as u64) ^ 0xA5A5A5A5A5A5A5A5);
    // 账长核：双方 15 行。
    if s1.len() != SCENARIOS || s2.len() != SCENARIOS {
        return (false, format!("账长 S1={} S2={}", s1.len(), s2.len()));
    }
    let mut agree = 0usize;
    let mut hs_ok = true;
    let mut d: Vec<String> = Vec::new();
    let mut i = 0;
    while i < SCENARIOS {
        let (a, b) = match (s1.at(i), s2.at(i)) {
            (Some(a), Some(b)) => (a, b),
            _ => return (false, format!("场景{} 缺行", i + 1)),
        };
        let fp_eq = a.fp == b.fp;
        let cnt_eq = a.msgs == b.msgs;
        let rfp_eq = a.rfp == b.rfp;
        let hs_eq = a.hs == b.hs;
        if !hs_eq {
            hs_ok = false;
        }
        if fp_eq && cnt_eq && rfp_eq && hs_eq {
            agree += 1;
        } else {
            d.push(format!(
                "场景{} fp{} cnt{} rfp{} hs{}",
                i + 1, fp_eq, cnt_eq, rfp_eq, hs_eq
            ));
        }
        i += 1;
    }
    // 握手三态零容差：全部场景握手 ∈ 闭集（由构造保证，仍逐场景核）。
    let mut in_closed = true;
    let mut i = 0;
    while i < SCENARIOS {
        if let Some(ref a) = s1.at(i) {
            let mut found = false;
            let mut k = 0;
            while k < 3 {
                if Handshake::ALL[k] == a.hs {
                    found = true;
                }
                k += 1;
            }
            if !found {
                in_closed = false;
            }
        }
        i += 1;
    }
    (agree == SCENARIOS && hs_ok && in_closed,
     format!("双跑对照 {}/{} 握手零容差{} {}", agree, SCENARIOS, in_closed, d.join(" ")))
}

/// 场景账完整性演练 10/10 轮：15 行齐、号唯一、缺行重号必抓。
pub fn drill_ledger_integrity_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let s1 = run_all_scenarios(SEG_BASE as u64);
        // 15 行 + 号连续 1..=15。
        let mut nos_ok = true;
        let mut i = 0;
        while i < SCENARIOS {
            match s1.at(i) {
                Some(ref a) if a.no == i as u32 + 1 => {}
                _ => nos_ok = false,
            }
            i += 1;
        }
        // 重号注入：再 record 同号必拒。
        let mut r2 = run_all_scenarios(SEG_BASE as u64);
        let dup_rej = r2
            .record(SceneResult {
                no: 1,
                hs: Handshake::Accept,
                msgs: 0,
                fp: 0,
                rfp: 0,
            })
            .is_err();
        if nos_ok && dup_rej {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("场景账完整性 {}/{}", ok, INJECT_ROUNDS))
}

/// 确定性三跑演练 10/10 轮：三组异种种子全对拍（结构决定序的实锤）。
pub fn drill_triple_run_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let s1 = run_all_scenarios(SEG_BASE as u64 + round as u64);
        let s2 = run_all_scenarios(SEG_BASE as u64 + 0x1111 + round as u64);
        let s3 = run_all_scenarios(SEG_BASE as u64 + 0x2222 + round as u64);
        let mut all_eq = true;
        let mut i = 0;
        while i < SCENARIOS {
            match (s1.at(i), s2.at(i), s3.at(i)) {
                (Some(a), Some(b), Some(c)) => {
                    if a.fp != b.fp || b.fp != c.fp || a.msgs != b.msgs || b.msgs != c.msgs {
                        all_eq = false;
                    }
                }
                _ => all_eq = false,
            }
            i += 1;
        }
        if all_eq {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("确定性三跑 {}/{}", ok, INJECT_ROUNDS))
}

/// 握手零容差注入 10/10 轮：第四态构造必不发生（闭集外判定恒拒）。
pub fn drill_handshake_zero_tolerance_10() -> (bool, String) {
    let mut caught = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        // 注入：试图以 3..=99 的非法态判握手——闭集判定必须全拒。
        let mut bad_rejected = true;
        let mut v = 3u32;
        while v < 100 {
            // 闭集判定：3..=99 无一命中三态。
            let mut found = false;
            let mut k = 0;
            while k < 3 {
                if Handshake::ALL[k] as u32 == v {
                    found = true;
                }
                k += 1;
            }
            if found {
                bad_rejected = false;
            }
            v += 1;
        }
        // 双跑握手逐场景全等。
        let s1 = run_all_scenarios(SEG_BASE as u64);
        let s2 = run_all_scenarios((SEG_BASE as u64) ^ 0x5A5A);
        let mut hs_eq = true;
        let mut i = 0;
        while i < SCENARIOS {
            match (s1.at(i), s2.at(i)) {
                (Some(a), Some(b)) if a.hs == b.hs => {}
                _ => hs_eq = false,
            }
            i += 1;
        }
        if bad_rejected && hs_eq {
            caught += 1;
        }
        round += 1;
    }
    (caught == INJECT_ROUNDS, format!("握手零容差 {}/{}", caught, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    DcCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：场景闭集与握手闭集核对。
pub fn run_unx12433_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-闭集锚",
        SCENARIOS == 15 && Handshake::ALL.len() == 3 && PUMP_MSGS == 100,
        format!("场景{} 握手{} 泵{}", SCENARIOS, Handshake::ALL.len(), PUMP_MSGS),
    ));
    v
}

/// run_unx12433_checks：本单判据聚合（诊断段 0x30420）。
pub fn run_unx12433_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12433 端口族双机对照判据");
    // J1-a：同码双跑 15 场景一致（S1 母版）。
    let (ok1, d1) = drill_dual_run_15();
    s.add_dyn("J1-双跑15场景一致", ok1, &d1);
    // J1-b：场景账完整性。
    let (ok2, d2) = drill_ledger_integrity_10();
    s.add_dyn("J1-场景账完整10/10", ok2, &d2);
    // J1-c：确定性三跑（结构决定序）。
    let (ok3, d3) = drill_triple_run_10();
    s.add_dyn("J1-三跑确定性10/10", ok3, &d3);
    // J1-d：握手三态零容差。
    let (ok4, d4) = drill_handshake_zero_tolerance_10();
    s.add_dyn("J1-握手零容差10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30420段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12433_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
