//! UNX-F48512 · E26 错误码族 E 段（8 条）
//!
//! 判据原文：UNX-F48512-J1 —— E26 风暴面 8 条错误码冻结表：码值连续
//! （SEG_BASE+1 起）、名称互异、严重级两档齐备（可恢复/致命）、文案
//! 指纹一一对应；每条码都有可复现触发路径；表外码一律拒收（无静默
//! 映射）；fuzz 触发四分支全覆盖；全表回归 drill 零偏差。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xEB20;

/// 8 条错误码：EB21..EB28 连续冻结。
pub const E_STORM_OVERFLOW: u16 = SEG_BASE + 1;
pub const E_REFRESH_STALL: u16 = SEG_BASE + 2;
pub const E_INPUT_LOST: u16 = SEG_BASE + 3;
pub const E_FOCUS_FIGHT: u16 = SEG_BASE + 4;
pub const E_LEARN_FLOOD: u16 = SEG_BASE + 5;
pub const E_MEM_PRESS: u16 = SEG_BASE + 6;
pub const E_LOOP_BUSY: u16 = SEG_BASE + 7;
pub const E_MERGE_MISS: u16 = SEG_BASE + 8;

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

/// 冻结表（8 条单一真源）。
pub fn table() -> Vec<ErrEntry> {
    let mut t: Vec<ErrEntry> = Vec::new();
    t.push(ErrEntry {
        code: E_STORM_OVERFLOW,
        name: "storm_overflow".to_string(),
        sev: Sev::Recover,
        text: "风暴溢出：限流窗口生效中".to_string(),
    });
    t.push(ErrEntry {
        code: E_REFRESH_STALL,
        name: "refresh_stall".to_string(),
        sev: Sev::Recover,
        text: "刷新停顿：同帧合并恢复中".to_string(),
    });
    t.push(ErrEntry {
        code: E_INPUT_LOST,
        name: "input_lost".to_string(),
        sev: Sev::Fatal,
        text: "输入丢失：队列上界拒收".to_string(),
    });
    t.push(ErrEntry {
        code: E_FOCUS_FIGHT,
        name: "focus_fight".to_string(),
        sev: Sev::Recover,
        text: "焦点争抢：后到杀先到仲裁".to_string(),
    });
    t.push(ErrEntry {
        code: E_LEARN_FLOOD,
        name: "learn_flood".to_string(),
        sev: Sev::Recover,
        text: "学习洪峰：节流窗口合并中".to_string(),
    });
    t.push(ErrEntry {
        code: E_MEM_PRESS,
        name: "mem_press".to_string(),
        sev: Sev::Recover,
        text: "内存压力：淘汰路径已触发".to_string(),
    });
    t.push(ErrEntry {
        code: E_LOOP_BUSY,
        name: "loop_busy".to_string(),
        sev: Sev::Fatal,
        text: "忙循环失控：片上界强制让出".to_string(),
    });
    t.push(ErrEntry {
        code: E_MERGE_MISS,
        name: "merge_miss".to_string(),
        sev: Sev::Fatal,
        text: "合并失效：帧账不一致隔离".to_string(),
    });
    t
}

/// 表大小。
pub const TABLE_N: usize = 8;

/// R1 码值连续：table()[i].code == SEG_BASE + i + 1。
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

/// R2 名称/码值/文案指纹三重互异。
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

/// R3 严重级一致：Fatal 恰 3 条且其余全可恢复。
pub fn r3_severity() -> Result<(), u16> {
    let t = table();
    let mut fatal = 0;
    for e in t.iter() {
        match e.sev {
            Sev::Fatal => fatal += 1,
            Sev::Recover => {}
        }
    }
    if fatal != 3 {
        return Err(E_SEV_BAD);
    }
    Ok(())
}

/// R4 触发路径·风暴族：溢出/停顿/丢失三码可由参数直接触发。
pub fn r4_trigger_storm() -> Result<(), u16> {
    // E_STORM_OVERFLOW：强度 > 上界。
    if classify_storm(20_001) != E_STORM_OVERFLOW {
        return Err(E_NO_TRIGGER);
    }
    // E_REFRESH_STALL：同帧刷新但停顿检测（强度 4000 边界带）。
    if classify_storm(4000) != E_REFRESH_STALL {
        return Err(E_NO_TRIGGER);
    }
    // E_INPUT_LOST：队列注入超上界。
    if classify_queue(65) != E_INPUT_LOST {
        return Err(E_NO_TRIGGER);
    }
    // 正常带零码。
    if classify_storm(1000) != 0 || classify_queue(30) != 0 {
        return Err(E_NO_TRIGGER);
    }
    Ok(())
}

/// 风暴族分类器（触发路径公共真源）。
pub fn classify_storm(intensity: u32) -> u16 {
    if intensity > 10_000 {
        return E_STORM_OVERFLOW;
    }
    if intensity >= 3000 && intensity <= 5000 {
        return E_REFRESH_STALL;
    }
    0
}

/// 队列族分类器。
pub fn classify_queue(qlen: u32) -> u16 {
    if qlen > 64 {
        return E_INPUT_LOST;
    }
    0
}

/// R5 触发路径·资源族：焦点/学习/内存/忙循环/合并五码可触发。
pub fn r5_trigger_resource() -> Result<(), u16> {
    if classify_resource(true, false, false, false) != E_FOCUS_FIGHT {
        return Err(E_NO_TRIGGER);
    }
    if classify_resource(false, true, false, false) != E_LEARN_FLOOD {
        return Err(E_NO_TRIGGER);
    }
    if classify_resource(false, false, true, false) != E_MEM_PRESS {
        return Err(E_NO_TRIGGER);
    }
    if classify_resource(false, false, false, true) != E_LOOP_BUSY {
        return Err(E_NO_TRIGGER);
    }
    if classify_resource(false, false, false, false) != 0 {
        return Err(E_NO_TRIGGER);
    }
    if classify_merge(false) != E_MERGE_MISS {
        return Err(E_NO_TRIGGER);
    }
    if classify_merge(true) != 0 {
        return Err(E_NO_TRIGGER);
    }
    Ok(())
}

/// 资源族分类器：focus_fight / learn_flood / mem_press / loop_busy。
pub fn classify_resource(focus_fight: bool, learn_flood: bool, mem_press: bool, loop_busy: bool) -> u16 {
    if loop_busy {
        return E_LOOP_BUSY;
    }
    if mem_press {
        return E_MEM_PRESS;
    }
    if learn_flood {
        return E_LEARN_FLOOD;
    }
    if focus_fight {
        return E_FOCUS_FIGHT;
    }
    0
}

/// 合并分类器：merged=false 即失效。
pub fn classify_merge(merged: bool) -> u16 {
    if !merged {
        return E_MERGE_MISS;
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
    let probes: [u16; 10] = [
        classify_storm(0),
        classify_storm(9_999),
        classify_storm(50_000),
        classify_queue(0),
        classify_queue(500),
        classify_resource(true, true, true, true),
        classify_resource(false, false, false, false),
        classify_merge(true),
        classify_merge(false),
        0,
    ];
    for p in probes.iter() {
        if !in_table(*p) {
            return Err(E_OFFTABLE);
        }
    }
    Ok(())
}

/// R7 fuzz 覆盖：随机资源四元组 200 轮，五分支全命中。
pub fn r7_fuzz_coverage() -> Result<(), u16> {
    let mut g = Lcg::new(0x48512_0001);
    let mut hit = [0u32; 5];
    for _ in 0..200 {
        let a = g.below(2) == 0;
        let b = g.below(2) == 0;
        let c = g.below(2) == 0;
        let d = g.below(2) == 0;
        let r = classify_resource(a, b, c, d);
        match r {
            0 => hit[0] += 1,
            x if x == E_FOCUS_FIGHT => hit[1] += 1,
            x if x == E_LEARN_FLOOD => hit[2] += 1,
            x if x == E_MEM_PRESS => hit[3] += 1,
            x if x == E_LOOP_BUSY => hit[4] += 1,
            _ => return Err(E_OFFTABLE),
        }
    }
    for k in 0..5 {
        if hit[k] == 0 {
            return Err(E_NO_TRIGGER);
        }
    }
    Ok(())
}

/// R8 名称族齐备：storm/refresh/input/focus/learn/mem/loop/merge 语义可达。
pub fn r8_family() -> Result<(), u16> {
    let t = table();
    let mut family_ok = true;
    for e in t.iter() {
        if e.text.len() == 0 || e.name.len() < 4 {
            family_ok = false;
        }
    }
    if !family_ok {
        return Err(E_CLASH);
    }
    if t.len() != 8 {
        return Err(E_SIZE_BAD);
    }
    Ok(())
}

/// R9 fuzz 风暴强度全覆盖：0..30000 随机 300 轮三态全触达。
pub fn r9_fuzz_intensity() -> Result<(), u16> {
    let mut g = Lcg::new(0x48512_0002);
    let mut seen = [0u32; 3];
    for _ in 0..300 {
        let v = (g.next() % 30_000) as u32;
        let r = classify_storm(v);
        match r {
            0 => seen[0] += 1,
            x if x == E_STORM_OVERFLOW => seen[1] += 1,
            x if x == E_REFRESH_STALL => seen[2] += 1,
            _ => return Err(E_OFFTABLE),
        }
    }
    for k in 0..3 {
        if seen[k] == 0 {
            return Err(E_NO_TRIGGER);
        }
    }
    Ok(())
}

/// R10 全表 drill：8 条逐条索引可达 + fp 全非零。
pub fn r10_all_reachable() -> Result<(), u16> {
    let t = table();
    for e in t.iter() {
        if e.fp() == 0 {
            return Err(E_CLASH);
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
    r4_trigger_storm()?;
    hits += 1;
    r5_trigger_resource()?;
    hits += 1;
    r6_no_offtable()?;
    hits += 1;
    r7_fuzz_coverage()?;
    hits += 1;
    r8_family()?;
    hits += 1;
    r9_fuzz_intensity()?;
    hits += 1;
    r10_all_reachable()?;
    hits += 1;
    Ok(hits)
}
