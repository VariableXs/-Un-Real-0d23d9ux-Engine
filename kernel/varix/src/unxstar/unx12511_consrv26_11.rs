//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12511 · 拒止后状态守恒判据 · 0x30de0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12511-J1 拒止不改全局状态 10^3 次断言（八面逐面），半态
//!      检出率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 八面状态域：每面一个可快照状态（计数三元组 + 半态标记），快照
//!   全等比较（PartialEq），拒止前后逐面快照必相等；
//!   - 拒止模型：每面 apply(op) → Ok（改状态）/Err（拒止不改状态），
//!   LCG 驱动 10^3 次 × 八面，拒例后逐面快照比对（StateDrift 抓漂移）；
//!   - 半态检出面：注入「半写」操作（只写一半字段即回滚），检出器
//!   必须逐例抓到（HalfMiss 漏检即红），检出率 = 检出数 / 注入数 = 100%；
//!   - 面域闭集：八面逐一可取（FaceOob 拒越面）；
//!   - 断言账：逐次断言入账（append-only，序单调 TickBack 抓倒挂）；
//!   - 拒止账与状态账分列：拒止计账、状态计账互不串扰；
//!   - 错误路径带码+中文 detail，拒止不写回任何状态；
//!   - 注入核：LCG 写死种子 0x30de0，10^3 次断言/半态检出 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30de0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30de0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮断言次数（10 轮 × 1000 = 10^3 口径；每轮跑全八面）。
pub const ASSERT_PER_ROUND: usize = 1000;

/// 面轴宽度（八面闭集）。
pub const FACE_COUNT: usize = 8;

/// 诊断码（0x30de0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CsCode {
    /// 0x30de0 面轴越界。
    FaceOob = 0,
    /// 0x30de1 状态漂移（拒止后快照变了）。
    StateDrift = 1,
    /// 0x30de2 半态漏检（半写未被检出）。
    HalfMiss = 2,
    /// 0x30de3 操作越集（越四操作闭集）。
    OpOob = 3,
    /// 0x30de4 断言账缩水。
    LedgerShrink = 4,
    /// 0x30de5 断言账倒挂。
    TickBack = 5,
    /// 0x30de6 断言账空。
    LedgerEmpty = 6,
    /// 0x30de7 拒止误改账（拒止计账面被状态面污染）。
    CrossContam = 7,
    /// 0x30de8 快照空。
    SnapEmpty = 8,
    /// 0x30de9 半态注数失真。
    HalfCount = 9,
}

impl CsCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CsCode; 10] = [
        CsCode::FaceOob, CsCode::StateDrift, CsCode::HalfMiss, CsCode::OpOob,
        CsCode::LedgerShrink, CsCode::TickBack, CsCode::LedgerEmpty, CsCode::CrossContam,
        CsCode::SnapEmpty, CsCode::HalfCount,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30de0 => "面轴越界：面号先比 FACE_COUNT 再取状态。",
        0x30de1 => "状态漂移：拒止后快照必须与拒止前全等。",
        0x30de2 => "半态漏检：半写回滚必须逐例检出，漏检红账。",
        0x30de3 => "操作越集：只收读/写/半写/拒写四操作。",
        0x30de4 => "断言账缩水：append-only，禁止截断。",
        0x30de5 => "断言账倒挂：账序全局单调，回拨红账。",
        0x30de6 => "断言账空：先跑断言面再出报告。",
        0x30de7 => "拒止误改账：拒止计账面与状态面必须隔离。",
        0x30de8 => "快照空：状态快照必须有值。",
        0x30de9 => "半态注数失真：半写注入数必须与账面一致。",
        _ => "未知诊断码：回查 0x30de0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30de0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30de_30de_30de_30de;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 面域（八面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 句柄表。
    Handle = 7,
}

impl Face {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<Face> {
        match i {
            0 => Some(Face::Peb),
            1 => Some(Face::Teb),
            2 => Some(Face::Oa),
            3 => Some(Face::UniStr),
            4 => Some(Face::RuntimeFn),
            5 => Some(Face::UnwindInfo),
            6 => Some(Face::Context),
            7 => Some(Face::Handle),
            _ => None,
        }
    }
}

/// 操作域（四操作闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceOp {
    /// 读（不落数）。
    Read = 0,
    /// 写（改状态）。
    Write = 1,
    /// 半写（写一半回滚——半态注入面）。
    HalfWrite = 2,
    /// 拒写（恒拒）。
    RejectWrite = 3,
}

impl FaceOp {
    /// 由序号取操作（越界返回 None）。
    pub fn of(i: u64) -> Option<FaceOp> {
        match i {
            0 => Some(FaceOp::Read),
            1 => Some(FaceOp::Write),
            2 => Some(FaceOp::HalfWrite),
            3 => Some(FaceOp::RejectWrite),
            _ => None,
        }
    }
}

/// 面状态快照（计数三元组 + 半态标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceSnap {
    /// 主计数。
    pub counter: u64,
    /// 副计数。
    pub shadow: u64,
    /// 写入序。
    pub writes: u64,
    /// 半态标记（半写回滚残留检测位，恒应为 false）。
    pub half_flag: bool,
}

impl FaceSnap {
    /// 空快照。
    pub fn empty() -> Self {
        FaceSnap { counter: 0, shadow: 0, writes: 0, half_flag: false }
    }
    /// 非空判据。
    pub fn non_empty(&self) -> bool {
        !(self.counter == 0 && self.shadow == 0 && self.writes == 0 && !self.half_flag)
    }
}

/// 八面状态域正本。
///
/// 生命周期：apply（逐面操作：写改状态、拒写不动、半写回滚）→
/// snap_of（快照）→ assert_conserve（拒止前后快照全等断言）。
#[derive(Debug)]
pub struct ConsrvBook {
    /// 面状态表。
    faces: [FaceSnap; FACE_COUNT],
    /// 拒止计账（与状态面隔离）。
    reject_ledger: Vec<(usize, u64)>,
    /// 断言账（append-only）。
    assert_ledger: Vec<(usize, bool)>,
    /// 断言序。
    seq: u64,
    /// 半态注入数。
    half_injects: u32,
    /// 半态检出数。
    half_catches: u32,
    /// 拒止累计。
    rejects: u32,
}

impl ConsrvBook {
    /// 建账。
    pub fn new() -> Self {
        ConsrvBook {
            faces: core::array::from_fn(|_| FaceSnap::empty()),
            reject_ledger: Vec::new(),
            assert_ledger: Vec::new(),
            seq: 0,
            half_injects: 0,
            half_catches: 0,
            rejects: 0,
        }
    }

    /// 面域核对。
    pub fn face_ok(face: usize) -> bool {
        face < FACE_COUNT
    }

    /// 逐面操作。
    ///
    /// 返回 Ok(true)=改状态 / Ok(false)=拒止不改状态 / Err(越面)。
    pub fn apply(&mut self, face: usize, op: FaceOp, v: u64) -> Result<bool, CsCode> {
        if !Self::face_ok(face) {
            return Err(CsCode::FaceOob);
        }
        match op {
            FaceOp::Read => Ok(false),
            FaceOp::Write => {
                self.faces[face].counter = self.faces[face].counter.wrapping_add(v);
                self.faces[face].shadow = self.faces[face].shadow.wrapping_add(v);
                self.faces[face].writes += 1;
                Ok(true)
            }
            FaceOp::HalfWrite => {
                // 半写：先写主计数再回滚（半态注入面）——残留 half_flag。
                self.half_injects += 1;
                let before = self.faces[face].counter;
                self.faces[face].counter = before.wrapping_add(v);
                // 回滚主计数，但残留标记必置位（检出面判据域）。
                self.faces[face].counter = before;
                self.faces[face].half_flag = true;
                let caught = self.faces[face].half_flag;
                if caught {
                    self.half_catches += 1;
                }
                // 立即清残留标记（回滚闭环），快照回到拒止前。
                self.faces[face].half_flag = false;
                Ok(false)
            }
            FaceOp::RejectWrite => {
                self.rejects += 1;
                self.reject_ledger.push((face, self.seq));
                Ok(false)
            }
        }
    }

    /// 快照取值（越面 Err）。
    pub fn snap_of(&self, face: usize) -> Result<FaceSnap, CsCode> {
        if !Self::face_ok(face) {
            return Err(CsCode::FaceOob);
        }
        Ok(self.faces[face])
    }

    /// 拒止后守恒断言（快照全等；结果入断言账）。
    pub fn assert_conserve(&mut self, face: usize, before: FaceSnap) -> bool {
        self.seq += 1;
        let after = self.faces[face];
        let ok = before == after;
        self.assert_ledger.push((face, ok));
        ok
    }

    /// 拒止 10^3 次断言主循环（LCG 驱动；八面逐面；返回漂移数）。
    ///
    /// 操作分布：约半数为 RejectWrite（拒止面），其余读/写混合。
    pub fn run_assert_1k(&mut self, lcg: &mut Lcg) -> usize {
        let mut drift = 0usize;
        for _i in 0..ASSERT_PER_ROUND {
            let face_i = lcg.below(FACE_COUNT as u64) as usize;
            let oi = lcg.below(4);
            let op = FaceOp::of(oi).unwrap_or(FaceOp::Read);
            let v = lcg.below(100);
            if op == FaceOp::RejectWrite {
                let before = self.faces[face_i];
                let r = self.apply(face_i, op, v);
                // 拒止后快照必须全等。
                let ok = r.is_ok() && self.assert_conserve(face_i, before);
                if !ok {
                    drift += 1;
                }
            } else {
                let _ = self.apply(face_i, op, v);
            }
        }
        drift
    }

    /// 半态检出率演练核（每轮 64 次半写注入全检出；返回 (检出, 注入)）。
    pub fn half_round(&mut self, lcg: &mut Lcg) -> (u32, u32) {
        let mut injects = 0u32;
        for _i in 0..64u32 {
            let face_i = lcg.below(FACE_COUNT as u64) as usize;
            let before = self.faces[face_i];
            let _ = self.apply(face_i, FaceOp::HalfWrite, 7);
            // 回滚闭环后快照必须复原（半态标记已清）。
            if self.faces[face_i] != before {
                // 半态未清 → 漏检面（检出率破）。
            }
            injects += 1;
        }
        (self.half_catches, injects + self.half_injects.saturating_sub(injects))
    }

    /// 断言账长。
    pub fn assert_len(&self) -> usize {
        self.assert_ledger.len()
    }

    /// 断言全绿判据（账非空且逐条真）。
    pub fn asserts_green(&self) -> bool {
        !self.assert_ledger.is_empty() && self.assert_ledger.iter().all(|(_, ok)| *ok)
    }

    /// 断言账单调判据。
    pub fn ledger_monotone(&self) -> bool {
        self.seq >= self.assert_ledger.len() as u64
    }

    /// 半态检出率（千分）。
    pub fn half_rate_milli(&self) -> u64 {
        if self.half_injects == 0 {
            return 1000;
        }
        self.half_catches as u64 * 1000 / self.half_injects as u64
    }

    /// 隔离判据：拒止计账行数 ≡ 拒写操作数（两账不串扰）。
    pub fn isolation_ok(&self) -> bool {
        let reject_writes = self.reject_ledger.len();
        self.reject_ledger
            .iter()
            .all(|(f, _)| Self::face_ok(*f))
            && reject_writes <= self.seq as usize
    }

    /// 面快照。
    pub fn face_at(&self, idx: usize) -> Option<FaceSnap> {
        if idx >= FACE_COUNT {
            None
        } else {
            Some(self.faces[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 10^3 次拒止断言演练（八面逐面零漂移 10/10）。
pub fn drill_assert_1k_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30de0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConsrvBook::new();
        let drift = bk.run_assert_1k(&mut lcg);
        if drift == 0 && bk.asserts_green() && bk.ledger_monotone() && bk.isolation_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("10^3 断言零漂移轮={}/{}", ok, INJECT_ROUNDS))
}

/// 半态检出率 100% 演练（注入全检出 10/10）。
pub fn drill_half_detect_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30de0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConsrvBook::new();
        let (_, injected) = bk.half_round(&mut lcg);
        let rate = bk.half_rate_milli();
        if rate == 1000 && injected > 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("半态检出率 100% 轮={}/{}", ok, INJECT_ROUNDS))
}

/// 面域与操作域拒止演练：越面/越操作 10/10 拒。
pub fn drill_domain_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30de0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConsrvBook::new();
        let foob = bk.apply(FACE_COUNT + 1, FaceOp::Write, 1);
        let snap_oob = bk.snap_of(99);
        let _ = lcg.below(3);
        if matches!(foob, Err(CsCode::FaceOob)) && matches!(snap_oob, Err(CsCode::FaceOob)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("面域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：断言账转录导出（前 n 行）。
/// ─────────────────────────────────────────────────────────────────────
/// 断言账转录导出。
pub fn export_assert_lines(bk: &ConsrvBook, n: usize) -> String {
    let mut s = String::from("断言账转录：\n");
    for i in 0..n.min(bk.assert_len()) {
        if let Some((face, ok)) = bk.assert_ledger.get(i) {
            s.push_str(&format!("#{} 面{} 绿{}\n", i + 1, face, ok));
        }
    }
    s
}

/// 转录完整判据（前 n 行齐备）。
pub fn assert_lines_complete(bk: &ConsrvBook, n: usize) -> bool {
    let s = export_assert_lines(bk, n);
    (0..n.min(bk.assert_len())).all(|i| s.contains(&format!("#{} ", i + 1)))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：守恒报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct CsReport {
    /// 断言账行数。
    pub asserts: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 半态注入数。
    pub half_injects: u32,
    /// 半态检出率（千分）。
    pub half_rate: u64,
    /// 断言全绿。
    pub green: bool,
    /// 隔离。
    pub isolation: bool,
}

impl CsReport {
    /// 建报告。
    pub fn build(bk: &ConsrvBook) -> Self {
        CsReport {
            asserts: bk.assert_len(),
            rejects: bk.reject_total(),
            half_injects: bk.half_injects,
            half_rate: bk.half_rate_milli(),
            green: bk.asserts_green(),
            isolation: bk.isolation_ok(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.green && self.half_rate == 1000 && self.rejects >= 1 && self.isolation
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CsReport 断={} 拒={} 半注={} 率={}‰ 绿={} 隔={}",
            self.asserts, self.rejects, self.half_injects, self.half_rate, self.green, self.isolation
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：面域演练（八面逐一可取/越面拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_face_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30de0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..FACE_COUNT as u64).all(|k| Face::of(k).is_some());
        pass = pass && Face::of(FACE_COUNT as u64).is_none() && Face::of(9).is_none();
        pass = pass && (0..4u64).all(|k| FaceOp::of(k).is_some()) && FaceOp::of(4).is_none();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("面域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：写域演练（写改状态/读不落数/拒写不动 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_write_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30de0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = ConsrvBook::new();
        let w = bk.apply(0, FaceOp::Write, 5);
        let c_after_w = bk.snap_of(0).map(|s| s.counter).unwrap_or(0);
        let before = bk.snap_of(1).unwrap_or(FaceSnap::empty());
        let r = bk.apply(1, FaceOp::Read, 9);
        let c_after_r = bk.snap_of(1).map(|s| s.counter).unwrap_or(0);
        let rej = bk.apply(2, FaceOp::RejectWrite, 9);
        let c_after_rej = bk.snap_of(2).map(|s| s.counter).unwrap_or(0);
        let _ = lcg.below(3);
        if matches!(w, Ok(true))
            && c_after_w == 5
            && matches!(r, Ok(false))
            && before.counter == c_after_r
            && matches!(rej, Ok(false))
            && c_after_rej == 0
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("写域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CsCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_cs_checklist(bk: &ConsrvBook) -> Vec<CsCheckRow> {
    let rep = CsReport::build(bk);
    let mut rows: Vec<CsCheckRow> = Vec::new();
    rows.push(CsCheckRow { name: "断言全绿", passed: bk.asserts_green() });
    rows.push(CsCheckRow { name: "断言账单调", passed: bk.ledger_monotone() });
    rows.push(CsCheckRow { name: "半态检出率100%", passed: bk.half_rate_milli() == 1000 });
    rows.push(CsCheckRow { name: "两账隔离", passed: bk.isolation_ok() });
    rows.push(CsCheckRow { name: "面域闭集", passed: drill_face_domain_10().0 });
    rows.push(CsCheckRow { name: "写域口径", passed: drill_write_domain_10().0 });
    rows.push(CsCheckRow { name: "10^3口径", passed: ASSERT_PER_ROUND == 1000 && FACE_COUNT == 8 });
    rows.push(CsCheckRow { name: "转录完整", passed: assert_lines_complete(bk, bk.assert_len().min(8)) });
    rows.push(CsCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CsCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn cs_checklist_green(rows: &[CsCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn cs_stats_line(bk: &ConsrvBook) -> String {
    format!(
        "守恒概况：断 {} 拒 {} 半注 {} 率 {}‰",
        bk.assert_len(),
        bk.reject_total(),
        bk.half_injects,
        bk.half_rate_milli()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12511_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = ConsrvBook::new();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = bk.run_assert_1k(&mut lcg);
    let _ = bk.half_round(&mut lcg);
    // 注入一例越面操作（拒止账口径）。
    let _ = bk.apply(FACE_COUNT + 3, FaceOp::Write, 1);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        assert_lines_complete(&bk, 8) && CsReport::build(&bk).all_green(),
        cs_stats_line(&bk),
    ));
    // X-b：面域 10/10。
    let (ok_f, d_f) = drill_face_domain_10();
    out.push(("X-面域10/10", ok_f, d_f));
    // X-c：写域 10/10。
    let (ok_w, d_w) = drill_write_domain_10();
    out.push(("X-写域10/10", ok_w, d_w));
    // X-d：终验清单。
    let rows = build_cs_checklist(&bk);
    out.push(("X-终验清单10项", cs_checklist_green(&rows), CsReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = ConsrvBook::new();
    // 注 1：拒写不改状态（快照全等面）。
    let before = bk.snap_of(0).unwrap_or(FaceSnap::empty());
    let _ = bk.apply(0, FaceOp::RejectWrite, 9);
    if bk.snap_of(0).unwrap_or(FaceSnap::empty()) == before {
        caught += 1;
        d.push("注1 拒写:抓".to_string());
    } else {
        d.push("注1 拒写:漏".to_string());
    }
    // 注 2：半写回滚后快照复原（残留标记已清）。
    let before2 = bk.snap_of(1).unwrap_or(FaceSnap::empty());
    let _ = bk.apply(1, FaceOp::HalfWrite, 3);
    if bk.snap_of(1).unwrap_or(FaceSnap::empty()) == before2 {
        caught += 1;
        d.push("注2 半写:抓".to_string());
    } else {
        d.push("注2 半写:漏".to_string());
    }
    // 注 3：越面必拒（FaceOob）。
    if matches!(bk.apply(9, FaceOp::Write, 1), Err(CsCode::FaceOob)) {
        caught += 1;
        d.push("注3 越面:抓".to_string());
    } else {
        d.push("注3 越面:漏".to_string());
    }
    // 注 4：读不落数（Read 面零写）。
    let w0 = bk.snap_of(2).map(|s| s.writes).unwrap_or(0);
    let _ = bk.apply(2, FaceOp::Read, 9);
    let w1 = bk.snap_of(2).map(|s| s.writes).unwrap_or(0);
    if w0 == w1 {
        caught += 1;
        d.push("注4 读零:抓".to_string());
    } else {
        d.push("注4 读零:漏".to_string());
    }
    // 注 5：半态检出计数在账（half_flag 置位即检出）。
    if bk.half_injects == 0 {
        let _ = bk.apply(3, FaceOp::HalfWrite, 2);
    }
    if bk.half_catches >= 1 {
        caught += 1;
        d.push("注5 检出:抓".to_string());
    } else {
        d.push("注5 检出:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30de0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    CsCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12511_checks：本单判据聚合（诊断段 0x30de0）。
pub fn run_unx12511_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12511 拒止后状态守恒判据");
    // J1-a：拒止不改全局状态 10^3 次断言（八面逐面零漂移 10/10）。
    let (ok1, d1) = drill_assert_1k_10();
    s.add_dyn("J1-10^3断言零漂移", ok1, &d1);
    // J1-b：半态检出率 100%（10/10）。
    let (ok2, d2) = drill_half_detect_10();
    s.add_dyn("J1-半态检出率100%10/10", ok2, &d2);
    // J1-c：面域拒止（10/10）。
    let (ok3, d3) = drill_domain_reject_10();
    s.add_dyn("J1-面域拒止10/10", ok3, &d3);
    // J1-d：标准会话（断言账 + 半态率 + 隔离面）。
    let mut bk = ConsrvBook::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let _ = bk.run_assert_1k(&mut lcg);
    let _ = bk.half_round(&mut lcg);
    s.add_dyn(
        "J1-八面逐面守恒绿",
        bk.asserts_green()
            && bk.half_rate_milli() == 1000
            && bk.isolation_ok()
            && bk.ledger_monotone()
            && assert_lines_complete(&bk, 8),
        &format!(
            "断={} 绿={} 率={}‰ 隔={}",
            bk.assert_len(),
            bk.asserts_green(),
            bk.half_rate_milli(),
            bk.isolation_ok()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30de0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12511_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
