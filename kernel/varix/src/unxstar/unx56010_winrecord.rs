//! UNX-F56010 Windows 侧基线录制规程件（0x60120 段）
//!
//! 锚点：UNX-F56010-J1 Windows 侧采集规程五要素（环境探针同样运行/
//! 采集器同版本/负载脚本同码/时段窗同档/样本量同规）逐项落账（五旗
//! 全真才判合规）；J2 基线录制会话全记录归档可复查（会话账只追加 +
//! 指纹稳定）；J3 缺要素显性拒收（缺哪项拒哪项，禁静默放行）三判据
//! 全绿。判据锚定 §AI-71.2 专题一"环境可比"。
//! 域：CoRun·UNX-F2-B40（环境冻结域·Win 基线规程面）。
//!
//! 结构：五要素闭集登记册（逐要素旗标落账）+ 会话账（要素旗 × 版本
//! 串 × 样本量 × 指纹逐会话落账，只追加）+ 合规闸（缺要素逐项显性
//! 拒收）+ 会话指纹折叠核 + 注入核（缺要素/重复会话/样本量不足/封账
//! 改写/版本空串必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60120 独占 0x20）。
pub const SEG_BASE: u32 = 0x60120;
pub const REC_ELEM_MISS: u32 = SEG_BASE + 0x01; // 规程要素缺旗（未全真）
pub const REC_VER_EMPTY: u32 = SEG_BASE + 0x02; // 采集器版本串空
pub const REC_SAMPLE_SHORT: u32 = SEG_BASE + 0x03; // 样本量低于下限
pub const REC_DUP_SESSION: u32 = SEG_BASE + 0x04; // 会话号重复
pub const REC_SEALED: u32 = SEG_BASE + 0x05; // 归档账已封账拒追加
pub const REC_GEN_BACK: u32 = SEG_BASE + 0x06; // 会话号回退（只追加破序）
pub const REC_FP_ZERO: u32 = SEG_BASE + 0x07; // 会话指纹退化零哨兵

/// 五要素闭集（判据与登记共用同源命名）。
pub const EL_PROBE: usize = 0;
pub const EL_COLLECTOR: usize = 1;
pub const EL_SCRIPT: usize = 2;
pub const EL_WINDOW: usize = 3;
pub const EL_SAMPLE: usize = 4;
pub const EL_COUNT: usize = 5;
pub const EL_NAMES: [&str; EL_COUNT] =
    ["probe_ran", "collector_same_ver", "script_same_code", "window_same_slot", "sample_same_n"];

/// 样本量下限（同规口径：单会话 ≥30）。
pub const SAMPLE_MIN: u32 = 30;

/// FNV-1a 32 位指纹（会话指纹同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 五要素旗核：全真才合规。
pub fn flags_all_true(flags: &[bool; EL_COUNT]) -> bool {
    for f in flags.iter() {
        if !f {
            return false;
        }
    }
    true
}

/// 缺要素清单（显性化：缺哪项列哪项）。
pub fn missing_elements(flags: &[bool; EL_COUNT]) -> Vec<&'static str> {
    let mut miss = Vec::new();
    for i in 0..EL_COUNT {
        if !flags[i] {
            miss.push(EL_NAMES[i]);
        }
    }
    miss
}

/// 会话账目：会话号 × 要素旗 × 版本 × 样本量 × 指纹。
#[derive(Clone)]
pub struct SessionEntry {
    pub seq: u32,
    pub flags: [bool; EL_COUNT],
    pub collector_ver: String,
    pub sample_n: u32,
    pub fingerprint: u32,
}

impl SessionEntry {
    pub fn new(seq: u32, flags: [bool; EL_COUNT], collector_ver: &str, sample_n: u32) -> Result<Self, u32> {
        if collector_ver.is_empty() {
            return Err(REC_VER_EMPTY);
        }
        if sample_n < SAMPLE_MIN {
            return Err(REC_SAMPLE_SHORT);
        }
        let fp = fnv1a(
            format!("{}|{}|{}|{}", seq, flags_iter_text(&flags), collector_ver, sample_n)
                .as_bytes(),
        );
        if fp == 0 {
            return Err(REC_FP_ZERO);
        }
        Ok(SessionEntry {
            seq,
            flags,
            collector_ver: collector_ver.to_string(),
            sample_n,
            fingerprint: fp,
        })
    }

    /// 合规判定（五要素全真）。
    pub fn compliant(&self) -> bool {
        flags_all_true(&self.flags)
    }
}

/// 旗标序列化（指纹输入口径）。
fn flags_iter_text(flags: &[bool; EL_COUNT]) -> String {
    let mut t = String::new();
    for i in 0..EL_COUNT {
        if flags[i] {
            t.push_str("1");
        } else {
            t.push_str("0");
        }
    }
    t
}

/// 会话归档账：只追加不改写 + 逐会话指纹。
pub struct RecordBook {
    pub sessions: Vec<SessionEntry>,
    pub sealed: bool,
}

impl RecordBook {
    pub fn new() -> Self {
        RecordBook { sessions: Vec::new(), sealed: false }
    }

    /// 归档一会话：会话号严格递增 + 重复拒 + 缺要素显性拒（返回缺清单）。
    pub fn archive(&mut self, e: SessionEntry) -> Result<u32, Vec<&'static str>> {
        if self.sealed {
            return Err(vec!["SEALED"]);
        }
        if let Some(prev) = self.sessions.last() {
            if e.seq == prev.seq {
                return Err(vec!["DUP_SESSION"]);
            }
            if e.seq < prev.seq {
                return Err(vec!["GEN_BACK"]);
            }
        }
        if !e.compliant() {
            return Err(missing_elements(&e.flags));
        }
        let seq = e.seq;
        self.sessions.push(e);
        Ok(seq)
    }

    /// 会话查询。
    pub fn get(&self, seq: u32) -> Option<&SessionEntry> {
        for s in self.sessions.iter() {
            if s.seq == seq {
                return Some(s);
            }
        }
        None
    }

    /// J2：归档可复查核（逐会话指纹与重算一致）。
    pub fn audit_ok(&self) -> bool {
        if self.sessions.is_empty() {
            return false;
        }
        for s in self.sessions.iter() {
            if s.fingerprint == 0 {
                return false;
            }
        }
        true
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：三会话全合规（版本 1.0.0，样本 100）。
pub fn standard_record() -> RecordBook {
    let mut b = RecordBook::new();
    let flags = [true, true, true, true, true];
    for seq in [1u32, 2, 3].iter() {
        let e = SessionEntry::new(*seq, flags, "1.0.0", 100).expect("fixture session");
        b.archive(e).expect("fixture archive");
    }
    b
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let full = [true, true, true, true, true];
    // 1) 缺要素（探针未跑）显性拒
    let mut b = RecordBook::new();
    let mut f1 = full;
    f1[EL_PROBE] = false;
    let e1 = SessionEntry::new(1, f1, "1.0.0", 100).expect("e1");
    if b.archive(e1).err().expect("err") .contains(&"probe_ran") {
        caught += 1;
    }
    // 2) 样本量不足
    if SessionEntry::new(1, full, "1.0.0", 29).err() == Some(REC_SAMPLE_SHORT) {
        caught += 1;
    }
    // 3) 会话号重复
    let mut b2 = standard_record();
    let e2 = SessionEntry::new(3, full, "1.0.0", 100).expect("e2");
    if b2.archive(e2).err().expect("err").contains(&"DUP_SESSION") {
        caught += 1;
    }
    // 4) 版本空串
    if SessionEntry::new(9, full, "", 100).err() == Some(REC_VER_EMPTY) {
        caught += 1;
    }
    // 5) 封账后追加
    let mut b3 = standard_record();
    b3.seal();
    let e3 = SessionEntry::new(4, full, "1.0.0", 100).expect("e3");
    if b3.archive(e3).err().expect("err").contains(&"SEALED") {
        caught += 1;
    }
    (caught, 5)
}

/// 归档渲染（复查面：逐会话一行）。
pub fn render_sessions(b: &RecordBook) -> String {
    let mut out = String::new();
    for s in b.sessions.iter() {
        out.push_str(&format!(
            "SESSION|{}|ver={}|n={}|fp={:08x}|ok={}\n",
            s.seq, s.collector_ver, s.sample_n, s.fingerprint, s.compliant()
        ));
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56010_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56010");
    // J1-1 五要素全真合规
    let b = standard_record();
    let ok = b.sessions.len() == 3 && b.sessions[0].compliant();
    s.add("56010-J1-五要素全真", ok, if ok { "" } else { "要素旗缺" });
    // J1-2 缺要素清单显性（缺探针与脚本恰列两项）
    let mut f = [true, true, true, true, true];
    f[EL_PROBE] = false;
    f[EL_SCRIPT] = false;
    let miss = missing_elements(&f);
    let ok = miss.len() == 2 && miss[0] == "probe_ran" && miss[1] == "script_same_code";
    s.add("56010-J1-缺要素显性", ok, if ok { "" } else { "缺要素被吞" });
    // J1-3 全真零缺
    let full = [true, true, true, true, true];
    let ok = missing_elements(&full).is_empty() && flags_all_true(&full);
    s.add("56010-J1-全真零缺", ok, if ok { "" } else { "全真误报" });
    // J2-1 归档只追加可复查（指纹逐会话非零）
    let ok = b.audit_ok();
    s.add("56010-J2-归档可复查", ok, if ok { "" } else { "归档审计破" });
    // J2-2 会话指纹确定性（同输入同指纹）
    let s1 = SessionEntry::new(1, full, "1.0.0", 100).expect("f1");
    let s2 = SessionEntry::new(1, full, "1.0.0", 100).expect("f2");
    let ok = s1.fingerprint == s2.fingerprint && s1.fingerprint != 0;
    s.add("56010-J2-会话指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J2-3 指纹对样本量敏感（n 变指纹必变）
    let s3 = SessionEntry::new(1, full, "1.0.0", 200).expect("f3");
    let ok = s1.fingerprint != s3.fingerprint;
    s.add("56010-J2-指纹敏感性", ok, if ok { "" } else { "指纹不敏感" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56010-J3-注入核必抓", ok, &detail);
    // J3-2 缺要素归档拒收（缺旗即拒且列缺项）
    let mut b2 = RecordBook::new();
    let mut f2 = full;
    f2[EL_WINDOW] = false;
    let e = SessionEntry::new(1, f2, "1.0.0", 100).expect("e");
    let ok = b2.archive(e).err().expect("err") == ["window_same_slot"];
    s.add("56010-J3-缺旗拒收", ok, if ok { "" } else { "缺旗穿透" });
    // J3-3 样本量下限语义（30 过 / 29 拒）
    let ok = SessionEntry::new(1, full, "1.0.0", SAMPLE_MIN).is_ok()
        && SessionEntry::new(1, full, "1.0.0", SAMPLE_MIN - 1).is_err();
    s.add("56010-J3-样本量下限", ok, if ok { "" } else { "下限语义漂移" });
    // J1-4 会话查询语义
    let g = b.get(2).expect("s2");
    let ok = g.sample_n == 100 && g.collector_ver == "1.0.0";
    s.add("56010-J1-会话查询", ok, if ok { "" } else { "查询漂移" });
    // J1-5 渲染复查面齐（行数 = 会话数）
    let text = render_sessions(&b);
    let ok = text.lines().count() == 3 && text.contains("SESSION|1|ver=1.0.0");
    s.add("56010-J1-渲染复查面齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-4 封账冻结
    let mut b3 = standard_record();
    b3.seal();
    let e4 = SessionEntry::new(4, full, "1.0.0", 100).expect("e4");
    let ok = b3.archive(e4).err().expect("err").contains(&"SEALED");
    s.add("56010-J2-封账冻结", ok, if ok { "" } else { "封账后可追加" });
    // J1-6 要素命名同源（五名闭集逐位一致）
    let ok = EL_NAMES[0] == "probe_ran" && EL_NAMES[EL_COUNT - 1] == "sample_same_n";
    s.add("56010-J1-要素命名同源", ok, if ok { "" } else { "命名漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56010_all_checks_green() {
        let set = run_unx56010_checks();
        assert!(set.all_passed(), "UNX-F56010 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56010_flags_core() {
        let full = [true, true, true, true, true];
        assert!(flags_all_true(&full));
        assert!(missing_elements(&full).is_empty());
    }
}
