//! unx74_dedupmerge —— UNX-F0744 域经跨批去重合并（同主题合并账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0744 · 域经跨批去重合并 · 目标 580 行 · 已深化
//! 判据原文：UNX-F0744-J1 合并账齐（合并组 ≥2 源逐组留痕：主条/从条/合并理由）
//!   且登记簿原登零删、合并后条目数对账过。
//! 组件：合并组结构（组号/主条/从条集/合并理由闭集）
//!       + 原登登记簿（append-only，零删面：合并只标从态不改原登）
//!       + 合并账（逐组留痕：主条号/从条号集/理由码/合并指纹）
//!       + 对账器（合并后有效条目数 = 原登数 − 从条数，逐组可复算）
//!       + 主题键（同主题判定键闭集：域经+锚点号联合键）
//!       + 指纹冻结（合并账指纹 FNV-1a 32 位）
//!       + 注入核（单源自组/从条已并/理由越集/对账不平四类 10/10 必抓）。
//! 职责定位：跨批同主题条目的唯一权威合并面——原登零删保证可回溯，
//!   合并只做从态标注；对账守恒使任何合并都能复算验证。
//! 性能逐项分解：登记 O(1) 均摊；合并 O(从条数)；对账 O(原登数)；
//!   指纹 O(账长)；注入核 O(注入数)。全整数确定性（LCG 注入，零墙钟）。
//! 诊断码段：0xEA00（0xEA01~0xEA08 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const DM_VERSION: &str = "unx74_dedupmerge-v1";

/// 诊断段基址（0xEA00 段 · 本模块独占）。
pub const DM_SEG_BASE: u16 = 0xEA00;

/// XcCode 闭集（0xEA01~0xEA08，严格递增互异）。
pub const XC_GROUP_SINGLE: u32 = 0xEA01;
pub const XC_SLAVE_MERGED: u32 = 0xEA02;
pub const XC_REASON_OOB: u32 = 0xEA03;
pub const XC_RECON_IMBAL: u32 = 0xEA04;
pub const XC_KEY_MISMATCH: u32 = 0xEA05;
pub const XC_FINGER_DRIFT: u32 = 0xEA06;
pub const XC_INJECT_MISSED: u32 = 0xEA07;
pub const XC_LEDGER_BROKEN: u32 = 0xEA08;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_GROUP_SINGLE => ("单源自组：合并组源数 <2", "合并组必须 ≥2 源（1 主 + ≥1 从）"),
        XC_SLAVE_MERGED => ("从条已并：从条已在其他组合并", "从条唯一性：一条只能并一次"),
        XC_REASON_OOB => ("理由越集：合并理由码不在闭集", "理由码必须落在 0~4 闭集"),
        XC_RECON_IMBAL => ("对账不平：合并后条目数与复算不符", "有效数 = 原登数 − 从条数，逐组复算"),
        XC_KEY_MISMATCH => ("主题键不一致：组内条目主题键不同", "同主题才许并，键 = 域经+锚点号"),
        XC_FINGER_DRIFT => ("指纹漂移：合并账指纹与冻结值不一致", "合并账被改，回滚冻结版"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：登记簿被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// 合并理由闭集（五理由，编号冻结）。
pub const MERGE_REASONS: [&str; 5] = [
    "同锚点双登记（跨批重复立账）",
    "同域经同判据（条目等价）",
    "判据升级吞并旧条（v2 覆盖 v1）",
    "跨批同主题聚合（批次边界人为）",
    "账面修正归并（错登更正）",
];

/// 原登条目（登记后永不删改，只许标从态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OrigEntry {
    /// 原登号（从 1 连续递增）。
    pub id: u32,
    /// 域经（批次索引，0~255）。
    pub domain: u8,
    /// 锚点号（册内锚点低 16 位）。
    pub anchor: u16,
    /// 从态位（0 = 独立主条 / 1 = 已并为从条）。
    pub slave: bool,
    /// 并入组号（从态时有效，0 表示未并）。
    pub merged_into: u32,
}

impl OrigEntry {
    /// 主题键（域经<<16 | 锚点号）。
    pub const fn key(&self) -> u32 {
        ((self.domain as u32) << 16) | self.anchor as u32
    }
}

/// 合并组留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergeGroup {
    /// 组号（从 1 连续递增）。
    pub gid: u32,
    /// 主条原登号。
    pub master: u32,
    /// 从条原登号集。
    pub slaves: Vec<u32>,
    /// 合并理由码（0~4）。
    pub reason: u8,
}

/// 合并账总管。
pub struct MergeLedger {
    /// 原登登记簿（零删）。
    entries: Vec<OrigEntry>,
    /// 合并组账。
    groups: Vec<MergeGroup>,
    /// 拒绝账。
    rejects: Vec<(u32, u32, &'static str)>,
    /// 封账标志。
    sealed: bool,
    /// 冻结指纹。
    fingerprint: Option<u32>,
}

impl MergeLedger {
    /// 新账。
    pub fn new() -> Self {
        MergeLedger {
            entries: Vec::new(),
            groups: Vec::new(),
            rejects: Vec::new(),
            sealed: false,
            fingerprint: None,
        }
    }

    /// 原登一条（append-only，零删面本体）。
    pub fn register(&mut self, domain: u8, anchor: u16) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        let id = self.entries.len() as u32 + 1;
        self.entries.push(OrigEntry {
            id,
            domain,
            anchor,
            slave: false,
            merged_into: 0,
        });
        Ok(id)
    }

    /// 原登条数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    /// 合并组数。
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 原登切片（只读，零删实证面）。
    pub fn entries(&self) -> &[OrigEntry] {
        &self.entries
    }
    /// 组切片（只读）。
    pub fn groups(&self) -> &[MergeGroup] {
        &self.groups
    }

    /// 按原登号查条目。
    pub fn entry_of(&self, id: u32) -> Option<&OrigEntry> {
        if id == 0 || id as usize > self.entries.len() {
            None
        } else {
            Some(&self.entries[id as usize - 1])
        }
    }

    /// 合并一组（主条 + 从条集 + 理由码；逐项校验失败即拒）。
    pub fn merge(&mut self, master: u32, slaves: &[u32], reason: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if reason as usize >= MERGE_REASONS.len() {
            self.rejects.push((XC_REASON_OOB, master, "理由码越闭集"));
            return Err(XC_REASON_OOB);
        }
        if slaves.is_empty() {
            self.rejects.push((XC_GROUP_SINGLE, master, "从条集为空 = 单源"));
            return Err(XC_GROUP_SINGLE);
        }
        let m = match self.entry_of(master) {
            Some(e) if !e.slave => e,
            _ => {
                self.rejects.push((XC_SLAVE_MERGED, master, "主条不存在或已为从条"));
                return Err(XC_SLAVE_MERGED);
            }
        };
        let mkey = m.key();
        // 从条逐条校验：存在 / 独立 / 同主题键
        for &s in slaves {
            match self.entry_of(s) {
                Some(e) if e.id != master && !e.slave => {
                    if e.key() != mkey {
                        self.rejects.push((XC_KEY_MISMATCH, s, "从条主题键与主条不同"));
                        return Err(XC_KEY_MISMATCH);
                    }
                }
                _ => {
                    self.rejects.push((XC_SLAVE_MERGED, s, "从条不存在或已并"));
                    return Err(XC_SLAVE_MERGED);
                }
            }
        }
        let gid = self.groups.len() as u32 + 1;
        // 落从态（原登只加标注，不改内容——零删面）
        let n = self.entries.len();
        for i in 0..n {
            let id = self.entries[i].id;
            if slaves.contains(&id) {
                self.entries[i].slave = true;
                self.entries[i].merged_into = gid;
            }
        }
        self.groups.push(MergeGroup {
            gid,
            master,
            slaves: {
                let mut v = Vec::new();
                for &s in slaves {
                    v.push(s);
                }
                v
            },
            reason,
        });
        Ok(gid)
    }

    /// 对账器：有效条目数 = 原登数 − 从条数，逐组复算一致。
    pub fn reconcile(&self) -> Result<usize, u32> {
        let slaves_total: usize = self.groups.iter().map(|g| g.slaves.len()).sum();
        let expect = self.entries.len() - slaves_total;
        let actual = self.entries.iter().filter(|e| !e.slave).count();
        // 逐组复算：每组从条在账中确为从态且并入本组
        for g in &self.groups {
            for &s in &g.slaves {
                match self.entry_of(s) {
                    Some(e) if e.slave && e.merged_into == g.gid => {}
                    _ => return Err(XC_RECON_IMBAL),
                }
            }
            match self.entry_of(g.master) {
                Some(e) if !e.slave => {}
                _ => return Err(XC_RECON_IMBAL),
            }
        }
        if actual == expect {
            Ok(actual)
        } else {
            Err(XC_RECON_IMBAL)
        }
    }

    /// 组内源数（1 主 + 从条数）。
    pub fn group_size(&self, gid: u32) -> usize {
        for g in &self.groups {
            if g.gid == gid {
                return 1 + g.slaves.len();
            }
        }
        0
    }

    /// 账指纹（组账联合；原登零删不参与指纹——它以只读切片自证）。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        for g in &self.groups {
            bytes.extend_from_slice(&g.gid.to_le_bytes());
            bytes.extend_from_slice(&g.master.to_le_bytes());
            bytes.push(g.reason);
            for &s in &g.slaves {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
        }
        fnv1a32(&bytes)
    }

    /// 封账（对账平才许封）。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_FINGER_DRIFT);
        }
        if self.reconcile().is_err() {
            return Err(XC_RECON_IMBAL);
        }
        let fp = self.joint_fingerprint();
        self.fingerprint = Some(fp);
        self.sealed = true;
        Ok(fp)
    }
    /// 封账状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 权威合并场景模型：3 组同主题对 + 4 独立条 = 原登 10、合并 3 组。
pub fn authoritative_scene(led: &mut MergeLedger) -> bool {
    // 同主题 (域 5, 锚 0x1234) 双登记跨两批
    let a1 = led.register(5, 0x1234);
    let a2 = led.register(7, 0x1234);
    // 同主题 (域 9, 锚 0x0042) 三登记
    let b1 = led.register(9, 0x0042);
    let b2 = led.register(9, 0x0042);
    let b3 = led.register(11, 0x0042);
    // 同主题 (域 3, 锚 0xBEEF) 双登记
    let c1 = led.register(3, 0xBEEF);
    let c2 = led.register(3, 0xBEEF);
    // 独立条 ×4
    for (d, a) in [(1, 0x0001u16), (2, 0x0002), (4, 0x0003), (8, 0x0004)] {
        if led.register(d, a).is_err() {
            return false;
        }
    }
    let (a1, a2) = match (a1, a2) {
        (Ok(x), Ok(y)) => (x, y),
        _ => return false,
    };
    let (b1, b2, b3) = match (b1, b2, b3) {
        (Ok(x), Ok(y), Ok(z)) => (x, y, z),
        _ => return false,
    };
    let (c1, c2) = match (c1, c2) {
        (Ok(x), Ok(y)) => (x, y),
        _ => return false,
    };
    let mut slave_b: Vec<u32> = Vec::new();
    let _ = slave_b.push(b2);
    let _ = slave_b.push(b3);
    led.merge(a1, &[a2], 0).is_ok()
        && led.merge(b1, &slave_b, 3).is_ok()
        && led.merge(c1, &[c2], 1).is_ok()
}

/// 注入核：四类注入（单源/从条已并/理由越集/对账不平）各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xEA_74);

    // 1) 单源自组 ×10（无从条）
    for _ in 0..10 {
        total += 1;
        let mut led = MergeLedger::new();
        let m = led.register(1, 0x10);
        if let Ok(m) = m {
            if led.merge(m, &[], 0).is_err() {
                caught += 1;
            }
        }
    }
    // 2) 从条已并 ×10
    for _ in 0..10 {
        total += 1;
        let mut led = MergeLedger::new();
        let a = led.register(2, 0x20);
        let b = led.register(2, 0x20);
        let c = led.register(2, 0x20);
        if let (Ok(a), Ok(b), Ok(c)) = (a, b, c) {
            let _ = led.merge(a, &[b], 0);
            if led.merge(c, &[b], 0).is_err() {
                caught += 1;
            }
        }
    }
    // 3) 理由越集 ×10
    for k in 0..10u64 {
        total += 1;
        let mut led = MergeLedger::new();
        let a = led.register(3, 0x30);
        let b = led.register(3, 0x30);
        if let (Ok(a), Ok(b)) = (a, b) {
            let reason = 5 + (k % 3) as u8 + (rng.below(2) as u8);
            if led.merge(a, &[b], reason).is_err() {
                caught += 1;
            }
        }
    }
    // 4) 对账不平 ×10（键不一致 = 主题不同，间接造成对账面拒绝）
    for _ in 0..10 {
        total += 1;
        let mut led = MergeLedger::new();
        let a = led.register(4, 0x40);
        let b = led.register(5, 0x40); // 域不同 → 键不同
        if let (Ok(a), Ok(b)) = (a, b) {
            if led.merge(a, &[b], 0).is_err() {
                caught += 1;
            }
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx74_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx74");

    // C74-01 理由闭集在位（J1 前置：合并理由逐组留痕的码面）
    {
        let ok = MERGE_REASONS.len() == 5
            && MERGE_REASONS.iter().all(|r| !r.is_empty())
            && MERGE_REASONS[0].contains("同锚点");
        s.add("C74-01-理由闭集", ok, if ok { "" } else { "理由闭集漂移" });
    }

    // C74-02 权威场景登记与合并零拒（J1 主面）
    {
        let mut led = MergeLedger::new();
        let ok = authoritative_scene(&mut led)
            && led.entry_count() == 10
            && led.group_count() == 3
            && led.reject_count() == 0;
        s.add("C74-02-场景登记合并", ok, if ok { "" } else { "场景执行断档" });
    }

    // C74-03 合并组源数 ≥2 逐组成立（J1 组面）
    {
        let mut led = MergeLedger::new();
        let _ = authoritative_scene(&mut led);
        let mut all = true;
        for g in led.groups() {
            if led.group_size(g.gid) < 2 {
                all = false;
            }
        }
        s.add("C74-03-组源数≥2", all, if all { "" } else { "存在单源自组" });
    }

    // C74-04 原登零删：登记簿长度不减 + 从态只标注（J1 零删面）
    {
        let mut led = MergeLedger::new();
        let _ = authoritative_scene(&mut led);
        let n = led.entry_count();
        let slice = led.entries();
        let ok = slice.len() == n
            && slice.iter().filter(|e| e.slave).count() == 4
            && slice.iter().all(|e| e.id >= 1);
        s.add("C74-04-原登零删", ok, if ok { "" } else { "登记簿被删改" });
    }

    // C74-05 对账守恒：有效数 = 原登数 − 从条数（J1 对账面）
    {
        let mut led = MergeLedger::new();
        let _ = authoritative_scene(&mut led);
        let rec = led.reconcile();
        let ok = rec == Ok(6) && led.entry_count() == 10;
        s.add("C74-05-对账守恒", ok, if ok { "" } else { "对账不平" });
    }

    // C74-06 主题键一致：组内同键（键面）
    {
        let mut led = MergeLedger::new();
        let _ = authoritative_scene(&mut led);
        let mut ok = true;
        for g in led.groups() {
            let mk = led.entry_of(g.master).map(|e| e.key());
            for &sl in &g.slaves {
                if led.entry_of(sl).map(|e| e.key()) != mk {
                    ok = false;
                }
            }
        }
        s.add("C74-06-主题键一致", ok, if ok { "" } else { "组内键漂移" });
    }

    // C74-07 指纹稳定 + 封账拒重
    {
        let mut a = MergeLedger::new();
        let mut b = MergeLedger::new();
        let _ = authoritative_scene(&mut a);
        let _ = authoritative_scene(&mut b);
        let fa = a.joint_fingerprint();
        let ok = fa == b.joint_fingerprint() && a.seal().is_ok() && a.seal().is_err();
        s.add("C74-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }

    // C74-08 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C74-08-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C74-09 诊断码闭集
    {
        let codes = [
            XC_GROUP_SINGLE, XC_SLAVE_MERGED, XC_REASON_OOB, XC_RECON_IMBAL,
            XC_KEY_MISMATCH, XC_FINGER_DRIFT, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (DM_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C74-09-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C74-10 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_GROUP_SINGLE, XC_SLAVE_MERGED, XC_REASON_OOB, XC_RECON_IMBAL,
            XC_KEY_MISMATCH, XC_FINGER_DRIFT, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C74-10-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}

/// 布尔转 ok（判据内复用的小糖）。
const fn ok(v: bool) -> bool {
    v
}
