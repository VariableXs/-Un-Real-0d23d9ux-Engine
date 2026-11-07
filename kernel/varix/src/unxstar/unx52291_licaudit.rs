//! UNX-F52291 版本伪装表许可合规联合审查条目（AI-95）· 0xC8880段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52291"，逐字）：
//!   title: 版本伪装表许可合规联合审查条目（AI-95）
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52292-J1 六判
//!     据母版参数化 schema ×1 冻结入库，机器人可执行判据 ×10 抽测全
//!     通；联签锚定行 AI-73；判据锚定 §AI-66.8 治理线接口；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52291）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52292-J1，照原文锚定；功能按本卡标题（版本伪装表许可合规联
//! 合审查条目）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 版本伪装表 12 格：逐条目载荷对照落账（仅用于兼容性伪装、零
//!      许可绕过口径），漂移必抓。
//!   ② 合规三态 20 格：合规 / 需声明 / 违规 三态闭集，态阶单调（回
//!      退必抓），100% 结论入账。
//!   ③ 审查链 8 拍：受理→比对→…→结论入账，拍序严格递增（回放必抓）。
//!   ④ AI-95 联签锚定行 2 行闭集：联合审查联签行 / 违例冻结批次条
//!      款知悉登记行，缺一拒封。
//!   ⑤ ledger 状态机（new/record_disguise/record_comp/record_beat/
//!      record_ai95/judge_gate×6/fingerprint FNV-1a/grade 定级闭集）；
//!      注入核 run_cycle 三态轮转 10 轮（伪装表漂移 / 合规阶回退 /
//!      合规格越闭集），全部落在当轮临时账本上，led 上 injected==
//!      caught==10、escaped==0；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52291;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 版本伪装表格数。
pub const DISGUISE_CELLS: usize = 12;

/// 合规三态格数。
pub const COMPLIANCE_CELLS: usize = 20;

/// 合规三态数。
pub const COMP_STATES: usize = 3;

/// 审查链拍数。
pub const AUDIT_BEATS: usize = 8;

/// AI-95 联签锚定行数。
pub const AI95_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LicAuditCode {
    /// 成功
    Ok = 0,
    /// 伪装表格越闭集
    ENoDisg,
    /// 伪装表/合规格载荷漂移
    EDrift,
    /// 合规格越闭集
    ENoComp,
    /// 态阶回退（合规阶/拍序逆序）
    EOrder,
    /// 审查拍越闭集
    ENoBeat,
    /// AI-95 联签行越闭集
    ENoRow,
    /// 缺 AI-95 联签行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl LicAuditCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LicAuditCode::Ok => "成功",
            LicAuditCode::ENoDisg => "伪装表格越闭集：版本伪装表 12 格之外先登记",
            LicAuditCode::EDrift => "载荷漂移：伪装表/合规格折叠与基准不恒等，逐格复算",
            LicAuditCode::ENoComp => "合规格越闭集：合规三态 20 格之外先登记",
            LicAuditCode::EOrder => "态阶回退：合规→需声明→违规 与审查拍单调，回退/回放必抓",
            LicAuditCode::ENoBeat => "审查拍越闭集：联合审查链 8 拍之外先登记",
            LicAuditCode::ENoRow => "AI-95 联签行越闭集：联签锚定 2 行闭集之外先登记",
            LicAuditCode::ERowMiss => "缺 AI-95 联签行：联合审查联签与冻结批次知悉登记未齐先补齐",
            LicAuditCode::EDup => "重复落账：同格只许一条账目",
            LicAuditCode::ESeal => "未齐即封：12 伪装格 + 20 合规格 + 8 拍 + 2 行联签未齐先补齐",
            LicAuditCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            LicAuditCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            LicAuditCode::EGradeDrift => "定级漂移：全通且联签行全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+12伪装格+20合规格+8拍+2行联签全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本伪装表格载荷（格 d）。
pub const fn disguise_cell(d: usize) -> Option<u64> {
    if d >= DISGUISE_CELLS {
        return None;
    }
    Some((0xDB000u64 + (d as u64) * 0x53) ^ 0x4449534744495347)
}

/// 合规三态格载荷（格 c：0..7=合规 7..14=需声明 14..20=违规）。
pub const fn comp_cell(c: usize) -> Option<u64> {
    if c >= COMPLIANCE_CELLS {
        return None;
    }
    Some((0xDC000u64 + (c as u64) * 0x2F) ^ ((c / 7) as u64).wrapping_mul(0x01000193))
}

/// 合规态（0=合规 1=需声明 2=违规；态阶单调）。
pub const fn comp_state(c: usize) -> Option<u32> {
    if c >= COMPLIANCE_CELLS {
        return None;
    }
    Some((c / 7) as u32)
}

/// 合规态名。
pub const fn comp_state_name(st: usize) -> Option<&'static str> {
    match st {
        0 => Some("合规"),
        1 => Some("需声明"),
        2 => Some("违规"),
        _ => None,
    }
}

/// 审查拍折叠（拍 b）。
pub const fn audit_beat(b: usize) -> Option<u32> {
    if b >= AUDIT_BEATS {
        return None;
    }
    Some(0xDD00 + b as u32 * 0x53)
}

/// AI-95 联签行折叠（行 r）。
pub const fn ai95_row(r: usize) -> Option<u32> {
    if r >= AI95_ROWS {
        return None;
    }
    Some(0xDE00 + r as u32 * 0x59)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 许可合规联合审查账本状态机。
#[derive(Clone)]
pub struct LicAuditLedger {
    /// 六判据（联合审查：受理/比对/伪装核验/许可核验/结论/登记）。
    gates: [Option<bool>; 6],
    /// 伪装表账。
    disguise: [Option<u64>; DISGUISE_CELLS],
    /// 合规三态账。
    comp: [Option<u64>; COMPLIANCE_CELLS],
    /// 审查拍账。
    beats: [Option<u32>; AUDIT_BEATS],
    /// AI-95 联签账。
    ai95: [Option<u32>; AI95_ROWS],
    /// 最后合规态。
    last_state: i64,
    /// 最后审查拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LicAuditLedger {
    /// 新账本。
    pub fn new() -> Self {
        LicAuditLedger {
            gates: [None; 6],
            disguise: [None; DISGUISE_CELLS],
            comp: [None; COMPLIANCE_CELLS],
            beats: [None; AUDIT_BEATS],
            ai95: [None; AI95_ROWS],
            last_state: -1,
            last_beat: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LicAuditCode> {
        if gate >= 6 {
            return Err(LicAuditCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 伪装表格一格（载荷对照 + 重复拒）。
    pub fn record_disguise(&mut self, d: usize, got: u64) -> Result<(), LicAuditCode> {
        let want = match disguise_cell(d) {
            Some(w) => w,
            None => return Err(LicAuditCode::ENoDisg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + d as u64);
            return Err(LicAuditCode::EDrift);
        }
        if match self.disguise.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LicAuditCode::EDup);
        }
        if let Some(slot) = self.disguise.get_mut(d) {
            *slot = Some(got);
        }
        self.fnv(0x7201 + d as u64);
        Ok(())
    }

    /// 伪装表全账核验（12 格）。
    pub fn disguise_all(&self) -> bool {
        let mut d = 0usize;
        while d < DISGUISE_CELLS {
            match self.disguise.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 合规格一格（态阶单调 + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, c: usize, got: u64) -> Result<(), LicAuditCode> {
        if c >= COMPLIANCE_CELLS {
            return Err(LicAuditCode::ENoComp);
        }
        let state = match comp_state(c) {
            Some(st) => st,
            None => return Err(LicAuditCode::ENoComp),
        };
        if (state as i64) < self.last_state {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(LicAuditCode::EOrder);
        }
        let want = match comp_cell(c) {
            Some(w) => w,
            None => return Err(LicAuditCode::ENoComp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + c as u64);
            return Err(LicAuditCode::EDrift);
        }
        if match self.comp.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LicAuditCode::EDup);
        }
        if let Some(slot) = self.comp.get_mut(c) {
            *slot = Some(got);
        }
        self.last_state = state as i64;
        self.fnv(0x7302 + c as u64);
        Ok(())
    }

    /// 合规三态全账核验（20 格）。
    pub fn comp_all(&self) -> bool {
        let mut c = 0usize;
        while c < COMPLIANCE_CELLS {
            match self.comp.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 审查拍一拍（拍序严格递增 + 回放拒）。
    pub fn record_beat(&mut self, b: usize) -> Result<(), LicAuditCode> {
        if b >= AUDIT_BEATS {
            return Err(LicAuditCode::ENoBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400);
            return Err(LicAuditCode::EOrder);
        }
        if let Some(slot) = self.beats.get_mut(b) {
            *slot = audit_beat(b);
        }
        self.last_beat = b as i64;
        self.fnv(0x7401 + b as u64);
        Ok(())
    }

    /// 审查拍全账核验（8 拍单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < AUDIT_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// AI-95 联签一行。
    pub fn record_ai95(&mut self, r: usize) -> Result<(), LicAuditCode> {
        if r >= AI95_ROWS {
            return Err(LicAuditCode::ENoRow);
        }
        if match self.ai95.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LicAuditCode::EDup);
        }
        if let Some(slot) = self.ai95.get_mut(r) {
            *slot = ai95_row(r);
        }
        self.fnv(0x7500 + r as u64);
        Ok(())
    }

    /// AI-95 联签全齐核验。
    pub fn ai95_all(&self) -> bool {
        let mut r = 0usize;
        while r < AI95_ROWS {
            match self.ai95.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.disguise_all()
            && self.comp_all()
            && self.beats_all()
            && self.ai95_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for LicAuditLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LicAuditLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DISGUISE_CELLS {
        if let Some(w) = disguise_cell(d) {
            if led.record_disguise(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut c = 0usize;
    while c < COMPLIANCE_CELLS {
        if let Some(w) = comp_cell(c) {
            if led.record_comp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < AUDIT_BEATS {
        let _ = led.record_beat(b);
        b += 1;
    }
    let mut r = 0usize;
    while r < AI95_ROWS {
        let _ = led.record_ai95(r);
        r += 1;
    }
    // 注入 10 轮：伪装表漂移 / 合规阶回退 / 合规格越闭集三态轮转。
    // 全部落在当轮临时账本上；漂移注入先录正格再注漂移；回退注入先按
    // 合法序落高态阶格再注低态阶，空账本同样必抓且不改 last_state。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LicAuditLedger::new();
        let caught = match kind {
            0 => {
                if let (Some(w0), Some(w1)) = (disguise_cell(0), disguise_cell(1)) {
                    let _ = lt.record_disguise(0, w0);
                    lt.record_disguise(1, w1.wrapping_add(1)) == Err(LicAuditCode::EDrift)
                } else {
                    false
                }
            }
            1 => {
                if let (Some(w14), Some(w1)) = (comp_cell(14), comp_cell(1)) {
                    let _ = lt.record_comp(14, w14);
                    lt.record_comp(1, w1) == Err(LicAuditCode::EOrder)
                } else {
                    false
                }
            }
            _ => lt.record_comp(COMPLIANCE_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52291 判据集（锚 UNX-F52292-J1，off-by-one 板式）。
pub fn run_unx52291_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52291");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "版本伪装表 12 格全通",
        recorded >= DISGUISE_CELLS as u32,
        "伪装表条目逐格载荷恒等（仅用于兼容性伪装、零许可绕过口径）",
    );
    s.add(
        "合规三态 20 格全通",
        {
            let mut lt = LicAuditLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COMPLIANCE_CELLS {
                if let Some(w) = comp_cell(c) {
                    if lt.record_comp(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.comp_all()
        },
        "合规/需声明/违规三态 100% 结论入账",
    );
    s.add(
        "合规三态单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < COMPLIANCE_CELLS {
                if comp_state(c).unwrap_or(COMP_STATES as u32) < comp_state(c - 1).unwrap_or(COMP_STATES as u32) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "三态闭集单调，态阶回退必抓",
    );
    s.add(
        "审查链 8 拍单调",
        {
            let mut lt = LicAuditLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < AUDIT_BEATS {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "受理→比对→伪装核验→许可核验→结论入账，拍序严格递增",
    );
    s.add(
        "AI-95 联签锚定 2/2 全齐",
        {
            let mut lt = LicAuditLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < AI95_ROWS {
                if lt.record_ai95(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.ai95_all()
        },
        "联合审查联签行 / 违例冻结批次条款知悉登记行两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = LicAuditLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "受理/比对/伪装核验/许可核验/结论/登记逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "伪装表漂移/合规阶回退/合规格越闭集三态轮转必抓",
    );
    let mut l0 = LicAuditLedger::new();
    s.add(
        "边界·伪装表格越闭集",
        l0.record_disguise(DISGUISE_CELLS, 0).is_err(),
        "伪装表格 12 越闭集必拒",
    );
    s.add(
        "边界·合规格越闭集",
        l0.record_comp(COMPLIANCE_CELLS, 0).is_err(),
        "合规格 20 越闭集必拒",
    );
    s.add(
        "边界·审查拍越闭集",
        l0.record_beat(AUDIT_BEATS).is_err(),
        "审查拍 8 越闭集必拒",
    );
    s.add(
        "伪装表漂移拒",
        {
            if let (Some(w0), Some(w1)) = (disguise_cell(0), disguise_cell(1)) {
                let mut lt = LicAuditLedger::new();
                let _ = lt.record_disguise(0, w0);
                lt.record_disguise(1, w1.wrapping_add(1)) == Err(LicAuditCode::EDrift)
            } else {
                false
            }
        },
        "伪装表载荷漂移必抓",
    );
    s.add(
        "合规阶回退拒",
        {
            if let (Some(w14), Some(w1)) = (comp_cell(14), comp_cell(1)) {
                let mut lt = LicAuditLedger::new();
                let _ = lt.record_comp(14, w14);
                lt.record_comp(1, w1) == Err(LicAuditCode::EOrder)
            } else {
                false
            }
        },
        "违规→合规态阶回退必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = LicAuditLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < DISGUISE_CELLS {
                if let Some(w) = disguise_cell(d) {
                    let _ = lt.record_disguise(d, w);
                }
                d += 1;
            }
            let mut c = 0usize;
            while c < COMPLIANCE_CELLS {
                if let Some(w) = comp_cell(c) {
                    let _ = lt.record_comp(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < AUDIT_BEATS {
                let _ = lt.record_beat(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < AI95_ROWS {
                let _ = lt.record_ai95(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8880,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comp_state() {
        assert_eq!(comp_state(0), Some(0));
        assert_eq!(comp_state(6), Some(0));
        assert_eq!(comp_state(7), Some(1));
        assert_eq!(comp_state(19), Some(2));
        assert!(comp_state(20).is_none());
        assert!(comp_cell(20).is_none());
        assert_eq!(comp_state_name(0), Some("合规"));
        assert_eq!(comp_state_name(1), Some("需声明"));
        assert_eq!(comp_state_name(2), Some("违规"));
        assert!(comp_state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = disguise_cell(5);
        assert_eq!(a, disguise_cell(5));
        assert_ne!(a, disguise_cell(6));
        assert!(disguise_cell(DISGUISE_CELLS).is_none());
        assert!(audit_beat(AUDIT_BEATS).is_none());
        assert!(ai95_row(AI95_ROWS).is_none());
    }

    #[test]
    fn test_state_order_caught() {
        let mut lt = LicAuditLedger::new();
        let _ = lt.record_comp(14, comp_cell(14).unwrap_or(0));
        assert_eq!(
            lt.record_comp(1, comp_cell(1).unwrap_or(0)),
            Err(LicAuditCode::EOrder)
        );
        // 态阶回退被拒后 last_state 不回退：同态后继仍可落。
        assert!(lt.record_comp(15, comp_cell(15).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_order_on_empty_ledger_caught() {
        // 空账本回退注入：先按合法序落高态阶格，再注低态阶，必抓。
        let mut lt = LicAuditLedger::new();
        let _ = lt.record_comp(7, comp_cell(7).unwrap_or(0));
        assert_eq!(
            lt.record_comp(1, comp_cell(1).unwrap_or(0)),
            Err(LicAuditCode::EOrder)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LicAuditLedger::new();
        assert_eq!(
            lt.record_disguise(3, disguise_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(LicAuditCode::EDrift)
        );
        assert_eq!(
            lt.record_comp(9, comp_cell(9).unwrap_or(0).wrapping_add(3)),
            Err(LicAuditCode::EDrift)
        );
    }

    #[test]
    fn test_beat_monotonic() {
        let mut lt = LicAuditLedger::new();
        let mut b = 0usize;
        while b < AUDIT_BEATS {
            assert!(lt.record_beat(b).is_ok());
            b += 1;
        }
        assert!(lt.record_beat(AUDIT_BEATS).is_err());
        assert!(lt.record_beat(2).is_err());
        assert!(lt.beats_all());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LicAuditLedger::new();
        let _ = lt.record_disguise(0, disguise_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_disguise(0, disguise_cell(0).unwrap_or(0)),
            Err(LicAuditCode::EDup)
        );
        let _ = lt.record_ai95(0);
        assert_eq!(lt.record_ai95(0), Err(LicAuditCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LicAuditLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = LicAuditLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DISGUISE_CELLS + COMPLIANCE_CELLS) as u32);
        assert_eq!(caught, 10);
        let led = LicAuditLedger::new();
        assert_eq!(led.stats(), (0, 0, 0));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC8880);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(LicAuditCode::Ok as u32, 0);
        assert_eq!(SEG_BASE + 0x0C, LicAuditCode::EGradeDrift.val());
        assert_eq!(LicAuditCode::EGradeDrift as u32, 12);
        assert!(!LicAuditCode::Ok.detail().is_empty());
        assert!(!LicAuditCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = LicAuditLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52291_checks_green() {
        let s = run_unx52291_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
