// unx3762 —— UNX-F3762 段协议（各域账的摘要段 schema）
// 判据原文 J1：段 schema（五段 v1：覆盖段/桩段/门禁段/断言段/发现段——
// 判据行内嵌五段字段表——段 schema const 扩段走 bump）；
// 段与域账的投影关系（段字段←域账字段（映射表显式））；
// M-520 段投影（五域账→五段逐字段对账 ==）。
// J2：段封闭（v1 字段集 const——未知字段拒绝（F3667 同律））。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFF00 独占（0xFF00~0xFF7F）。
//
// 设计（纯功能 no_std）：
//   * 五段闭集：cover（覆盖）/stub（桩）/gate（门禁）/assert（断言）/finding（发现）；
//   * 字段集 const 冻结 v1：未知字段拒绝；扩段走 bump（schema_rev 递增）；
//   * 显式映射表：段字段 ← 域账字段（投影逐字段对账 ==）；
//   * 注入核：未知字段/投影缺列/段号越界/封账后投影 四类必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xFF00 独占） ─────────────────────────────────────────────
pub const XC_SEG_FIELD: u32 = 0xFF01; // 未知字段
pub const XC_SEG_PROJ: u32 = 0xFF02; // 投影缺列
pub const XC_SEG_OOB: u32 = 0xFF03; // 段号越界
pub const XC_SEG_SEAL: u32 = 0xFF04; // 封账后投影

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 五段闭集。
pub const SEG_NAMES: [&str; 5] = ["cover", "stub", "gate", "assert", "finding"];

pub fn seg_idx(name: &str) -> Option<usize> {
    SEG_NAMES.iter().position(|s| *s == name)
}

/// schema v1：五段各自字段集（const 刻印，扩段走 bump）。
pub const SCHEMA_REV: u32 = 1;
pub const FIELDS_COVER: [&str; 3] = ["domain", "covered", "total"];
pub const FIELDS_STUB: [&str; 2] = ["stub_count", "stub_sites"];
pub const FIELDS_GATE: [&str; 3] = ["gate_name", "threshold", "passed"];
pub const FIELDS_ASSERT: [&str; 3] = ["assert_total", "assert_pass", "assert_red"];
pub const FIELDS_FINDING: [&str; 3] = ["finding_id", "severity", "digest"];

pub fn fields_of(seg: usize) -> Option<&'static [&'static str]> {
    match seg {
        0 => Some(&FIELDS_COVER),
        1 => Some(&FIELDS_STUB),
        2 => Some(&FIELDS_GATE),
        3 => Some(&FIELDS_ASSERT),
        4 => Some(&FIELDS_FINDING),
        _ => None,
    }
}

pub fn field_known(seg: usize, field: &str) -> bool {
    fields_of(seg).map(|fs| fs.contains(&field)).unwrap_or(false)
}

/// 域账（真源）：每域若干键值（u64 域）。
#[derive(Clone)]
pub struct DomainLedger {
    pub name: &'static str,
    pub values: Vec<(&'static str, u64)>,
}

impl DomainLedger {
    pub fn get(&self, k: &str) -> Option<u64> {
        self.values.iter().find(|v| v.0 == k).map(|v| v.1)
    }
}

/// 段（投影目标）：段号 + 字段值集。
pub struct Seg {
    pub idx: usize,
    pub values: Vec<(&'static str, u64)>,
}

/// 投影器：域账 → 段（显式映射表驱动）。
pub struct Projector {
    /// 显式映射表：(段号, 段字段, 域账键)。
    map: Vec<(usize, &'static str, &'static str)>,
    frozen: Option<u32>,
    rejects: Vec<(u32, &'static str)>,
}

impl Projector {
    pub fn new() -> Self {
        Projector { map: Vec::new(), frozen: None, rejects: Vec::new() }
    }

    /// 登记显式映射（未知字段拒绝）。
    pub fn bind(&mut self, seg: usize, field: &'static str, ledger_key: &'static str) -> Result<(), u32> {
        if self.frozen.is_some() {
            self.rejects.push((XC_SEG_SEAL, "封账后投影"));
            return Err(XC_SEG_SEAL);
        }
        if seg >= 5 {
            self.rejects.push((XC_SEG_OOB, "段号越界"));
            return Err(XC_SEG_OOB);
        }
        if !field_known(seg, field) {
            self.rejects.push((XC_SEG_FIELD, "未知字段"));
            return Err(XC_SEG_FIELD);
        }
        self.map.push((seg, field, ledger_key));
        Ok(())
    }

    /// 投影：域账 → 段（逐字段取值，缺列拒绝）。
    pub fn project(&mut self, seg: usize, led: &DomainLedger) -> Result<Seg, u32> {
        if self.frozen.is_some() {
            self.rejects.push((XC_SEG_SEAL, "封账后投影"));
            return Err(XC_SEG_SEAL);
        }
        if seg >= 5 {
            self.rejects.push((XC_SEG_OOB, "段号越界"));
            return Err(XC_SEG_OOB);
        }
        let mut values: Vec<(&'static str, u64)> = Vec::new();
        for (s, f, k) in &self.map {
            if *s != seg {
                continue;
            }
            match led.get(k) {
                Some(v) => values.push((f, v)),
                None => {
                    self.rejects.push((XC_SEG_PROJ, "投影缺列"));
                    return Err(XC_SEG_PROJ);
                }
            }
        }
        Ok(Seg { idx: seg, values })
    }

    /// M-520 段投影对账：段值 == 域账值（经显式映射表逐字段 ==）。
    pub fn verify(&self, seg: &Seg, led: &DomainLedger) -> bool {
        // 段字段覆盖核：schema 全字段都有值
        let fs = match fields_of(seg.idx) {
            Some(fs) => fs,
            None => return false,
        };
        if !fs.iter().all(|f| seg.values.iter().any(|(vf, _)| vf == f)) {
            return false;
        }
        // 经映射表取域账键，逐字段 == 比对
        for (s, f, k) in &self.map {
            if *s != seg.idx {
                continue;
            }
            let sv = seg.values.iter().find(|(vf, _)| vf == f).map(|(_, v)| *v);
            let lv = led.get(k);
            match (sv, lv) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    pub fn freeze(&mut self) -> u32 {
        let mut b = Vec::new();
        for (s, f, k) in &self.map {
            b.push(*s as u8);
            b.extend_from_slice(f.as_bytes());
            b.push(0xFF);
            b.extend_from_slice(k.as_bytes());
            b.push(0xFE);
        }
        b.extend_from_slice(&SCHEMA_REV.to_le_bytes());
        let h = fnv1a32(&b);
        self.frozen = Some(h);
        h
    }
    pub fn fingerprint(&self) -> u32 {
        self.frozen.unwrap_or(0)
    }
    pub fn map_len(&self) -> usize {
        self.map.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 标准映射 + 五域账（M-520 全投影）。
pub fn standard_setup() -> (Projector, [DomainLedger; 5]) {
    let mut p = Projector::new();
    // cover 段 ← 覆盖域账
    let _ = p.bind(0, "domain", "dname");
    let _ = p.bind(0, "covered", "cov");
    let _ = p.bind(0, "total", "tot");
    // stub 段 ← 桩域账
    let _ = p.bind(1, "stub_count", "scnt");
    let _ = p.bind(1, "stub_sites", "ssites");
    // gate 段 ← 门禁域账
    let _ = p.bind(2, "gate_name", "gname");
    let _ = p.bind(2, "threshold", "gthr");
    let _ = p.bind(2, "passed", "gpass");
    // assert 段 ← 断言域账
    let _ = p.bind(3, "assert_total", "atot");
    let _ = p.bind(3, "assert_pass", "apass");
    let _ = p.bind(3, "assert_red", "ared");
    // finding 段 ← 发现域账
    let _ = p.bind(4, "finding_id", "fid");
    let _ = p.bind(4, "severity", "fsev");
    let _ = p.bind(4, "digest", "fdig");

    let ledgers = [
        DomainLedger {
            name: "cover",
            values: alloc::vec![("dname", 1), ("cov", 30), ("tot", 30)],
        },
        DomainLedger {
            name: "stub",
            values: alloc::vec![("scnt", 4), ("ssites", 12)],
        },
        DomainLedger {
            name: "gate",
            values: alloc::vec![("gname", 7), ("gthr", 300), ("gpass", 1)],
        },
        DomainLedger {
            name: "assert",
            values: alloc::vec![("atot", 560), ("apass", 560), ("ared", 0)],
        },
        DomainLedger {
            name: "finding",
            values: alloc::vec![("fid", 42), ("fsev", 2), ("fdig", 0xA11CE)],
        },
    ];
    (p, ledgers)
}

/// 注入核：未知字段/投影缺列/段号越界/封账后投影 四类必抓。
pub struct SegInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl SegInjectCore {
    pub fn new() -> Self {
        SegInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_unknown_field(&mut self) {
        self.total += 1;
        let mut p = Projector::new();
        let r = p.bind(0, "x9", "k");
        if r == Err(XC_SEG_FIELD) && p.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_proj_missing(&mut self) {
        self.total += 1;
        let (mut p, _) = standard_setup();
        let bad_led = DomainLedger {
            name: "stub",
            values: alloc::vec![("scnt", 4)], // 缺 ssites
        };
        let r = p.project(1, &bad_led);
        if matches!(r, Err(XC_SEG_PROJ)) {
            self.caught += 1;
        }
    }
    pub fn inject_seg_oob(&mut self) {
        self.total += 1;
        let (mut p, led) = standard_setup();
        let r = p.project(5, &led[0]);
        if matches!(r, Err(XC_SEG_OOB)) {
            self.caught += 1;
        }
    }
    pub fn inject_proj_after_seal(&mut self) {
        self.total += 1;
        let (mut p, led) = standard_setup();
        let _ = p.freeze();
        let r = p.project(0, &led[0]);
        if matches!(r, Err(XC_SEG_SEAL)) && p.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx3762_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx3762");

    // C62-01 五段闭集 + 段号映射
    {
        let ok = SEG_NAMES.len() == 5
            && seg_idx("cover") == Some(0)
            && seg_idx("finding") == Some(4)
            && seg_idx("x9").is_none();
        s.add("C62-01-五段闭集", ok, if ok { "" } else { "段集漂移" });
    }
    // C62-02 M-520 五域账→五段逐字段对账 ==
    {
        let (mut p, led) = standard_setup();
        let mut ok = true;
        for i in 0..5 {
            match p.project(i, &led[i]) {
                Ok(seg) => {
                    if !p.verify(&seg, &led[i]) {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        s.add("C62-02-段投影对账", ok, if ok { "" } else { "投影对账断" });
    }
    // C62-03 显式映射表完整性（14 绑定）
    {
        let (p, _) = standard_setup();
        s.add(
            "C62-03-映射表",
            p.map_len() == 14,
            if p.map_len() == 14 { "" } else { "绑定缺列" },
        );
    }
    // C62-04 schema v1 字段集 const（扩段走 bump）
    {
        let ok = SCHEMA_REV == 1
            && field_known(0, "covered")
            && field_known(4, "digest")
            && !field_known(0, "digest");
        s.add("C62-04-schema 冻结", ok, if ok { "" } else { "字段集漂移" });
    }
    // C62-05 冻结哈希恒等
    {
        let (mut p, _) = standard_setup();
        let h = p.freeze();
        s.add(
            "C62-05-哈希入册",
            h != 0 && p.fingerprint() == h,
            if h != 0 { "" } else { "指纹漂移" },
        );
    }
    // C62-06 注入核 4/4
    {
        let mut inj = SegInjectCore::new();
        inj.inject_unknown_field();
        inj.inject_proj_missing();
        inj.inject_seg_oob();
        inj.inject_proj_after_seal();
        s.add(
            "C62-06-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
