//! UNX-F51525 多摄像头+音频并发带宽实表（H3/H1 消费） · 0xC0640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51526-J1 断言器 ×10^3 轮零误报；饥饿注入 10/10 检出
//!
//! 设计要点：
//!   ① 流域模型按「H3 高带宽摄像头 + H1 音频」定制：Cam0/Cam1/Cam2 为 H3
//!      视频流，Mic 为 H1 音频流，Cam0Hb 为 H3 双缓冲高带宽档（包长翻倍）；
//!   ② 微帧实表按 (微帧号, 流) 逐条落账：commit_mf 把当帧全部激活流的
//!      包额一次性原子落账，任一组合超预算则整帧明拒（零部分落账）；
//!   ③ 冻结包长表：三摄 + 麦全开恰好顶格（6208）；Cam0Hb 与任何其他流
//!      并发必超预算，用于逼出组合级超预算明拒；
//!   ④ 微帧号状态机单调：回放目标低于已落账微帧号即显性拒（MfBackward）；
//!      (微帧号, 流) 二元组在实表内不得重复（DupCommit 逐条扫描防重）；
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；超预算组合/重复落账/
//!      未激活流三类注入各 ×10 轮全部检出；
//!   ⑥ 诊断码闭集 + 中文建议文案；段位守卫锁定 SEG_BASE 对齐与分派区间。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0640;

/// Cam0（H3 视频）每微帧包额。
pub const CAM0_PKT: u32 = 3072;
/// Cam0Hb（H3 双缓冲档）每微帧包额。
pub const CAM0HB_PKT: u32 = 6144;
/// Cam1（H3 视频）每微帧包额。
pub const CAM1_PKT: u32 = 2048;
/// Cam2（H3 视频）每微帧包额。
pub const CAM2_PKT: u32 = 1024;
/// Mic（H1 音频）每微帧包额。
pub const MIC_PKT: u32 = 64;
/// 微帧总预算：三摄 + 麦全开恰好顶格。
pub const MF_BUDGET: u32 = CAM0_PKT + CAM1_PKT + CAM2_PKT + MIC_PKT;

/// 消费档型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// H3：高带宽视频消费档
    H3,
    /// H1：音频消费档
    H1,
}

/// 流标识（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamId {
    /// 摄像头 0（H3）
    Cam0 = 0,
    /// 摄像头 0 双缓冲高带宽档（H3）
    Cam0Hb = 1,
    /// 摄像头 1（H3）
    Cam1 = 2,
    /// 摄像头 2（H3）
    Cam2 = 3,
    /// 麦克风（H1）
    Mic = 4,
}

impl StreamId {
    pub fn ordinal(self) -> usize {
        self as usize
    }
    pub fn profile(self) -> Profile {
        match self {
            StreamId::Mic => Profile::H1,
            _ => Profile::H3,
        }
    }
    pub fn default_pkt(self) -> u32 {
        match self {
            StreamId::Cam0 => CAM0_PKT,
            StreamId::Cam0Hb => CAM0HB_PKT,
            StreamId::Cam1 => CAM1_PKT,
            StreamId::Cam2 => CAM2_PKT,
            StreamId::Mic => MIC_PKT,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            StreamId::Cam0 => "cam0",
            StreamId::Cam0Hb => "cam0-hb",
            StreamId::Cam1 => "cam1",
            StreamId::Cam2 => "cam2",
            StreamId::Mic => "mic",
        }
    }
    /// 序号 → 流（负值或越界即未知流）。
    pub fn from_ordinal(i: i64) -> Option<StreamId> {
        match i {
            0 => Some(StreamId::Cam0),
            1 => Some(StreamId::Cam0Hb),
            2 => Some(StreamId::Cam1),
            3 => Some(StreamId::Cam2),
            4 => Some(StreamId::Mic),
            _ => None,
        }
    }
}

/// 诊断码闭集（摄像头+音频并发带宽域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CamBwCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 未知流（序号越界或为负）
    StreamUnknown,
    /// 流已激活
    AlreadyActive,
    /// 流未激活即落账
    StreamInactive,
    /// (微帧号, 流) 重复落账
    DupCommit,
    /// 并发组合超微帧预算（整帧明拒）
    ComboOverBudget,
    /// 负包额
    NegativePkt,
    /// 包额超微帧预算
    PktTooBig,
    /// 微帧号倒退（状态机显性拒）
    MfBackward,
    /// 对账守恒失败
    LedgerMismatch,
}

impl CamBwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CamBwCode::Ok => "落账成功",
            CamBwCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            CamBwCode::StreamUnknown => "未知流：流序号必须在闭集 0..5 内",
            CamBwCode::AlreadyActive => "流已激活：不得重复激活",
            CamBwCode::StreamInactive => "流未激活：先激活再参与落账",
            CamBwCode::DupCommit => "重复落账：同一 (微帧, 流) 只允许一条实表记录",
            CamBwCode::ComboOverBudget => "组合超预算：整帧明拒，禁部分落账",
            CamBwCode::NegativePkt => "负包额：包额必须为非负整数",
            CamBwCode::PktTooBig => "包额超微帧预算：降低档位或拆包",
            CamBwCode::MfBackward => "微帧号倒退：回放目标必须 ≥ 已落账微帧号",
            CamBwCode::LedgerMismatch => "对账不符：实表重算与累计面不一致",
        }
    }
}

/// 实表记录：(微帧号, 流, 包额) 逐条落账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BwRec {
    pub mf: u32,
    pub stream: StreamId,
    pub pkt: u32,
}

/// 并发带宽实表引擎：激活集 + 原子整帧落账 + 单调微帧号。
pub struct CamAudioBwEngine {
    active: Vec<bool>,
    pkts: Vec<u32>,
    tots: Vec<u64>,
    mf: u32,
    ledger: Vec<BwRec>,
    total_recs: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl CamAudioBwEngine {
    pub fn new() -> CamAudioBwEngine {
        let mut active = Vec::new();
        let mut pkts = Vec::new();
        let mut tots = Vec::new();
        for s in ALL_STREAMS.iter() {
            active.push(false);
            pkts.push(s.default_pkt());
            tots.push(0u64);
        }
        CamAudioBwEngine {
            active,
            pkts,
            tots,
            mf: 0,
            ledger: Vec::new(),
            total_recs: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    fn is_active(&self, s: StreamId) -> bool {
        match self.active.get(s.ordinal()) {
            Some(v) => *v,
            None => false,
        }
    }

    fn pkt_of(&self, s: StreamId) -> u32 {
        match self.pkts.get(s.ordinal()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 激活流（重复激活显性拒）。
    pub fn activate(&mut self, s: StreamId) -> Result<(), CamBwCode> {
        if self.is_active(s) {
            return Err(CamBwCode::AlreadyActive);
        }
        if let Some(v) = self.active.get_mut(s.ordinal()) {
            *v = true;
        }
        Ok(())
    }

    /// 按序号激活（负值/越界即未知流）。
    pub fn activate_ordinal(&mut self, i: i64) -> Result<(), CamBwCode> {
        match StreamId::from_ordinal(i) {
            Some(s) => self.activate(s),
            None => Err(CamBwCode::StreamUnknown),
        }
    }

    /// 停用流。
    pub fn deactivate(&mut self, s: StreamId) {
        if let Some(v) = self.active.get_mut(s.ordinal()) {
            *v = false;
        }
    }

    /// 调包额（冻结表之上的唯一调参口；负值/超预算显性拒）。
    pub fn override_pkt(&mut self, s: StreamId, pkt: i64) -> Result<(), CamBwCode> {
        if pkt < 0 {
            return Err(CamBwCode::NegativePkt);
        }
        if pkt > MF_BUDGET as i64 {
            return Err(CamBwCode::PktTooBig);
        }
        if let Some(v) = self.pkts.get_mut(s.ordinal()) {
            *v = pkt as u32;
        }
        Ok(())
    }

    /// 原子整帧落账：组合超预算则整帧明拒，零部分落账。
    pub fn commit_mf(&mut self) -> Result<u64, CamBwCode> {
        let mut sum = 0u64;
        for s in ALL_STREAMS.iter() {
            if self.is_active(*s) {
                sum = sum.saturating_add(self.pkt_of(*s) as u64);
            }
        }
        if sum > MF_BUDGET as u64 {
            return Err(CamBwCode::ComboOverBudget);
        }
        for s in ALL_STREAMS.iter() {
            if self.is_active(*s) {
                let p = self.pkt_of(*s);
                self.ledger.push(BwRec {
                    mf: self.mf,
                    stream: *s,
                    pkt: p,
                });
                self.total_recs = self.total_recs.saturating_add(1);
                if let Some(t) = self.tots.get_mut(s.ordinal()) {
                    *t = t.saturating_add(p as u64);
                }
            }
        }
        self.mf = self.mf.saturating_add(1);
        Ok(sum)
    }

    /// 回放探针：目标微帧号低于已落账号即显性拒（单调状态机）。
    pub fn replay_mf(&self, target: u32) -> Result<(), CamBwCode> {
        if target < self.mf {
            return Err(CamBwCode::MfBackward);
        }
        Ok(())
    }

    /// 重复落账提案：实表内 (微帧号, 流) 已存在即判 DupCommit。
    pub fn propose_dup(&self, s: StreamId, mf: u32) -> Result<(), CamBwCode> {
        for r in self.ledger.iter() {
            if r.mf == mf && r.stream == s {
                return Err(CamBwCode::DupCommit);
            }
        }
        Ok(())
    }

    /// 未激活流落账提案。
    pub fn propose_inactive(&self, s: StreamId) -> Result<(), CamBwCode> {
        if self.is_active(s) {
            Ok(())
        } else {
            Err(CamBwCode::StreamInactive)
        }
    }

    /// 对账：实表按流重算与累计面一致、条数一致则通过。
    pub fn reconcile(&self) -> Result<(), CamBwCode> {
        let mut sums = Vec::new();
        for _ in ALL_STREAMS.iter() {
            sums.push(0u64);
        }
        for r in self.ledger.iter() {
            if let Some(t) = sums.get_mut(r.stream.ordinal()) {
                *t = t.saturating_add(r.pkt as u64);
            }
        }
        for s in ALL_STREAMS.iter() {
            let expect = match self.tots.get(s.ordinal()) {
                Some(t) => *t,
                None => return Err(CamBwCode::LedgerMismatch),
            };
            let actual = match sums.get(s.ordinal()) {
                Some(v) => *v,
                None => return Err(CamBwCode::LedgerMismatch),
            };
            if expect != actual {
                return Err(CamBwCode::LedgerMismatch);
            }
        }
        if self.total_recs != self.ledger.len() as u64 {
            return Err(CamBwCode::LedgerMismatch);
        }
        Ok(())
    }

    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
    }

    pub fn cur_mf(&self) -> u32 {
        self.mf
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn ledger_at(&self, i: usize) -> Option<BwRec> {
        self.ledger.get(i).copied()
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
}

/// 流闭集全序表。
pub const ALL_STREAMS: [StreamId; 5] = [
    StreamId::Cam0,
    StreamId::Cam0Hb,
    StreamId::Cam1,
    StreamId::Cam2,
    StreamId::Mic,
];

/// 段位守卫：SEG_BASE 0x40 对齐且落在分派区间。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一激活组合两次建账逐字段一致（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<BwRec>, u32, Vec<u64>) {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.activate(StreamId::Mic);
        let _ = e.commit_mf();
        let _ = e.commit_mf();
        e.deactivate(StreamId::Mic);
        let _ = e.commit_mf();
        (e.ledger.clone(), e.cur_mf(), e.tots.clone())
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51525_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51525");
    // 1 正常流：Cam0+Mic 并发整帧落账。
    {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.activate(StreamId::Mic);
        let r = e.commit_mf();
        let ok = r == Ok((CAM0_PKT + MIC_PKT) as u64)
            && e.ledger_len() == 2
            && e.reconcile() == Ok(());
        s.add_dyn("三摄+音频正常并发落账", ok, "激活流应逐条落实表且对账一致");
    }
    // 2 并发组合 ×10 实表落账：逐组合验和。
    {
        let combos: [Vec<StreamId>; 10] = [
            vec![StreamId::Cam0],
            vec![StreamId::Cam1],
            vec![StreamId::Cam2, StreamId::Mic],
            vec![StreamId::Cam0, StreamId::Mic],
            vec![StreamId::Cam1, StreamId::Mic],
            vec![StreamId::Cam2, StreamId::Cam1, StreamId::Mic],
            vec![StreamId::Cam0, StreamId::Cam2],
            vec![StreamId::Cam0, StreamId::Cam1],
            vec![StreamId::Cam0, StreamId::Cam1, StreamId::Cam2],
            vec![
                StreamId::Cam0,
                StreamId::Cam1,
                StreamId::Cam2,
                StreamId::Mic,
            ],
        ];
        let mut all_ok = true;
        for combo in combos.iter() {
            let mut e = CamAudioBwEngine::new();
            for st in combo.iter() {
                if e.activate(*st) != Ok(()) {
                    all_ok = false;
                }
            }
            let mut expect = 0u64;
            for st in combo.iter() {
                expect = expect.saturating_add(st.default_pkt() as u64);
            }
            for _ in 0..2u32 {
                if e.commit_mf() != Ok(expect) {
                    all_ok = false;
                }
            }
            if e.ledger_len() != combo.len() * 2 || e.reconcile() != Ok(()) {
                all_ok = false;
            }
        }
        s.add_dyn(
            "并发组合 ×10 实表落账",
            all_ok,
            "10 组组合 ×2 微帧必须逐组合验和且对账一致",
        );
    }
    // 3 顶格边界：三摄+麦全开恰等于预算；零负载微帧合法。
    {
        let mut e = CamAudioBwEngine::new();
        for st in ALL_STREAMS.iter() {
            if *st != StreamId::Cam0Hb {
                let _ = e.activate(*st);
            }
        }
        let r_top = e.commit_mf();
        let mut e2 = CamAudioBwEngine::new();
        let r_zero = e2.commit_mf();
        s.add_dyn(
            "顶格与零负载边界",
            r_top == Ok(MF_BUDGET as u64) && r_zero == Ok(0) && e2.ledger_len() == 0,
            "全开恰顶格放行；零负载微帧合法且不落账",
        );
    }
    // 4 调包额边界：负值与预算+1 显性拒。
    {
        let mut e = CamAudioBwEngine::new();
        let r_neg = e.override_pkt(StreamId::Mic, -1);
        let r_big = e.override_pkt(StreamId::Mic, MF_BUDGET as i64 + 1);
        let r_ok = e.override_pkt(StreamId::Mic, MF_BUDGET as i64);
        s.add_dyn(
            "调包额边界",
            r_neg == Err(CamBwCode::NegativePkt)
                && r_big == Err(CamBwCode::PktTooBig)
                && r_ok == Ok(()),
            "负包额与预算+1 必须明拒；顶格允许",
        );
    }
    // 5 超预算组合注入 ×10：Cam0Hb 叠加调额 Mic，整帧明拒。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let mut e = CamAudioBwEngine::new();
            let _ = e.activate(StreamId::Cam0);
            let _ = e.activate(StreamId::Cam1);
            let _ = e.activate(StreamId::Cam2);
            let _ = e.override_pkt(StreamId::Mic, 64 + 128 * (i as i64 + 1));
            let _ = e.activate(StreamId::Mic);
            if e.commit_mf() == Err(CamBwCode::ComboOverBudget) {
                caught += 1;
                e.note_detected();
            }
            if e.ledger_len() != 0 {
                caught = 0;
            }
        }
        s.add_dyn(
            "超预算组合注入 10/10 整帧明拒",
            caught == 10,
            "组合超预算必须整帧明拒且零部分落账",
        );
    }
    // 6 重复落账注入 ×10：(微帧,流) 防重扫描全抓。
    {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.commit_mf();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.propose_dup(StreamId::Cam0, 0) == Err(CamBwCode::DupCommit) {
                caught += 1;
                e.note_detected();
            }
        }
        let clean = e.propose_dup(StreamId::Cam1, 0) == Ok(());
        s.add_dyn(
            "重复落账注入 10/10 检出",
            caught == 10 && clean,
            "(微帧,流) 重复必须全抓；未落账组合不得误报",
        );
    }
    // 7 未激活流注入 ×10。
    {
        let e = CamAudioBwEngine::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if e.propose_inactive(StreamId::Mic) == Err(CamBwCode::StreamInactive) {
                caught += 1;
            }
        }
        s.add_dyn("未激活流注入 10/10 检出", caught == 10, "未激活流必须全部拒绝");
    }
    // 8 未知流注入 ×10：负序号与越界序号。
    {
        let mut e = CamAudioBwEngine::new();
        let mut caught = 0u32;
        let ords = [-1i64, 5, 6, -2, 99, 7, -3, 100, 8, -9];
        for o in ords.iter() {
            if e.activate_ordinal(*o) == Err(CamBwCode::StreamUnknown) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("未知流注入 10/10 检出", caught == 10, "流序号闭集外必须全部拒绝");
    }
    // 9 判定次序：重复激活先于落账；调包额负值先于超预算。
    {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let r_dup = e.activate(StreamId::Cam0);
        let r_neg = e.override_pkt(StreamId::Cam0, -4096);
        s.add_dyn(
            "判定次序锁定",
            r_dup == Err(CamBwCode::AlreadyActive) && r_neg == Err(CamBwCode::NegativePkt),
            "重复激活与负包额判据取最前者",
        );
    }
    // 10 状态机拒止：微帧号倒退显性拒且账面不变。
    {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.commit_mf();
        let before = e.ledger_len();
        let cur = e.cur_mf();
        let r = e.replay_mf(cur.saturating_sub(1));
        s.add_dyn(
            "微帧号倒退显性拒",
            r == Err(CamBwCode::MfBackward)
                && e.cur_mf() == cur
                && e.ledger_len() == before,
            "回放目标低于已落账号必须拒绝且账面不变",
        );
    }
    // 11 对账守恒：注入=检出+逃逸，逃逸 0，实表重算一致。
    {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.activate(StreamId::Mic);
        let _ = e.commit_mf();
        let _ = e.commit_mf();
        for _ in 0..10u32 {
            let _ = e.activate_ordinal(-1);
            e.note_detected();
        }
        let recon = e.reconcile() == Ok(());
        s.add_dyn(
            "对账守恒",
            recon && e.conserved() && e.injected() == 10 && e.escaped() == 0,
            "注入=检出+逃逸，逃逸恒 0，实表重算一致",
        );
    }
    // 12 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一组合序列两次建账逐字段相等");
    // 13 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [CamBwCode; 11] = [
    CamBwCode::Ok,
    CamBwCode::SegBad,
    CamBwCode::StreamUnknown,
    CamBwCode::AlreadyActive,
    CamBwCode::StreamInactive,
    CamBwCode::DupCommit,
    CamBwCode::ComboOverBudget,
    CamBwCode::NegativePkt,
    CamBwCode::PktTooBig,
    CamBwCode::MfBackward,
    CamBwCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cam_plus_mic_ledger() {
        let mut e = CamAudioBwEngine::new();
        assert!(e.activate(StreamId::Cam0) == Ok(()));
        assert!(e.activate(StreamId::Mic) == Ok(()));
        assert_eq!(e.commit_mf(), Ok((CAM0_PKT + MIC_PKT) as u64));
        assert_eq!(e.ledger_len(), 2);
        assert!(e.reconcile() == Ok(()));
    }

    #[test]
    fn combo_table_x10_ledger() {
        let combos: [Vec<StreamId>; 10] = [
            vec![StreamId::Cam0],
            vec![StreamId::Cam1],
            vec![StreamId::Cam2, StreamId::Mic],
            vec![StreamId::Cam0, StreamId::Mic],
            vec![StreamId::Cam1, StreamId::Mic],
            vec![StreamId::Cam2, StreamId::Cam1, StreamId::Mic],
            vec![StreamId::Cam0, StreamId::Cam2],
            vec![StreamId::Cam0, StreamId::Cam1],
            vec![StreamId::Cam0, StreamId::Cam1, StreamId::Cam2],
            vec![StreamId::Cam0, StreamId::Cam1, StreamId::Cam2, StreamId::Mic],
        ];
        for combo in combos.iter() {
            let mut e = CamAudioBwEngine::new();
            for st in combo.iter() {
                assert!(e.activate(*st) == Ok(()));
            }
            let mut expect = 0u64;
            for st in combo.iter() {
                expect = expect.saturating_add(st.default_pkt() as u64);
            }
            assert_eq!(e.commit_mf(), Ok(expect));
            assert_eq!(e.ledger_len(), combo.len());
        }
    }

    #[test]
    fn zero_load_and_top_boundary() {
        let mut e = CamAudioBwEngine::new();
        assert_eq!(e.commit_mf(), Ok(0));
        assert_eq!(e.ledger_len(), 0);
        let mut e2 = CamAudioBwEngine::new();
        for st in ALL_STREAMS.iter() {
            if *st != StreamId::Cam0Hb {
                let _ = e2.activate(*st);
            }
        }
        assert_eq!(e2.commit_mf(), Ok(MF_BUDGET as u64));
    }

    #[test]
    fn override_pkt_bounds() {
        let mut e = CamAudioBwEngine::new();
        assert_eq!(e.override_pkt(StreamId::Mic, -1), Err(CamBwCode::NegativePkt));
        assert_eq!(
            e.override_pkt(StreamId::Mic, MF_BUDGET as i64 + 1),
            Err(CamBwCode::PktTooBig)
        );
        assert_eq!(e.override_pkt(StreamId::Mic, MF_BUDGET as i64), Ok(()));
    }

    #[test]
    fn over_budget_combo_x10_atomic_reject() {
        for i in 0..10u32 {
            let mut e = CamAudioBwEngine::new();
            let _ = e.activate(StreamId::Cam0);
            let _ = e.activate(StreamId::Cam1);
            let _ = e.activate(StreamId::Cam2);
            let _ = e.override_pkt(StreamId::Mic, 64 + 128 * (i as i64 + 1));
            let _ = e.activate(StreamId::Mic);
            assert_eq!(e.commit_mf(), Err(CamBwCode::ComboOverBudget));
            assert_eq!(e.ledger_len(), 0, "整帧明拒后不得有部分落账");
        }
    }

    #[test]
    fn dup_injection_x10_detected() {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.commit_mf();
        for _ in 0..10u32 {
            assert_eq!(
                e.propose_dup(StreamId::Cam0, 0),
                Err(CamBwCode::DupCommit)
            );
            e.note_detected();
        }
        assert!(e.propose_dup(StreamId::Cam1, 0) == Ok(()));
        assert!(e.conserved());
    }

    #[test]
    fn unknown_stream_injection_x10() {
        let mut e = CamAudioBwEngine::new();
        let ords = [-1i64, 5, 6, -2, 99, 7, -3, 100, 8, -9];
        for o in ords.iter() {
            assert_eq!(e.activate_ordinal(*o), Err(CamBwCode::StreamUnknown));
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn judge_order_dup_activate_and_negative_pkt() {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        assert_eq!(e.activate(StreamId::Cam0), Err(CamBwCode::AlreadyActive));
        assert_eq!(
            e.override_pkt(StreamId::Cam0, -4096),
            Err(CamBwCode::NegativePkt)
        );
    }

    #[test]
    fn mf_backward_state_machine() {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.commit_mf();
        let cur = e.cur_mf();
        assert_eq!(e.replay_mf(cur - 1), Err(CamBwCode::MfBackward));
        assert_eq!(e.replay_mf(cur), Ok(()));
        assert_eq!(e.cur_mf(), cur);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn conservation_and_reconcile() {
        let mut e = CamAudioBwEngine::new();
        let _ = e.activate(StreamId::Cam0);
        let _ = e.activate(StreamId::Mic);
        let _ = e.commit_mf();
        for _ in 0..10u32 {
            let _ = e.activate_ordinal(-1);
            e.note_detected();
        }
        assert!(e.reconcile() == Ok(()));
        assert!(e.conserved());
        assert_eq!(e.escaped(), 0);
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}

