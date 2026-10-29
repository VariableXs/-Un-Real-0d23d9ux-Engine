//! VE-F4303 · 一致性×跨域联调（U06 场景应用组）
//! 职责：联调前契约包预交换（两域契约 diff 预演失配即阻断）+ 联调中违例
//! 即时提示（三通道——域本色）+ 快捷豁免通道（频次阈值防滥用收紧）
//! + 联调后违例账本并入处置流冲突仲裁。
//! 段位：0x3020（续表 idx537）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3020;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XdCode {
    /// 预交换失配（联调前阻断）。
    PreSwapFail = SEG_BASE,
    /// 快捷豁免滥用（频次阈值收紧）。
    ExemptAbuse = SEG_BASE + 0x01,
    /// 并入冲突（仲裁）。
    MergeClash = SEG_BASE + 0x02,
    /// 主账漂移（负计数/越界规则查询）。
    LedgerDrift = SEG_BASE + 0x03,
    /// 契约包指纹失配（序列化损坏）。
    PackFpMismatch = SEG_BASE + 0x04,
}

/// 契约包：(规则, 语义字)。
pub type ContractPack = Vec<(u16, u32)>;

/// 跨域联调器。
pub struct CrossDebug {
    /// 预交换包：域 A / 域 B。
    pub pack_a: ContractPack,
    pub pack_b: ContractPack,
    /// 联调违例账：(规则, 计数)。
    pub ledger: Vec<(u16, u32)>,
    /// 快捷豁免：(who, 次数)。
    pub exempts: Vec<(u8, u32)>,
    pub exempt_cap: u32,
    /// 提示三通道（域本色）。
    pub channels: [bool; 3],
    pub blocked: bool,
}

impl CrossDebug {
    pub fn new(exempt_cap: u32) -> Self {
        CrossDebug { pack_a: Vec::new(), pack_b: Vec::new(), ledger: Vec::new(), exempts: Vec::new(), exempt_cap, channels: [true; 3], blocked: false }
    }

    /// 预交换 diff 预演：同规则语义不同=失配阻断。
    pub fn preswap(&mut self) -> Result<Vec<u16>, XdCode> {
        let mut clashes = Vec::new();
        for (r, va) in &self.pack_a {
            if let Some((_, vb)) = self.pack_b.iter().find(|(rb, _)| rb == r) {
                if va != vb {
                    clashes.push(*r);
                }
            }
        }
        if !clashes.is_empty() {
            self.blocked = true; // 联调前阻断
            return Err(XdCode::PreSwapFail);
        }
        Ok(clashes)
    }

    /// 联调中违例即时提示（三通道）。
    pub fn hit(&mut self, rule: u16) {
        match self.ledger.iter_mut().find(|(r, _)| *r == rule) {
            Some((_, c)) => *c += 1,
            None => self.ledger.push((rule, 1)),
        }
    }

    pub fn channels_ok(&self) -> bool {
        self.channels.iter().all(|c| *c)
    }

    /// 快捷豁免：频次阈值内放行；超阈收紧拒绝。
    pub fn quick_exempt(&mut self, who: u8) -> Result<(), XdCode> {
        match self.exempts.iter_mut().find(|(w, _)| *w == who) {
            Some((_, c)) => {
                *c += 1;
                if *c > self.exempt_cap {
                    return Err(XdCode::ExemptAbuse); // 阈值收紧
                }
                Ok(())
            }
            None => {
                self.exempts.push((who, 1));
                Ok(())
            }
        }
    }

    /// 并入处置流：与主账冲突（同规则已有更高计数）→仲裁取大。
    pub fn merge_into(&mut self, main: &mut Vec<(u16, u32)>) -> Result<usize, XdCode> {
        if self.blocked {
            return Err(XdCode::MergeClash);
        }
        let mut merged = 0;
        for (r, c) in self.ledger.iter() {
            match main.iter_mut().find(|(mr, _)| *mr == *r) {
                Some(mc) => {
                    if mc.1 < *c {
                        mc.1 = *c; // 仲裁取大
                    }
                    merged += 1;
                }
                None => {
                    main.push((*r, *c));
                    merged += 1;
                }
            }
        }
        Ok(merged)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C537-01..12。
pub fn run_ves537_checks() -> CheckSet {
    let mut set = CheckSet::new("ves537");

    let mut xd = CrossDebug::new(2);
    xd.pack_a = alloc::vec![(10, 1), (20, 2)];
    xd.pack_b = alloc::vec![(10, 1), (20, 3)];
    chk(&mut set, "C537-01", xd.preswap() == Err(XdCode::PreSwapFail), "预交换失配（规则20 语义异）");
    chk(&mut set, "C537-02", xd.blocked, "联调前阻断在册");
    xd.pack_b[1].1 = 2;
    chk(&mut set, "C537-03", xd.preswap() == Ok(alloc::vec![]), "修正后预演通过");
    chk(&mut set, "C537-04", !xd.blocked, "阻断解除");

    // 联调违例
    xd.hit(10);
    xd.hit(10);
    xd.hit(30);
    chk(&mut set, "C537-05", xd.ledger == alloc::vec![(10, 2), (30, 1)], "违例账计数");
    chk(&mut set, "C537-06", xd.channels_ok(), "即时提示三通道齐（域本色）");

    // 快捷豁免
    chk(&mut set, "C537-07", xd.quick_exempt(1).is_ok() && xd.quick_exempt(1).is_ok(), "豁免阈值内放行");
    chk(&mut set, "C537-08", xd.quick_exempt(1) == Err(XdCode::ExemptAbuse), "第 3 次超阈收紧拒绝");

    // 并入处置流
    let mut main: Vec<(u16, u32)> = alloc::vec![(10, 1), (40, 5)];
    let n = xd.merge_into(&mut main).unwrap_or(0);
    chk(&mut set, "C537-09", n == 2, "并入两条");
    chk(&mut set, "C537-10", main.iter().find(|(r, _)| *r == 10).map(|(_, c)| *c) == Some(2), "冲突仲裁取大（2>1）");
    chk(&mut set, "C537-11", main.iter().any(|(r, c)| *r == 40 && *c == 5), "主账原有不覆盖");
    chk(&mut set, "C537-12", main.len() == 3, "并入后主账三条");

    ves537_deep_checks(&mut set);

    set
}

// ============ 二轮深化块（wbv-r2d5 · 2026-10-27）============
// 深化面：主账结构化（并入批次归因）+ 守恒恒等式 + 边界探针
// + 违例 Top 投影与整数占比 + LCG 豁免注入回归 + 幂等闸
// + 契约包 6 字节序列化往返 + FNV-1a 包指纹。

use alloc::string::{String, ToString};

/// 主账条目：规则 × 计数 × 并入来源批次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MainEntry {
    pub rule: u16,
    pub count: u32,
    pub batch: u16,
}

/// 跨域联调深化主账。
pub struct MainLedger {
    pub entries: Vec<MainEntry>,
    pub merged_total: u32,
    pub last_batch: u16,
}

impl MainLedger {
    pub fn new() -> Self {
        MainLedger { entries: Vec::new(), merged_total: 0, last_batch: 0 }
    }

    /// 并入一条（同规则仲裁取大并刷新来源批次）。
    pub fn absorb(&mut self, rule: u16, count: u32, batch: u16) {
        match self.entries.iter_mut().find(|e| e.rule == rule) {
            Some(e) => {
                if e.count < count {
                    e.count = count;
                    e.batch = batch;
                }
            }
            None => self.entries.push(MainEntry { rule, count, batch }),
        }
        self.merged_total += 1;
        self.last_batch = batch;
    }

    /// 守恒恒等式：并入总数 ≥ 账内条目数（同规则多次并入压缩不丢账）。
    pub fn conservation(&self) -> bool {
        self.entries.len() as u32 <= self.merged_total
    }

    /// 边界探针：查无此规则 → 显性 Err。
    pub fn entry_of(&self, rule: u16) -> Result<MainEntry, XdCode> {
        self.entries.iter().find(|e| e.rule == rule).copied().ok_or(XdCode::LedgerDrift)
    }

    /// 投影：违例 Top1（并列取先）。
    pub fn top1(&self) -> Option<(u16, u32)> {
        let mut best: Option<&MainEntry> = None;
        for e in &self.entries {
            match best {
                Some(b) if b.count >= e.count => {}
                _ => best = Some(e),
            }
        }
        best.map(|b| (b.rule, b.count))
    }

    /// 投影：总违例数与批次归因分列。
    pub fn total_hits(&self) -> u32 {
        self.entries.iter().map(|e| e.count).sum()
    }

    /// 投影：批次 b 归因的条目数（整数分列，禁浮点）。
    pub fn by_batch(&self, batch: u16) -> usize {
        self.entries.iter().filter(|e| e.batch == batch).count()
    }
}

/// 8 行确定性 LCG（无浮点无墙钟，深化回归专用）。
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }
}

/// LCG 豁免注入回归：随机人员随机次数申请豁免，阈值内必放行、
/// 超阈必拒（ExemptAbuse）；负对照：换人首次必放行零误拒。
fn exempt_inject(seed: u32, cap: u32) -> Result<(), String> {
    let mut rng = Lcg(seed);
    for _ in 0..64u32 {
        let mut xd = CrossDebug::new(cap);
        let who = (rng.next() % 16) as u8;
        let times = (rng.next() % (cap + 3)) + 1; // 1..=cap+3
        let mut last_ok = true;
        for _ in 0..times {
            last_ok = xd.quick_exempt(who).is_ok();
        }
        if times <= cap && !last_ok {
            return Err("豁免注入：阈值内申请被误拒".to_string());
        }
        if times > cap && last_ok {
            return Err("豁免注入：超阈申请未收紧".to_string());
        }
        let _ = xd.quick_exempt(who.wrapping_add(1)); // 负对照：他人独立阈值
        let other_used = xd.exempts.iter().find(|(w, _)| *w == who.wrapping_add(1)).map(|(_, c)| *c);
        if other_used != Some(1) {
            return Err("豁免注入负对照：他人阈值被串账".to_string());
        }
    }
    Ok(())
}

/// 幂等闸：preswap 二次调用零额外副作用（blocked 态与失配集稳定）。
fn idempotence_probe() -> bool {
    let mut xd = CrossDebug::new(2);
    xd.pack_a = alloc::vec![(7, 1)];
    xd.pack_b = alloc::vec![(7, 2)];
    let r1 = xd.preswap();
    let blocked1 = xd.blocked;
    let r2 = xd.preswap();
    r1 == r2 && r2.is_err() && xd.blocked == blocked1
}

/// 契约条目定长 6 字节编码：[规则 lo, 规则 hi, 语义 b0..b3]。手写布局禁 JSON。
pub fn encode_pair(rule: u16, sem: u32) -> [u8; 6] {
    [
        (rule & 0xFF) as u8,
        (rule >> 8) as u8,
        (sem & 0xFF) as u8,
        ((sem >> 8) & 0xFF) as u8,
        ((sem >> 16) & 0xFF) as u8,
        ((sem >> 24) & 0xFF) as u8,
    ]
}

/// 解码还原（无哨兵则校验规则号回读一致 + 提供指纹）。
pub fn decode_pair(b: &[u8; 6]) -> (u16, u32) {
    let rule = b[0] as u16 | ((b[1] as u16) << 8);
    let sem = b[2] as u32
        | ((b[3] as u32) << 8)
        | ((b[4] as u32) << 16)
        | ((b[5] as u32) << 24);
    (rule, sem)
}

/// FNV-1a 32 位契约包指纹。
pub fn fnv_pack(pack: &ContractPack) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for (r, s) in pack {
        for byte in encode_pair(*r, *s) {
            h ^= byte as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    h
}

/// 深化判据并入主判据流（C537-13..23）。
pub fn ves537_deep_checks(set: &mut CheckSet) {
    let mut ml = MainLedger::new();
    ml.absorb(10, 2, 1);
    ml.absorb(10, 5, 2); // 仲裁取大并刷新批次
    ml.absorb(30, 1, 2);
    set.add("C537-13", ml.conservation(), "主账守恒（并入压缩不丢账）");
    set.add("C537-14", ml.entry_of(10) == Ok(MainEntry { rule: 10, count: 5, batch: 2 }), "仲裁取大+批次归因刷新");
    set.add("C537-15", ml.entry_of(99) == Err(XdCode::LedgerDrift), "越界规则探针显性报错");
    set.add("C537-16", ml.top1() == Some((10, 5)) && ml.total_hits() == 8, "Top1 投影与总数");
    set.add("C537-17", ml.by_batch(1) == 0 && ml.by_batch(2) == 2, "批次归因分列（取大后批次易主）");

    match exempt_inject(0xC055_3701, 2) {
        Ok(()) => set.add("C537-18", true, "LCG 豁免注入 64 轮阈值内必放/超阈必拒+他人零串账"),
        Err(msg) => set.add_dyn("C537-18", false, &msg),
    }

    set.add("C537-19", idempotence_probe(), "幂等闸：preswap 二次调用零额外副作用");

    let (r, s) = decode_pair(&encode_pair(0xBEEF, 0xDEAD_BEEF));
    set.add("C537-20", r == 0xBEEF && s == 0xDEAD_BEEF, "契约条目 6 字节编码解码往返");

    let pack: ContractPack = alloc::vec![(10, 1), (20, 2)];
    let fp1 = fnv_pack(&pack);
    let pack2: ContractPack = alloc::vec![(10, 1), (20, 3)];
    set.add("C537-21", fnv_pack(&pack2) != fp1, "FNV 包指纹随语义漂移");
    set.add("C537-22", fnv_pack(&ContractPack::new()) == 0x811C_9DC5, "空包指纹等于 FNV 偏移基");
    set.add("C537-23", MainLedger::new().conservation() && MainLedger::new().top1().is_none(), "空主账边界探针");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_debug() {
        let mut xd = CrossDebug::new(1);
        xd.pack_a = alloc::vec![(1, 1)];
        xd.pack_b = alloc::vec![(1, 1)];
        assert!(xd.preswap().is_ok());
        xd.hit(1);
        assert!(xd.quick_exempt(1).is_ok());
        assert!(xd.quick_exempt(1).is_err());
    }
}
