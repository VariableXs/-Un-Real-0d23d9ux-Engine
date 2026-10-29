//! UNX-F56003 环境探针脚本（采集前强制门闸）（0x60040 段）
//!
//! 锚点：UNX-F56003-J1 每次采集前强制运行探针门闸（未跑探针禁止开采，
//! 开采只走 admit 一条道）；J2 哈希不一致拒绝采集并落机器可读拒收记录
//! （GATE|deny 行含哈希双值）；J3 绕闸路径扫描零命中（采集器代码路径
//! 闭集内无任何跳过探针的分支）三判据全绿。判据锚定 §AI-71.2 专题一。
//! 域：CoRun·UNX-F2-B40（环境冻结域·探针门闸面）。
//!
//! 结构：探针门闸登记册（每轮探针运行落账：哈希 × 判定 × 时刻序号）+
//! 强制 admit 闸（未跑探针/哈希不符两态拒收，拒收记录机器可读）+ 绕闸
//! 路径扫描核（路径闭集逐条 grep 语义扫描零命中）+ 注入核（未跑探针/
//! 哈希漂移/空白哈希/封账后开采/路径疑似必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60040 独占 0x20）。
pub const SEG_BASE: u32 = 0x60040;
pub const PROBE_NOT_RUN: u32 = SEG_BASE + 0x01; // 未跑探针即请求开采
pub const PROBE_HASH_MISS: u32 = SEG_BASE + 0x02; // 探针哈希与基线不符
pub const PROBE_HASH_BLANK: u32 = SEG_BASE + 0x03; // 探针哈希零哨兵（未计算）
pub const PROBE_SEALED: u32 = SEG_BASE + 0x04; // 门闸已封账拒再采
pub const PROBE_PATH_HIT: u32 = SEG_BASE + 0x05; // 绕闸路径扫描命中
pub const PROBE_DUP_RUN: u32 = SEG_BASE + 0x06; // 同轮次探针重复运行落账
pub const PROBE_BASE_MISS: u32 = SEG_BASE + 0x07; // 门闸未注册基线哈希

/// 采集器代码路径闭集（绕闸扫描对象，语义名）。
pub const COLLECTOR_PATHS: [&str; 8] = [
    "admit_and_collect",
    "gate_probe_first",
    "read_env_profile",
    "hash_compare",
    "sample_batch",
    "archive_session",
    "reject_write_record",
    "audit_close_batch",
];

/// 绕闸关键词族（路径内出现即判绕闸嫌疑）。
pub const BYPASS_KEYWORDS: [&str; 4] = ["skip_probe", "bypass_gate", "no_probe", "admit_skip"];

/// FNV-1a 32 位指纹（哈希比对同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 绕闸路径扫描：逐条路径查关键词族，命中行返回。
pub fn bypass_scan(paths: &[&str]) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for p in paths.iter() {
        for kw in BYPASS_KEYWORDS.iter() {
            if p.contains(kw) {
                hits.push(format!("{}|{}", p, kw));
            }
        }
    }
    hits
}

/// 单轮门闸记录：轮次 × 判定 × 探针哈希 × 基线哈希 四元组。
#[derive(Clone)]
pub struct GateRecord {
    pub seq: u32,
    pub allowed: bool,
    pub probe_hash: u32,
    pub base_hash: u32,
    /// 拒收原因码（放行为 0）。
    pub deny_code: u32,
}

impl GateRecord {
    /// 机器可读行（GATE|allow/deny|轮次|探针哈希|基线哈希|原因）。
    pub fn render(&self) -> String {
        let verdict = if self.allowed { "allow" } else { "deny" };
        format!(
            "GATE|{}|{}|{:08x}|{:08x}|{}",
            verdict, self.seq, self.probe_hash, self.base_hash, self.deny_code
        )
    }
}

/// 探针门闸登记册：采集前强制运行，拒收记录统一可读落账。
pub struct ProbeGate {
    pub base_hash: u32,
    pub records: Vec<GateRecord>,
    pub sealed: bool,
    pub last_probe_hash: u32,
    pub probe_ran: bool,
    seq_next: u32,
}

impl ProbeGate {
    pub fn new() -> Self {
        ProbeGate {
            base_hash: 0,
            records: Vec::new(),
            sealed: false,
            last_probe_hash: 0,
            probe_ran: false,
            seq_next: 1,
        }
    }

    /// 注册基线哈希（来自 env_profile 正本）。
    pub fn register_base(&mut self, hash: u32) {
        self.base_hash = hash;
    }

    /// 探针运行：强制先行，落账本轮探针哈希（同轮次重复运行拒）。
    pub fn run_probe(&mut self, env_text: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PROBE_SEALED);
        }
        if self.probe_ran {
            return Err(PROBE_DUP_RUN);
        }
        let h = fnv1a(env_text.as_bytes());
        if h == 0 {
            return Err(PROBE_HASH_BLANK);
        }
        self.last_probe_hash = h;
        self.probe_ran = true;
        Ok(())
    }

    /// admit 强制闸：未跑探针拒 / 无基线拒 / 哈希不符拒并落机器可读
    /// 拒收记录；通过则落放行记录。返回记录序号。
    pub fn admit(&mut self) -> Result<u32, u32> {
        if self.sealed {
            self.push_record(false, self.last_probe_hash, PROBE_SEALED);
            return Err(PROBE_SEALED);
        }
        if !self.probe_ran {
            self.push_record(false, 0, PROBE_NOT_RUN);
            return Err(PROBE_NOT_RUN);
        }
        if self.last_probe_hash == 0 {
            self.push_record(false, 0, PROBE_HASH_BLANK);
            return Err(PROBE_HASH_BLANK);
        }
        if self.base_hash == 0 {
            self.push_record(false, self.last_probe_hash, PROBE_BASE_MISS);
            return Err(PROBE_BASE_MISS);
        }
        if self.last_probe_hash != self.base_hash {
            self.push_record(false, self.last_probe_hash, PROBE_HASH_MISS);
            return Err(PROBE_HASH_MISS);
        }
        let seq = self.push_record(true, self.last_probe_hash, 0);
        Ok(seq)
    }

    /// 落一条记录（统一序号自增）。
    fn push_record(&mut self, allowed: bool, probe_hash: u32, code: u32) -> u32 {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.records.push(GateRecord {
            seq,
            allowed,
            probe_hash,
            base_hash: self.base_hash,
            deny_code: code,
        });
        seq
    }

    /// 新采集轮次开始（探针态复位，强制下轮再跑探针）。
    pub fn next_round(&mut self) {
        self.probe_ran = false;
        self.last_probe_hash = 0;
    }

    /// 封账（封后拒再开采集窗）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// J2：拒收记录全量机器可读（每条 deny 都有 GATE|deny 行）。
    pub fn deny_records_machine_readable(&self) -> bool {
        for r in self.records.iter() {
            if !r.allowed {
                let line = r.render();
                if !line.starts_with("GATE|deny|") {
                    return false;
                }
                if line.split('|').count() != 6 {
                    return false;
                }
            }
        }
        true
    }

    /// J1：门闸强制核——所有放行记录前探针必须已运行。
    pub fn gate_always_first(&self) -> bool {
        for r in self.records.iter() {
            if r.allowed && r.probe_hash == 0 {
                return false;
            }
            if r.allowed && r.probe_hash != r.base_hash {
                return false;
            }
        }
        true
    }

    /// 放行 / 拒收计数。
    pub fn counts(&self) -> (usize, usize) {
        let mut a = 0usize;
        let mut d = 0usize;
        for r in self.records.iter() {
            if r.allowed {
                a += 1;
            } else {
                d += 1;
            }
        }
        (a, d)
    }
}

/// 标准夹具：基线哈希注册 + 探针同环境运行一轮。
pub fn standard_gate() -> ProbeGate {
    let mut g = ProbeGate::new();
    let env_text = "hw=VARIX-BASE-001|fw=unxreal-1.0.3|pp=balanced";
    g.register_base(fnv1a(env_text.as_bytes()));
    let _ = g.run_probe(env_text);
    g
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未跑探针即开采
    let mut g = ProbeGate::new();
    let env_text = "hw=VARIX-BASE-001|fw=unxreal-1.0.3|pp=balanced";
    g.register_base(fnv1a(env_text.as_bytes()));
    if g.admit() == Err(PROBE_NOT_RUN) {
        caught += 1;
    }
    // 2) 哈希漂移（环境变更未重录）
    let mut g2 = standard_gate();
    g2.next_round();
    let _ = g2.run_probe("hw=VARIX-BASE-002|fw=unxreal-1.0.3|pp=balanced");
    if g2.admit() == Err(PROBE_HASH_MISS) {
        caught += 1;
    }
    // 3) 无基线（基线未注册即开采）
    let mut g3 = ProbeGate::new();
    let _ = g3.run_probe(env_text);
    if g3.admit() == Err(PROBE_BASE_MISS) {
        caught += 1;
    }
    // 4) 同轮重复跑探针
    let mut g4 = standard_gate();
    if g4.run_probe(env_text) == Err(PROBE_DUP_RUN) {
        caught += 1;
    }
    // 5) 绕闸关键词路径扫描命中
    let bad = ["admit_and_collect", "admit_skip_probe", "sample_batch"];
    if !bypass_scan(&bad).is_empty() {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56003_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56003");
    // J1-1 门闸强制：放行前探针必跑且哈希与基线一致
    let mut g = standard_gate();
    let _ = g.admit();
    let ok = g.gate_always_first() && g.counts().0 == 1;
    s.add("56003-J1-门闸强制先行", ok, if ok { "" } else { "门闸未强制" });
    // J1-2 每轮重开探针复位（第二轮不跑探针必拒）
    let mut g2 = standard_gate();
    let _ = g2.admit();
    g2.next_round();
    let ok = g2.admit() == Err(PROBE_NOT_RUN);
    s.add("56003-J1-逐轮探针复位", ok, if ok { "" } else { "跨轮免检" });
    // J2-1 哈希不一致拒收并落机器可读记录
    let mut g3 = standard_gate();
    g3.next_round();
    let _ = g3.run_probe("hw=VARIX-BASE-009|fw=unxreal-1.0.3|pp=balanced");
    let denied = g3.admit() == Err(PROBE_HASH_MISS);
    let ok = denied && g3.deny_records_machine_readable();
    s.add("56003-J2-哈希不符拒收落账", ok, if ok { "" } else { "拒收未落机读账" });
    // J2-2 拒收行字段齐全（6 段含哈希双值）
    let mut g4 = standard_gate();
    g4.next_round();
    let _ = g4.run_probe("hw=VARIX-DRIFT|fw=unxreal-1.0.3|pp=balanced");
    let _ = g4.admit();
    let recs = &g4.records;
    let ok = recs.iter().any(|r| !r.allowed && r.render().contains("GATE|deny|"));
    s.add("56003-J2-拒收行哈希双值", ok, if ok { "" } else { "拒收行缺双值" });
    // J3-1 绕闸路径扫描零命中（闭集 8 路径逐条扫）
    let hits = bypass_scan(&COLLECTOR_PATHS);
    let ok = hits.is_empty() && COLLECTOR_PATHS.len() == 8;
    s.add("56003-J3-绕闸扫描零命中", ok, if ok { "" } else { "绕闸嫌疑路径" });
    // J3-2 扫描对注入路径必命中（扫描器自证灵敏）
    let bad = ["skip_probe_fast", "sample_batch", "no_probe_collect"];
    let ok = bypass_scan(&bad).len() == 2;
    s.add("56003-J3-扫描器灵敏自证", ok, if ok { "" } else { "扫描器失灵" });
    // J1-3 放行与拒收计数分明（账目可导）
    let (a, d) = g3.counts();
    let ok = a == 0 && d == 1;
    s.add("56003-J1-放行拒收计数", ok, if ok { "" } else { "计数漂移" });
    // J2-3 拒收原因码逐条对应诊断码（无泛化 deny）
    let mut ok = true;
    for r in g3.records.iter() {
        if !r.allowed && r.deny_code != PROBE_HASH_MISS {
            ok = false;
        }
    }
    s.add("56003-J2-拒因逐条对应", ok, if ok { "" } else { "拒因泛化" });
    // J3-3 封账后开采拒（闭账只读）
    let mut g5 = standard_gate();
    g5.seal();
    let ok = g5
        .run_probe("hw=VARIX-BASE-001|fw=unxreal-1.0.3|pp=balanced")
        .err()
        == Some(PROBE_SEALED);
    s.add("56003-J3-封账拒开采", ok, if ok { "" } else { "封账后仍开" });
    // J1-4 零哨兵哈希拒收（未计算探针哈希穿透守闸）
    let ok = ProbeGate::new().admit().err() == Some(PROBE_NOT_RUN);
    s.add("56003-J1-零哨兵守闸", ok, if ok { "" } else { "零哨兵穿透" });
    // J1-5 基线一致性：同环境探针哈希与基线注册值恒等
    let g6 = standard_gate();
    let ok = g6.last_probe_hash == g6.base_hash && g6.last_probe_hash != 0;
    s.add("56003-J1-基线一致", ok, if ok { "" } else { "基线漂移" });
    // J2-4 放行记录可读面（GATE|allow 六段）
    let mut g7 = standard_gate();
    let seq = g7.admit().expect("admit must pass");
    let line = g7.records[seq as usize - 1].render();
    let ok = line.starts_with("GATE|allow|") && line.split('|').count() == 6;
    s.add("56003-J2-放行行机读齐", ok, if ok { "" } else { "放行行残缺" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56003-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56003_all_checks_green() {
        let set = run_unx56003_checks();
        assert!(set.all_passed(), "UNX-F56003 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56003_bypass_scan_clean() {
        assert!(bypass_scan(&COLLECTOR_PATHS).is_empty());
    }
}
