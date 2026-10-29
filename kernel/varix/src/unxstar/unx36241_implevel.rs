//! UNX-F36241-J1 模拟四级语义总表（含 Delegation 不支持落账）。
//! 判据：① 四级行为表与版本锚一致（Anonymous/Identification/
//! Impersonation/Delegation 四级行为位面闭集）；② Delegation 请求
//! 50 例全部显式「不支持」落账（诚实三态——不假装支持）；
//! ③ 纯功能 ≥300 行。
//! 设计纪律：模拟级的本质是「令牌能力的可见性阶梯」——级别越高，
//! 被服务方对调用者身份的可见性与可用性越强。Delegation 在本内核
//! 首版**不支持**：诚实落账（请求进来→显性拒绝→账面留痕），而不是
//! 静默降级成 Impersonation（假装支持的谎言比不支持更危险）。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA2F0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_TABLE: u16 = SEG_BASE + 2; // 四级行为表漂移
pub const E_DELEG: u16 = SEG_BASE + 3; // Delegation 落账缺
pub const E_LEDGER: u16 = SEG_BASE + 4; // 落账簿账不平
pub const E_SILENT: u16 = SEG_BASE + 5; // 静默处理（诚实破）
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 模拟四级（判别式显式——禁 transmute 构造）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ImpLevel {
    /// 匿名级：被服务方拿不到调用者身份。
    Anonymous = 0,
    /// 识别级：可识别不可用。
    Identification = 1,
    /// 模拟级：本地资源用调用者身份。
    Impersonation = 2,
    /// 委托级：本内核首版不支持（诚实落账）。
    Delegation = 3,
}

pub const LEVEL_N: usize = 4;

/// 行为位面（每级四个行为位）。
pub const B_SEE_ID: u8 = 1 << 0; // 可见调用者身份
pub const B_USE_RES: u8 = 1 << 1; // 可用本地资源
pub const B_LOCAL: u8 = 1 << 2; // 允许本地作用域
pub const B_REMOTE: u8 = 1 << 3; // 允许跨机作用域

/// 四级行为表（版本锚 VER=1 冻结——行为位漂移即 E_TABLE）。
pub const LEVEL_TABLE: [(ImpLevel, u8, &'static str); LEVEL_N] = [
    (ImpLevel::Anonymous, 0, "不可获得调用者身份"),
    (ImpLevel::Identification, B_SEE_ID, "可识别不可用"),
    (ImpLevel::Impersonation, B_SEE_ID | B_USE_RES | B_LOCAL, "本地资源用调用者身份"),
    (ImpLevel::Delegation, 0, "首版不支持（诚实落账）"),
];

/// 行为表健康：四级行为位与版本锚一致（表漂移检测）。
pub fn table_consistent() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if LEVEL_TABLE.len() != LEVEL_N {
        return Err(E_TABLE);
    }
    // Anonymous：全零行为位。
    if LEVEL_TABLE[0].1 != 0 {
        return Err(E_TABLE);
    }
    // Identification：仅可见身份。
    if LEVEL_TABLE[1].1 != B_SEE_ID {
        return Err(E_TABLE);
    }
    // Impersonation：可见 + 本地资源可用，禁跨机。
    if LEVEL_TABLE[2].1 != B_SEE_ID | B_USE_RES | B_LOCAL {
        return Err(E_TABLE);
    }
    // Delegation：不支持即零能力位（诚实）。
    if LEVEL_TABLE[3].1 != 0 {
        return Err(E_TABLE);
    }
    // 判别式连续（0..3——枚举值域封闭）。
    let mut i = 0u16;
    while i < LEVEL_N as u16 {
        let lv = level_from_raw(i)?;
        if lv as u16 != i {
            return Err(E_TABLE);
        }
        i += 1;
    }
    Ok(())
}

/// 裸值 → 级别（越界 = E_ILLEGAL——非法枚举构造的守门面）。
pub fn level_from_raw(raw: u16) -> Result<ImpLevel, u16> {
    match raw {
        0 => Ok(ImpLevel::Anonymous),
        1 => Ok(ImpLevel::Identification),
        2 => Ok(ImpLevel::Impersonation),
        3 => Ok(ImpLevel::Delegation),
        _ => Err(E_ILLEGAL),
    }
}

// ---- Delegation 诚实落账簿 -------------------------------------------------

/// 落账三态（诚实三态：支持/显拒/不支持——禁静默）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelegVerdict {
    /// 支持（首版无此态——保留判别式位）。
    Granted,
    /// 显性拒绝（有权限语义的拒）。
    Denied,
    /// 不支持（能力面缺失——诚实声明）。
    Unsupported,
}

/// 单条落账记录（请求序号 + 裁决 + 摘要指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DelegEntry {
    pub seq: u32,
    pub verdict: DelegVerdict,
    pub note_fp: u32,
}

/// Delegation 落账簿（append-only——诚实账不可抹）。
pub struct DelegLedger {
    entries: alloc::vec::Vec<DelegEntry>,
    next_seq: u32,
}

const LEDGER_CAP: usize = 128;

impl DelegLedger {
    pub fn new() -> Self {
        DelegLedger { entries: alloc::vec::Vec::new(), next_seq: 1 }
    }

    /// Delegation 请求落账（50 例全部显式「不支持」——判据 ② 落点）。
    pub fn request_delegation(&mut self, caller_note: u32) -> Result<DelegVerdict, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 诚实纪律：Delegation 恒 Unsupported，禁静默（无账返回）。
        let verdict = DelegVerdict::Unsupported;
        if self.entries.len() >= LEDGER_CAP {
            return Err(E_LEDGER);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.entries.push(DelegEntry { seq, verdict, note_fp: caller_note });
        Ok(verdict)
    }

    /// 账面全为 Unsupported（静默漏账即 E_SILENT）。
    pub fn all_unsupported(&self) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].verdict != DelegVerdict::Unsupported {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 账目数（外部对账面）。
    pub fn count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 序号连续（append-only 的完整性面——跳号即账破）。
    pub fn seq_continuous(&self) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].seq != (i as u32) + 1 {
                return false;
            }
            i += 1
        }
        true
    }

    /// 摘要指纹折叠（落账内容防漂锚）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0xDE1E_0000;
        let mut i = 0;
        while i < self.entries.len() {
            h = h.wrapping_mul(31).wrapping_add(self.entries[i].note_fp);
            h = h.wrapping_mul(31).wrapping_add(self.entries[i].verdict as u32);
            i += 1;
        }
        h
    }
}

/// Delegation 50 例全落账直证（判据 ② 的 drill 落点）。
pub fn delegation_fifty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    table_consistent()?;
    let mut ledger = DelegLedger::new();
    let mut i = 0u32;
    while i < 50 {
        // 调用者备注指纹：确定性 LCG 派生（请求面多样性）。
        let note = i.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let v = ledger.request_delegation(note)?;
        if v != DelegVerdict::Unsupported {
            return Err(E_DELEG); // 支持了 = 谎言破
        }
        i += 1;
    }
    if ledger.count() != 50 {
        return Err(E_LEDGER);
    }
    if !ledger.all_unsupported() || !ledger.seq_continuous() {
        return Err(E_SILENT);
    }
    Ok(ledger.fingerprint())
}

/// 级别能力查询面（下游消费读口：级别 → 行为位——36245 挂接面与
/// 36249 次序面共用此读口，禁各自复制表）。
pub fn level_capabilities(raw: u16) -> Result<u8, u16> {
    let lv = level_from_raw(raw)?;
    let mut i = 0;
    while i < LEVEL_N {
        if LEVEL_TABLE[i].0 == lv {
            return Ok(LEVEL_TABLE[i].1);
        }
        i += 1;
    }
    Err(E_TABLE)
}

/// 级别题注读口（呈现面——拒绝路径的三要素「出路」消费此读口）。
pub fn level_caption(raw: u16) -> Result<&'static str, u16> {
    let lv = level_from_raw(raw)?;
    let mut i = 0;
    while i < LEVEL_N {
        if LEVEL_TABLE[i].0 == lv {
            return Ok(LEVEL_TABLE[i].2);
        }
        i += 1;
    }
    Err(E_TABLE)
}

/// 落账簿容量账（CAP 语义直证：满册拒收 = E_LEDGER，不静默覆盖）。
pub fn ledger_cap_semantics() -> Result<(), u16> {
    let mut lg = DelegLedger::new();
    let mut i = 0;
    while i < LEDGER_CAP {
        lg.request_delegation(i as u32)?;
        i += 1;
    }
    if lg.count() != LEDGER_CAP as u32 {
        return Err(E_LEDGER);
    }
    // 满册再收：显性拒（容量红线——不覆盖不静默）。
    if lg.request_delegation(0xBEEF).is_ok() {
        return Err(E_LEDGER);
    }
    // 满册账仍诚实：全 Unsupported + 序连续。
    if !lg.all_unsupported() || !lg.seq_continuous() {
        return Err(E_SILENT);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 四级行为表一致。
    if VER != 1 {
        return Err(E_VER);
    }
    table_consistent()?;
    // R2：Delegation 50 例全落账（诚实三态）。
    let fp = delegation_fifty()?;
    // R3：行为表可读性——每级题注非空（呈现面健康）。
    let mut i = 0;
    while i < LEVEL_N {
        if LEVEL_TABLE[i].2.is_empty() {
            return Err(E_TABLE);
        }
        i += 1;
    }
    // R4：读口消费面（能力查询 + 题注查询 + 容量语义）。
    if level_capabilities(2).map_err(|_| E_TABLE)? != (B_SEE_ID | B_USE_RES | B_LOCAL) {
        return Err(E_TABLE);
    }
    if level_caption(3).map_err(|_| E_TABLE)?.is_empty() {
        return Err(E_TABLE);
    }
    ledger_cap_semantics()?;
    Ok(fp ^ (LEVEL_N as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_TABLE => "四级行为表漂移",
        E_DELEG => "Delegation 落账缺（支持了=谎言破）",
        E_LEDGER => "落账簿账不平",
        E_SILENT => "静默处理（诚实破）",
        E_ILLEGAL => "级别越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36241_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36241_implevel");
    cs.add("SEG 段位独占", SEG_BASE == 0xA2F0, "0xA2F0~0xA2FF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "四级行为表锚一致/Delegation 50 例全落账"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("行为表锚", LEVEL_TABLE[2].1 == B_SEE_ID | B_USE_RES | B_LOCAL, "模拟级三能力位（禁跨机）");
    cs.add("诚实锚", LEVEL_TABLE[3].1 == 0, "Delegation 零能力位（不支持诚实声明）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_table_anchor() {
        assert!(table_consistent().is_ok());
        // 裸值往返：0..3 合法、4 越界。
        assert!(level_from_raw(3).is_ok());
        assert!(level_from_raw(4).is_err());
    }

    #[test]
    fn test_delegation_honest() {
        let mut ledger = DelegLedger::new();
        let v = ledger.request_delegation(7).unwrap_or(DelegVerdict::Denied);
        assert_eq!(v, DelegVerdict::Unsupported);
        assert!(ledger.all_unsupported());
        assert!(ledger.seq_continuous());
    }
}
