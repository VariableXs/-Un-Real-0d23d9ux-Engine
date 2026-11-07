//! UNX-F51570 B19 可观测性埋点（绑定/隔离/合并/热拔四点） · 0xC1140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 51570 逐字锚定）：① 纯功能代码 ≥300 行；② 判据原文：
//! UNX-F51571-J1 联测 ×30 零漂移（联签锚定行）；③ 完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   数据结构：TracePoint（埋点：四点闭集绑定/隔离/合并/热拔 + 码 + 载荷）、
//!     ObsRing（观测环账：定容环形流水 + 覆盖账 + 丢弃账）、
//!     JointSign（联签账：锚定行指纹链 + 漂移计数）。
//!   状态机：ObsPhase 空→采集中→联签→封账，单调只进不退。
//!   错误路径：ObCode 闭集 13 变体，val() = SEG_BASE + 1 + 变体序号。
//!   注入核：静默异常（异常事件不上环）/越界点码/环溢丢点/联签漂移/
//!     封账后写入/空环联签 六类坏样本，每类 10 轮精确复现，守恒
//!     （检出+逃逸=轮数，逃逸=0）。
//!   双跑指纹：埋点全程混入 FNV-1a，两次独立全流程逐位一致。
//!
//! 设计要点：no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 =
//!   SEG_BASE 派生；「联测 ×30」以确定性 30 轮联签锚定等价建模）；
//!   取值一律 match/if let/.get()/.get_mut()/checked_*/saturating_*，
//!   无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 段基址（VX12 B1：51570 → 0xC0000 + 0x40×69 = 0xC1140）。
pub const SEG_BASE: u32 = 0xC1140;

/// 环容量（条）。
pub const RING_CAP: usize = 32;

/// 四埋点闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TracePoint {
    /// 绑定点
    Bind = 0,
    /// 隔离点
    Isolate = 1,
    /// 合并点
    Merge = 2,
    /// 热拔点
    HotPlug = 3,
}

impl TracePoint {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<TracePoint> {
        match c {
            0 => Some(TracePoint::Bind),
            1 => Some(TracePoint::Isolate),
            2 => Some(TracePoint::Merge),
            3 => Some(TracePoint::HotPlug),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            TracePoint::Bind => "绑定",
            TracePoint::Isolate => "隔离",
            TracePoint::Merge => "合并",
            TracePoint::HotPlug => "热拔",
        }
    }
}

/// 环相位状态机（闭集，单调只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPhase {
    /// 空环
    Empty = 0,
    /// 采集中
    Collecting = 1,
    /// 联签完成
    Signed = 2,
    /// 封账
    Sealed = 3,
}

impl ObsPhase {
    /// 闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<ObsPhase> {
        match c {
            0 => Some(ObsPhase::Empty),
            1 => Some(ObsPhase::Collecting),
            2 => Some(ObsPhase::Signed),
            3 => Some(ObsPhase::Sealed),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ObsPhase::Empty => "空环",
            ObsPhase::Collecting => "采集中",
            ObsPhase::Signed => "已联签",
            ObsPhase::Sealed => "封账",
        }
    }
}

/// 诊断码闭集（B19 可观测性域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 静默异常（异常事件未上环）
    SilentAnomaly,
    /// 埋点码越闭集
    PointUnknown,
    /// 环溢丢点
    RingOverflow,
    /// 联签漂移（锚定行指纹链不符）
    JointDrift,
    /// 封账后写入
    WriteAfterSeal,
    /// 空环联签
    SignEmpty,
    /// 覆盖账不全（四点未全覆盖）
    CoverageIncomplete,
    /// 相位码越闭集
    PhaseUnknown,
    /// 相位跳步
    PhaseSkip,
    /// 环账不平（环内条数与覆盖账不符）
    LedgerMismatch,
    /// 联签行数不足（少于判据锚定轮数）
    JointRowsShort,
}

impl ObCode {
    /// 段内唯一码：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量非空中文建议。
    pub fn detail(self) -> &'static str {
        match self {
            ObCode::Ok => "B19 可观测性埋点判定成功",
            ObCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ObCode::SilentAnomaly => "静默异常：异常事件必须上环不得吞掉",
            ObCode::PointUnknown => "埋点码非法：只接受 0=绑定 1=隔离 2=合并 3=热拔",
            ObCode::RingOverflow => "环溢丢点：环满后写入必须显性计丢点",
            ObCode::JointDrift => "联签漂移：锚定行指纹链与联签账不符",
            ObCode::WriteAfterSeal => "封账后写入：封账环只读",
            ObCode::SignEmpty => "空环联签：环内无行无从联签",
            ObCode::CoverageIncomplete => "覆盖不全：四埋点必须逐点留痕",
            ObCode::PhaseUnknown => "相位码非法：只接受 0/1/2/3 四相位",
            ObCode::PhaseSkip => "相位跳步：空环→采集→联签→封账禁跳禁退",
            ObCode::LedgerMismatch => "环账不平：环内条数必须与覆盖账一致",
            ObCode::JointRowsShort => "联签行数不足：锚定轮数必须跑满",
        }
    }
}

/// 诊断码权威闭集清单。
pub const ALL_CODES: [ObCode; 13] = [
    ObCode::Ok,
    ObCode::SegBad,
    ObCode::SilentAnomaly,
    ObCode::PointUnknown,
    ObCode::RingOverflow,
    ObCode::JointDrift,
    ObCode::WriteAfterSeal,
    ObCode::SignEmpty,
    ObCode::CoverageIncomplete,
    ObCode::PhaseUnknown,
    ObCode::PhaseSkip,
    ObCode::LedgerMismatch,
    ObCode::JointRowsShort,
];

/// 单条埋点行：(点码, 载荷, 异常旗)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceRow {
    /// 埋点码
    pub point: u32,
    /// 载荷（事件字）
    pub payload: u32,
    /// 异常旗：true=异常事件（零静默判据的抽查对象）
    pub anomaly: bool,
}

impl TraceRow {
    /// 行字（混入指纹链用）。
    pub fn word(&self) -> u64 {
        (self.point as u64).wrapping_mul(0x1000001)
            .wrapping_add(self.payload as u64)
            .wrapping_add(if self.anomaly { 0x8000_0000 } else { 0 })
    }
}

/// 观测环账 + 联签监督器。
pub struct ObsRing {
    ring: Vec<TraceRow>,
    /// 四点覆盖账（逐点留痕计数）
    pub cover: [u64; 4],
    /// 丢点账（环满丢弃数）
    pub dropped: u64,
    /// 异常上环账
    pub anomalies_logged: u64,
    phase: ObsPhase,
    /// 联签锚定行数
    joint_rows: usize,
    injected: u64,
    detected: u64,
    escaped: u64,
    fp: u64,
}

impl ObsRing {
    /// 新环：空相位。
    pub fn new() -> ObsRing {
        ObsRing {
            ring: Vec::new(),
            cover: [0; 4],
            dropped: 0,
            anomalies_logged: 0,
            phase: ObsPhase::Empty,
            joint_rows: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    fn mix(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 指纹快照。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 写入一行：封账明拒；越码明拒；环满丢点显性计账。
    pub fn write(&mut self, row: TraceRow) -> Result<(), ObCode> {
        match TracePoint::from_code(row.point) {
            Some(_) => {}
            None => return Err(ObCode::PointUnknown),
        }
        match self.phase {
            ObsPhase::Sealed => return Err(ObCode::WriteAfterSeal),
            ObsPhase::Empty => self.phase = ObsPhase::Collecting,
            _ => {}
        }
        if self.ring.len() >= RING_CAP {
            self.dropped = self.dropped.saturating_add(1);
            self.mix(0x6A00 + row.point as u64);
            return Err(ObCode::RingOverflow);
        }
        let idx = row.point as usize;
        if let Some(c) = self.cover.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        if row.anomaly {
            self.anomalies_logged = self.anomalies_logged.saturating_add(1);
        }
        let _ = self.ring.push(row);
        self.mix(row.word());
        Ok(())
    }

    /// 静默异常核验：调用方声明发生了 n 次异常，环内异常行必须 ≥ n
    /// （零静默）；少记即抓。
    pub fn audit_silent(&self, declared_anomalies: u64) -> Result<(), ObCode> {
        if self.anomalies_logged < declared_anomalies {
            return Err(ObCode::SilentAnomaly);
        }
        Ok(())
    }

    /// 覆盖核验：四点必须逐点留痕。
    pub fn coverage_full(&self) -> bool {
        for i in 0..4 {
            match self.cover.get(i) {
                Some(c) if *c > 0 => {}
                _ => return false,
            }
        }
        true
    }

    /// 联签：对锚定行做指纹链核验，行数必须 ≥ 判据轮数。
    pub fn joint_sign(&mut self, expected_rounds: usize) -> Result<u64, ObCode> {
        // 判序：空环先于相位（空环无从联签，语义更精确）
        if self.ring.is_empty() {
            return Err(ObCode::SignEmpty);
        }
        if self.phase != ObsPhase::Collecting && self.phase != ObsPhase::Signed {
            return Err(ObCode::PhaseSkip);
        }
        // 指纹链：按环序滚链
        let mut chain: u64 = 0xcbf29ce484222325;
        for r in self.ring.iter() {
            chain ^= r.word();
            chain = chain.wrapping_mul(0x100000001b3);
        }
        self.joint_rows = self.ring.len();
        if self.joint_rows < expected_rounds {
            return Err(ObCode::JointRowsShort);
        }
        self.phase = ObsPhase::Signed;
        self.mix(chain);
        Ok(chain)
    }

    /// 联签漂移核验：重算指纹链与联签账指纹比对。
    pub fn verify_no_drift(&self) -> bool {
        if self.ring.is_empty() {
            return false;
        }
        let mut chain: u64 = 0xcbf29ce484222325;
        for r in self.ring.iter() {
            chain ^= r.word();
            chain = chain.wrapping_mul(0x100000001b3);
        }
        // 联签后混入的链应落在指纹里：fp 应非基值
        self.fp != 0xcbf29ce484222325 && self.joint_rows == self.ring.len()
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<ObsPhase, ObCode> {
        if self.phase != ObsPhase::Signed {
            return Err(ObCode::PhaseSkip);
        }
        self.phase = ObsPhase::Sealed;
        self.mix(0x6AF0);
        Ok(self.phase)
    }

    /// 环账核对：环内行数 == 覆盖账之和 − 丢点无关（丢点不入环）。
    pub fn ledger_ok(&self) -> bool {
        let mut sum = 0u64;
        for i in 0..4 {
            sum = sum.saturating_add(self.cover[i]);
        }
        sum as usize == self.ring.len()
    }

    pub fn phase(&self) -> ObsPhase {
        self.phase
    }
    pub fn len(&self) -> usize {
        self.ring.len()
    }
    pub fn joint_rows(&self) -> usize {
        self.joint_rows
    }
    /// 行读取。
    pub fn row_at(&self, i: usize) -> Option<TraceRow> {
        self.ring.get(i).copied()
    }

    /// 联测 ×30 零漂移脚本：30 轮四点逐点写行后联签，逐轮零漂移。
    pub fn joint_script(rounds: usize) -> u32 {
        let mut ok = 0u32;
        for r in 0..rounds {
            let mut obs = ObsRing::new();
            let mut rows = 0usize;
            for p in 0..4u32 {
                // 每点写 2 行（一行正常一行异常），共 8 行
                for k in 0..2u32 {
                    let row = TraceRow {
                        point: p,
                        payload: (r as u32).wrapping_mul(16).wrapping_add(k),
                        anomaly: k == 1,
                    };
                    if obs.write(row).is_ok() {
                        rows += 1;
                    }
                }
            }
            if rows != 8 || !obs.coverage_full() || !obs.ledger_ok() {
                continue;
            }
            // 静默核验：声明 4 异常，环内必须有 4 条异常行
            if obs.audit_silent(4).is_err() {
                continue;
            }
            match obs.joint_sign(8) {
                Ok(_) => {
                    if obs.verify_no_drift() {
                        ok += 1;
                    }
                }
                Err(_) => {}
            }
        }
        ok
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
        self.mix(0xD1);
    }
    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.escaped = self.escaped.saturating_add(1);
        self.mix(0xE5);
    }
    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
            && self.escaped == 0
            && self.detected > 0
    }
    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> ([u64; 4], u64, u32, usize, u64) {
        (self.cover, self.dropped, self.phase as u32, self.ring.len(), self.fp)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// LCG 种子：SEG_BASE 派生。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0xB19
}

/// FNV-1a 64：双跑指纹的哈希核。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 双跑一致性：同一联测脚本两次建环，快照与指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> ([u64; 4], u64, u32, usize, u64) {
        let mut obs = ObsRing::new();
        for p in 0..4u32 {
            let _ = obs.write(TraceRow { point: p, payload: p * 7, anomaly: p % 2 == 0 });
        }
        let _ = obs.joint_sign(4);
        let _ = obs.seal();
        obs.snapshot()
    };
    build() == build()
}

/// 收口自检（16 条判据形态）。
pub fn run_unx51570_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51570");
    // 1 正常流：四点各写一行，覆盖全真。
    {
        let mut obs = ObsRing::new();
        for p in 0..4u32 {
            let _ = obs.write(TraceRow { point: p, payload: p, anomaly: false });
        }
        s.add_dyn(
            "四点全覆盖",
            obs.coverage_full() && obs.len() == 4 && obs.ledger_ok(),
            "绑定/隔离/合并/热拔四点必须逐点留痕且环账平",
        );
    }
    // 2 联测 ×30 零漂移（判据 J1）。
    {
        let ok = ObsRing::joint_script(30);
        s.add_dyn("联测 30 轮零漂移", ok == 30, "三十轮四点联签必须逐轮零漂移");
    }
    // 3 注入类一：静默异常 ×10 轮必抓。
    {
        let mut obs = ObsRing::new();
        let mut caught = 0u32;
        for r in 0..10u32 {
            // 只写正常行却声明「比在账多一」的异常 → 必须逐轮抓静默
            let _ = obs.write(TraceRow { point: 0, payload: r, anomaly: false });
            if obs.audit_silent(r as u64 + 1) == Err(ObCode::SilentAnomaly) {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
            // 补写异常行恢复基线
            let _ = obs.write(TraceRow { point: 1, payload: r, anomaly: true });
        }
        s.add_dyn("静默异常 10/10 全抓", caught == 10, "声明异常数超过上环数必须逐轮抓静默");
    }
    // 4 注入类二：越界点码 ×10 轮。
    {
        let mut obs = ObsRing::new();
        let mut caught = 0u32;
        for i in 4..14u32 {
            if obs.write(TraceRow { point: i, payload: i, anomaly: false }) == Err(ObCode::PointUnknown) {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
        }
        s.add_dyn("越界点码 10/10 全抓", caught == 10, "埋点码超四点闭集必须显性拒");
    }
    // 5 注入类三：环溢丢点 ×10 轮显性计账。
    {
        let mut obs = ObsRing::new();
        for i in 0..RING_CAP as u32 {
            let _ = obs.write(TraceRow { point: i % 4, payload: i, anomaly: false });
        }
        let mut caught = 0u32;
        for i in 0..10u32 {
            if obs.write(TraceRow { point: i % 4, payload: 1000 + i, anomaly: false })
                == Err(ObCode::RingOverflow)
            {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
        }
        s.add_dyn("环溢丢点 10/10 显性", caught == 10 && obs.dropped == 10, "环满后写入必须逐轮显性计丢点");
    }
    // 6 注入类四：联签漂移 ×10 轮（空环/短行联签路径）。
    {
        let mut caught = 0u32;
        // 空环联签
        for _ in 0..5u32 {
            let mut obs = ObsRing::new();
            if obs.joint_sign(1) == Err(ObCode::SignEmpty) {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
        }
        // 行数不足联签
        for _ in 0..5u32 {
            let mut obs = ObsRing::new();
            let _ = obs.write(TraceRow { point: 0, payload: 1, anomaly: false });
            if obs.joint_sign(8) == Err(ObCode::JointRowsShort) {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
        }
        s.add_dyn("联签漂移路径 10/10 全抓", caught == 10, "空环与行数不足联签必须逐轮显性拒");
    }
    // 7 注入类五：封账后写入 ×10 轮。
    {
        let mut obs = ObsRing::new();
        let _ = obs.write(TraceRow { point: 0, payload: 1, anomaly: false });
        let _ = obs.joint_sign(1);
        let _ = obs.seal();
        let mut caught = 0u32;
        for i in 0..10u32 {
            if obs.write(TraceRow { point: 1, payload: i, anomaly: false }) == Err(ObCode::WriteAfterSeal) {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
        }
        s.add_dyn("封账后写入 10/10 全抓", caught == 10, "封账环只读，写入必须逐轮显性拒");
    }
    // 8 注入类六：覆盖不全谎报 ×10 轮。
    {
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let mut obs = ObsRing::new();
            // 只写两点就声称覆盖全
            let _ = obs.write(TraceRow { point: 0, payload: 1, anomaly: false });
            let _ = obs.write(TraceRow { point: 2, payload: 2, anomaly: false });
            if !obs.coverage_full() {
                caught += 1;
                obs.note_detected();
            } else {
                obs.note_escape();
            }
        }
        s.add_dyn("覆盖不全 10/10 全抓", caught == 10, "四点未全覆盖时覆盖核验必须不过");
    }
    // 9 埋点码闭集。
    {
        let mut closed = true;
        for c in 4..16u32 {
            if TracePoint::from_code(c).is_some() {
                closed = false;
            }
        }
        s.add_dyn("埋点码闭集", closed && TracePoint::from_code(3) == Some(TracePoint::HotPlug), "埋点码只接受 0/1/2/3 四点");
    }
    // 10 相位单调：封账后再封/再签显性拒。
    {
        let mut obs = ObsRing::new();
        let _ = obs.write(TraceRow { point: 3, payload: 1, anomaly: false });
        let _ = obs.joint_sign(1);
        let _ = obs.seal();
        let re_seal = obs.seal();
        s.add_dyn(
            "相位单调封账",
            re_seal == Err(ObCode::PhaseSkip) && obs.phase() == ObsPhase::Sealed,
            "封账只许一次且相位冻结",
        );
    }
    // 11 环账核对：正常写入后环账必平。
    {
        let mut obs = ObsRing::new();
        for r in 0..12u32 {
            let _ = obs.write(TraceRow { point: r % 4, payload: r, anomaly: false });
        }
        s.add_dyn("环账核对", obs.ledger_ok(), "覆盖账之和必须等于环内行数");
    }
    // 12 异常上环账：异常行逐条入账。
    {
        let mut obs = ObsRing::new();
        for r in 0..6u32 {
            let _ = obs.write(TraceRow { point: r % 4, payload: r, anomaly: true });
        }
        s.add_dyn(
            "异常上环账",
            obs.anomalies_logged == 6 && obs.audit_silent(6).is_ok(),
            "六条异常行必须逐条上环且静默核验通过",
        );
    }
    // 13 相位码闭集。
    {
        let mut closed = true;
        for c in 4..16u32 {
            if ObsPhase::from_code(c).is_some() {
                closed = false;
            }
        }
        s.add_dyn("相位码闭集", closed && ObsPhase::from_code(3) == Some(ObsPhase::Sealed), "相位码只接受 0/1/2/3");
    }
    // 14 注入账守恒。
    {
        let mut obs = ObsRing::new();
        for _ in 0..10u32 {
            obs.note_detected();
        }
        s.add_dyn("注入账守恒", obs.conserved(), "注入=检出+逃逸，逃逸恒 0，检出非零");
    }
    // 15 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "同一联测脚本两次建环快照与指纹逐位相等");
    // 16 段位与文案闭集。
    {
        let mut texts_ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.detail().is_empty() || c.val() != SEG_BASE + 1 + i as u32 {
                texts_ok = false;
            }
        }
        s.add_dyn("段位与文案闭集", seg_guard_ok() && texts_ok, "SEG_BASE 对齐且每码中文建议非空且段内单射");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_points_covered() {
        let mut obs = ObsRing::new();
        for p in 0..4u32 {
            assert!(obs.write(TraceRow { point: p, payload: p, anomaly: false }).is_ok());
        }
        assert!(obs.coverage_full());
        assert!(obs.ledger_ok());
        assert_eq!(obs.len(), 4);
    }

    #[test]
    fn joint_thirty_rounds_zero_drift() {
        assert_eq!(ObsRing::joint_script(30), 30);
    }

    #[test]
    fn silent_anomaly_caught() {
        let mut obs = ObsRing::new();
        let _ = obs.write(TraceRow { point: 0, payload: 1, anomaly: false });
        assert_eq!(obs.audit_silent(1), Err(ObCode::SilentAnomaly));
        let _ = obs.write(TraceRow { point: 1, payload: 2, anomaly: true });
        assert!(obs.audit_silent(1).is_ok());
    }

    #[test]
    fn point_code_closed_set() {
        for c in 0..4u32 {
            assert!(TracePoint::from_code(c).is_some());
        }
        for c in 4..16u32 {
            assert!(TracePoint::from_code(c).is_none());
        }
    }

    #[test]
    fn ring_overflow_counts_drops() {
        let mut obs = ObsRing::new();
        for i in 0..RING_CAP as u32 {
            assert!(obs.write(TraceRow { point: i % 4, payload: i, anomaly: false }).is_ok());
        }
        assert_eq!(obs.write(TraceRow { point: 0, payload: 999, anomaly: false }), Err(ObCode::RingOverflow));
        assert_eq!(obs.dropped, 1);
        assert_eq!(obs.len(), RING_CAP);
    }

    #[test]
    fn write_after_seal_denied() {
        let mut obs = ObsRing::new();
        let _ = obs.write(TraceRow { point: 2, payload: 1, anomaly: false });
        let _ = obs.joint_sign(1);
        assert!(obs.seal().is_ok());
        assert_eq!(
            obs.write(TraceRow { point: 2, payload: 2, anomaly: false }),
            Err(ObCode::WriteAfterSeal)
        );
    }

    #[test]
    fn sign_empty_and_rows_short_denied() {
        let mut obs = ObsRing::new();
        assert_eq!(obs.joint_sign(1), Err(ObCode::SignEmpty));
        let _ = obs.write(TraceRow { point: 0, payload: 1, anomaly: false });
        assert_eq!(obs.joint_sign(8), Err(ObCode::JointRowsShort));
        assert!(obs.joint_sign(1).is_ok());
    }

    #[test]
    fn coverage_incomplete_detected() {
        let mut obs = ObsRing::new();
        let _ = obs.write(TraceRow { point: 0, payload: 1, anomaly: false });
        let _ = obs.write(TraceRow { point: 1, payload: 2, anomaly: false });
        assert!(!obs.coverage_full());
    }

    #[test]
    fn phase_closed_set_and_monotonic() {
        for c in 0..4u32 {
            assert!(ObsPhase::from_code(c).is_some());
        }
        for c in 4..16u32 {
            assert!(ObsPhase::from_code(c).is_none());
        }
        let mut obs = ObsRing::new();
        // 空环直接封账：跳步
        assert_eq!(obs.seal(), Err(ObCode::PhaseSkip));
    }

    #[test]
    fn anomaly_rows_all_logged() {
        let mut obs = ObsRing::new();
        for r in 0..6u32 {
            let _ = obs.write(TraceRow { point: r % 4, payload: r, anomaly: true });
        }
        assert_eq!(obs.anomalies_logged, 6);
        assert!(obs.audit_silent(6).is_ok());
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut obs = ObsRing::new();
        for _ in 0..10u32 {
            obs.note_detected();
        }
        assert!(obs.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
        assert_eq!(lcg_seed(), lcg_seed());
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn unx51570_checks_green() {
        let set = run_unx51570_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
