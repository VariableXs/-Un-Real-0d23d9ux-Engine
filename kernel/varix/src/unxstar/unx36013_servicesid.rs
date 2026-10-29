//! unx36013_servicesid — UNX-F36013 服务 SID（S-1-5-6 类语义）与按服务
//! 身份标记锚。
//!
//! 判据原文：
//!   J1 服务名→服务 SID 发射确定性 20 例双跑一致；伪服务名注入 10 例
//!      拒绝并留审计事件（联 J5 通道格式）。
//! 设计纪律：
//!   1. 服务 SID 锚：S-1-5-80-<a>-<b>（IA=5、前缀 80、双子权 RID 面），
//!      (a,b) 由服务名规范化大写后的 FNV-1a 64 拆分——同服务名恒得同
//!      SID（确定性发射，幂等账留痕）。
//!   2. 服务名纪律：非空、≤64 字节、字符闭集 [A-Za-z0-9_.-]、首字符
//!      必须字母或数字；违者构造期拒绝 + 审计事件（J5 通道格式：
//!      kind/名字指纹/时刻序）。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x73F0;

// ── 诊断码闭集（0x73F0~0x740F，独占）─────────────────────────
const E_NAME: u16 = SEG_BASE + 1; // 服务名非法（空/超长/坏字符/首字符）
const E_CAP: u16 = SEG_BASE + 2; // 发射册满（显性拒）
const E_DET: u16 = SEG_BASE + 3; // 确定性失配（双跑 SID 漂移）
const E_AUDIT: u16 = SEG_BASE + 4; // 审计账失配（事件数漂移）
const E_BAL: u16 = SEG_BASE + 5; // 账目守恒破
const E_IDEM: u16 = SEG_BASE + 6; // 幂等失配（重发 SID 变化）

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// 服务名长度上限。
pub const NAME_MAX: usize = 64;
/// 发射册容量。
pub const SVC_CAP: usize = 32;
/// 审计账容量。
pub const AUDIT_CAP: usize = 16;

/// 审计事件（J5 通道格式 v1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SvcAudit {
    pub kind: u8,     // 1=伪服务名拒
    pub name_fp: u64, // 名字指纹（不落原文，账面最小化）
    pub seq: u32,     // 时刻序（确定性序号）
}

/// 服务 SID 发射册。
pub struct SvcSidBook {
    /// (名字指纹, RID 双子权) 发射账。
    issued: [Option<(u64, u32, u32)>; SVC_CAP],
    audit: [Option<SvcAudit>; AUDIT_CAP],
    pub emitted: u32,
    pub idem_hits: u32,
    pub rejected: u32,
    pub req_total: u32,
    pub audit_count: u32,
    audit_seq: u32,
}

/// 名字规范化大写（ASCII 就地 +1 字节余量）。
pub fn upper_name(name: &[u8], out: &mut [u8]) -> Result<usize, u16> {
    if name.is_empty() || name.len() > NAME_MAX || out.len() < name.len() {
        return Err(E_NAME);
    }
    let c0 = name[0];
    if !c0.is_ascii_alphanumeric() {
        return Err(E_NAME);
    }
    let mut i = 0;
    while i < name.len() {
        let c = name[i];
        let ok = c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.';
        if !ok {
            return Err(E_NAME);
        }
        out[i] = c.to_ascii_uppercase();
        i += 1;
    }
    Ok(name.len())
}

/// FNV-1a 64。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

impl SvcSidBook {
    pub fn new() -> SvcSidBook {
        SvcSidBook {
            issued: [None; SVC_CAP],
            audit: [None; AUDIT_CAP],
            emitted: 0,
            idem_hits: 0,
            rejected: 0,
            req_total: 0,
            audit_count: 0,
            audit_seq: 0,
        }
    }

    /// 发射：服务名 → 服务 SID（确定性 + 幂等）。
    pub fn issue(&mut self, name: &[u8]) -> Result<Sid, u16> {
        self.req_total += 1;
        let mut buf = [0u8; NAME_MAX];
        let n = upper_name(name, &mut buf)?;
        let fp = fnv1a(&buf[..n]);
        let a = (fp >> 32) as u32;
        let b = (fp & 0xFFFF_FFFF) as u32;
        // 幂等：同名字指纹复检。
        let mut i = 0;
        while i < SVC_CAP {
            if let Some((f, ra, rb)) = self.issued[i] {
                if f == fp {
                    self.idem_hits += 1;
                    return self.build(a, b, ra, rb);
                }
            }
            i += 1;
        }
        // 新发射登记。
        let mut j = 0;
        while j < SVC_CAP {
            if self.issued[j].is_none() {
                self.issued[j] = Some((fp, a, b));
                self.emitted += 1;
                return self.build(a, b, a, b);
            }
            j += 1;
        }
        Err(E_CAP)
    }

    fn build(&self, a: u32, b: u32, ra: u32, rb: u32) -> Result<Sid, u16> {
        if ra != a || rb != b {
            return Err(E_DET);
        }
        Sid::new(1, Sid::ia_from(5), &[80, a, b])
    }

    /// 伪服务名拒绝 + 审计（J5 通道格式）。
    pub fn issue_audited(&mut self, name: &[u8]) -> Result<Sid, u16> {
        match self.issue(name) {
            Ok(s) => Ok(s),
            Err(c) => {
                if c == E_NAME {
                    // 名字指纹落账（原文不入账，最小化面）；超长名指纹
                    // 取截断前缀，空名取 0。
                    let fp = if name.is_empty() {
                        0
                    } else {
                        let n = if name.len() > NAME_MAX { NAME_MAX } else { name.len() };
                        let mut tmp = [0u8; NAME_MAX];
                        let mut i = 0;
                        while i < n {
                            tmp[i] = name[i];
                            i += 1;
                        }
                        fnv1a(&tmp[..n])
                    };
                    let ev = SvcAudit { kind: 1, name_fp: fp, seq: self.audit_seq };
                    self.audit_seq += 1;
                    let mut k = 0;
                    while k < AUDIT_CAP {
                        if self.audit[k].is_none() {
                            self.audit[k] = Some(ev);
                            self.audit_count += 1;
                            break;
                        }
                        k += 1;
                    }
                }
                self.rejected += 1;
                Err(c)
            }
        }
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.emitted + self.idem_hits + self.rejected == self.req_total
    }

    /// 审计事件计数（按 kind）。
    pub fn audit_of_kind(&self, kind: u8) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < AUDIT_CAP {
            if let Some(e) = self.audit[i] {
                if e.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

/// 确定性演练：20 例双跑一致 + 10 例伪名全拒留痕（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：20 例双跑一致——10 服务名两册独立发射，逐例 SID 指纹全等。
    let names: [&[u8]; 10] = [
        b"wuauserv", b"Schedule", b"EventLog", b"Dnscache", b"WinMgmt",
        b"BITS", b"dhcp", b"LanmanServer", b"mpssvc", b"WinDefend",
    ];
    let mut run1 = [0u64; 10];
    let mut run2 = [0u64; 10];
    let mut b1 = SvcSidBook::new();
    let mut b2 = SvcSidBook::new();
    let mut i = 0;
    while i < 10 {
        let s1 = b1.issue(names[i])?;
        run1[i] = s1.fingerprint();
        if s1.ia_value() != 5 || s1.subs()[0] != 80 {
            return Err(E_DET);
        }
        i += 1;
    }
    let mut i = 0;
    while i < 10 {
        let s2 = b2.issue(names[i])?;
        run2[i] = s2.fingerprint();
        i += 1;
    }
    let mut i = 0;
    while i < 10 {
        if run1[i] != run2[i] || run1[i] == 0 {
            return Err(E_DET);
        }
        i += 1;
    }
    hits += 1;

    // R2：幂等——同册重发同服务名，SID 恒等且 idem 命中。
    let mut b3 = SvcSidBook::new();
    let s1 = b3.issue(b"Schedule")?;
    let s2 = b3.issue(b"schedule")?; // 大小写规范化后同 SID
    if s1.fingerprint() != s2.fingerprint() {
        return Err(E_IDEM);
    }
    if b3.idem_hits != 1 {
        return Err(E_IDEM);
    }
    hits += 1;

    // R3：伪服务名注入 10 例全拒 + 审计留痕（J5 通道格式 kind=1）。
    let mut b4 = SvcSidBook::new();
    let bads: [&[u8]; 10] = [
        b"", b"-lead", b".dot", b"has space", b"bad/slash",
        b"bad\\back", b"bad:colon", b"over-long-name-0123456789-0123456789-0123456789-0123456789-0123456789-x",
        b"#hash", b"=eq",
    ];
    let mut k = 0;
    while k < 10 {
        if b4.issue_audited(bads[k]).is_ok() {
            return Err(E_NAME);
        }
        k += 1;
    }
    if b4.audit_of_kind(1) != 10 {
        return Err(E_AUDIT);
    }
    if b4.rejected != 10 {
        return Err(E_BAL);
    }
    // 审计序确定性：seq 逐事件 +1。
    let mut k = 0;
    while k < 10 {
        // 全部落账（容量 16 ≥ 10），seq 必为 k。
        let mut found = false;
        let mut i = 0;
        while i < AUDIT_CAP {
            if let Some(e) = b4.audit[i] {
                if e.kind == 1 && e.seq == k as u32 {
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            return Err(E_AUDIT);
        }
        k += 1;
    }
    hits += 1;

    // R4：守恒——三册账面对平。
    if !b1.conserve() || !b3.conserve() || !b4.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36013_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36013_servicesid");
    cs.add("SEG 段位独占", SEG_BASE == 0x73F0, "0x73F0~0x740F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NAME => "服务名非法（空/超长/坏字符/首字符）",
        E_CAP => "发射册满（显性拒）",
        E_DET => "确定性失配（双跑 SID 漂移）",
        E_AUDIT => "审计账失配（事件数漂移）",
        E_BAL => "账目守恒破",
        E_IDEM => "幂等失配（重发 SID 变化）",
        _ => "未知诊断码",
    }
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
