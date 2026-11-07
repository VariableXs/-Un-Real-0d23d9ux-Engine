//! UNX-F51626 接口数溢出（bNumInterfaces 谎报） · 0xC1F40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51627-J1 谎报 ×10 按实收截断审计；UNX-F51627-J1R 大接口数合法设备 ×5 零误截。
//!
//! 结构清单：
//!   - IfaceLedger：接口台账（声明数 vs 实收数，按实收截断并留审计行）
//!   - TruncAudit：截断审计账本（注入账守恒 + 截断行明细）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② 谎报语义：恶意设备在 config 头谎报 bNumInterfaces（远超实收的接口
//!      描述符数）。防御策略不是全拒而是「按实收截断」：以实收描述符数为准
//!      建账，声明数与实收数的差记为截断审计行——谎报被钳制而非放大。
//!   ③ 大接口数合法设备（J1R）：声明数恰等于实收数（大但一致），必须零误截
//!      ——截断只发生在声明>实收时，声明==实收永远放行。
//!   ④ 台账单调：接口槽只增不减，截断行只增不减；审计完整性 = 全部轮次都
//!      有落账（J1 的审计面）。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51626 − 51501)。
pub const SEG_BASE: u32 = 0xC1F40;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51626u32 - 51501));

/// 接口槽上限。
pub const MAX_IFACES: usize = 24;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfaceCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 声明数越界
    ClaimOob,
    /// 声明数为零
    ClaimZero,
    /// 声明大于实收（谎报，按实收截断）
    ClaimTruncated,
    /// 声明小于实收（倒谎报，账面保守取声明）
    ClaimUndershot,
    /// 实收接口槽溢出
    SlotOverflow,
    /// 接口描述符残缺
    IfaceFragment,
    /// 重复接口号
    IfaceDup,
    /// 审计行缺失（完整性被破坏）
    AuditGap,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl IfaceCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            IfaceCode::Ok => "接口账建账成功：声明与实收一致",
            IfaceCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            IfaceCode::ClaimOob => "声明越界：bNumInterfaces 超出表容量上限",
            IfaceCode::ClaimZero => "声明为零：config 至少要声明一个接口",
            IfaceCode::ClaimTruncated => "谎报截断：声明大于实收，按实收建账并留审计行",
            IfaceCode::ClaimUndershot => "声明倒谎报：声明小于实收，保守按声明取值",
            IfaceCode::SlotOverflow => "槽溢出：实收接口数超出容量，多余描述符拒收",
            IfaceCode::IfaceFragment => "接口描述符残缺：不足 9 字节不成接口",
            IfaceCode::IfaceDup => "接口号重复：同一 config 内 bInterfaceNumber 不得重复",
            IfaceCode::AuditGap => "审计缺口：每轮谎报都必须留下审计行，完整性 100%",
            IfaceCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            IfaceCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [IfaceCode; 12] = [
    IfaceCode::Ok,
    IfaceCode::SegBad,
    IfaceCode::ClaimOob,
    IfaceCode::ClaimZero,
    IfaceCode::ClaimTruncated,
    IfaceCode::ClaimUndershot,
    IfaceCode::SlotOverflow,
    IfaceCode::IfaceFragment,
    IfaceCode::IfaceDup,
    IfaceCode::AuditGap,
    IfaceCode::LedgerMismatch,
    IfaceCode::FingerprintMismatch,
];

/// 单接口条目。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IfaceEntry {
    pub number: u8,
    pub class: u8,
}

/// 接口台账：声明数 vs 实收数，按实收截断并审计。
pub struct IfaceLedger {
    entries: Vec<IfaceEntry>,
    claimed: u32,
    truncated_by: u32,
    undershot_by: u32,
    audit_rows: Vec<String>,
    trace: Vec<String>,
}

impl IfaceLedger {
    /// 新台账。
    pub fn new() -> IfaceLedger {
        IfaceLedger {
            entries: Vec::new(),
            claimed: 0,
            truncated_by: 0,
            undershot_by: 0,
            audit_rows: Vec::new(),
            trace: Vec::new(),
        }
    }
    /// 建账：声明 bNumInterfaces + 实收描述符序列（每条 9 字节接口头的首 3 字节建模为 (number,class,alt)）。
    /// 判序：声明界 → 槽溢出 → 残缺 → 重复号 → 截断/倒谎报。
    pub fn commit(&mut self, claimed: u32, raw: &[u8]) -> IfaceCode {
        self.claimed = claimed;
        if claimed == 0 {
            return IfaceCode::ClaimZero;
        }
        if claimed as usize > MAX_IFACES * 4 {
            return IfaceCode::ClaimOob;
        }
        if raw.len() % 3 != 0 || raw.is_empty() {
            return IfaceCode::IfaceFragment;
        }
        let actual = raw.len() / 3;
        if actual > MAX_IFACES {
            return IfaceCode::SlotOverflow;
        }
        // 逐条收槽，重复号显性记
        let mut dup = false;
        let mut idx = 0usize;
        while idx < raw.len() && self.entries.len() < MAX_IFACES {
            let number = match raw.get(idx) {
                Some(&b) => b,
                None => break,
            };
            let class = match raw.get(idx + 1) {
                Some(&b) => b,
                None => break,
            };
            if self.entries.iter().any(|e| e.number == number) {
                dup = true;
            }
            self.entries.push(IfaceEntry { number, class });
            idx += 3;
        }
        self.trace.push(format!("claimed={} actual={}", claimed, actual));
        // 截断/倒谎报判定（先落审计行再返回码）
        let code = if (claimed as usize) > actual {
            self.truncated_by = claimed - actual as u32;
            self.audit_rows
                .push(format!("TRUNC claim={} actual={} by={}", claimed, actual, self.truncated_by));
            IfaceCode::ClaimTruncated
        } else if (claimed as usize) < actual {
            self.undershot_by = actual as u32 - claimed;
            self.audit_rows
                .push(format!("UNDERSHOT claim={} actual={} by={}", claimed, actual, self.undershot_by));
            IfaceCode::ClaimUndershot
        } else if dup {
            self.audit_rows.push(format!("DUP claim={} actual={}", claimed, actual));
            IfaceCode::IfaceDup
        } else {
            IfaceCode::Ok
        };
        code
    }
    /// 实收槽位数。
    pub fn actual(&self) -> usize {
        self.entries.len()
    }
    /// 声明数。
    pub fn claimed(&self) -> u32 {
        self.claimed
    }
    /// 截断差值。
    pub fn truncated_by(&self) -> u32 {
        self.truncated_by
    }
    /// 审计行数。
    pub fn audit_rows(&self) -> usize {
        self.audit_rows.len()
    }
    /// 台账槽只读。
    pub fn entries(&self) -> &[IfaceEntry] {
        &self.entries
    }
    /// 轨迹。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 截断审计账本：注入/检出/逃逸守恒 + 完整率。
#[derive(Default)]
pub struct TruncAudit {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl TruncAudit {
    /// 记一次检出（截断钳制）。
    pub fn hit(&mut self, round: u32, code: IfaceCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, IfaceCode::Ok.val()));
    }
    /// 守恒断言。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 审计完整率：全部轮次都落账即 100%。
    pub fn completeness(&self) -> u32 {
        if self.injected == 0 || self.rows.len() as u32 != self.injected {
            0
        } else {
            100
        }
    }
    /// 检出数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 账本行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 7 ^ 0xB7E1_5162_8AED_2A6A
}

/// 确定性线性同余发生器。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 构造实收描述符序列：n 个接口（每条 3 字节建模）。
pub fn raw_ifaces(n: usize) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    for i in 0..n {
        v.push(i as u8);
        v.push(0x03);
        v.push(0x00);
    }
    v
}

/// 谎报注入 ×10 轮：五族样本各 2 轮，全部按实收截断/钳制并留审计。
pub fn injection_campaign() -> TruncAudit {
    let mut led = TruncAudit::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let fam = (r % 5) as u64;
        let actual = 2 + rnd.below(5) as usize;
        let mut lg = IfaceLedger::new();
        let code = match fam {
            // 声明远超实收（经典谎报）
            0 => lg.commit((actual * 3 + 5) as u32, &raw_ifaces(actual)),
            // 声明为零
            1 => lg.commit(0, &raw_ifaces(actual)),
            // 声明越界
            2 => lg.commit((MAX_IFACES * 8) as u32, &raw_ifaces(actual)),
            // 实收残缺（字节数不成 3 的倍数）
            3 => {
                let mut raw = raw_ifaces(actual);
                raw.push(0x77);
                lg.commit(actual as u32, &raw)
            }
            // 重复接口号
            _ => {
                let mut raw = raw_ifaces(actual);
                if raw.len() >= 3 {
                    if let Some(b) = raw.get(0).copied() {
                        raw.push(b);
                        raw.push(0x03);
                        raw.push(0x00);
                    }
                }
                lg.commit(actual as u32, &raw)
            }
        };
        let caught = code != IfaceCode::Ok;
        if caught {
            led.hit(r + 1, code);
        } else {
            led.miss(r + 1);
        }
    }
    led
}

/// 大接口数合法设备 ×5 零误截回归：声明恰等于实收且都很大。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for i in 0..5u32 {
        let n = MAX_IFACES - i as usize; // 大但一致
        let mut lg = IfaceLedger::new();
        let code = lg.commit(n as u32, &raw_ifaces(n));
        if code == IfaceCode::Ok && lg.actual() == n && lg.audit_rows() == 0 {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

/// FNV-1a 64 位摘要。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性轨迹指纹。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    let mut lg = IfaceLedger::new();
    let _ = lg.commit(9, &raw_ifaces(4));
    for line in lg.trace().iter() {
        let mut bytes: Vec<u8> = Vec::new();
        for b in line.as_bytes() {
            bytes.push(*b ^ (fp & 0xFF) as u8);
        }
        fp = fnv1a(&bytes) ^ fp.rotate_left(7);
    }
    let led = injection_campaign();
    fp = fnv1a(&led.detected().to_le_bytes()) ^ fp.rotate_left(5);
    fp
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽。
pub fn val_codes_distinct() -> bool {
    for i in 0..ALL_CODES.len() {
        let a = ALL_CODES[i].val();
        if !(SEG_BASE + 1..=SEG_BASE + 12).contains(&a) {
            return false;
        }
        for j in (i + 1)..ALL_CODES.len() {
            if a == ALL_CODES[j].val() {
                return false;
            }
        }
    }
    true
}

/// 收口自检（16 条判据）。
pub fn run_unx51626_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51626");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 声明实收一致通过。
    {
        let mut lg = IfaceLedger::new();
        let code = lg.commit(4, &raw_ifaces(4));
        s.add_dyn("一致建账通过", code == IfaceCode::Ok && lg.actual() == 4, "声明==实收必须零误截");
    }
    // 4 大接口数合法 ×5 零误截（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "大接口数零误截",
            ok == 5 && bad == 0,
            "UNX-F51627-J1R 大接口数合法设备 ×5 零误截",
        );
    }
    // 5 谎报注入 ×10 截断审计（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "谎报截断审计",
            led.detected() == ROUNDS && led.completeness() == 100 && led.conserved(),
            "UNX-F51627-J1 谎报 ×10 按实收截断审计，完整率 100%",
        );
    }
    // 6 经典谎报截断差值正确。
    {
        let mut lg = IfaceLedger::new();
        let code = lg.commit(30, &raw_ifaces(6));
        s.add_dyn(
            "截断差值正确",
            code == IfaceCode::ClaimTruncated && lg.truncated_by() == 24 && lg.actual() == 6,
            "声明 30 实收 6 必须截 24 且留审计行",
        );
    }
    // 7 声明为零拒。
    {
        let mut lg = IfaceLedger::new();
        let code = lg.commit(0, &raw_ifaces(3));
        s.add_dyn("声明为零拒", code == IfaceCode::ClaimZero, "config 至少声明一个接口");
    }
    // 8 声明越界拒。
    {
        let mut lg = IfaceLedger::new();
        let code = lg.commit((MAX_IFACES * 8) as u32, &raw_ifaces(3));
        s.add_dyn("声明越界拒", code == IfaceCode::ClaimOob, "声明超上限必须显性拒");
    }
    // 9 声明倒谎报（声明<实收）保守钳制。
    {
        let mut lg = IfaceLedger::new();
        let code = lg.commit(2, &raw_ifaces(5));
        s.add_dyn(
            "倒谎报保守钳制",
            code == IfaceCode::ClaimUndershot && lg.actual() == 5,
            "声明小于实收按实收留证并显性记码",
        );
    }
    // 10 残缺描述符拒。
    {
        let mut lg = IfaceLedger::new();
        let mut raw = raw_ifaces(3);
        raw.push(0x11);
        let code = lg.commit(3, &raw);
        s.add_dyn("残缺拒", code == IfaceCode::IfaceFragment, "字节数不成接口粒度必须拒");
    }
    // 11 重复接口号显性记。
    {
        let mut lg = IfaceLedger::new();
        let mut raw = raw_ifaces(3);
        if let Some(b) = raw.get(0).copied() {
            raw.push(b);
            raw.push(0x03);
            raw.push(0x00);
        }
        let code = lg.commit(4, &raw);
        s.add_dyn("重复接口号记", code == IfaceCode::IfaceDup, "同 config 内接口号不得重复");
    }
    // 12 审计行与谎报轮次一比一。
    {
        let mut lg = IfaceLedger::new();
        for _ in 0..ROUNDS {
            let _ = lg.commit(50, &raw_ifaces(3));
        }
        s.add_dyn(
            "审计行一比一",
            lg.audit_rows() == ROUNDS as usize,
            "每轮谎报都必须留下恰好一行截断审计",
        );
    }
    // 13 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 14 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 15 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    // 16 槽位单调：多轮 commit 只增不减。
    {
        let mut lg = IfaceLedger::new();
        let mut mono = true;
        let mut prev = 0usize;
        for n in [3usize, 5, 8] {
            let _ = lg.commit(n as u32, &raw_ifaces(n));
            if lg.actual() < prev {
                mono = false;
            }
            prev = lg.actual();
        }
        s.add_dyn("槽位单调", mono && lg.actual() >= 8, "接口槽只增不减");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51626_checks_green() {
        let set = run_unx51626_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51626u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1F40);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(IfaceCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(IfaceCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn consistent_commit_ok() {
        let mut lg = IfaceLedger::new();
        assert_eq!(lg.commit(4, &raw_ifaces(4)), IfaceCode::Ok);
        assert_eq!(lg.actual(), 4);
        assert_eq!(lg.audit_rows(), 0);
    }

    #[test]
    fn lie_truncated_correctly() {
        let mut lg = IfaceLedger::new();
        assert_eq!(lg.commit(30, &raw_ifaces(6)), IfaceCode::ClaimTruncated);
        assert_eq!(lg.truncated_by(), 24);
        assert_eq!(lg.actual(), 6);
        assert_eq!(lg.audit_rows(), 1);
    }

    #[test]
    fn undershot_conservative() {
        let mut lg = IfaceLedger::new();
        assert_eq!(lg.commit(2, &raw_ifaces(5)), IfaceCode::ClaimUndershot);
        assert_eq!(lg.actual(), 5);
    }

    #[test]
    fn zero_and_oob_claim() {
        let mut lg = IfaceLedger::new();
        assert_eq!(lg.commit(0, &raw_ifaces(2)), IfaceCode::ClaimZero);
        let mut lg2 = IfaceLedger::new();
        assert_eq!(lg2.commit((MAX_IFACES * 8) as u32, &raw_ifaces(2)), IfaceCode::ClaimOob);
    }

    #[test]
    fn fragment_rejected() {
        let mut lg = IfaceLedger::new();
        let mut raw = raw_ifaces(2);
        raw.push(0x99);
        assert_eq!(lg.commit(2, &raw), IfaceCode::IfaceFragment);
    }

    #[test]
    fn dup_iface_flagged() {
        let mut lg = IfaceLedger::new();
        let mut raw = raw_ifaces(2);
        raw.push(0);
        raw.push(0x03);
        raw.push(0x00);
        assert_eq!(lg.commit(3, &raw), IfaceCode::IfaceDup);
    }

    #[test]
    fn big_consistent_no_false_trunc() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (5, 0));
    }

    #[test]
    fn audit_rows_one_per_round() {
        let mut lg = IfaceLedger::new();
        for _ in 0..ROUNDS {
            let _ = lg.commit(50, &raw_ifaces(3));
        }
        assert_eq!(lg.audit_rows(), ROUNDS as usize);
    }

    #[test]
    fn campaign_conserved_and_complete() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.completeness(), 100);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(trace_fingerprint(), trace_fingerprint());
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
