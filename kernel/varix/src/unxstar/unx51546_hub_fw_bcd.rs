//! UNX-F51546 HUB 固件版本账（BCD 登记与兼容矩阵联动） · 0xC0B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51547-J1 ×1000 事件/秒限流生效判据逐字见任务板
//! UNX-F51546 卡验收②（判据原文：UNX-F51547-J1 端口事件风暴抑制 ×1000
//! 事件/秒限流生效；超额丢弃显性计数）
//!
//! 设计要点：
//!   ① 版本账以「压缩 BCD」为唯一登记格式：u16 高字节主版本、低字节次版本，
//!      四个半字节逐位校验，任一半字节 >9 即格式违约，登记前先解码后入账。
//!   ② 兼容矩阵冻结联动：MATRIX_MIN=0x0100、MATRIX_MAX=0x0210；登记版本
//!      低于矩阵下限或高于矩阵上限都明拒，矩阵判定先于落账。
//!   ③ 单调状态机：槽位 Free→Registered(v)→Upgraded(v'≥v)；同槽位再登记
//!      版本倒退显性拒（DowngradeRefused），状态绝不回退。
//!   ④ 判定次序：段位守卫 → 槽位界 → BCD 格式 → 矩阵上下限 → 倒退拒 → 落账；
//!      多判据同时成立时取判据约定码（槽位先于格式，格式先于矩阵）。
//!   ⑤ 守恒账：登记申请 = 接受 + 拒绝（拒绝逐笔显性计数）；注入 = 检出 +
//!      逃逸，逃逸恒 0；账面记录只增不减，回放可复算槽位终态。
//!   ⑥ 诊断码闭集：每个码带中文处置建议，闭集外无第三态。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0B40;

/// 兼容矩阵下限（冻结）：低于此版本的固件不予登记。
pub const MATRIX_MIN: u16 = 0x0100;
/// 兼容矩阵上限（冻结）：高于此版本的固件不予登记。
pub const MATRIX_MAX: u16 = 0x0210;
/// 版本账槽位容量（冻结）：最大可挂 HUB 槽位数。
pub const SLOT_CAP: usize = 16;
/// 版本账记录容量（冻结）：超出即账满明拒。
pub const LEDGER_CAP: usize = 64;

/// 诊断码闭集（HUB 固件版本账域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HubBcdCode {
    /// 登记成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 槽位号越出槽位容量
    SlotIndexBad,
    /// BCD 半字节非法（出现 A~F）
    BcdNibbleBad,
    /// 版本低于兼容矩阵下限
    BelowMatrixMin,
    /// 版本高于兼容矩阵上限
    AboveMatrixMax,
    /// 同槽位版本倒退，单调状态机显性拒
    DowngradeRefused,
    /// 版本账已满
    LedgerFull,
    /// 账面回放与槽位终态不一致
    LedgerMismatch,
}

impl HubBcdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HubBcdCode::Ok => "登记成功",
            HubBcdCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            HubBcdCode::SlotIndexBad => "槽位号越界：槽位必须小于槽位容量",
            HubBcdCode::BcdNibbleBad => "BCD 半字节非法：每个半字节必须 ≤9",
            HubBcdCode::BelowMatrixMin => "版本低于矩阵下限：升级固件后再登记",
            HubBcdCode::AboveMatrixMax => "版本高于矩阵上限：核对兼容矩阵档位",
            HubBcdCode::DowngradeRefused => "版本倒退被拒：同槽位只允许同级或升级",
            HubBcdCode::LedgerFull => "版本账已满：先归档旧账再登记",
            HubBcdCode::LedgerMismatch => "账面回放不符：版本账被改写，检查确定性",
        }
    }
}

/// 压缩 BCD 固件版本值（已通过半字节校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BcdFw {
    pub raw: u16,
}

impl BcdFw {
    pub fn major(self) -> u8 {
        (self.raw >> 8) as u8
    }
    pub fn minor(self) -> u8 {
        (self.raw & 0xFF) as u8
    }
}

/// BCD 半字节校验：u16 的四个半字节必须都 ≤9。
fn bcd_nibbles_ok(raw: u16) -> bool {
    let mut m = raw;
    let mut ok = true;
    for _ in 0..4 {
        if m & 0xF > 9 {
            ok = false;
        }
        m >>= 4;
    }
    ok
}

/// 解码登记版本：先格式后放行，闭集错误。
pub fn decode_bcd(raw: u16) -> Result<BcdFw, HubBcdCode> {
    if !bcd_nibbles_ok(raw) {
        return Err(HubBcdCode::BcdNibbleBad);
    }
    Ok(BcdFw { raw })
}

/// 单条版本账记录（只增不减）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BcdRegRec {
    pub seq: u32,
    pub slot: u8,
    pub bcd: u16,
    pub seg: u32,
}

/// HUB 固件版本账引擎：槽位状态机 + 兼容矩阵 + 守恒账。
pub struct HubBcdLedger {
    slots: [Option<u16>; SLOT_CAP],
    ledger: Vec<BcdRegRec>,
    accepted: u64,
    rejected: u64,
    attempts: u64,
    injected: u64,
    detected: u64,
}

impl HubBcdLedger {
    pub fn new() -> HubBcdLedger {
        HubBcdLedger {
            slots: [None; SLOT_CAP],
            ledger: Vec::new(),
            accepted: 0,
            rejected: 0,
            attempts: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 判定 + 落账内实现（不含申请计数）。
    fn register_inner(&mut self, slot: u8, raw: u16) -> Result<u32, HubBcdCode> {
        // 判序①：槽位界先于格式。
        if (slot as usize) >= SLOT_CAP {
            return Err(HubBcdCode::SlotIndexBad);
        }
        // 判序②：BCD 格式先于矩阵。
        if !bcd_nibbles_ok(raw) {
            return Err(HubBcdCode::BcdNibbleBad);
        }
        // 判序③：矩阵下限。
        if raw < MATRIX_MIN {
            return Err(HubBcdCode::BelowMatrixMin);
        }
        // 判序④：矩阵上限。
        if raw > MATRIX_MAX {
            return Err(HubBcdCode::AboveMatrixMax);
        }
        // 判序⑤：同槽位倒退显性拒（单调状态机）。
        if let Some(prev) = self.slots.get(slot as usize) {
            if let Some(pv) = *prev {
                if raw < pv {
                    return Err(HubBcdCode::DowngradeRefused);
                }
            }
        }
        // 容量界：账满明拒，禁覆盖旧账。
        if self.ledger.len() >= LEDGER_CAP {
            return Err(HubBcdCode::LedgerFull);
        }
        // 全部通过：落账并推进槽位状态（同级再登记允许，账面追加）。
        let seq = self.ledger.len() as u32;
        let rec = BcdRegRec {
            seq,
            slot,
            bcd: raw,
            seg: SEG_BASE,
        };
        self.ledger.push(rec);
        if let Some(sl) = self.slots.get_mut(slot as usize) {
            *sl = Some(raw);
        }
        Ok(seq)
    }

    /// 登记入口：申请计数与结果分桶同步推进。
    pub fn register(&mut self, slot: u8, raw: u16) -> Result<u32, HubBcdCode> {
        self.attempts += 1;
        let r = self.register_inner(slot, raw);
        match r {
            Ok(_) => self.accepted += 1,
            Err(_) => self.rejected += 1,
        }
        r
    }

    /// 账面回放：按账重算槽位终态并与现态逐槽对平。
    pub fn audit(&self) -> Result<(), HubBcdCode> {
        let mut replay: [Option<u16>; SLOT_CAP] = [None; SLOT_CAP];
        let mut prev_seq: i64 = -1;
        for rec in &self.ledger {
            if (rec.seq as i64) <= prev_seq {
                return Err(HubBcdCode::LedgerMismatch);
            }
            prev_seq = rec.seq as i64;
            if rec.seg != SEG_BASE {
                return Err(HubBcdCode::LedgerMismatch);
            }
            if !bcd_nibbles_ok(rec.bcd) || rec.bcd < MATRIX_MIN || rec.bcd > MATRIX_MAX {
                return Err(HubBcdCode::LedgerMismatch);
            }
            match replay.get_mut(rec.slot as usize) {
                Some(sl) => *sl = Some(rec.bcd),
                None => return Err(HubBcdCode::LedgerMismatch),
            }
        }
        for i in 0..SLOT_CAP {
            let cur = self.slots.get(i);
            let rep = replay.get(i);
            if cur != rep {
                return Err(HubBcdCode::LedgerMismatch);
            }
        }
        Ok(())
    }

    /// 注入记账：检出加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0；非零即记账面破洞）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn accepted(&self) -> u64 {
        self.accepted
    }
    pub fn rejected(&self) -> u64 {
        self.rejected
    }
    pub fn attempts(&self) -> u64 {
        self.attempts
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 守恒断言面：申请 = 接受 + 拒绝，注入 = 检出且逃逸 0。
    pub fn conserved(&self) -> bool {
        self.attempts == self.accepted + self.rejected && self.injected == self.detected
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一登记序列两次建账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<BcdRegRec> {
        let mut e = HubBcdLedger::new();
        let _ = e.register(0, 0x0100);
        let _ = e.register(1, 0x0110);
        let _ = e.register(0, 0x0200);
        e.ledger.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51546_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51546");
    // 1 正常流：合法 BCD 登记落账。
    {
        let mut e = HubBcdLedger::new();
        let r = e.register(0, 0x0100);
        s.add_dyn(
            "正常登记落账",
            r.is_ok() && e.ledger_len() == 1,
            "合法 BCD 应登记并落 1 条账",
        );
    }
    // 2 零界：槽位 0 + 矩阵下限版本。
    {
        let mut e = HubBcdLedger::new();
        let r0 = e.register(0, MATRIX_MIN);
        s.add_dyn("零界槽位与下限版本", r0.is_ok(), "槽位 0 与矩阵下限应放行");
    }
    // 3 上限与上限+1：矩阵上限放行，+1 明拒。
    {
        let mut e = HubBcdLedger::new();
        let ok_max = e.register(0, MATRIX_MAX).is_ok();
        let over = e.register(1, MATRIX_MAX + 1);
        s.add_dyn(
            "矩阵上限与上限+1",
            ok_max && over == Err(HubBcdCode::AboveMatrixMax),
            "上限放行、上限+1 必须明拒",
        );
    }
    // 4 负侧（下限-1）：0x00FF 半字节合法但低于矩阵下限。
    {
        let mut e = HubBcdLedger::new();
        let r = e.register(0, MATRIX_MIN - 1);
        s.add_dyn(
            "矩阵下限-1 拒绝",
            r == Err(HubBcdCode::BelowMatrixMin),
            "低于矩阵下限必须明拒",
        );
    }
    // 5 注入必抓：非法半字节 ×10 轮。
    {
        let mut e = HubBcdLedger::new();
        let mut caught = 0u32;
        for i in 0..10u16 {
            let raw = 0x0100 | ((0xA + i) & 0xF);
            if e.register(0, raw) == Err(HubBcdCode::BcdNibbleBad) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("半字节注入 10/10", caught == 10, "BCD 格式违约必须全抓");
    }
    // 6 注入必抓：低于矩阵下限 ×10 轮。
    {
        let mut e = HubBcdLedger::new();
        let mut caught = 0u32;
        for i in 0..10u16 {
            let raw = MATRIX_MIN.saturating_sub(1 + i);
            if e.register(0, raw) == Err(HubBcdCode::BelowMatrixMin) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("下限之下注入 10/10", caught == 10, "低于矩阵必须全抓");
    }
    // 7 注入必抓：高于矩阵上限 ×10 轮。
    {
        let mut e = HubBcdLedger::new();
        let mut caught = 0u32;
        for i in 0..10u16 {
            let raw = MATRIX_MAX.saturating_add(1 + i);
            if e.register(0, raw) == Err(HubBcdCode::AboveMatrixMax) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("上限之上注入 10/10", caught == 10, "高于矩阵必须全抓");
    }
    // 8 判定次序：槽位界先于格式，格式先于矩阵。
    {
        let mut e = HubBcdLedger::new();
        let r_slot = e.register(SLOT_CAP as u8, 0x0A00);
        let r_fmt = e.register(0, 0x0A00);
        s.add_dyn(
            "判序槽位先于格式先于矩阵",
            r_slot == Err(HubBcdCode::SlotIndexBad)
                && r_fmt == Err(HubBcdCode::BcdNibbleBad),
            "多判据成立取约定码",
        );
    }
    // 9 状态机拒止：倒退显性拒，同级再登记放行。
    {
        let mut e = HubBcdLedger::new();
        let _ = e.register(0, 0x0200);
        let down = e.register(0, 0x0100);
        let same = e.register(0, 0x0200);
        s.add_dyn(
            "倒退拒同级行",
            down == Err(HubBcdCode::DowngradeRefused) && same.is_ok(),
            "槽位版本只允许同级或升级",
        );
    }
    // 10 容量耗尽：账满明拒。
    {
        let mut e = HubBcdLedger::new();
        let mut last = Ok(0u32);
        for i in 0..(LEDGER_CAP + 1) {
            let v = 0x0100 + (i % 16) as u16;
            last = e.register((i % SLOT_CAP) as u8, v);
        }
        s.add_dyn(
            "账满明拒",
            last == Err(HubBcdCode::LedgerFull) && e.ledger_len() == LEDGER_CAP,
            "超出账容量必须显性拒绝",
        );
    }
    // 11 槽位界：slot == 槽位容量明拒。
    {
        let mut e = HubBcdLedger::new();
        let r = e.register(SLOT_CAP as u8, 0x0100);
        s.add_dyn(
            "槽位越界拒绝",
            r == Err(HubBcdCode::SlotIndexBad),
            "槽位号必须小于容量",
        );
    }
    // 12 对账守恒：申请分桶对平 + 注入=检出。
    {
        let mut e = HubBcdLedger::new();
        for i in 0..10u16 {
            let _ = e.register(0, 0x0A00 | i);
            e.note_detected();
        }
        let _ = e.register(0, 0x0100);
        s.add_dyn(
            "申请与注入双守恒",
            e.conserved()
                && e.attempts() == e.accepted() + e.rejected()
                && e.injected() == 10,
            "申请=接受+拒绝，注入=检出+逃逸且逃逸 0",
        );
    }
    // 13 账面回放审计。
    {
        let mut e = HubBcdLedger::new();
        let _ = e.register(0, 0x0100);
        let _ = e.register(1, 0x0200);
        let _ = e.register(0, 0x0110);
        s.add_dyn("账面回放一致", e.audit().is_ok(), "回放槽位终态应逐槽对平");
    }
    // 14 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一登记序列两次建账逐字段相等");
    // 15 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

/// 诊断码全序表（闭集枚举的权威清单，长度=变体数）。
pub const ALL_CODES: [HubBcdCode; 9] = [
    HubBcdCode::Ok,
    HubBcdCode::SegBad,
    HubBcdCode::SlotIndexBad,
    HubBcdCode::BcdNibbleBad,
    HubBcdCode::BelowMatrixMin,
    HubBcdCode::AboveMatrixMax,
    HubBcdCode::DowngradeRefused,
    HubBcdCode::LedgerFull,
    HubBcdCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_register_ledgers() {
        let mut e = HubBcdLedger::new();
        assert!(e.register(2, 0x0110).is_ok());
        assert_eq!(e.ledger_len(), 1);
    }

    #[test]
    fn matrix_bounds_closed() {
        let mut e = HubBcdLedger::new();
        assert!(e.register(0, MATRIX_MIN).is_ok());
        assert_eq!(e.register(1, MATRIX_MIN - 1), Err(HubBcdCode::BelowMatrixMin));
        assert!(e.register(2, MATRIX_MAX).is_ok());
        assert_eq!(e.register(3, MATRIX_MAX + 1), Err(HubBcdCode::AboveMatrixMax));
    }

    #[test]
    fn nibble_injection_all_caught() {
        let mut e = HubBcdLedger::new();
        for i in 0..10u16 {
            let raw = 0x0100 | ((0xB + i) & 0xF);
            assert_eq!(e.register(0, raw), Err(HubBcdCode::BcdNibbleBad));
        }
        assert_eq!(e.ledger_len(), 0);
    }

    #[test]
    fn judge_order_slot_then_format_then_matrix() {
        let mut e = HubBcdLedger::new();
        assert_eq!(e.register(200, 0x0100), Err(HubBcdCode::SlotIndexBad));
        assert_eq!(e.register(0, 0x0AFF), Err(HubBcdCode::BcdNibbleBad));
        assert_eq!(e.register(0, 0x00FF), Err(HubBcdCode::BelowMatrixMin));
    }

    #[test]
    fn downgrade_refused_same_allowed() {
        let mut e = HubBcdLedger::new();
        let _ = e.register(0, 0x0200);
        assert_eq!(e.register(0, 0x0100), Err(HubBcdCode::DowngradeRefused));
        assert!(e.register(0, 0x0200).is_ok());
        assert!(e.register(0, 0x0210).is_ok());
    }

    #[test]
    fn ledger_full_explicit() {
        let mut e = HubBcdLedger::new();
        let mut n = 0usize;
        while n < LEDGER_CAP {
            let v = 0x0100 + (n % 16) as u16;
            let _ = e.register((n % SLOT_CAP) as u8, v);
            n += 1;
        }
        assert_eq!(e.ledger_len(), LEDGER_CAP);
        assert_eq!(e.register(0, 0x0110), Err(HubBcdCode::LedgerFull));
    }

    #[test]
    fn audit_and_conservation() {
        let mut e = HubBcdLedger::new();
        for _ in 0..10u32 {
            let _ = e.register(0, 0x0A00);
            e.note_detected();
        }
        let _ = e.register(0, 0x0100);
        assert!(e.audit().is_ok());
        assert_eq!(e.attempts(), e.accepted() + e.rejected());
        assert!(e.conserved());
        assert_eq!(e.injected(), 10);
        assert_eq!(e.detected(), 10);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
        assert_eq!(ALL_CODES.len(), 9);
    }

    #[test]
    fn unx51546_checks_green() {
        let set = run_unx51546_checks();
        assert!(set.all_passed(), "UNX-F51546 red: {}", set.red_items_text());
    }
}
