//! UNX-F10962 原始模式切换联测（ICANON/ECHO 关闭全链）（0x1d7c0 段）
//!
//! 卡片判据（UNX-F10962-J1）：
//! - 原始模式全位关闭即时生效（10/10），键入逐字符可见（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模终端原始模式切换全链：
//! termios 四个模式位（ICANON/ECHO/ISIG/IEXTEN）在 raw 切换时
//! 全位关闭并即时生效；键入在原始模式下逐字符可见（不再等行），
//! 规范模式下按行缓冲；回读校验模式位与缓冲行为一致。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10962-10900)*0x20 = 0x1d7c0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d7c0;

/// RM_ENOTTY：fd 不是终端
pub const RM_ENOTTY: u32 = SEG_BASE + 0x01;
/// RM_EBADF：fd 无效
pub const RM_EBADF: u32 = SEG_BASE + 0x02;
/// RM_EINVAL：属性掩码非法
pub const RM_EINVAL: u32 = SEG_BASE + 0x03;
/// RM_EMODE：切换后模式位与期望不符（自检面）
pub const RM_EMODE: u32 = SEG_BASE + 0x04;
/// RM_EBUF：行缓冲状态不一致（自检面）
pub const RM_EBUF: u32 = SEG_BASE + 0x05;
/// RM_EIO：终端对端失效
pub const RM_EIO: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 模式位
pub const ICANON: u32 = 0x0001;
pub const ECHO: u32 = 0x0002;
pub const ISIG: u32 = 0x0004;
pub const IEXTEN: u32 = 0x0008;
/// 原始模式应清除的全部位
pub const RAW_MASK: u32 = ICANON | ECHO | ISIG | IEXTEN;
/// 有效属性掩码
pub const ATTR_MASK: u32 = RAW_MASK;
/// 缓冲容量
const BUF_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 终端模式条目
#[derive(Clone, Debug, PartialEq)]
pub struct TermMode {
    pub fd: i32,
    pub flags: u32,
    pub alive: bool,
}

impl TermMode {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.fd as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.flags as u64).wrapping_mul(0x100000001b3);
        h = (h ^ (self.alive as u64)).wrapping_mul(0x100000001b3);
        h
    }
}

/// 原始模式切换台
#[derive(Clone, Debug)]
pub struct RawDesk {
    pub terms: Vec<TermMode>,
    /// 行缓冲（规范模式生效）
    pub line_buf: String,
    /// 原始模式下立即可见的字符流
    pub raw_stream: String,
    /// 错误计数
    pub n_enotty: u32,
    pub n_ebadf: u32,
    pub n_einval: u32,
    /// 切换次数
    pub n_raw_on: u32,
    pub n_raw_off: u32,
}

impl RawDesk {
    pub fn new() -> Self {
        RawDesk {
            terms: Vec::new(),
            line_buf: String::new(),
            raw_stream: String::new(),
            n_enotty: 0,
            n_ebadf: 0,
            n_einval: 0,
            n_raw_on: 0,
            n_raw_off: 0,
        }
    }

    /// 登记终端 fd（默认规范+回显）
    pub fn attach(&mut self, fd: i32) -> Result<(), u32> {
        if fd < 0 {
            self.n_ebadf = self.n_ebadf.wrapping_add(1);
            return Err(RM_EBADF);
        }
        if self.terms.iter().any(|t| t.fd == fd) {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(RM_EINVAL);
        }
        self.terms.push(TermMode { fd, flags: ICANON | ECHO | ISIG | IEXTEN, alive: true });
        Ok(())
    }

    /// fd 解析
    fn find(&mut self, fd: i32) -> Result<usize, u32> {
        match self.terms.iter().position(|t| t.fd == fd) {
            Some(i) if self.terms[i].alive => Ok(i),
            Some(_) => Err(RM_EIO),
            None => {
                self.n_ebadf = self.n_ebadf.wrapping_add(1);
                Err(RM_EBADF)
            }
        }
    }

    /// 设置属性位：掩码越界 EINVAL；全位关闭即时生效
    pub fn set_flags(&mut self, fd: i32, clear: u32, set: u32) -> Result<u32, u32> {
        if clear > ATTR_MASK || set > ATTR_MASK {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(RM_EINVAL);
        }
        let i = self.find(fd)?;
        if (clear & set) != 0 {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(RM_EINVAL);
        }
        let old = self.terms[i].flags;
        let newf = ((old & !clear) | set) & ATTR_MASK;
        self.terms[i].flags = newf;
        Ok(newf)
    }

    /// 原始模式全开：四位全清，即时生效
    pub fn raw_on(&mut self, fd: i32) -> Result<u32, u32> {
        let r = self.set_flags(fd, RAW_MASK, 0)?;
        // 即时生效自检：切换后立即清空行缓冲
        self.line_buf.clear();
        self.n_raw_on = self.n_raw_on.wrapping_add(1);
        Ok(r)
    }

    /// 恢复规范模式
    pub fn raw_off(&mut self, fd: i32) -> Result<u32, u32> {
        let r = self.set_flags(fd, 0, ICANON | ECHO)?;
        self.n_raw_off = self.n_raw_off.wrapping_add(1);
        Ok(r)
    }

    /// 键入一个字符：原始模式立即进入 raw_stream；规范模式进行缓冲
    pub fn key_in(&mut self, fd: i32, ch: char) -> Result<bool, u32> {
        let i = self.find(fd)?;
        if !self.terms[i].alive {
            return Err(RM_EIO);
        }
        let raw = (self.terms[i].flags & ICANON) == 0;
        if raw {
            // 逐字符可见
            self.raw_stream.push(ch);
            Ok(true)
        } else {
            self.line_buf.push(ch);
            if ch == '\n' {
                // 整行交付
                self.raw_stream.push_str(&self.line_buf);
                self.line_buf.clear();
                Ok(true)
            } else {
                // 缓冲中，未可见
                Ok(false)
            }
        }
    }

    /// 逐字符可见数
    pub fn visible_len(&self) -> usize {
        self.raw_stream.len()
    }

    /// 全位关闭自检
    pub fn is_raw(&self, fd: i32) -> bool {
        match self.terms.iter().position(|t| t.fd == fd) {
            Some(i) => (self.terms[i].flags & RAW_MASK) == 0,
            None => false,
        }
    }

    /// 一致性自检：raw 模式下缓冲必空、可见流完整
    pub fn consistent(&self) -> bool {
        for t in &self.terms {
            if !t.alive {
                continue;
            }
            let raw = (t.flags & ICANON) == 0;
            if raw && !self.line_buf.is_empty() {
                return false;
            }
            if t.flags & !ATTR_MASK != 0 {
                return false;
            }
        }
        true
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_on: u32) -> bool {
        self.n_raw_on == expect_on
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for t in &self.terms {
            h = (h ^ t.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.raw_stream.len() as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        let mut s = String::from("raw:");
        for t in &self.terms {
            let tag = if (t.flags & RAW_MASK) == 0 { "R" } else { "C" };
            s.push_str(&crate::alloc::format!("/{}:{} ", t.fd, tag));
        }
        s
    }
}

impl Default for RawDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut d = RawDesk::new();
    let _ = d.attach(0);
    // 路1：未登记 fd → EBADF
    let r1 = matches!(d.raw_on(9), Err(x) if x == RM_EBADF);
    // 路2：越界掩码 → EINVAL
    let r2 = matches!(d.set_flags(0, 0x10, 0), Err(x) if x == RM_EINVAL);
    // 路3：clear/set 冲突位 → EINVAL
    let r3 = matches!(d.set_flags(0, ICANON, ICANON), Err(x) if x == RM_EINVAL);
    // 路4：重复登记 → EINVAL
    let r4 = matches!(d.attach(0), Err(x) if x == RM_EINVAL);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：奇偶轮交替原始/规范切换，原始轮键入逐字符可见，
/// 规范轮按行缓冲；两种语义都正确记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut d = RawDesk::new();
    if d.attach(1).is_err() {
        return 0;
    }
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let raw_turn = r % 2 == 0;
        let expect_flag: u32 = if raw_turn { 0 } else { ICANON | ECHO };
        // 原始轮四位全清；规范轮只置位不清位（清/置同掩码会按 EINVAL 拒收）。
        let setr = if raw_turn {
            d.set_flags(1, RAW_MASK, 0)
        } else {
            d.set_flags(1, 0, ICANON | ECHO)
        };
        let flag_ok = setr == Ok(expect_flag);
        if raw_turn {
            // 原始轮：三字符逐个立即可见
            let before = d.visible_len();
            let v1 = d.key_in(1, 'a');
            let v2 = d.key_in(1, 'b');
            let v3 = d.key_in(1, 'c');
            let after = d.visible_len();
            let ok = flag_ok
                && v1 == Ok(true)
                && v2 == Ok(true)
                && v3 == Ok(true)
                && after == before + 3
                && d.is_raw(1);
            if ok {
                good = good.wrapping_add(1);
            }
        } else {
            // 规范轮：前两字符不可见，'\n' 后整行可见
            let before = d.visible_len();
            let v1 = d.key_in(1, 'x');
            let v2 = d.key_in(1, 'y');
            let v3 = d.key_in(1, '\n');
            let after = d.visible_len();
            let ok = flag_ok
                && v1 == Ok(false)
                && v2 == Ok(false)
                && v3 == Ok(true)
                && after == before + 3
                && !d.is_raw(1);
            if ok {
                good = good.wrapping_add(1);
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10962_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10962");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EBADF/EINVAL 坏样本全部命中");

    // J1 全位关闭即时生效 10/10
    let mut on10 = 0u32;
    for i in 0..10i32 {
        let mut d = RawDesk::new();
        let _ = d.attach(2 + i);
        let r = d.raw_on(2 + i);
        if r == Ok(0) && d.is_raw(2 + i) && d.line_buf.is_empty() {
            on10 = on10.wrapping_add(1);
        }
    }
    s.add("J1-全位关闭即时生效10/10", on10 == 10, "raw_on 后四位全零且缓冲即清");

    // 键入逐字符可见 10/10
    let mut vis10 = 0u32;
    for _ in 0..10 {
        let mut d = RawDesk::new();
        let _ = d.attach(1);
        let _ = d.raw_on(1);
        let _ = d.key_in(1, 'k');
        let _ = d.key_in(1, 'e');
        let _ = d.key_in(1, 'y');
        if d.visible_len() == 3 && d.raw_stream == "key" {
            vis10 = vis10.wrapping_add(1);
        }
    }
    s.add("J2-逐字符可见10/10", vis10 == 10, "原始模式三字符立即全部可见");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮交替语义", good == 1000, "原始/规范千轮交替语义全对");

    // 规范模式行缓冲对照
    let mut d2 = RawDesk::new();
    let _ = d2.attach(1);
    let _ = d2.key_in(1, 'a');
    let _ = d2.key_in(1, 'b');
    let canon_ok = d2.visible_len() == 0 && d2.line_buf == "ab"
        && d2.key_in(1, '\n') == Ok(true)
        && d2.visible_len() == 3;
    s.add("J4-规范行缓冲对照", canon_ok, "规范模式等行交付");

    // 错误码分段
    let codes = [RM_ENOTTY, RM_EBADF, RM_EINVAL, RM_EMODE, RM_EBUF, RM_EIO];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d7c0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "六码两两互异");

    // 一致性
    s.add("J7-一致性自检", d2.consistent(), "模式位与缓冲行为一致");

    // 账目对平
    s.add("J8-账目对平", d2.ledger_ok(0), "本台未做 raw_on，账面为零");

    // raw_off 恢复
    let mut d3 = RawDesk::new();
    let _ = d3.attach(1);
    let _ = d3.raw_on(1);
    let off = d3.raw_off(1) == Ok(ICANON | ECHO) && !d3.is_raw(1);
    s.add("J9-恢复面", off, "raw_off 恢复规范+回显");

    // 判定序：掩码校验先于 fd 查找
    let mut d4 = RawDesk::new();
    let seq_ok = matches!(d4.set_flags(7, 0x99, 0), Err(x) if x == RM_EINVAL);
    s.add("J10-判定序", seq_ok, "越界掩码先报 EINVAL");

    // fold 稳定
    let h1 = d3.fold_all();
    let h2 = d3.fold_all();
    s.add("J11-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染（原始态与规范态标记可区分）
    let mut d5 = RawDesk::new();
    let _ = d5.attach(3);
    let _ = d5.raw_on(3);
    let r = d5.render();
    s.add(
        "J12-渲染非空",
        r.starts_with("raw:") && r.contains("/3:R") && d3.render().contains("/1:C"),
        "摘要含原始/规范模式标记",
    );

    // 动态判据
    s.add_dyn(
        "J13-切换账",
        d3.n_raw_on == 1 && d3.n_raw_off == 1,
        &crate::alloc::format!("on={} off={}", d3.n_raw_on, d3.n_raw_off),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-可见流长度",
        d2.visible_len() == 3,
        &crate::alloc::format!("可见流={}", d2.visible_len()),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10962_rawmode_switch_flow() {
        let mut d = RawDesk::new();
        let _ = d.attach(1);
        assert_eq!(d.raw_on(1), Ok(0));
        assert!(d.is_raw(1));
        assert_eq!(d.key_in(1, 'z'), Ok(true));
        assert_eq!(d.visible_len(), 1);
        assert_eq!(d.raw_off(1), Ok(ICANON | ECHO));
        assert!(!d.is_raw(1));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10962_checks_green() {
        let s = run_unx10962_checks();
        assert!(s.all_passed(), "UNX-F10962 判据未全绿");
    }
}
