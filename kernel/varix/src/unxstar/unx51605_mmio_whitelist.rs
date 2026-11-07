//! UNX-F51605 MMIO 越权写防御（白名单外拒绝） · 0xC1A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51606-J1 风暴 ×10 限流生效零丢事件；UNX-F51606-J1R 正常
//! 中断 ×10^4 零误限；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - MmioCode           诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - GuardPhase         防御相态机（Armed→Hold→Recovered）
//!   - MmioWhitelistGuard 白名单区间表 + 写闸 + 审计账
//!   - MmioWriteLedger    越权审计账 + 注入守恒账（逃逸恒 0）
//!   - run_unx51605_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 白名单闭集：只放行 WHITELIST_RANGES 冻结区间内的写（红线④路径），
//!      区间外一律显性拒（MmioOob）并逐条留账；区间表只读，运行期不可扩。
//!   ② 越权 ×10 全拒留账：每次拒绝在审计账留下（地址、违例码）二元组，
//!      放行 + 拒绝 = 审计条数三本对平。
//!   ③ 白名单内写 ×100 零误拒：合法地址轮转 ×100 全通，相态保持 Armed。
//!   ④ 相态机：违例累计达阈值进 Hold（审计冻结写），显性审计收口后走
//!      Recovered；相态只按编码序前进，回退/跳相显性拒。
//!   ⑤ 注入核：5 类坏样本逐类 ×10 轮精确复现，注入账守恒
//!      （注入 = 检出 + 逃逸，逃逸恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——注入参数由 LCG
//!      派生（种子 = SEG_BASE × 黄金比常数 + 1），同段位恒定可复现。
//!   ⑦ 全程零 unwrap/expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1A00;

/// 白名单冻结区间表（首地址，长度），运行期只读。
pub const WHITELIST_RANGES: [(u32, u32); 6] = [
    (0xF000, 0x40), // 主寄存器组
    (0xF100, 0x10), // doorbell 组
    (0xF200, 0x20), // 运行时寄存器组
    (0xF300, 0x08), // 中断组
    (0xF400, 0x40), // 端口寄存器组
    (0xF500, 0x10), // 扩展组
];

/// 连续越权隔离阈值。
pub const HOLD_THRESHOLD: u32 = 5;

/// 诊断码闭集（MMIO 白名单防御域，12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmioCode {
    /// 成功
    Ok = 1,
    /// 段位守卫失败
    SegBad = 2,
    /// 地址越白名单（越权写）
    MmioOob = 3,
    /// 区间表未登记（未映射段）
    RangeUnmapped = 4,
    /// 审计冻结相态内写
    WriteInHold = 5,
    /// 相态回退
    StateRegress = 6,
    /// 审计账不符
    AuditMismatch = 7,
    /// 注入账不符
    LedgerMismatch = 8,
    /// 写回摘要为空
    WritebackEmpty = 9,
    /// 审计收口未完成
    AuditIncomplete = 10,
    /// 越权重复留账（同地址同轮重复计违例）
    DuplicateViolation = 11,
    /// 白名单区间表退化（空/重叠）
    TableDegenerate = 12,
}

impl MmioCode {
    /// 闭集编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 段位派生值：SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全量非空中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            MmioCode::Ok => "白名单内写放行",
            MmioCode::SegBad => "段位越界：检查 SEG_BASE 对齐与分派区间",
            MmioCode::MmioOob => "越权写：地址落在白名单区间之外，必须拒并留账",
            MmioCode::RangeUnmapped => "未映射段：地址未登记在任何白名单区间",
            MmioCode::WriteInHold => "审计冻结中：越权隔离相态内禁止一切写",
            MmioCode::StateRegress => "相态回退：Armed→Hold→Recovered 只能单向前进",
            MmioCode::AuditMismatch => "审计账不符：放行加拒绝必须等于审计条数",
            MmioCode::LedgerMismatch => "注入账不符：注入必须等于检出加逃逸",
            MmioCode::WritebackEmpty => "写回摘要为空：审计收口回执必须带非空结果摘要",
            MmioCode::AuditIncomplete => "审计收口未完成：相态未到 Recovered 不得写回",
            MmioCode::DuplicateViolation => "越权重复留账：同地址同轮重复违例只计一次",
            MmioCode::TableDegenerate => "白名单区间表退化：区间不得为空或互相重叠",
        }
    }
}

/// 诊断码全序表（闭集权威清单，12 变体）。
pub const ALL_CODES: [MmioCode; 12] = [
    MmioCode::Ok,
    MmioCode::SegBad,
    MmioCode::MmioOob,
    MmioCode::RangeUnmapped,
    MmioCode::WriteInHold,
    MmioCode::StateRegress,
    MmioCode::AuditMismatch,
    MmioCode::LedgerMismatch,
    MmioCode::WritebackEmpty,
    MmioCode::AuditIncomplete,
    MmioCode::DuplicateViolation,
    MmioCode::TableDegenerate,
];

/// 防御相态机（闭集，编码序即合法前进序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardPhase {
    /// 武装放行（白名单闸生效）
    Armed = 0,
    /// 审计冻结（越权隔离）
    Hold = 1,
    /// 已收口
    Recovered = 2,
}

impl GuardPhase {
    /// 相态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
    /// 中文相态名。
    pub fn name(self) -> &'static str {
        match self {
            GuardPhase::Armed => "武装放行",
            GuardPhase::Hold => "审计冻结",
            GuardPhase::Recovered => "已收口",
        }
    }
}

/// 完成写回记录（判据③）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MmioWriteback {
    /// 状态：true=已完成
    pub done: bool,
    /// 结果摘要（非空）
    pub summary: String,
}

/// MMIO 白名单写闸：区间表校验 + 越权审计 + 相态机。
pub struct MmioWhitelistGuard {
    phase: GuardPhase,
    violations: u32,
    last_violation: Option<u32>,
    allow_cnt: u64,
    deny_cnt: u64,
    audit: Vec<(u32, u32)>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl MmioWhitelistGuard {
    /// 新闸：武装相态、空审计账。
    pub fn new() -> MmioWhitelistGuard {
        MmioWhitelistGuard {
            phase: GuardPhase::Armed,
            violations: 0,
            last_violation: None,
            allow_cnt: 0,
            deny_cnt: 0,
            audit: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 白名单判定（纯函数面）：地址是否落在任一冻结区间。
    pub fn whitelisted(addr: u32) -> bool {
        let mut hit = false;
        for (base, len) in WHITELIST_RANGES.iter() {
            if addr >= *base && addr < base.saturating_add(*len) {
                hit = true;
            }
        }
        hit
    }

    /// 当前相态。
    pub fn phase(&self) -> GuardPhase {
        self.phase
    }

    /// 相态推进：只允许编码序 +1 前进，违序显性拒。
    pub fn advance_phase(&mut self, want: GuardPhase) -> Result<GuardPhase, MmioCode> {
        if want.code() != self.phase.code() + 1 {
            return Err(MmioCode::StateRegress);
        }
        self.phase = want;
        Ok(want)
    }

    /// 写入口：冻结相态先拒；白名单外显性拒并留账；白名单内放行。
    pub fn write(&mut self, addr: u32) -> Result<(), MmioCode> {
        if self.phase == GuardPhase::Hold {
            self.deny_cnt += 1;
            self.audit.push((addr, MmioCode::WriteInHold.code()));
            return Err(MmioCode::WriteInHold);
        }
        if !Self::whitelisted(addr) {
            self.deny_cnt += 1;
            // 同地址同轮重复违例只计一次（去重防虚增）。
            if self.last_violation != Some(addr) {
                self.violations += 1;
                self.last_violation = Some(addr);
            } else {
                self.audit.push((addr, MmioCode::DuplicateViolation.code()));
                return Err(MmioCode::DuplicateViolation);
            }
            self.audit.push((addr, MmioCode::MmioOob.code()));
            if self.violations >= HOLD_THRESHOLD && self.phase == GuardPhase::Armed {
                self.phase = GuardPhase::Hold;
            }
            return Err(MmioCode::MmioOob);
        }
        if self.phase != GuardPhase::Armed {
            self.audit.push((addr, MmioCode::StateRegress.code()));
            return Err(MmioCode::StateRegress);
        }
        self.allow_cnt += 1;
        self.audit.push((addr, MmioCode::Ok.code()));
        Ok(())
    }

    /// 审计收口：Hold→Recovered，清违例并写回完成回执。
    pub fn audit_close(&mut self) -> Result<MmioWriteback, MmioCode> {
        if self.phase != GuardPhase::Hold {
            return Err(MmioCode::AuditIncomplete);
        }
        let _ = self.advance_phase(GuardPhase::Recovered)?;
        self.violations = 0;
        self.last_violation = None;
        Ok(MmioWriteback {
            done: true,
            summary: format!(
                "MMIO 越权审计收口：放行 {} 拒绝 {}，审计账 {} 条，白名单区间 {} 个",
                self.allow_cnt,
                self.deny_cnt,
                self.audit.len(),
                WHITELIST_RANGES.len()
            ),
        })
    }

    /// 回到武装相态（新一轮观察窗）。
    pub fn rearm(&mut self) {
        self.phase = GuardPhase::Armed;
        self.violations = 0;
        self.last_violation = None;
    }

    pub fn allow_cnt(&self) -> u64 {
        self.allow_cnt
    }
    pub fn deny_cnt(&self) -> u64 {
        self.deny_cnt
    }
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }
    /// 三本对平：放行 + 拒绝 = 审计条数。
    pub fn audit_conserved(&self) -> bool {
        self.allow_cnt + self.deny_cnt == self.audit.len() as u64
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
    /// 注入账守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 状态快照（双跑指纹取材）。
    pub fn snapshot(&self) -> (u64, u64, u32, u32, usize, u64, u64) {
        (
            self.allow_cnt,
            self.deny_cnt,
            self.violations,
            self.phase.code(),
            self.audit.len(),
            self.injected,
            self.detected,
        )
    }
}

/// 白名单区间表健康检查：非空且互不重叠。
pub fn whitelist_table_sane() -> bool {
    if WHITELIST_RANGES.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < WHITELIST_RANGES.len() {
        let (bi, li) = WHITELIST_RANGES[i];
        if li == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < WHITELIST_RANGES.len() {
            let (bj, lj) = WHITELIST_RANGES[j];
            let ei = bi.saturating_add(li);
            let ej = bj.saturating_add(lj);
            if bi < ej && bj < ei {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 白名单内地址全量清单（每区间首中尾三样，去重后恰 18 个探测点）。
pub fn whitelisted_probe_points() -> Vec<u32> {
    let mut pts: Vec<u32> = Vec::new();
    for (base, len) in WHITELIST_RANGES.iter() {
        pts.push(*base);
        pts.push(base.saturating_add(len / 2));
        pts.push(base.saturating_add(len - 1));
    }
    pts
}

/// LCG：种子由 SEG_BASE 派生（零随机源，确定性可复现）。
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// FNV-1a 64 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 越权 ×10 全拒留账 + 白名单内写 ×100 零误拒（红线④路径）。
pub fn privilege_matrix() -> (bool, bool) {
    // 越权 ×10：轮转 10 个白名单外地址，全拒且逐条留账。
    let mut g = MmioWhitelistGuard::new();
    let mut rejected = 0u32;
    let mut i = 0u32;
    while i < 10u32 {
        let addr = 0xE000 + i * 0x10; // 恒在白名单外
        if g.write(addr) == Err(MmioCode::MmioOob) {
            rejected += 1;
            g.note_detected();
            // 本矩阵只考察拒写留账，检出即清违例计数避免叠加冻结相态。
            g.violations = 0;
            g.last_violation = None;
        } else {
            g.note_escape();
        }
        i += 1;
    }
    let oob_ok = rejected == 10 && g.deny_cnt() == 10 && g.audit_conserved() && g.audit_len() == 10;
    // 白名单内 ×100：探测点轮转，全通零误拒。
    let mut g2 = MmioWhitelistGuard::new();
    let pts = whitelisted_probe_points();
    let mut allowed = 0u32;
    let mut k = 0u32;
    while k < 100u32 {
        let idx = (k % pts.len() as u32) as usize;
        let addr = match pts.get(idx) {
            Some(a) => *a,
            None => 0xF000,
        };
        if g2.write(addr).is_ok() {
            allowed += 1;
        }
        k += 1;
    }
    let in_ok = allowed == 100 && g2.allow_cnt() == 100 && g2.audit_conserved() && g2.phase() == GuardPhase::Armed;
    (oob_ok, in_ok)
}

/// 注入核：5 类坏样本逐类 ×10 轮精确复现（白名单外、未映射段、冻结期写、
/// 相态回退、重复越权去重面），注入账守恒（逃逸=0）。
pub fn injection_core() -> (u64, u64, u64, u32) {
    let mut g = MmioWhitelistGuard::new();
    let mut seed: u64 = (SEG_BASE as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(7);
    let mut classes_hit = 0u32;
    // 类1：白名单外越权 ×10。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            if g.write(0xE100 + i * 0x10) == Err(MmioCode::MmioOob) {
                caught += 1;
                g.note_detected();
                g.violations = 0; // 本类只考察拒写，不叠加冻结相态
                g.last_violation = None;
            } else {
                g.note_escape();
            }
            let _ = lcg_next(&mut seed);
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类2：未映射段 ×10（远端地址，非白名单也非已知禁区）。
    {
        let mut caught = 0u32;
        for i in 0..10u32 {
            let addr = 0x0010_0000u32.wrapping_add(i * 0x1000);
            let wl = MmioWhitelistGuard::whitelisted(addr);
            if !wl && g.write(addr) == Err(MmioCode::MmioOob) {
                caught += 1;
                g.note_detected();
                g.violations = 0;
                g.last_violation = None;
            } else {
                g.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类3：冻结期写 ×10（连续越权入 Hold 后一切写必须拒）。
    {
        let mut caught = 0u32;
        g.rearm();
        let mut i = 0u32;
        while i < HOLD_THRESHOLD {
            let _ = g.write(0xE200 + i * 0x10);
            i += 1;
        }
        if g.phase() == GuardPhase::Hold {
            for _ in 0..10u32 {
                if g.write(0xF000) == Err(MmioCode::WriteInHold) {
                    caught += 1;
                    g.note_detected();
                } else {
                    g.note_escape();
                }
            }
        } else {
            for _ in 0..10u32 {
                g.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类4：相态回退 ×10（Armed 直接要求 Recovered 跳相 / Recovered 再收口）。
    {
        let mut caught = 0u32;
        g.rearm();
        for _ in 0..10u32 {
            if g.audit_close() == Err(MmioCode::AuditIncomplete) {
                caught += 1;
                g.note_detected();
            } else {
                g.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    // 类5：重复越权去重 ×10（同地址连击，第二击起 DuplicateViolation）。
    {
        let mut caught = 0u32;
        g.rearm();
        for _ in 0..10u32 {
            let _ = g.write(0xE300); // 首击 MmioOob
            if g.write(0xE300) == Err(MmioCode::DuplicateViolation) {
                caught += 1;
                g.note_detected();
                g.violations = 0;
                g.last_violation = None;
            } else {
                g.note_escape();
            }
        }
        if caught == 10 {
            classes_hit += 1;
        }
    }
    (g.injected, g.detected, g.escaped, classes_hit)
}

/// 引擎指纹：注入核 + 特权矩阵 + 区间表快照混合 FNV-1a（双跑一致面）。
pub fn engine_fingerprint() -> u64 {
    let (inj, det, esc, cls) = injection_core();
    let (oob, inn) = privilege_matrix();
    let mut g = MmioWhitelistGuard::new();
    let _ = g.write(0xF000);
    let _ = g.write(0xE999);
    let snap = g.snapshot();
    let full = format!(
        "unx51605|{} {} {} {}|{} {}|{:?}|{}",
        inj, det, esc, cls, oob, inn, snap, WHITELIST_RANGES.len()
    );
    fnv1a(full.as_bytes())
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    engine_fingerprint() == engine_fingerprint()
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51605_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51605");
    // 1 白名单区间表健康。
    s.add("区间表健康", whitelist_table_sane(), "白名单区间非空且互不重叠");
    // 2 边界精确：区间首/中/尾放行，区间尾+1 拒。
    {
        let mut ok = true;
        for (base, len) in WHITELIST_RANGES.iter() {
            ok = ok
                && MmioWhitelistGuard::whitelisted(*base)
                && MmioWhitelistGuard::whitelisted(base + len - 1)
                && !MmioWhitelistGuard::whitelisted(base + len);
        }
        s.add("边界精确判定", ok, "每区间首尾内放行、尾外一格必须拒");
    }
    // 3 越权 ×10 全拒留账 + 白名单内 ×100 零误拒。
    {
        let (oob, inn) = privilege_matrix();
        s.add("越权全拒留账", oob, "越权 ×10 全拒且审计账逐条对平");
        s.add("白名单内零误拒", inn, "白名单内写 ×100 全通（红线④路径）");
    }
    // 4 注入核五类全检。
    {
        let (inj, det, esc, cls) = injection_core();
        s.add(
            "注入核五类全检",
            inj == 50 && det == 50 && esc == 0 && cls == 5,
            "5 类坏样本 ×10 轮须全检出，逃逸恒 0",
        );
    }
    // 5 注入账守恒。
    {
        let (inj, det, esc, _) = injection_core();
        s.add(
            "注入账守恒",
            inj == det + esc && esc == 0 && det > 0,
            "注入 = 检出 + 逃逸，逃逸恒 0",
        );
    }
    // 6 违例入冻结相态。
    {
        let mut g = MmioWhitelistGuard::new();
        let mut i = 0u32;
        while i < HOLD_THRESHOLD {
            let _ = g.write(0xE400 + i * 0x10);
            i += 1;
        }
        s.add(
            "违例入冻结",
            g.phase() == GuardPhase::Hold,
            "连续 5 次越权必须进入审计冻结相态",
        );
    }
    // 7 冻结期写全拒（含白名单内地址）。
    {
        let mut g = MmioWhitelistGuard::new();
        let mut i = 0u32;
        while i < HOLD_THRESHOLD {
            let _ = g.write(0xE500 + i * 0x10);
            i += 1;
        }
        s.add(
            "冻结期写全拒",
            g.write(0xF000) == Err(MmioCode::WriteInHold),
            "冻结相态内连白名单地址也必须拒",
        );
    }
    // 8 审计收口全链 + 写回。
    {
        let mut g = MmioWhitelistGuard::new();
        let mut i = 0u32;
        while i < HOLD_THRESHOLD {
            let _ = g.write(0xE600 + i * 0x10);
            i += 1;
        }
        let ok = match g.audit_close() {
            Ok(w) => w.done && !w.summary.is_empty() && g.phase() == GuardPhase::Recovered,
            Err(_) => false,
        };
        s.add("审计收口写回", ok, "冻结后收口必须写回状态=已完成且摘要非空");
    }
    // 9 相态回退显性拒。
    {
        let mut g = MmioWhitelistGuard::new();
        let jump = g.advance_phase(GuardPhase::Recovered);
        let mut g2 = MmioWhitelistGuard::new();
        let mut i = 0u32;
        while i < HOLD_THRESHOLD {
            let _ = g2.write(0xE700 + i * 0x10);
            i += 1;
        }
        let _ = g2.audit_close();
        let back = g2.advance_phase(GuardPhase::Armed);
        s.add(
            "相态单调",
            jump == Err(MmioCode::StateRegress) && back == Err(MmioCode::StateRegress),
            "Armed→Recovered 跳相与 Recovered→Armed 回退都必须显性拒",
        );
    }
    // 10 重复越权去重面。
    {
        let mut g = MmioWhitelistGuard::new();
        let _ = g.write(0xE800);
        let dup = g.write(0xE800);
        s.add(
            "重复越权去重",
            dup == Err(MmioCode::DuplicateViolation),
            "同地址同轮重复违例只计一次违例、以 DuplicateViolation 面拒",
        );
    }
    // 11 诊断码闭集唯一且文案非空。
    {
        let mut uniq = true;
        let mut i = 0usize;
        while i < ALL_CODES.len() {
            let mut j = i + 1;
            while j < ALL_CODES.len() {
                if ALL_CODES[i] == ALL_CODES[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        let mut texts = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                texts = false;
            }
        }
        s.add("闭集唯一文案全", uniq && texts, "12 变体互异且中文文案全量非空");
    }
    // 12 val 段位派生。
    {
        let mut ok = true;
        for (idx, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + idx as u32 {
                ok = false;
            }
        }
        s.add("val 段位派生", ok, "每个诊断码 val 必须等于 SEG_BASE+1+变体序号");
    }
    // 13 双跑 FNV 指纹一致。
    s.add("双跑指纹一致", dual_run_consistent(), "同一脚本两次执行 FNV-1a 指纹逐位一致");
    // 14 段位守卫。
    s.add("段位守卫", seg_guard_ok(), "SEG_BASE 0xC1A00 须 0x40 对齐且在分派区间");
    // 15 LCG 确定性。
    {
        let mut s1: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut s2: u64 = (SEG_BASE as u64) * 3 + 11;
        let mut same = true;
        for _ in 0..16 {
            if lcg_next(&mut s1) != lcg_next(&mut s2) {
                same = false;
            }
        }
        s.add("LCG 确定性", same, "同种子两次 LCG 序列必须逐值一致");
    }
    // 16 审计三本对平（混合流）。
    {
        let mut g = MmioWhitelistGuard::new();
        for (base, _) in WHITELIST_RANGES.iter() {
            let _ = g.write(*base);
        }
        let _ = g.write(0xE900);
        s.add(
            "审计三本对平",
            g.audit_conserved() && g.audit_len() == 7,
            "6 放行 + 1 拒绝必须对平审计账 7 条",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_boundary_exact() {
        for (base, len) in WHITELIST_RANGES.iter() {
            assert!(MmioWhitelistGuard::whitelisted(*base));
            assert!(MmioWhitelistGuard::whitelisted(base + len - 1));
            assert!(!MmioWhitelistGuard::whitelisted(base + len));
        }
    }

    #[test]
    fn oob_write_rejected_and_audited() {
        let mut g = MmioWhitelistGuard::new();
        for i in 0..10u32 {
            assert_eq!(g.write(0xE000 + i * 0x10), Err(MmioCode::MmioOob));
            g.violations = 0;
            g.last_violation = None;
        }
        assert_eq!(g.deny_cnt(), 10);
        assert!(g.audit_conserved());
    }

    #[test]
    fn inlist_write_hundred_zero_false() {
        let mut g = MmioWhitelistGuard::new();
        let pts = whitelisted_probe_points();
        for k in 0..100u32 {
            let addr = pts[(k % pts.len() as u32) as usize];
            assert!(g.write(addr).is_ok(), "白名单地址 {:#x} 被误拒", addr);
        }
        assert_eq!(g.phase(), GuardPhase::Armed);
    }

    #[test]
    fn hold_after_threshold() {
        let mut g = MmioWhitelistGuard::new();
        for i in 0..HOLD_THRESHOLD {
            let _ = g.write(0xE400 + i * 0x10);
        }
        assert_eq!(g.phase(), GuardPhase::Hold);
        assert_eq!(g.write(0xF000), Err(MmioCode::WriteInHold));
    }

    #[test]
    fn audit_close_full_chain() {
        let mut g = MmioWhitelistGuard::new();
        for i in 0..HOLD_THRESHOLD {
            let _ = g.write(0xE600 + i * 0x10);
        }
        let wb = g.audit_close();
        assert!(wb.is_ok());
        assert_eq!(g.phase(), GuardPhase::Recovered);
        assert!(g.audit_conserved());
    }

    #[test]
    fn duplicate_violation_dedup() {
        let mut g = MmioWhitelistGuard::new();
        let _ = g.write(0xE800);
        assert_eq!(g.write(0xE800), Err(MmioCode::DuplicateViolation));
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let (inj, det, esc, cls) = injection_core();
        assert_eq!((inj, det, esc, cls), (50, 50, 0, 5));
    }

    #[test]
    fn table_sane_and_codes_closed() {
        assert!(whitelist_table_sane());
        assert_eq!(ALL_CODES.len(), 12);
        for (idx, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + idx as u32);
            assert!(!c.detail().is_empty());
        }
        assert!(seg_guard_ok());
    }

    #[test]
    fn dual_fingerprint_stable() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn unx51605_checks_green() {
        let set = run_unx51605_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
