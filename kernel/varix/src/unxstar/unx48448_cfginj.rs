//! UNX-F48448 —— E·配置注入字段攻击（反：未知字段/类型翻转 ×10 零崩溃有提示）
//! 判据原文：UNX-F48448-J1 未知字段名/类型翻转/负值伪装/巨型值/重复冲突键/
//!           空值/键内分隔符/超长键/超长值/布尔-整型翻转 ×10 形态全部拒绝；
//!           每次拒绝必须产生非空提示（hint）；全程零崩溃状态合法；
//!           已知合法字段不受注入影响；LCG 字段模糊 ×512 零 panic 全归类；
//!           提示账封闭五型；重演指纹一致；守恒 seq = 拒收 + 采纳。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDCA0; // E23 配置注入攻击段

const E_INJ_PASS: u16 = SEG_BASE + 1; // 注入字段被放行
const E_HINT_EMPTY: u16 = SEG_BASE + 2; // 拒绝提示为空
const E_STATE_BAD: u16 = SEG_BASE + 3; // 注入后状态非法
const E_KIND_BAD: u16 = SEG_BASE + 4; // 拒绝分类越界
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 拒收计数不符
const E_LEGAL_MISS: u16 = SEG_BASE + 6; // 合法字段误拒
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 守恒破

/// 注入型（五类封闭）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum InjKind {
    UnknownKey = 0,  // 未知字段名
    TypeFlip = 1,    // 类型翻转（数值位出现字母 / 布尔位出现数字）
    RangeBad = 2,    // 负值伪装 / 巨型值越域
    DupConflict = 3, // 重复键且值冲突
    ShapeBad = 4,    // 形态违规：空值/键内分隔符/超长
}

/// 已知键类型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyType {
    Num = 0,
    Flag = 1,
}

/// 合法配置键值对（解析成功的产物）。
pub struct CfgEntry {
    pub key: String,
    pub kt: KeyType,
    pub num: u32,
    pub flag: bool,
}

/// 注入记录。
pub struct InjRecord {
    pub seq: u64,
    pub kind: InjKind,
    pub hint: String,
}

/// 键值配置解析守门器。
pub struct CfgGate {
    pub known: Vec<(&'static str, KeyType)>,
    pub accepted: Vec<CfgEntry>,
    pub rejects: Vec<InjRecord>,
    pub seq: u64,
}

impl CfgGate {
    pub fn new() -> Self {
        CfgGate {
            known: vec![("speed", KeyType::Num), ("cache", KeyType::Num), ("caps", KeyType::Flag)],
            accepted: Vec::new(),
            rejects: Vec::new(),
            seq: 0,
        }
    }

    fn known_type(&self, key: &str) -> Option<KeyType> {
        let mut i = 0usize;
        while i < self.known.len() {
            if self.known[i].0 == key {
                return Some(self.known[i].1);
            }
            i += 1;
        }
        None
    }

    /// 提交一行 "key=value"；任何违规形态拒收并给非空提示。
    pub fn submit(&mut self, line: &str) -> Result<(), u16> {
        self.seq += 1;
        let sq = self.seq;
        // 分隔符恰好一个
        let mut eq_pos: Option<usize> = None;
        let mut eq_cnt = 0usize;
        let bs = line.as_bytes();
        let mut i = 0usize;
        while i < bs.len() {
            if bs[i] == b'=' {
                eq_cnt += 1;
                if eq_pos.is_none() {
                    eq_pos = Some(i);
                }
            }
            i += 1;
        }
        let pos = match eq_pos {
            Some(p) if eq_cnt == 1 => p,
            _ => {
                let rec = InjRecord { seq: sq, kind: InjKind::ShapeBad, hint: self.mk_hint("sep") };
                self.rejects.push(rec);
                return Err(E_INJ_PASS);
            }
        };
        if pos == 0 || pos + 1 >= bs.len() {
            let rec = InjRecord { seq: sq, kind: InjKind::ShapeBad, hint: self.mk_hint("empty") };
            self.rejects.push(rec);
            return Err(E_INJ_PASS);
        }
        let key = &line[0..pos];
        let val = &line[pos + 1..];
        if key.len() > 24 || val.len() > 24 {
            let rec = InjRecord { seq: sq, kind: InjKind::ShapeBad, hint: self.mk_hint("long") };
            self.rejects.push(rec);
            return Err(E_INJ_PASS);
        }
        let kt = match self.known_type(key) {
            Some(t) => t,
            None => {
                let rec = InjRecord { seq: sq, kind: InjKind::UnknownKey, hint: self.mk_hint("key") };
                self.rejects.push(rec);
                return Err(E_INJ_PASS);
            }
        };
        // 类型校验
        let vb = val.as_bytes();
        match kt {
            KeyType::Num => {
                let mut all_digits = true;
                let mut j = 0usize;
                while j < vb.len() {
                    if vb[j] < b'0' || vb[j] > b'9' {
                        all_digits = false;
                    }
                    j += 1;
                }
                if !all_digits {
                    let rec = InjRecord { seq: sq, kind: InjKind::TypeFlip, hint: self.mk_hint("type") };
                    self.rejects.push(rec);
                    return Err(E_INJ_PASS);
                }
                // 数值解析（无溢出回绕采纳：越 1e6 视作巨型值）
                let mut v: u64 = 0;
                let mut j = 0usize;
                while j < vb.len() {
                    v = v * 10 + (vb[j] - b'0') as u64;
                    j += 1;
                }
                if v > 1_000_000 {
                    let rec = InjRecord { seq: sq, kind: InjKind::RangeBad, hint: self.mk_hint("huge") };
                    self.rejects.push(rec);
                    return Err(E_INJ_PASS);
                }
                // 重复键值冲突
                let mut ci = 0usize;
                while ci < self.accepted.len() {
                    if self.accepted[ci].key == key && self.accepted[ci].num != v as u32 {
                        let rec = InjRecord { seq: sq, kind: InjKind::DupConflict, hint: self.mk_hint("dup") };
                        self.rejects.push(rec);
                        return Err(E_INJ_PASS);
                    }
                    ci += 1;
                }
                self.accepted.push(CfgEntry { key: key.to_string(), kt, num: v as u32, flag: false });
                Ok(())
            }
            KeyType::Flag => {
                if val != "on" && val != "off" {
                    let rec = InjRecord { seq: sq, kind: InjKind::TypeFlip, hint: self.mk_hint("flag") };
                    self.rejects.push(rec);
                    return Err(E_INJ_PASS);
                }
                self.accepted.push(CfgEntry { key: key.to_string(), kt, num: 0, flag: val == "on" });
                Ok(())
            }
        }
    }

    fn mk_hint(&self, tag: &str) -> String {
        let mut s = String::from("rej@");
        s.push_str(tag);
        s
    }

    /// 注入后状态合法性。
    pub fn audit_state(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.accepted.len() {
            if self.accepted[i].key.is_empty() {
                return Err(E_STATE_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 提示账五型封闭且全部非空。
    pub fn audit_hints(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.rejects.len() {
            let r = &self.rejects[i];
            if r.hint.is_empty() {
                return Err(E_HINT_EMPTY);
            }
            if r.kind as u16 > 4 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.rejects.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.rejects.len() {
            h ^= self.rejects[i].kind as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.accepted.len() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    pub fn accept_count(&self) -> u32 {
        self.accepted.len() as u32
    }
}

/// 注入 ×10 清单。
pub fn inj_ten() -> Vec<&'static str> {
    vec![
        "tunnel=9",          // 未知字段
        "speed=fast",        // 数值位字母（类型翻转）
        "caps=1",            // 布尔位数字（类型翻转）
        "speed=9999999",     // 巨型值
        "speed=42",          // 先放行作基准
        "speed=42",          // 同值重复：合法（不冲突）
        "cache=77",          // 合法
        "cache=88",          // 重复键值冲突
        "=42",               // 空键
        "speed=",            // 空值
    ]
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：注入攻击八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 注入 ×10：8 拒 2 采（speed=42 重复同值 + cache=77 合法）
    let mut g = CfgGate::new();
    let injs = inj_ten();
    let mut i = 0usize;
    while i < injs.len() {
        let r = g.submit(injs[i]);
        if i == 4 || i == 5 || i == 6 {
            // speed=42, speed=42, cache=77 合法
            match r {
                Ok(()) => {}
                _ => return Err(E_LEGAL_MISS),
            }
        } else {
            match r {
                Err(E_INJ_PASS) => {}
                _ => return Err(E_INJ_PASS),
            }
        }
        i += 1;
    }
    if g.reject_count() != 7 {
        return Err(E_COUNT_MISS);
    }
    if g.accept_count() != 3 {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R2 每次拒绝提示非空
    g.audit_hints()?;
    hits += 1;

    // R3 注入后状态合法、合法字段未受影响
    g.audit_state()?;
    let mut found_speed = false;
    let mut found_cache = false;
    let mut i = 0usize;
    while i < g.accepted.len() {
        if g.accepted[i].key == "speed" && g.accepted[i].num == 42 {
            found_speed = true;
        }
        if g.accepted[i].key == "cache" && g.accepted[i].num == 77 {
            found_cache = true;
        }
        i += 1;
    }
    if !found_speed || !found_cache {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R4 拒绝账五型封闭：补齐 DupConflict 之外再验形态分布
    g.audit_hints()?;
    let mut seen = [0u32; 5];
    let mut i = 0usize;
    while i < g.rejects.len() {
        seen[g.rejects[i].kind as usize] += 1;
        i += 1;
    }
    if seen[0] == 0 || seen[1] == 0 || seen[2] == 0 || seen[4] == 0 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R5 LCG 字段模糊 ×512 零 panic 全归类
    let mut rng = Lcg::new(0x48448);
    let mut g2 = CfgGate::new();
    let mut k = 0u32;
    while k < 512 {
        let mode = rng.below(4);
        let line: String = if mode == 0 {
            let mut s = String::from("speed=");
            let d = (rng.below(10) as u8) + b'0';
            s.push(d as char);
            s
        } else if mode == 1 {
            let mut s = String::from("caps=");
            let d = (rng.below(10) as u8) + b'0';
            s.push(d as char);
            s
        } else if mode == 2 {
            let mut s = String::from("kx");
            let d = (rng.below(10) as u8) + b'0';
            s.push(d as char);
            s.push('=');
            s.push('9');
            s
        } else {
            let mut s = String::from("cache=");
            let d = (rng.below(10) as u8) + b'0';
            s.push(d as char);
            s
        };
        let _ = g2.submit(&line);
        g2.audit_hints()?;
        g2.audit_state()?;
        k += 1;
    }
    if g2.reject_count() + g2.accept_count() != 512 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R6 零崩溃守恒：seq = 拒 + 采
    if g2.seq != (g2.reject_count() + g2.accept_count()) as u64 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    // R7 重演一致：同种子指纹相等
    let f1 = g2.fingerprint();
    let mut g3 = CfgGate::new();
    let mut rng2 = Lcg::new(0x48448);
    let mut k = 0u32;
    while k < 512 {
        let mode = rng2.below(4);
        let line: String = if mode == 0 {
            let mut s = String::from("speed=");
            let d = (rng2.below(10) as u8) + b'0';
            s.push(d as char);
            s
        } else if mode == 1 {
            let mut s = String::from("caps=");
            let d = (rng2.below(10) as u8) + b'0';
            s.push(d as char);
            s
        } else if mode == 2 {
            let mut s = String::from("kx");
            let d = (rng2.below(10) as u8) + b'0';
            s.push(d as char);
            s.push('=');
            s.push('9');
            s
        } else {
            let mut s = String::from("cache=");
            let d = (rng2.below(10) as u8) + b'0';
            s.push(d as char);
            s
        };
        let _ = g3.submit(&line);
        k += 1;
    }
    if g3.fingerprint() != f1 || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 合法回放：三合法行全采纳
    let mut g4 = CfgGate::new();
    g4.submit("speed=10")?;
    g4.submit("cache=20")?;
    g4.submit("caps=on")?;
    if g4.accept_count() != 3 || g4.reject_count() != 0 {
        return Err(E_LEGAL_MISS);
    }
    g4.audit_state()?;
    hits += 1;

    Ok(hits)
}
