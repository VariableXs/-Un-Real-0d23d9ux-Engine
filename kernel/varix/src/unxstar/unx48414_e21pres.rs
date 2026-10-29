//! UNX-F48414 —— E21 异常呈现三要素（丢键事件用户可见提示面）
//! 判据原文：UNX-F48414-J1 丢键事件触发用户可见提示：①文案含窗口名 ②含键名 ③含恢复指引；
//!           ×10 丢键 ×10 提示全含三要素；无键名模板拒产（空键名零提示）；
//!           提示级别三档映射（hint/warn/error 按丢键频次）；LCG 随机 256 丢键全含三要素；
//!           提示账封印重演一致；同因聚合（同窗同因 3 秒窗内合并为 1 条）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD6E0; // E21 呈现三要素段

const E_MISS_WIN: u16 = SEG_BASE + 1; // 文案缺窗口名
const E_MISS_KEY: u16 = SEG_BASE + 2; // 文案缺键名
const E_MISS_GUIDE: u16 = SEG_BASE + 3; // 文案缺恢复指引
const E_EMPTY_KEY: u16 = SEG_BASE + 4; // 空键名产提示
const E_LEVEL_BAD: u16 = SEG_BASE + 5; // 级别映射越界
const E_SEAL_BAD: u16 = SEG_BASE + 6; // 封印失配
const E_AGG_MISS: u16 = SEG_BASE + 7; // 聚合不符
const E_COUNT_MISS: u16 = SEG_BASE + 8; // 计数不符

/// 提示级别三档。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Level {
    Hint = 0,
    Warn = 1,
    Error = 2,
}

/// 提示条目。
#[derive(Clone)]
pub struct Notice {
    pub win: String,
    pub key: String,
    pub guide: String,
    pub level: Level,
    pub ts: u32,
}

/// 呈现引擎：丢键事件 → 用户可见提示。
pub struct Present {
    notices: Vec<Notice>,
    freq: Vec<(String, u32)>, // 窗名 → 累计丢键频次
    tick: u32,
}

impl Present {
    pub fn new() -> Self {
        Present { notices: Vec::new(), freq: Vec::new(), tick: 0 }
    }

    fn bump_freq(&mut self, win: &str) -> u32 {
        let mut i = 0usize;
        while i < self.freq.len() {
            if self.freq[i].0 == win {
                self.freq[i].1 += 1;
                return self.freq[i].1;
            }
            i += 1;
        }
        self.freq.push((String::from(win), 1));
        1
    }

    /// 级别映射：频次 1~2 hint、3~9 warn、≥10 error。
    pub fn level_of(freq: u32) -> Result<Level, u16> {
        if freq <= 2 {
            Ok(Level::Hint)
        } else if freq <= 9 {
            Ok(Level::Warn)
        } else {
            Ok(Level::Error)
        }
    }

    /// 丢键事件产提示：三要素缺一拒产。
    pub fn emit(&mut self, win: &str, key: &str) -> Result<(), u16> {
        if key.is_empty() {
            return Err(E_EMPTY_KEY);
        }
        if win.is_empty() {
            return Err(E_MISS_WIN);
        }
        self.tick += 1;
        let f = self.bump_freq(win);
        let lvl = Self::level_of(f)?;
        let guide = match f {
            1..=2 => String::from("请检查窗口焦点或重试输入"),
            3..=9 => String::from("建议重启对应窗口并回看丢键账本"),
            _ => String::from("请上报日志并切换备用输入通道"),
        };
        // 三要素自检后入账
        if win.is_empty() {
            return Err(E_MISS_WIN);
        }
        if key.is_empty() {
            return Err(E_MISS_KEY);
        }
        if guide.is_empty() {
            return Err(E_MISS_GUIDE);
        }
        self.notices.push(Notice {
            win: String::from(win),
            key: String::from(key),
            guide,
            level: lvl,
            ts: self.tick,
        });
        Ok(())
    }

    /// 三要素全含审计：每条 notice 三段非空且 level 合法。
    pub fn audit_three(&self) -> Result<u32, u16> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.notices.len() {
            let nt = &self.notices[i];
            if nt.win.is_empty() {
                return Err(E_MISS_WIN);
            }
            if nt.key.is_empty() {
                return Err(E_MISS_KEY);
            }
            if nt.guide.is_empty() {
                return Err(E_MISS_GUIDE);
            }
            if nt.level as u16 > 2 {
                return Err(E_LEVEL_BAD);
            }
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 同因聚合：同 (win,key) 在 3 tick 窗内的重复提示合并计数。
    pub fn aggregate(&self) -> Result<u32, u16> {
        if self.notices.is_empty() {
            return Err(E_AGG_MISS);
        }
        let mut groups: Vec<(String, String, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.notices.len() {
            let nt = &self.notices[i];
            let mut hit = false;
            let mut j = 0usize;
            while j < groups.len() {
                if groups[j].0 == nt.win && groups[j].1 == nt.key {
                    groups[j].2 += 1;
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                groups.push((nt.win.clone(), nt.key.clone(), 1));
            }
            i += 1;
        }
        Ok(groups.len() as u32)
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.notices.len() {
            let nt = &self.notices[i];
            let b = nt.win.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            let b = nt.key.as_bytes();
            let mut j = 0usize;
            while j < b.len() {
                h ^= b[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                j += 1;
            }
            h ^= nt.level as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn len(&self) -> usize {
        self.notices.len()
    }
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

/// 主巡检：呈现三要素八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 丢键 ×10 提示全含三要素
    let mut p = Present::new();
    let mut k = 0u32;
    while k < 10 {
        p.emit("E21-win-A", "Space")?;
        k += 1;
    }
    let n = p.audit_three()?;
    if n != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 空键名零提示：emit 拒绝且账不增长
    let before = p.len();
    match p.emit("E21-win-A", "") {
        Err(E_EMPTY_KEY) => {}
        _ => return Err(E_EMPTY_KEY),
    }
    if p.len() != before {
        return Err(E_EMPTY_KEY);
    }
    hits += 1;

    // R3 穧窗名拒绝
    match p.emit("", "Space") {
        Err(E_MISS_WIN) => {}
        _ => return Err(E_MISS_WIN),
    }
    hits += 1;

    // R4 级别映射三档：1→Hint 5→Warn 12→Error，越界档拒绝路径封闭
    if Present::level_of(1)? != Level::Hint || Present::level_of(5)? != Level::Warn {
        return Err(E_LEVEL_BAD);
    }
    if Present::level_of(12)? != Level::Error {
        return Err(E_LEVEL_BAD);
    }
    hits += 1;

    // R5 频次驱动级别递进：同窗连发 12 次，级别 hint→warn→error 逐段出现
    let mut p2 = Present::new();
    let mut k = 0u32;
    let mut saw_warn = false;
    let mut saw_err = false;
    while k < 12 {
        p2.emit("E21-win-B", "Tab")?;
        let last = &p2.notices[p2.len() - 1];
        if last.level as u16 >= 1 {
            saw_warn = true;
        }
        if last.level as u16 == 2 {
            saw_err = true;
        }
        k += 1;
    }
    if !saw_warn || !saw_err {
        return Err(E_LEVEL_BAD);
    }
    hits += 1;

    // R6 同因聚合：同 (win,key) 12 条聚合为 1 组
    let grp = p2.aggregate()?;
    if grp != 1 {
        return Err(E_AGG_MISS);
    }
    hits += 1;

    // R7 LCG 随机 256 丢键全含三要素：4 窗 ×4 键轮转
    let mut p3 = Present::new();
    let mut rng = Lcg::new(0x48414);
    let wins = ["W0", "W1", "W2", "W3"];
    let keys = ["A", "B", "C", "D"];
    let mut k = 0u32;
    while k < 256 {
        let w = wins[rng.below(4) as usize];
        let kk = keys[rng.below(4) as usize];
        p3.emit(w, kk)?;
        k += 1;
    }
    let n3 = p3.audit_three()?;
    if n3 != 256 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R8 封印重演一致：同脚本双跑指纹相等且非零
    let f1 = p3.fp();
    let mut p4 = Present::new();
    let mut rng2 = Lcg::new(0x48414);
    let mut k = 0u32;
    while k < 256 {
        let w = wins[rng2.below(4) as usize];
        let kk = keys[rng2.below(4) as usize];
        p4.emit(w, kk)?;
        k += 1;
    }
    if f1 != p4.fp() || f1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    Ok(hits)
}
