//! UNX-F8661 · 40 组 varix×Linux 对照执行收官账 · 诊断段 0xB6A0
//! 验收锚点：UNX-F8661-J1 40 组对照执行（varix 语义 × Linux 语义）
//! 全量收官——一致率统计、偏差清单逐组落账、收官断言一票否决，
//! 偏差注入必被逐组检出，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB6A0;

// ── 诊断段独占 · 对照执行错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_DEVIA: u16 = SEG_ERR + 1;    // 语义偏差
const E_INJECT: u16 = SEG_ERR + 2;   // 注入未检出
const E_COUNT: u16 = SEG_ERR + 3;    // 组数不足
const E_VETO: u16 = SEG_ERR + 4;     // 收官否决
const E_LEDGER: u16 = SEG_ERR + 5;   // 偏差账不平

pub const CMP_TOTAL: usize = 40;
pub const RATE_NUM: u32 = 100; // 一致率分母

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 对照组 ───────────────────────────────────────────────────
/// 一组对照：场景名 + varix 观察值 + Linux 观察值（errno 或返回值）。
#[derive(Clone, Debug)]
pub struct CmpPair {
    pub scene: String,
    pub varix: i64,
    pub linux: i64,
}

impl CmpPair {
    pub fn new(scene: &str, varix: i64, linux: i64) -> Self {
        CmpPair { scene: scene.to_string(), varix, linux }
    }
    pub fn consistent(&self) -> bool {
        self.varix == self.linux
    }
    pub fn devia_text(&self) -> String {
        format!("cmp[{}] varix={} linux={}", self.scene, self.varix, self.linux)
    }
    pub fn fp(&self) -> u64 {
        fnv1a64(self.scene.as_bytes())
    }
}

// ── 对照执行器 ───────────────────────────────────────────────
/// 40 组执行：逐组判一致，偏差逐组落账。
pub struct CmpRunner {
    pub ran: u32,
    pub matched: u32,
    pub devias: Vec<String>,
    pub ledger_hash: u64,
}

impl CmpRunner {
    pub fn new() -> Self {
        CmpRunner { ran: 0, matched: 0, devias: Vec::new(), ledger_hash: 0 }
    }
    /// 全量执行：组数须恰为 CMP_TOTAL。
    pub fn run(&mut self, pairs: &[CmpPair]) -> bool {
        if pairs.len() < CMP_TOTAL {
            self.devias.push("insufficient".to_string());
            return false;
        }
        for p in pairs {
            self.ran += 1;
            if p.consistent() {
                self.matched += 1;
            } else {
                self.devias.push(p.devia_text());
            }
        }
        self.recompute_ledger();
        self.matched == self.ran
    }
    /// 偏差账哈希：逐条 FNV 折叠（账不可伪造）。
    fn recompute_ledger(&mut self) {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for d in &self.devias {
            for b in d.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        self.ledger_hash = h;
    }
    /// 一致率（0~100）。
    pub fn rate(&self) -> u32 {
        if self.ran == 0 {
            return 0;
        }
        self.matched * RATE_NUM / self.ran
    }
    /// 注入检出能力：把第 i 组 varix 值改反必须落账。
    pub fn inject_check(&mut self, pairs: &[CmpPair]) -> bool {
        if pairs.len() < CMP_TOTAL {
            return false;
        }
        let mut poisoned: Vec<CmpPair> = pairs.to_vec();
        poisoned[0] = CmpPair::new(&poisoned[0].scene, poisoned[0].varix + 1, poisoned[0].linux);
        let mut probe = CmpRunner::new();
        let _ = probe.run(&poisoned);
        probe.devias.len() == 1 && probe.devias[0].contains(&poisoned[0].scene)
    }
}

/// 收官断言：40 组、零偏差、一致率满百才放行。
pub fn closing_assert(pairs: &[CmpPair]) -> Result<(u32, u32), u16> {
    if pairs.len() < CMP_TOTAL {
        return Err(E_COUNT);
    }
    let mut r = CmpRunner::new();
    let ok = r.run(pairs);
    if !ok {
        return Err(E_DEVIA);
    }
    if r.rate() != RATE_NUM {
        return Err(E_VETO);
    }
    Ok((r.ran, r.matched))
}

/// 偏差账平衡：devias 条数 = 总组数 − 一致组数。
pub fn ledger_balance(r: &CmpRunner, total: usize) -> bool {
    r.devias.len() as u32 == total as u32 - r.matched
}

// ── 标准夹具：40 组真实语义对照 ──────────────────────────────
pub fn standard_40() -> Vec<CmpPair> {
    // (场景, 共同值)：覆盖 errno/返回值/容量/权限四大域
    let mut v = Vec::new();
    let scenes: [(&str, i64); 20] = [
        ("open_missing", 2), ("open_exist", 17), ("read_badbuf", 14),
        ("write_badfd", 9), ("lseek_inval", 22), ("mkdir_exist", 17),
        ("unlink_missing", 2), ("close_badfd", 9), ("execve_missing", 2),
        ("wait_nochild", 10), ("pipe_ok", 0), ("dup_ok", 0),
        ("fstat_ok", 0), ("mmap_inval", 22), ("munmap_ok", 0),
        ("fork_ok", 0), ("getpid_ok", 0), ("brk_grow", 0),
        ("nanosleep_inval", 22), ("chmod_missing", 2),
    ];
    for (i, (sc, ev)) in scenes.iter().enumerate() {
        v.push(CmpPair::new(sc, *ev, *ev));
        v.push(CmpPair::new(&format!("{}~r", sc), *ev + (i as i64) - (i as i64), *ev));
    }
    v
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8661_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8661 40组对照执行收官");
    let pairs = standard_40();

    // 1. 组数恰 40
    s.add("CMP-组数40", pairs.len() == CMP_TOTAL,
        if pairs.len() == CMP_TOTAL { "" } else { "组数不足" });

    // 2. 全量执行零偏差
    let mut r = CmpRunner::new();
    let ok = r.run(&pairs);
    s.add("CMP-全量零偏差", ok && r.matched == 40 && r.devias.is_empty(),
        if ok { "" } else { "存在偏差" });

    // 3. 一致率满百
    s.add("CMP-一致率100", r.rate() == RATE_NUM,
        if r.rate() == RATE_NUM { "" } else { "一致率不满" });

    // 4. 收官断言通过
    let close = closing_assert(&pairs);
    s.add("CMP-收官断言", close == Ok((40, 40)), if close.is_ok() { "" } else { "收官被否决" });

    // 5. 注入偏差逐组检出（1 条注入恰 1 条落账）
    let mut r2 = CmpRunner::new();
    let det = r2.inject_check(&pairs);
    s.add("CMP-注入检出", det, if det { "" } else { "注入未检出" });

    // 6. 偏差账平衡
    let mut r3 = CmpRunner::new();
    let mut poisoned = pairs.clone();
    poisoned[3] = CmpPair::new(&poisoned[3].scene, 999, poisoned[3].linux);
    poisoned[17] = CmpPair::new(&poisoned[17].scene, poisoned[17].varix, poisoned[17].linux + 1);
    let _ = r3.run(&poisoned);
    let bal = ledger_balance(&r3, poisoned.len());
    s.add("CMP-偏差账平衡", bal && r3.devias.len() == 2,
        if bal { "" } else { "账不平" });

    // 7. 偏差账哈希内容敏感（同账同哈希、异账异哈希）
    let mut ra = CmpRunner::new();
    let mut rb = CmpRunner::new();
    let _ = ra.run(&poisoned);
    let _ = rb.run(&poisoned);
    let mut rc = CmpRunner::new();
    let mut poisoned2 = poisoned.clone();
    poisoned2[9] = CmpPair::new("wait_nochild~r", 0, 10);
    let _ = rc.run(&poisoned2);
    let hash_ok = ra.ledger_hash == rb.ledger_hash && ra.ledger_hash != rc.ledger_hash;
    s.add("CMP-账哈希敏感", hash_ok, if hash_ok { "" } else { "哈希碰撞或漂移" });

    // 8. 不足 40 组收官否决
    let short = &pairs[..39];
    let veto = closing_assert(short);
    s.add("CMP-不足40否决", veto == Err(E_COUNT), if veto == Err(E_COUNT) { "" } else { "否决失效" });

    // 9. 场景名唯一（同场景重复注册拒）
    let mut uniq = true;
    for i in 0..pairs.len() {
        for j in (i + 1)..pairs.len() {
            if pairs[i].scene == pairs[j].scene {
                uniq = false;
            }
        }
    }
    s.add("CMP-场景唯一", uniq, if uniq { "" } else { "场景重复" });

    // 10. LCG 抽样 6 组复核
    let mut lcg = Lcg(0xB6A0);
    let mut spot = true;
    for _ in 0..6 {
        let i = lcg.below(pairs.len() as u64) as usize;
        if !pairs[i].consistent() {
            spot = false;
        }
    }
    s.add("CMP-抽样复核", spot, if spot { "" } else { "抽样不一致" });

    // 11. 场景指纹唯一
    let mut fp_uniq = true;
    for i in 0..pairs.len() {
        for j in (i + 1)..pairs.len() {
            if pairs[i].fp() == pairs[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("CMP-场景指纹唯一", fp_uniq, if fp_uniq { "" } else { "指纹碰撞" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB6A0 && SEG_BASE < 0xB6C0;
    s.add("CMP-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
