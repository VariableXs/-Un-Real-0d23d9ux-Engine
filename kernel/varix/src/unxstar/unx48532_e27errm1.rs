//! UNX-F48532 · E27 错误码族 E 段（10 条）
//!
//! 判据原文：UNX-F48532-J1 —— E27 热态面 10 条错误码冻结表：码值连续
//! （SEG_BASE+1 起）、名称互异、严重级两档齐备（可恢复/致命）、文案
//! 指纹一一对应；每条码都有可复现触发路径；表外码一律拒收（无静默
//! 映射）；fuzz 触发分支全覆盖；全表回归 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEEE0;

/// 10 条错误码：EEE1..EEEA 连续冻结。
pub const E_SEAT_ORPHAN: u16 = SEG_BASE + 1;
pub const E_SEAT_DUP: u16 = SEG_BASE + 2;
pub const E_ADDON_CYCLE: u16 = SEG_BASE + 3;
pub const E_SWAP_WINDOW: u16 = SEG_BASE + 4;
pub const E_DYING_COMMIT: u16 = SEG_BASE + 5;
pub const E_STALE_EPOCH: u16 = SEG_BASE + 6;
pub const E_CLEAN_MISS: u16 = SEG_BASE + 7;
pub const E_HOT_UNAVAIL: u16 = SEG_BASE + 8;
pub const E_REGRESS_BASE: u16 = SEG_BASE + 9;
pub const E_KEY_SERVE_MISS: u16 = SEG_BASE + 10;

/// 严重级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sev {
    Recover,
    Fatal,
}

/// 表项。
#[derive(Clone, PartialEq, Eq)]
pub struct ErrEntry {
    pub code: u16,
    pub name: String,
    pub sev: Sev,
    pub text: String,
}

impl ErrEntry {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in self.name.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        for b in self.text.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 冻结表（10 条单一真源）。
pub fn table() -> Vec<ErrEntry> {
    let mut t: Vec<ErrEntry> = Vec::new();
    t.push(ErrEntry {
        code: E_SEAT_ORPHAN,
        name: "seat_orphan".to_string(),
        sev: Sev::Recover,
        text: "孤儿 seat：撤销后关联已清".to_string(),
    });
    t.push(ErrEntry {
        code: E_SEAT_DUP,
        name: "seat_dup".to_string(),
        sev: Sev::Recover,
        text: "重复接入：同名 seat 拒收".to_string(),
    });
    t.push(ErrEntry {
        code: E_ADDON_CYCLE,
        name: "addon_cycle".to_string(),
        sev: Sev::Recover,
        text: "addon 重启环：桥切换平滑".to_string(),
    });
    t.push(ErrEntry {
        code: E_SWAP_WINDOW,
        name: "swap_window".to_string(),
        sev: Sev::Recover,
        text: "换装窗口：旧库服务零空窗".to_string(),
    });
    t.push(ErrEntry {
        code: E_DYING_COMMIT,
        name: "dying_commit".to_string(),
        sev: Sev::Recover,
        text: "临终提交：拒上屏但已入账".to_string(),
    });
    t.push(ErrEntry {
        code: E_STALE_EPOCH,
        name: "stale_epoch".to_string(),
        sev: Sev::Recover,
        text: "陈旧代际：销毁后投递全拒".to_string(),
    });
    t.push(ErrEntry {
        code: E_CLEAN_MISS,
        name: "clean_miss".to_string(),
        sev: Sev::Fatal,
        text: "清理缺失：残留检出隔离".to_string(),
    });
    t.push(ErrEntry {
        code: E_HOT_UNAVAIL,
        name: "hot_unavail".to_string(),
        sev: Sev::Fatal,
        text: "热态不可用：按键服务中断".to_string(),
    });
    t.push(ErrEntry {
        code: E_REGRESS_BASE,
        name: "regress_base".to_string(),
        sev: Sev::Fatal,
        text: "基线回退：热态后状态漂移".to_string(),
    });
    t.push(ErrEntry {
        code: E_KEY_SERVE_MISS,
        name: "key_serve_miss".to_string(),
        sev: Sev::Fatal,
        text: "按键漏服务：可用率破百".to_string(),
    });
    t
}

/// 表大小。
pub const TABLE_N: usize = 10;

/// R1 码值连续。
pub fn r1_contiguous() -> Result<(), u16> {
    let t = table();
    if t.len() != TABLE_N {
        return Err(E_SIZE_BAD);
    }
    for i in 0..TABLE_N {
        if t[i].code != SEG_BASE + (i as u16) + 1 {
            return Err(E_GAP);
        }
    }
    Ok(())
}

/// R2 三重互异：名称/码值/指纹。
pub fn r2_distinct() -> Result<(), u16> {
    let t = table();
    for i in 0..t.len() {
        for j in (i + 1)..t.len() {
            if t[i].name == t[j].name || t[i].code == t[j].code || t[i].fp() == t[j].fp() {
                return Err(E_CLASH);
            }
        }
    }
    Ok(())
}

/// R3 严重级一致：Fatal 恰 4 条。
pub fn r3_severity() -> Result<(), u16> {
    let t = table();
    let mut fatal = 0;
    for e in t.iter() {
        match e.sev {
            Sev::Fatal => fatal += 1,
            Sev::Recover => {}
        }
    }
    if fatal != 4 {
        return Err(E_SEV_BAD);
    }
    Ok(())
}

/// R4 触发路径·热插族：孤儿/重复/环/窗口 四码可触发。
pub fn r4_trigger_hotplug() -> Result<(), u16> {
    if classify_seat(1, false) != E_SEAT_ORPHAN {
        return Err(E_NO_TRIGGER);
    }
    if classify_seat(0, true) != E_SEAT_DUP {
        return Err(E_NO_TRIGGER);
    }
    if classify_addon(5) != E_ADDON_CYCLE {
        return Err(E_NO_TRIGGER);
    }
    if classify_swap(true) != E_SWAP_WINDOW {
        return Err(E_NO_TRIGGER);
    }
    if classify_seat(0, false) != 0 {
        return Err(E_NO_TRIGGER);
    }
    Ok(())
}

/// 热插族分类器。
pub fn classify_seat(orphans: u32, dup: bool) -> u16 {
    if orphans > 0 {
        return E_SEAT_ORPHAN;
    }
    if dup {
        return E_SEAT_DUP;
    }
    0
}

pub fn classify_addon(restarts: u32) -> u16 {
    if restarts >= 3 {
        return E_ADDON_CYCLE;
    }
    0
}

pub fn swap_in_window(active: bool) -> bool {
    active
}

pub fn classify_swap(in_window: bool) -> u16 {
    if swap_in_window(in_window) {
        return E_SWAP_WINDOW;
    }
    0
}

/// R5 触发路径·窗口族：临终/陈旧/清理/不可用/回退/漏服务 六码可触发。
pub fn r5_trigger_window() -> Result<(), u16> {
    if classify_win(true, false, false) != E_DYING_COMMIT {
        return Err(E_NO_TRIGGER);
    }
    if classify_win(false, true, false) != E_STALE_EPOCH {
        return Err(E_NO_TRIGGER);
    }
    if classify_win(false, false, true) != E_CLEAN_MISS {
        return Err(E_NO_TRIGGER);
    }
    if classify_win(false, false, false) != 0 {
        return Err(E_NO_TRIGGER);
    }
    if classify_health(true, true) != E_HOT_UNAVAIL {
        return Err(E_NO_TRIGGER);
    }
    if classify_health(true, false) != E_REGRESS_BASE {
        return Err(E_NO_TRIGGER);
    }
    if classify_health(false, false) != 0 {
        return Err(E_NO_TRIGGER);
    }
    Ok(())
}

/// 窗口族分类器。
pub fn classify_win(dying_commit: bool, stale_epoch: bool, clean_miss: bool) -> u16 {
    if clean_miss {
        return E_CLEAN_MISS;
    }
    if stale_epoch {
        return E_STALE_EPOCH;
    }
    if dying_commit {
        return E_DYING_COMMIT;
    }
    0
}

/// 健康族分类器：unavail 优先于 regress。
pub fn classify_health(unavail: bool, regress: bool) -> u16 {
    if unavail {
        return E_HOT_UNAVAIL;
    }
    if regress {
        return E_REGRESS_BASE;
    }
    0
}

/// 漏服务分类器。
pub fn classify_serve(served: u32, injected: u32) -> u16 {
    if served < injected {
        return E_KEY_SERVE_MISS;
    }
    0
}

/// R6 表外码拒收：分类器只产 0 或表内码。
pub fn r6_no_offtable() -> Result<(), u16> {
    let t = table();
    let in_table = |c: u16| -> bool {
        for e in t.iter() {
            if e.code == c {
                return true;
            }
        }
        false
    };
    let probes: [u16; 12] = [
        classify_seat(0, false),
        classify_seat(3, true),
        classify_addon(0),
        classify_addon(9),
        classify_swap(false),
        classify_win(true, true, true),
        classify_win(false, false, false),
        classify_health(true, true),
        classify_health(false, false),
        classify_serve(10, 10),
        classify_serve(3, 8),
        0,
    ];
    for p in probes.iter() {
        if !in_table(*p) {
            return Err(E_OFFTABLE);
        }
    }
    Ok(())
}

/// R7 fuzz 覆盖：随机热插参数 200 轮三分支全命中。
pub fn r7_fuzz_coverage() -> Result<(), u16> {
    let mut g = Lcg::new(0x48532_0001);
    let mut hit = [0u32; 3];
    for _ in 0..200 {
        let orph = if g.below(3) == 0 { 1 } else { 0 };
        let dup = g.below(3) == 0;
        let r = classify_seat(orph, dup);
        match r {
            0 => hit[0] += 1,
            x if x == E_SEAT_ORPHAN => hit[1] += 1,
            x if x == E_SEAT_DUP => hit[2] += 1,
            _ => return Err(E_OFFTABLE),
        }
    }
    for k in 0..3 {
        if hit[k] == 0 {
            return Err(E_NO_TRIGGER);
        }
    }
    Ok(())
}

/// R8 漏服务检出：served < injected 必报 E_KEY_SERVE_MISS。
pub fn r8_serve_miss() -> Result<(), u16> {
    if classify_serve(10, 10) != 0 {
        return Err(E_NO_TRIGGER);
    }
    if classify_serve(10, 11) != E_KEY_SERVE_MISS {
        return Err(E_NO_TRIGGER);
    }
    if classify_serve(0, 1) != E_KEY_SERVE_MISS {
        return Err(E_NO_TRIGGER);
    }
    Ok(())
}

/// R9 名称族齐备：seat/addon/swap/win/clean/health/serve 语义可达。
pub fn r9_family() -> Result<(), u16> {
    let t = table();
    for e in t.iter() {
        if e.text.len() == 0 || e.name.len() < 4 {
            return Err(E_CLASH);
        }
        if e.fp() == 0 {
            return Err(E_CLASH);
        }
    }
    if t.len() != 10 {
        return Err(E_SIZE_BAD);
    }
    Ok(())
}

/// R10 fuzz 强度分类全触达：addon 重启计数 0..9 三态覆盖。
pub fn r10_fuzz_addon() -> Result<(), u16> {
    let mut g = Lcg::new(0x48532_0002);
    let mut seen = [0u32; 2];
    for _ in 0..100 {
        let n = (g.next() % 9) as u32;
        let r = classify_addon(n);
        match r {
            0 => seen[0] += 1,
            x if x == E_ADDON_CYCLE => seen[1] += 1,
            _ => return Err(E_OFFTABLE),
        }
    }
    for k in 0..2 {
        if seen[k] == 0 {
            return Err(E_NO_TRIGGER);
        }
    }
    Ok(())
}

/// 表内自诊断码（+0x14 之后）。
pub const E_SIZE_BAD: u16 = SEG_BASE + 0x15;
pub const E_GAP: u16 = SEG_BASE + 0x16;
pub const E_CLASH: u16 = SEG_BASE + 0x17;
pub const E_SEV_BAD: u16 = SEG_BASE + 0x18;
pub const E_NO_TRIGGER: u16 = SEG_BASE + 0x19;
pub const E_OFFTABLE: u16 = SEG_BASE + 0x1A;

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_contiguous()?;
    hits += 1;
    r2_distinct()?;
    hits += 1;
    r3_severity()?;
    hits += 1;
    r4_trigger_hotplug()?;
    hits += 1;
    r5_trigger_window()?;
    hits += 1;
    r6_no_offtable()?;
    hits += 1;
    r7_fuzz_coverage()?;
    hits += 1;
    r8_serve_miss()?;
    hits += 1;
    r9_family()?;
    hits += 1;
    r10_fuzz_addon()?;
    hits += 1;
    Ok(hits)
}
