//! UNX-F12021 NTSTATUS 编码面全景：位域结构与编码空间账（0x194C0 段）
//!
//! 判据原文：NTSTATUS 为 32 位编码面——位 30-31 严重级（2 位闭集）、
//! 位 29 客户位、位 16-28 设施号（12 位）、位 0-15 码值（16 位）；编码
//! 组装须逐域验界；设施号登记册唯一登记（重复登记即冲突）；解码回读
//! 必须与编码面逐域一致。
//!
//! 结构：编码面（NtStatusCode 位域 + encode/decode 双向）+ 设施登记册
//! （FacilityBook：设施号 → 名称 → 已用码账）+ 登记核（四码守卫）+
//! 折叠指纹 + 印账冻结 + 注入核 + 判据聚合（run_unx12021_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x194C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x194C0;
pub const NF_BAD_SEVERITY: u32 = SEG_BASE + 0x01; // 严重级越 2 位域
pub const NF_BAD_FACILITY: u32 = SEG_BASE + 0x02; // 设施号越 12 位域
pub const NF_BAD_CODE: u32 = SEG_BASE + 0x03; // 码值越 16 位域
pub const NF_DUP_FACILITY: u32 = SEG_BASE + 0x04; // 设施重复登记
pub const NF_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// 严重级闭集（2 位域）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Success = 0,
    Informational = 1,
    Warning = 2,
    Error = 3,
}

pub fn severity_of(v: u32) -> Option<Severity> {
    match v {
        0 => Some(Severity::Success),
        1 => Some(Severity::Informational),
        2 => Some(Severity::Warning),
        3 => Some(Severity::Error),
        _ => None,
    }
}

/// 严重级标签面：四档诊断文本（闭集全覆盖）。
pub fn severity_label(v: u32) -> &'static str {
    match severity_of(v) {
        Some(Severity::Success) => "SUCCESS",
        Some(Severity::Informational) => "INFO",
        Some(Severity::Warning) => "WARN",
        Some(Severity::Error) => "ERROR",
        None => "<bad>",
    }
}

/// 渲染十六进制状态码（0x 前缀定宽 8）。
pub fn hex_ntstatus(v: u32) -> String {
    format!("0x{:08X}", v)
}

/// 解码后的位域四元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NtParts {
    pub severity: u32,
    pub customer: u32,
    pub facility: u32,
    pub code: u32,
}

/// 编码组装核：逐域验界后拼装。
pub fn encode(severity: u32, customer: u32, facility: u32, code: u32) -> Result<u32, u32> {
    if severity > 3 {
        return Err(NF_BAD_SEVERITY);
    }
    if customer > 1 {
        return Err(NF_BAD_SEVERITY);
    }
    if facility > 0xFFF {
        return Err(NF_BAD_FACILITY);
    }
    if code > 0xFFFF {
        return Err(NF_BAD_CODE);
    }
    Ok((severity << 30) | (customer << 29) | (facility << 16) | code)
}

/// 解码核：位域切分回读。
pub fn decode(v: u32) -> NtParts {
    NtParts {
        severity: v >> 30,
        customer: (v >> 29) & 1,
        facility: (v >> 16) & 0xFFF,
        code: v & 0xFFFF,
    }
}

/// 编码空间账：设施登记册。
pub struct FacilityBook {
    pub entries: Vec<(u32, String, u32)>, // (设施号, 名称, 已用码数)
    pub sealed: bool,
}

impl FacilityBook {
    pub fn new() -> FacilityBook {
        FacilityBook { entries: Vec::new(), sealed: false }
    }

    /// 登记核：设施号 12 位域内 + 唯一登记。
    pub fn register(&mut self, fac: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(NF_FROZEN);
        }
        if name.is_empty() || fac > 0xFFF {
            return Err(NF_BAD_FACILITY);
        }
        if self.entries.iter().any(|e| e.0 == fac) {
            return Err(NF_DUP_FACILITY);
        }
        self.entries.push((fac, name.to_string(), 0));
        Ok(())
    }

    /// 记一笔码使用。
    pub fn use_code(&mut self, fac: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(NF_FROZEN);
        }
        let i = match self.entries.iter().position(|e| e.0 == fac) {
            Some(i) => i,
            None => return Err(NF_BAD_FACILITY),
        };
        self.entries[i].2 += 1;
        Ok(())
    }

    /// 已用码数回读。
    pub fn used_of(&self, fac: u32) -> Option<u32> {
        self.entries.iter().find(|e| e.0 == fac).map(|e| e.2)
    }

    /// 登记数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 台账摘要：设施号 + 名称 + 已用码（诊断面用）。
    pub fn summary_line(&self, fac: u32) -> String {
        match self.entries.iter().find(|e| e.0 == fac) {
            Some(e) => format!("fac[{}|{}|used={}]", e.0, e.1, e.2),
            None => format!("fac[{}|<none>]", fac),
        }
    }

    /// 设施名回查。
    pub fn name_of(&self, fac: u32) -> Option<&str> {
        self.entries.iter().find(|e| e.0 == fac).map(|e| e.1.as_str())
    }

    /// 全台账已用码总量。
    pub fn total_used(&self) -> u32 {
        let mut t = 0;
        for e in &self.entries {
            t += e.2;
        }
        t
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.0);
            h = h.wrapping_mul(211).wrapping_add(e.2);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 严重级越 2 位域 → NF_BAD_SEVERITY。
    got[0] = encode(4, 0, 0, 1).err() == Some(NF_BAD_SEVERITY);
    // 2) 设施号越 12 位域 → NF_BAD_FACILITY。
    got[1] = encode(3, 0, 0x1000, 1).err() == Some(NF_BAD_FACILITY);
    // 3) 码值越 16 位域 → NF_BAD_CODE。
    got[2] = encode(3, 0, 1, 0x10000).err() == Some(NF_BAD_CODE);
    // 4) 设施重复登记 → NF_DUP_FACILITY。
    let mut b = FacilityBook::new();
    let _ = b.register(11, "nt-debug");
    got[3] = b.register(11, "dup").err() == Some(NF_DUP_FACILITY);
    // 5) 印账后登记 → NF_FROZEN。
    b.seal();
    got[4] = b.register(12, "post").err() == Some(NF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12021");

    // J1 编解码往返：STATUS_ACCESS_DENIED=0xC0000022 逐域回读。
    let j1 = {
        let v = encode(3, 0, 0x000, 0x0022).unwrap_or(0);
        let p = decode(v);
        v == 0xC000_0022 && p.severity == 3 && p.customer == 0 && p.facility == 0 && p.code == 0x22
    };
    s.add("J1-编解码往返", j1, "0xC0000022 四域对账");

    // J2 客户位语义：位置 1 后位 29 回读为 1。
    let j2 = {
        let v = encode(3, 1, 0, 0x22).unwrap_or(0);
        let p = decode(v);
        p.customer == 1 && v & 0x2000_0000 != 0
    };
    s.add("J2-客户位", j2, "位 29 回读一致");

    // J3 严重级四值闭集。
    let j3 = severity_of(0).is_some()
        && severity_of(1).is_some()
        && severity_of(2).is_some()
        && severity_of(3).is_some()
        && severity_of(4).is_none();
    s.add("J3-严重级闭集", j3, "0..3 收 4 拒");

    // J4 严重级越界拒。
    let j4 = encode(4, 0, 0, 1).err() == Some(NF_BAD_SEVERITY)
        && encode(0, 2, 0, 1).err() == Some(NF_BAD_SEVERITY);
    s.add("J4-严重级越界拒", j4, "sev>3 与 cust>1 同拒");

    // J5 设施号越界拒。
    let j5 = encode(3, 0, 0x1000, 1).err() == Some(NF_BAD_FACILITY);
    s.add("J5-设施越界拒", j5, "超 12 位必拒");

    // J6 码值越界拒。
    let j6 = encode(3, 0, 0, 0x10000).err() == Some(NF_BAD_CODE);
    s.add("J6-码值越界拒", j6, "超 16 位必拒");

    // J7 边界收：各域最大值恰合法。
    let j7 = {
        let v = encode(3, 1, 0xFFF, 0xFFFF).unwrap_or(0);
        let p = decode(v);
        p.severity == 3 && p.facility == 0xFFF && p.code == 0xFFFF
    };
    s.add("J7-满域边界", j7, "全 1 域往返一致");

    // J8 设施登记册正路径。
    let j8 = {
        let mut b = FacilityBook::new();
        let _ = b.register(11, "debug");
        let _ = b.register(0, "ntcore");
        b.count() == 2 && b.used_of(11) == Some(0)
    };
    s.add("J8-设施登记", j8, "双设施入册");

    // J9 重复登记拒。
    let j9 = {
        let mut b = FacilityBook::new();
        let _ = b.register(11, "a");
        b.register(11, "b").err() == Some(NF_DUP_FACILITY)
    };
    s.add("J9-重复登记拒", j9, "设施唯一");

    // J10 用码账：逐笔记数。
    let j10 = {
        let mut b = FacilityBook::new();
        let _ = b.register(11, "d");
        let _ = b.use_code(11);
        let _ = b.use_code(11);
        b.used_of(11) == Some(2)
    };
    s.add("J10-用码记账", j10, "两笔计数=2");

    // J11 印账冻结。
    let j11 = {
        let mut b = FacilityBook::new();
        let _ = b.register(11, "d");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2
            && b.register(12, "post").err() == Some(NF_FROZEN)
            && b.use_code(11).err() == Some(NF_FROZEN)
    };
    s.add("J11-指纹封账", j11, "双面操作印后全拒");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例：三大常用码编解码。
    let mut ok = true;
    for (sev, code) in [(3u32, 0x22u32), (3, 0x08), (0, 0x00)].iter() {
        let v = encode(*sev, 0, 0, *code).unwrap_or(0xFFFF_FFFF);
        let p = decode(v);
        if p.severity != *sev || p.code != *code {
            ok = false;
        }
    }
    s.add("样例三码往返", ok, "AV/IH/SUCCESS 全对");
    s.add("样例解码幂等", decode(decode(0xC000_0005).code).code == 5, "码域自反");

    // J13 汇总面补充：标签/渲染/台账摘要三面。
    let j13 = {
        let labels = severity_label(0) == "SUCCESS"
            && severity_label(1) == "INFO"
            && severity_label(2) == "WARN"
            && severity_label(3) == "ERROR"
            && severity_label(9) == "<bad>";
        let render = hex_ntstatus(0xC000_0005) == "0xC0000005";
        let mut b = FacilityBook::new();
        let _ = b.register(11, "WIN32K");
        let _ = b.use_code(11);
        let _ = b.use_code(11);
        let summary = b.summary_line(11).contains("WIN32K")
            && b.summary_line(99).contains("<none>")
            && b.name_of(11) == Some("WIN32K")
            && b.total_used() == 2;
        labels && render && summary
    };
    s.add("J13-标签渲染台账", j13, "三诊断面全对");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12021_encode_decode() {
        assert_eq!(encode(3, 0, 0, 0x22), Ok(0xC000_0022));
        assert_eq!(encode(0, 0, 0, 0), Ok(0));
        assert_eq!(encode(4, 0, 0, 0), Err(NF_BAD_SEVERITY));
        let p = decode(0xC000_0005);
        assert_eq!(p.code, 5);
        assert_eq!(p.severity, 3);
    }

    #[test]
    fn unx12021_labels_render() {
        assert_eq!(severity_label(3), "ERROR");
        assert_eq!(severity_label(0), "SUCCESS");
        assert_eq!(hex_ntstatus(0x0000_0000), "0x00000000");
        let mut b = FacilityBook::new();
        assert!(b.register(9, "NTERR").is_ok());
        assert_eq!(b.name_of(9), Some("NTERR"));
        assert_eq!(b.summary_line(9), "fac[9|NTERR|used=0]");
    }

    #[test]
    fn unx12021_checks_green() {
        let s = run_unx12021_checks();
        assert!(s.all_passed(), "UNX-F12021 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12021_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
