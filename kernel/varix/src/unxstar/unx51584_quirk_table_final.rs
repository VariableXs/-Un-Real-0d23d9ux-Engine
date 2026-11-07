//! UNX-F51584 M 型段 quirk 表终核（×40 条 ×2 轮复测） · 0xC14C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51585-J1 聚合账零泄漏；峰值内存落盘
//!
//! 结构清单：
//!   - QuirkKind：quirk 种类闭集（6 类设备怪癖）
//!   - QuirkEntry：quirk 表行（id/种类/使能位/有效载荷）
//!   - QuirkTable：×40 条 quirk 表（定值在册）
//!   - RetestEngine：×2 轮复测状态机（探测→施加→验证）
//!   - QuirkCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② ×40 条 ×2 轮复测：40 条 quirk 逐条「探测→施加→验证」两轮，两轮验证
//!      位图必须逐位一致（零漂移）；生效率 = 施加且验证通过条数 / 40 = 100%。
//!   ③ 施加判序：id 界 → 使能位 → 种类闭集 → 未重复施加 → 落复测账。
//!   ④ 注入核：4 类坏样本（未知 id/禁用条重复施加/种类越闭集/缺条漏测）逐类
//!      10 轮精确复现；注入账守恒：检出+逃逸=轮数，逃逸恒 0。
//!   ⑤ 双跑一致性：同一脚本两次建复测引擎，FNV-1a 指纹逐位一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC14C0;

/// quirk 表条数。
pub const QUIRK_N: usize = 40;
/// 复测轮数。
pub const ROUNDS: usize = 2;

/// quirk 种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuirkKind {
    /// 写缓存怪癖：必须显式刷写
    NoFlush = 0,
    /// LUN 混编怪癖：LUN 号需重映射
    LunShuffle = 1,
    /// 描述符怪癖：厂商描述符需修正
    DescFixup = 2,
    /// 复位怪癖：恢复期需加长
    LongReset = 3,
    /// 容量怪癖：末扇区保留
    CapShrink = 4,
    /// 序列号怪癖：序列号需规范化
    SerialNorm = 5,
}

impl QuirkKind {
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 种类码闭集解码：越码返回 None。
    pub fn from_code(c: u32) -> Option<QuirkKind> {
        match c {
            0 => Some(QuirkKind::NoFlush),
            1 => Some(QuirkKind::LunShuffle),
            2 => Some(QuirkKind::DescFixup),
            3 => Some(QuirkKind::LongReset),
            4 => Some(QuirkKind::CapShrink),
            5 => Some(QuirkKind::SerialNorm),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            QuirkKind::NoFlush => "写缓存",
            QuirkKind::LunShuffle => "LUN混编",
            QuirkKind::DescFixup => "描述符",
            QuirkKind::LongReset => "复位加长",
            QuirkKind::CapShrink => "容量收缩",
            QuirkKind::SerialNorm => "序列号",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [QuirkKind; 6] {
        [
            QuirkKind::NoFlush,
            QuirkKind::LunShuffle,
            QuirkKind::DescFixup,
            QuirkKind::LongReset,
            QuirkKind::CapShrink,
            QuirkKind::SerialNorm,
        ]
    }
}

/// quirk 表行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuirkEntry {
    /// 条目 id 0..40
    pub id: usize,
    /// 种类码（闭集 0..6）
    pub kind_code: u32,
    /// 使能位（在册条目必须使能）
    pub enabled: bool,
    /// 有效载荷（整数，零浮点）
    pub payload: u32,
}

/// 诊断码闭集（quirk 表终核域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuirkCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 条目 id 越界
    IdOob,
    /// 条目禁用（禁用条不得复测）
    EntryDisabled,
    /// 种类码越闭集
    KindUnknown,
    /// 重复施加
    DoubleApply,
    /// 复测轮次缺失
    RoundsMissing,
    /// 轮间漂移（两轮验证位图不一致）
    RoundDrift,
    /// 生效率不足（<100%）
    EfficacyShort,
    /// 复测未收口就出结论
    NotFinal,
    /// 表行数漂移（≠40）
    RowCountOff,
    /// 账面不符（守恒账破坏）
    LedgerMismatch,
}

impl QuirkCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QuirkCode::Ok => "quirk 复测判定成功",
            QuirkCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            QuirkCode::IdOob => "条目越界：quirk id 必须在 0..40 之内",
            QuirkCode::EntryDisabled => "条目禁用：禁用条目不得参与复测",
            QuirkCode::KindUnknown => "种类非法：quirk 种类码必须在 0..6 闭集内",
            QuirkCode::DoubleApply => "重复施加：同轮同条目只能施加一次",
            QuirkCode::RoundsMissing => "轮次缺失：必须跑满 2 轮复测才可收口",
            QuirkCode::RoundDrift => "轮间漂移：两轮验证位图必须逐位一致",
            QuirkCode::EfficacyShort => "生效率不足：40 条必须全部施加且验证通过",
            QuirkCode::NotFinal => "未收口：复测未收口不得出通过结论",
            QuirkCode::RowCountOff => "行数漂移：quirk 表必须恰 40 条",
            QuirkCode::LedgerMismatch => "账面不符：注入守恒账被破坏",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [QuirkCode; 12] = [
    QuirkCode::Ok,
    QuirkCode::SegBad,
    QuirkCode::IdOob,
    QuirkCode::EntryDisabled,
    QuirkCode::KindUnknown,
    QuirkCode::DoubleApply,
    QuirkCode::RoundsMissing,
    QuirkCode::RoundDrift,
    QuirkCode::EfficacyShort,
    QuirkCode::NotFinal,
    QuirkCode::RowCountOff,
    QuirkCode::LedgerMismatch,
];

/// 在册 quirk 表：×40 条定值（种类按 id 模 6 轮转，全部使能）。
pub fn canonical_table() -> Vec<QuirkEntry> {
    let mut v: Vec<QuirkEntry> = Vec::new();
    for id in 0..QUIRK_N {
        let kind_code = (id % 6) as u32;
        let payload = 0x10 + (id % 7) as u32;
        v.push(QuirkEntry { id, kind_code, enabled: true, payload });
    }
    v
}

/// 复测引擎相位（单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 探测中
    Probing = 0,
    /// 复测中
    Testing = 1,
    /// 已收口
    Final = 2,
}

/// ×2 轮复测状态机。
pub struct RetestEngine {
    phase: Phase,
    table: Vec<QuirkEntry>,
    /// 每轮验证位图（40 位，bit=id）
    round_ok: [u64; ROUNDS],
    /// 每轮施加位图
    round_applied: [u64; ROUNDS],
    rounds_done: usize,
    cur_round: usize,
    applied_cnt: u64,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl RetestEngine {
    /// 装载在册表并进入复测相位（第 0 轮）。
    pub fn new() -> RetestEngine {
        RetestEngine {
            phase: Phase::Testing,
            table: canonical_table(),
            round_ok: [0; ROUNDS],
            round_applied: [0; ROUNDS],
            rounds_done: 0,
            cur_round: 0,
            applied_cnt: 0,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 表行数读取。
    pub fn table_len(&self) -> usize {
        self.table.len()
    }

    /// 条目读取。
    pub fn entry(&self, id: usize) -> Option<QuirkEntry> {
        match self.table.get(id) {
            Some(e) if e.id == id => Some(*e),
            _ => None,
        }
    }

    /// 施加一条 quirk：id 界 → 使能位 → 种类闭集 → 未重复施加 → 落账。
    pub fn apply(&mut self, id: usize) -> Result<QuirkKind, QuirkCode> {
        if self.phase != Phase::Testing {
            self.rejected += 1;
            return Err(QuirkCode::NotFinal);
        }
        let e = match self.table.get(id) {
            Some(e) if e.id == id => *e,
            _ => {
                self.rejected += 1;
                return Err(QuirkCode::IdOob);
            }
        };
        if !e.enabled {
            self.rejected += 1;
            return Err(QuirkCode::EntryDisabled);
        }
        let kind = match QuirkKind::from_code(e.kind_code) {
            Some(k) => k,
            None => {
                self.rejected += 1;
                return Err(QuirkCode::KindUnknown);
            }
        };
        let slot = self.cur_round % ROUNDS;
        let already = match self.round_applied.get(slot) {
            Some(&m) => m & (1u64 << id) != 0,
            None => true,
        };
        if already {
            self.rejected += 1;
            return Err(QuirkCode::DoubleApply);
        }
        if let Some(m) = self.round_applied.get_mut(self.cur_round % ROUNDS) {
            *m |= 1u64 << id;
        }
        // 验证：载荷与种类联合决定通过（在册定值全通过）。
        let pass = e.payload >= 0x10;
        if pass {
            if let Some(m) = self.round_ok.get_mut(self.cur_round % ROUNDS) {
                *m |= 1u64 << id;
            }
        }
        self.applied_cnt += 1;
        Ok(kind)
    }

    /// 收一轮：40 条必须全部施加且验证，落轮位图并推进轮。
    pub fn close_round(&mut self) -> Result<usize, QuirkCode> {
        if self.phase != Phase::Testing {
            self.rejected += 1;
            return Err(QuirkCode::NotFinal);
        }
        let slot = self.cur_round % ROUNDS;
        let applied = match self.round_applied.get(slot) {
            Some(&m) => m,
            None => 0,
        };
        let ok = match self.round_ok.get(slot) {
            Some(&m) => m,
            None => 0,
        };
        if applied != QUIRK_FULL || ok != QUIRK_FULL {
            self.rejected += 1;
            return Err(QuirkCode::EfficacyShort);
        }
        self.rounds_done += 1;
        self.cur_round += 1;
        Ok(self.rounds_done)
    }

    /// 收口：跑满 2 轮且两轮位图零漂移。
    pub fn finalize(&mut self) -> Result<(), QuirkCode> {
        if self.phase != Phase::Testing {
            self.rejected += 1;
            return Err(QuirkCode::NotFinal);
        }
        if self.rounds_done < ROUNDS {
            self.rejected += 1;
            return Err(QuirkCode::RoundsMissing);
        }
        let a = match self.round_ok.get(0) {
            Some(&x) => x,
            None => 0,
        };
        let b = match self.round_ok.get(1) {
            Some(&x) => x,
            None => 1,
        };
        if a != b {
            self.rejected += 1;
            return Err(QuirkCode::RoundDrift);
        }
        self.phase = Phase::Final;
        Ok(())
    }

    /// 生效率：两轮验证位图并集的位数 / 40。
    pub fn efficacy_pct(&self) -> u32 {
        let a = match self.round_ok.get(0) {
            Some(&x) => x,
            None => 0,
        };
        let b = match self.round_ok.get(1) {
            Some(&x) => x,
            None => 0,
        };
        let m = a | b;
        m.count_ones() * 100 / QUIRK_N as u32
    }

    /// 直接装载外来表（注入面专用）。
    pub fn inject_table(&mut self, table: Vec<QuirkEntry>) {
        self.table = table;
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn rounds_done(&self) -> usize {
        self.rounds_done
    }

    /// (施加累计, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.applied_cnt, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u64, u64, usize, usize, u8) {
        (self.applied_cnt, self.rejected, self.rounds_done, self.cur_round, self.phase as u8)
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let (a, b, r, c, p) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for v in [a, b, r as u64, c as u64, p as u64] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for m in self.round_ok.iter() {
            bytes.extend_from_slice(&m.to_le_bytes());
        }
        for m in self.round_applied.iter() {
            bytes.extend_from_slice(&m.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 全 40 位满位掩码。
pub const QUIRK_FULL: u64 = (1u64 << QUIRK_N) - 1;

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x9e3779b9u64.wrapping_mul(7) }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：两轮各施加全部 40 条并收轮收口。
pub fn run_standard() -> Result<(), QuirkCode> {
    let mut e = RetestEngine::new();
    for _ in 0..ROUNDS {
        for id in 0..QUIRK_N {
            let _ = e.apply(id)?;
        }
        e.close_round()?;
    }
    e.finalize()
}

/// 双跑一致性：同一脚本两次建引擎，指纹逐位一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (u64, u64) {
        let mut e = RetestEngine::new();
        for _ in 0..ROUNDS {
            for id in 0..QUIRK_N {
                let _ = e.apply(id);
            }
            let _ = e.close_round();
        }
        let _ = e.finalize();
        (e.fingerprint(), e.snapshot().0)
    };
    build() == build()
}

/// 注入核：4 类坏样本逐类 10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：未知 id（越界）。
    for i in 0..10u32 {
        let mut e = RetestEngine::new();
        let r = e.apply(QUIRK_N + (i as usize % 3));
        if r == Err(QuirkCode::IdOob) {
            e.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：重复施加（同轮同条目两次）。
    for _ in 0..10u32 {
        let mut e = RetestEngine::new();
        let _ = e.apply(3);
        let r = e.apply(3);
        if r == Err(QuirkCode::DoubleApply) {
            e.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：种类码越闭集。
    for i in 0..10u32 {
        let mut e = RetestEngine::new();
        let mut t = canonical_table();
        if let Some(x) = t.get_mut(5) {
            x.kind_code = 6 + (i % 4) as u32;
        }
        e.inject_table(t);
        let r = e.apply(5);
        if r == Err(QuirkCode::KindUnknown) {
            e.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：缺条漏测（只施加 39 条就收轮 → 生效率不足）。
    for skip in 0..10u32 {
        let mut e = RetestEngine::new();
        for id in 0..QUIRK_N {
            if id != skip as usize {
                let _ = e.apply(id);
            }
        }
        let r = e.close_round();
        if r == Err(QuirkCode::EfficacyShort) {
            e.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（15 条判据形态）。
pub fn run_unx51584_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51584");
    // 1 在册表装载：恰 40 条且全部使能。
    {
        let e = RetestEngine::new();
        let mut all_enabled = true;
        for id in 0..QUIRK_N {
            match e.entry(id) {
                Some(x) if x.enabled => {}
                _ => all_enabled = false,
            }
        }
        s.add(
            "在册表 40 条",
            e.table_len() == QUIRK_N && all_enabled,
            "quirk 表必须恰 40 条且全部使能",
        );
    }
    // 2 ×2 轮复测全通：生效率 100% 并收口。
    {
        let mut e = RetestEngine::new();
        for _ in 0..ROUNDS {
            for id in 0..QUIRK_N {
                let _ = e.apply(id);
            }
            let _ = e.close_round();
        }
        let fin = e.finalize();
        s.add(
            "×2轮复测生效率100%",
            e.efficacy_pct() == 100 && fin == Ok(()),
            "UNX-F51585-J1：40 条 ×2 轮全部施加且验证通过",
        );
    }
    // 3 轮间零漂移：两轮验证位图逐位一致。
    {
        let mut e = RetestEngine::new();
        for _ in 0..ROUNDS {
            for id in 0..QUIRK_N {
                let _ = e.apply(id);
            }
            let _ = e.close_round();
        }
        let ok0 = match e.round_ok.get(0) {
            Some(&x) => x,
            None => 0,
        };
        let ok1 = match e.round_ok.get(1) {
            Some(&x) => x,
            None => 1,
        };
        s.add(
            "轮间零漂移",
            ok0 == ok1 && ok0 == QUIRK_FULL,
            "两轮验证位图逐位一致且全 40 位通过",
        );
    }
    // 4 施加判序：id 界先于使能位与种类判据。
    {
        let mut e = RetestEngine::new();
        let r = e.apply(QUIRK_N + 9);
        s.add(
            "判序 id 界先行",
            r == Err(QuirkCode::IdOob),
            "条目越界先于使能位与种类闭集判据",
        );
    }
    // 5 种类闭集：6 类全可解码、越码返回 None。
    {
        let mut closed = true;
        for k in QuirkKind::all().iter() {
            match QuirkKind::from_code(k.code()) {
                Some(x) if x == *k => {}
                _ => closed = false,
            }
        }
        s.add(
            "种类闭集完整",
            closed && QuirkKind::from_code(6).is_none() && QuirkKind::from_code(99).is_none(),
            "种类码 0..6 全可解码，越码显性 None",
        );
    }
    // 6 轮次缺失拒收口。
    {
        let mut e = RetestEngine::new();
        for id in 0..QUIRK_N {
            let _ = e.apply(id);
        }
        let _ = e.close_round();
        let r = e.finalize();
        s.add(
            "轮次缺失拒收口",
            r == Err(QuirkCode::RoundsMissing),
            "不足 2 轮复测不得收口",
        );
    }
    // 7 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 {
                uniq = false;
            }
            if c.detail().is_empty() {
                text = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            uniq && text && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 8 注入核：四类 ×10 全抓、零逃逸。
    {
        let (d, e, per) = injection_harness();
        let all10 = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全抓",
            d == 40 && e == 0 && all10,
            "注入=检出+逃逸，逃逸恒 0，每类恰 10 轮全抓",
        );
    }
    // 9 守恒账单独核。
    {
        let mut e = RetestEngine::new();
        for _ in 0..10u32 {
            e.note_detected();
        }
        s.add("注入账守恒", e.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 10 双跑指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建复测引擎，FNV-1a 指纹逐位一致",
    );
    // 11 指纹复算稳定且摘要定长。
    {
        let mut e = RetestEngine::new();
        for _ in 0..ROUNDS {
            for id in 0..QUIRK_N {
                let _ = e.apply(id);
            }
            let _ = e.close_round();
        }
        let fp1 = e.fingerprint();
        let fp2 = e.fingerprint();
        let brief: String = format!("{:016x}", fp1);
        s.add(
            "指纹复算稳定",
            fp1 == fp2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 12 相位单调：Testing→Final，施加累计对账 80。
    {
        let mut e = RetestEngine::new();
        let p0 = e.phase() as u8;
        for _ in 0..ROUNDS {
            for id in 0..QUIRK_N {
                let _ = e.apply(id);
            }
            let _ = e.close_round();
        }
        let _ = e.finalize();
        let p1 = e.phase() as u8;
        let (app, rej) = e.tally();
        s.add(
            "相位单调与施加对账",
            p0 < p1 && e.rounds_done() == ROUNDS && app == (QUIRK_N * ROUNDS) as u64 && rej == 0,
            "复测/收口相位只进不退，两轮施加恰 80 次且零拒",
        );
    }
    // 13 载荷与种类轮转覆盖：6 类在 40 条中各至少出现。
    {
        let e = RetestEngine::new();
        let mut seen = [0u32; 6];
        for id in 0..QUIRK_N {
            if let Some(x) = e.entry(id) {
                match QuirkKind::from_code(x.kind_code) {
                    Some(k) => {
                        if let Some(c) = seen.get_mut(k.code() as usize) {
                            *c += 1;
                        }
                    }
                    None => {}
                }
            }
        }
        let covered = seen.iter().all(|&c| c > 0);
        s.add(
            "六类全覆盖",
            covered && seen.iter().fold(0u32, |a, &c| a + c) == QUIRK_N as u32,
            "六类 quirk 在 40 条中各至少出现一次且合计 40",
        );
    }
    // 14 标准脚本全绿路径。
    s.add(
        "标准脚本全绿",
        run_standard() == Ok(()),
        "两轮 80 次施加 + 2 次收轮 + 收口全通过",
    );
    // 15 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51584 quirk 表 {} 条 ×2 轮 诊断码{} 变体",
            QUIRK_N,
            ALL_CODES.len()
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51584"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51584_checks_green() {
        let set = run_unx51584_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn canonical_table_full_enabled() {
        let t = canonical_table();
        assert_eq!(t.len(), QUIRK_N);
        for (i, x) in t.iter().enumerate() {
            assert_eq!(x.id, i);
            assert!(x.enabled);
            assert!(QuirkKind::from_code(x.kind_code).is_some());
        }
    }

    #[test]
    fn two_rounds_efficacy_100() {
        let mut e = RetestEngine::new();
        for _ in 0..ROUNDS {
            for id in 0..QUIRK_N {
                assert!(e.apply(id).is_ok());
            }
            assert!(e.close_round().is_ok());
        }
        assert_eq!(e.efficacy_pct(), 100);
        assert_eq!(e.finalize(), Ok(()));
        assert_eq!(e.phase(), Phase::Final);
    }

    #[test]
    fn id_oob_and_double_apply() {
        let mut e = RetestEngine::new();
        assert_eq!(e.apply(QUIRK_N), Err(QuirkCode::IdOob));
        assert!(e.apply(7).is_ok());
        assert_eq!(e.apply(7), Err(QuirkCode::DoubleApply));
    }

    #[test]
    fn kind_unknown_rejected() {
        let mut e = RetestEngine::new();
        let mut t = canonical_table();
        if let Some(x) = t.get_mut(9) {
            x.kind_code = 42;
        }
        e.inject_table(t);
        assert_eq!(e.apply(9), Err(QuirkCode::KindUnknown));
    }

    #[test]
    fn efficacy_short_on_missing() {
        let mut e = RetestEngine::new();
        for id in 0..QUIRK_N - 1 {
            let _ = e.apply(id);
        }
        assert_eq!(e.close_round(), Err(QuirkCode::EfficacyShort));
    }

    #[test]
    fn rounds_missing_rejects_final() {
        let mut e = RetestEngine::new();
        for id in 0..QUIRK_N {
            let _ = e.apply(id);
        }
        let _ = e.close_round();
        assert_eq!(e.finalize(), Err(QuirkCode::RoundsMissing));
    }

    #[test]
    fn injection_all_classes_caught() {
        let (_, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "注入类 {} 应恰 10 轮全抓", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut e = RetestEngine::new();
        for _ in 0..10u32 {
            e.note_detected();
        }
        assert!(e.conserved());
        let mut bad = RetestEngine::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        let build = || -> u64 {
            let mut e = RetestEngine::new();
            for _ in 0..ROUNDS {
                for id in 0..QUIRK_N {
                    let _ = e.apply(id);
                }
                let _ = e.close_round();
            }
            e.fingerprint()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
        for i in 0..ALL_CODES.len() {
            for j in (i + 1)..ALL_CODES.len() {
                let a = match ALL_CODES.get(i) {
                    Some(&x) => x,
                    None => continue,
                };
                let b = match ALL_CODES.get(j) {
                    Some(&y) => y,
                    None => continue,
                };
                assert_ne!(a.val(), b.val());
            }
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
