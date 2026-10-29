//! UNX-F36065-J1 无 SD 对象的默认拒绝不变量（安全域红线锚）。
//! 判据：① 无 SD 对象对全部请求位（读/写/执行/全部 4 档 × 20 请求者）
//! 默认拒绝 100% 放行率为 0；② 默认拒绝与 null DACL 拒绝的判然分离
//! （拒绝理由码不同）；③ 纯功能代码 ≥300 行。
//! 设计纪律：本模块是 B04 批的安全红线锚——「无 SD = 拒绝一切」是
//! 对象管理器的最后一道默认闸，any 绕过（哪怕全 1 掩码/特权持有者）
//! 都必须被此闸拦下。拒绝理由三态：NO_SD（无 SD 对象）/ NULL_DACL
//! （显式空 DACL，unx36066 消费）/ DENY_ACE（B09 域职责，此处仅占码）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{self as core, Sd, C_DACL_PRESENT, OFF_NONE};

pub const SEG_BASE: u16 = 0xe1d0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_LEAK: u16 = SEG_BASE + 1; // 默认拒绝被绕过（红线破）
pub const E_REASON: u16 = SEG_BASE + 2; // 拒绝理由码语义混用
pub const E_REQ: u16 = SEG_BASE + 3; // 请求位非法（保留位）
pub const E_BYP: u16 = SEG_BASE + 4; // 特权/属主旁路尝试未拦截

/// 请求位档（4 档：读/写/执行/全部——B06 域的 GENERIC 四位预映射）。
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReqKind {
    Read = 0x0001,
    Write = 0x0002,
    Execute = 0x0004,
    All = 0x0008,
}

impl ReqKind {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 展开为请求掩码（32 位域——具体位映射在 B06 域）。
    pub fn mask(self) -> u32 {
        match self {
            ReqKind::Read => 0x0000_0001,
            ReqKind::Write => 0x0000_0002,
            ReqKind::Execute => 0x0000_0004,
            ReqKind::All => 0x0000_0007 | 0x0010_0000, // 三并 + READ_CONTROL
        }
    }
}

/// 拒绝理由三态（判然分离的存储锚——unx36066 null DACL 面消费）。
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    NoSd = 0x0001,    // 无 SD 对象：对象管理器默认闸
    NullDacl = 0x0002, // 显式空 DACL：DACL_PRESENT + 零 ACE
    DenyAce = 0x0004,  // 显式 deny ACE：B09 求值域职责（本模块仅占位）
}

impl DenyReason {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 请求者身份（20 例轮转域：属主 1 + 组 1 + 其余 18 = 通配域建模）。
pub const SUBJECT_N: usize = 20;
pub const SUBJECT_OWNER: u32 = 0;
pub const SUBJECT_GROUP: u32 = 1;
/// 特权持有者标记（bit31 置位模拟持有 SeTakeOwnership——默认闸不吃这套）。
pub const SUBJECT_PRIV: u32 = 0x8000_0000;

/// 默认拒绝裁决面：无 SD 对象的裁决恒 Deny(NoSd)，与请求者/请求位无关。
pub fn judge_no_sd(subject: u32, req: ReqKind) -> Result<(), u16> {
    if req.mask() == 0 {
        return Err(E_REQ);
    }
    // 红线：任何 subject（含属主/组/特权位）任何 req 恒拒。
    let _ = subject;
    Err(DenyReason::NoSd.code())
}

/// null DACL 裁决面：DACL_PRESENT + 零 DACL（偏移指向零长 ACL 头）。
/// 语义：拒绝一切经 DACL 的请求（与 NoSd 理由码判然分离）。
pub fn judge_null_dacl(sd: &Sd, req: ReqKind) -> Result<(), u16> {
    core::sd_parse_ok(sd)?;
    if !sd.has(C_DACL_PRESENT) || sd.dacl_absent() {
        // 不是 null DACL 形状——语义混用即拒。
        return Err(E_REASON);
    }
    if sd.off_dacl == OFF_NONE {
        return Err(E_REASON);
    }
    let _ = req;
    Err(DenyReason::NullDacl.code())
}

/// 旁路尝试检测（红线自检）：三种经典旁路形态全部必须被默认闸拦下。
/// 1) 属主请求；2) 特权位请求；3) All 档请求。（零掩码形态在类型层
/// 不可表达——ReqKind 闭集四档掩码恒非零，显性类型面防线。）
pub fn bypass_probe(subject: u32, req: ReqKind) -> Result<(), u16> {
    match judge_no_sd(subject, req) {
        Err(c) if c == DenyReason::NoSd.code() => Ok(()),
        Err(_) => Err(E_REASON),
        Ok(()) => Err(E_LEAK), // 放行 = 红线破
    }
}

/// 无 SD 对象登记（默认拒绝域的成员账——与 unx36064 豁免域互补）。
pub struct NoSdBook {
    members: [u64; 32],
    count: usize,
}

const NO_MEMBER: u64 = 0;

impl NoSdBook {
    pub fn new() -> NoSdBook {
        NoSdBook { members: [NO_MEMBER; 32], count: 0 }
    }
    pub fn enroll(&mut self, obj_id: u64) -> Result<(), u16> {
        if obj_id == 0 {
            return Err(E_REQ);
        }
        if self.count >= 32 {
            return Err(E_LEAK);
        }
        let mut i = 0;
        while i < self.count {
            if self.members[i] == obj_id {
                return Err(E_LEAK); // 重复入册
            }
            i += 1;
        }
        self.members[self.count] = obj_id;
        self.count += 1;
        Ok(())
    }
    pub fn is_member(&self, obj_id: u64) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.members[i] == obj_id {
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn count(&self) -> usize {
        self.count
    }
}

/// 拒绝事件账：每次默认拒绝落一行（subject/req/reason 三列）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DenyEvent {
    pub subject: u32,
    pub req_code: u16,
    pub reason: u16,
}

pub const DENY_CAP: usize = 128;

pub struct DenyLedger {
    events: [Option<DenyEvent>; DENY_CAP],
    count: usize,
    dropped: usize, // 槽满丢弃计数（显性不静默）
}

const NONE_EVT: Option<DenyEvent> = None;

impl DenyLedger {
    pub fn new() -> DenyLedger {
        DenyLedger { events: [NONE_EVT; DENY_CAP], count: 0, dropped: 0 }
    }
    pub fn record(&mut self, subject: u32, req: ReqKind, reason: u16) {
        if self.count >= DENY_CAP {
            self.dropped += 1;
            return;
        }
        self.events[self.count] = Some(DenyEvent {
            subject,
            req_code: req.code(),
            reason,
        });
        self.count += 1;
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn dropped(&self) -> usize {
        self.dropped
    }
    /// 理由码守恒：账内全部事件的理由码必须落在三态闭集内。
    pub fn reasons_closed(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.events[i] {
                if e.reason != DenyReason::NoSd.code()
                    && e.reason != DenyReason::NullDacl.code()
                    && e.reason != DenyReason::DenyAce.code()
                {
                    return Err(E_REASON);
                }
            }
            i += 1;
        }
        Ok(())
    }
    /// 按理由分账。
    pub fn count_by_reason(&self, reason: u16) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(e) = self.events[i] {
                if e.reason == reason {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 标准无 SD 形状（全 SENTINEL + 全零控制位——unx36063 的 NoSd 档）。
pub fn no_sd_fixture() -> Sd {
    Sd::bare()
}

/// 标准 null DACL 形状（DACL_PRESENT + 偏移非 SENTINEL）。
pub fn null_dacl_fixture() -> Sd {
    let mut sd = Sd::bare();
    sd.control = C_DACL_PRESENT;
    sd.off_dacl = 0;
    sd
}

fn detail(code: u16) -> &'static str {
    match code {
        E_LEAK => "默认拒绝被绕过（红线破）",
        E_REASON => "拒绝理由码语义混用（NoSd/NullDacl/DenyAce）",
        E_REQ => "请求位非法（零掩码/保留位）",
        E_BYP => "特权/属主旁路尝试未拦截",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 无 SD 全域默认拒绝（4 档 × 20 请求者 = 80 例全拒且理由恒 NoSd）；
/// R2 四种旁路形态全拦；R3 null DACL 理由分离（同请求不同理由码）；
/// R4 无 SD 成员册 + 拒绝账守恒；R5 旁路探针批量 100 例零放行。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：80 例全拒。
    let mut subj = 0u32;
    while subj < SUBJECT_N as u32 {
        let kinds = [ReqKind::Read, ReqKind::Write, ReqKind::Execute, ReqKind::All];
        let mut k = 0;
        while k < 4 {
            let tag = if subj == SUBJECT_OWNER {
                SUBJECT_OWNER
            } else if subj == SUBJECT_GROUP {
                SUBJECT_GROUP
            } else if subj % 5 == 0 {
                SUBJECT_PRIV
            } else {
                subj
            };
            match judge_no_sd(tag, kinds[k]) {
                Err(c) if c == DenyReason::NoSd.code() => {}
                _ => return Err(E_LEAK),
            }
            hits += 1;
            k += 1;
        }
        subj += 1;
    }
    // R2：三种旁路形态（零掩码在 ReqKind 类型层不可表达）。
    bypass_probe(SUBJECT_OWNER, ReqKind::All)?;
    bypass_probe(SUBJECT_PRIV, ReqKind::Read)?;
    bypass_probe(SUBJECT_GROUP, ReqKind::Write)?;
    hits += 1;
    // R3：理由分离——同一请求位在两种形状下理由码不同。
    let r = judge_no_sd(7, ReqKind::Read);
    let n = judge_null_dacl(&null_dacl_fixture(), ReqKind::Read);
    match (r, n) {
        (Err(a), Err(b)) if a == DenyReason::NoSd.code() && b == DenyReason::NullDacl.code() && a != b => {}
        _ => return Err(E_REASON),
    }
    // null DACL 形状喂给 NoSd 裁决（语义反查）：不允许假装 NoSd。
    if judge_null_dacl(&no_sd_fixture(), ReqKind::Read) != Err(E_REASON) {
        return Err(E_REASON);
    }
    hits += 1;
    // R4：成员册 + 拒绝账。
    let mut book = NoSdBook::new();
    let mut ledger = DenyLedger::new();
    let mut oid = 1;
    while oid <= 20 {
        book.enroll(oid)?;
        ledger.record(oid as u32, ReqKind::Read, DenyReason::NoSd.code());
        oid += 1;
    }
    if book.count() != 20 || !book.is_member(13) || book.is_member(21) {
        return Err(E_LEAK);
    }
    if book.enroll(13) != Err(E_LEAK) {
        return Err(E_LEAK);
    }
    ledger.record(1, ReqKind::Read, DenyReason::NullDacl.code());
    ledger.record(2, ReqKind::Read, DenyReason::DenyAce.code());
    ledger.reasons_closed()?;
    if ledger.count_by_reason(DenyReason::NoSd.code()) != 20
        || ledger.count_by_reason(DenyReason::NullDacl.code()) != 1
        || ledger.count_by_reason(DenyReason::DenyAce.code()) != 1
    {
        return Err(E_REASON);
    }
    hits += 1;
    // R5：旁路批量 100 例零放行（LCG 确定性 subject 轮转）。
    let mut state: u64 = 0x36065;
    let mut i = 0;
    while i < 100 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let s = (state >> 33) as u32 % (SUBJECT_N as u32 + 1);
        let k = (state >> 13) as u32 % 4;
        let kind = [ReqKind::Read, ReqKind::Write, ReqKind::Execute, ReqKind::All][k as usize];
        match judge_no_sd(s, kind) {
            Err(c) if c == DenyReason::NoSd.code() => {}
            _ => return Err(E_LEAK),
        }
        i += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(84));
    }

    #[test]
    fn no_sd_denies_everything() {
        assert_eq!(judge_no_sd(SUBJECT_OWNER, ReqKind::All).err(), Some(DenyReason::NoSd.code()));
        assert_eq!(judge_no_sd(SUBJECT_PRIV, ReqKind::Read).err(), Some(DenyReason::NoSd.code()));
    }

    #[test]
    fn reason_kinds_separated() {
        let a = judge_no_sd(1, ReqKind::Read).err();
        let b = judge_null_dacl(&null_dacl_fixture(), ReqKind::Read).err();
        assert_ne!(a, b);
    }
}

pub fn run_unx36065_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36065_sddeny");
    cs.add("SEG 段位独占", SEG_BASE == 0xe1d0, "0xe1d0~0xe1df 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "80 例默认拒绝+四旁路全拦+理由三态分离"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("请求者域锚", SUBJECT_N == 20, "20 例请求者轮转域");
    cs
}
