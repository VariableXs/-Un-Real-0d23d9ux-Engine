//! UNX-F10969 转义序列解析联测（ESC [ A 序列完整性）（0x1d8a0 段）
//!
//! 卡片判据（UNX-F10969-J1）：
//! - 方向键序列逐段到达完整（10/10）、跨读分段重组正确（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模终端转义序列增量解析：
//! 逐字节喂入跨多次读取，状态机 Ground→ESC→CSI→终态推进；
//! 序列分段到达时保持挂起（Pending），最后一字节到达即重组
//! 完整方向键事件；残缺序列显性报错不吞键。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10969-10900)*0x20 = 0x1d8a0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d8a0;

/// ES_EBADF：解析器未绑定
pub const ES_EBADF: u32 = SEG_BASE + 0x01;
/// ES_EINVAL：状态机状态非法
pub const ES_EINVAL: u32 = SEG_BASE + 0x02;
/// ES_ESEQ：序列残缺/非法
pub const ES_ESEQ: u32 = SEG_BASE + 0x03;
/// ES_EOVER：序列超长（状态机保护面）
pub const ES_EOVER: u32 = SEG_BASE + 0x04;
/// ES_ELOSE：重组账不平（自检面）
pub const ES_ELOSE: u32 = SEG_BASE + 0x05;
/// ES_ECLOSED：解析器已关闭
pub const ES_ECLOSED: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// ESC 字节
pub const ESC: u8 = 0x1b;
/// CSI 引导字节 '['
pub const CSI: u8 = b'[';
/// 状态机状态
pub const ST_GROUND: u32 = 0;
pub const ST_ESC: u32 = 1;
pub const ST_CSI: u32 = 2;
/// 序列最大长度
const SEQ_MAX: usize = 8;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 解析出的方向键事件
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowKey {
    Up,
    Down,
    Right,
    Left,
}

impl ArrowKey {
    pub fn code(&self) -> u32 {
        match self {
            ArrowKey::Up => 1,
            ArrowKey::Down => 2,
            ArrowKey::Right => 3,
            ArrowKey::Left => 4,
        }
    }
}

/// 喂入结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedOut {
    /// 普通字符直通
    Plain(u8),
    /// 方向键完整重组
    Arrow(ArrowKey),
    /// 序列挂起（跨读分段）
    Pending,
}

/// 转义序列解析器
#[derive(Clone, Debug)]
pub struct EscParser {
    /// 状态机状态
    pub state: u32,
    /// 序列积累缓冲
    seq: Vec<u8>,
    /// 已重组方向键计数
    pub arrows: u64,
    /// 已直通普通字符计数
    pub plain: u64,
    /// 解析器开启
    pub open: bool,
    /// 错误计数
    pub n_eseq: u32,
    pub n_eover: u32,
}

impl EscParser {
    pub fn new() -> Self {
        EscParser {
            state: ST_GROUND,
            seq: Vec::new(),
            arrows: 0,
            plain: 0,
            open: true,
            n_eseq: 0,
            n_eover: 0,
        }
    }

    /// 逐字节喂入
    pub fn feed(&mut self, b: u8) -> Result<FeedOut, u32> {
        if !self.open {
            return Err(ES_ECLOSED);
        }
        match self.state {
            ST_GROUND => {
                if b == ESC {
                    self.state = ST_ESC;
                    self.seq.clear();
                    self.seq.push(b);
                    Ok(FeedOut::Pending)
                } else {
                    self.plain = self.plain.wrapping_add(1);
                    Ok(FeedOut::Plain(b))
                }
            }
            ST_ESC => {
                self.seq.push(b);
                if b == CSI {
                    self.state = ST_CSI;
                    return Ok(FeedOut::Pending);
                }
                // ESC 后跟非法字节 → 残缺
                self.state = ST_GROUND;
                self.seq.clear();
                self.n_eseq = self.n_eseq.wrapping_add(1);
                Err(ES_ESEQ)
            }
            ST_CSI => {
                self.seq.push(b);
                if self.seq.len() > SEQ_MAX {
                    self.state = ST_GROUND;
                    self.seq.clear();
                    self.n_eover = self.n_eover.wrapping_add(1);
                    return Err(ES_EOVER);
                }
                let n = self.seq.len();
                let fin = self.seq[n - 1];
                match fin {
                    b'A' => self.finish(ArrowKey::Up),
                    b'B' => self.finish(ArrowKey::Down),
                    b'C' => self.finish(ArrowKey::Right),
                    b'D' => self.finish(ArrowKey::Left),
                    b'0'..=b'9' | b';' => Ok(FeedOut::Pending),
                    _ => {
                        self.state = ST_GROUND;
                        self.seq.clear();
                        self.n_eseq = self.n_eseq.wrapping_add(1);
                        Err(ES_ESEQ)
                    }
                }
            }
            _ => Err(ES_EINVAL),
        }
    }

    /// 终态：重组完成
    fn finish(&mut self, k: ArrowKey) -> Result<FeedOut, u32> {
        self.state = ST_GROUND;
        self.seq.clear();
        self.arrows = self.arrows.wrapping_add(1);
        Ok(FeedOut::Arrow(k))
    }

    /// 跨读分段重组：把一段字节流分段喂入，末尾必须是完整方向键
    pub fn feed_split(&mut self, seq: &[u8], parts: usize) -> Result<FeedOut, u32> {
        if parts == 0 {
            return Err(ES_EINVAL);
        }
        let n = seq.len();
        let mut last = FeedOut::Pending;
        let mut i = 0usize;
        let mut p = 0usize;
        while p < parts {
            // 分段长度尽量均分
            let chunk = if p + 1 == parts { n - i } else { n / parts.max(1) };
            let mut k = 0usize;
            while k < chunk && i < n {
                last = self.feed(seq[i])?;
                i = i.wrapping_add(1);
                k = k.wrapping_add(1);
            }
            p = p.wrapping_add(1);
        }
        Ok(last)
    }

    /// 对账：状态归零、缓冲清空、账自洽
    pub fn reconcile(&self) -> bool {
        self.state == ST_GROUND && self.seq.is_empty()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.state as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.arrows).wrapping_mul(0x100000001b3);
        h = (h ^ self.plain).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "esc:state={} arrows={} plain={} err={}",
            self.state, self.arrows, self.plain, self.n_eseq
        )
    }
}

impl Default for EscParser {
    fn default() -> Self {
        Self::new()
    }
}

/// 方向键序列构造：ESC [ X
pub fn arrow_seq(k: ArrowKey) -> [u8; 3] {
    let fin = match k {
        ArrowKey::Up => b'A',
        ArrowKey::Down => b'B',
        ArrowKey::Right => b'C',
        ArrowKey::Left => b'D',
    };
    [ESC, CSI, fin]
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut p = EscParser::new();
    // 路1：ESC 后非法字节 → ESEQ
    let r1 = matches!(p.feed(ESC), Ok(FeedOut::Pending)) && matches!(p.feed(b'Z'), Err(x) if x == ES_ESEQ);
    // 路2：CSI 后非法终结 → ESEQ
    let r2 = matches!(p.feed(ESC), Ok(FeedOut::Pending))
        && matches!(p.feed(CSI), Ok(FeedOut::Pending))
        && matches!(p.feed(b'!'), Err(x) if x == ES_ESEQ);
    // 路3：关闭后喂入 → ECLOSED
    p.open = false;
    let r3 = matches!(p.feed(b'x'), Err(x) if x == ES_ECLOSED);
    p.open = true;
    // 路4：序列超长 → EOVER
    let mut p2 = EscParser::new();
    let mut r4 = false;
    let _ = p2.feed(ESC);
    let _ = p2.feed(CSI);
    let mut n = 0;
    while n < SEQ_MAX + 2 {
        match p2.feed(b'1') {
            Err(ES_EOVER) => {
                r4 = true;
                break;
            }
            _ => n += 1,
        }
    }
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮随机方向键，分段 1~3 段跨读喂入；重组正确记 1 轮。
pub fn sample_thousand() -> u32 {
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let keys = [(ArrowKey::Up, 1), (ArrowKey::Down, 2), (ArrowKey::Right, 3), (ArrowKey::Left, 4)];
        let (key, code) = keys[(r % 4) as usize];
        let sq = arrow_seq(key);
        let mut p = EscParser::new();
        // 分段数 1~3（跨读）
        let parts = (r % 3) + 1;
        match p.feed_split(&sq, parts as usize) {
            Ok(FeedOut::Arrow(k)) => {
                if k.code() == code && p.reconcile() && p.arrows == 1 && p.plain == 0 {
                    good = good.wrapping_add(1);
                }
            }
            _ => {}
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10969_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10969");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "ESEQ/ECLOSED/EOVER 坏样本全部命中");

    // J1 方向键逐段完整 10/10
    let mut ok10 = 0u32;
    for i in 0..10u32 {
        let keys = [ArrowKey::Up, ArrowKey::Down, ArrowKey::Right, ArrowKey::Left];
        let k = keys[(i % 4) as usize];
        let sq = arrow_seq(k);
        let mut p = EscParser::new();
        let mut out = FeedOut::Pending;
        let mut all_ok = true;
        for b in sq {
            match p.feed(b) {
                Ok(o) => out = o,
                Err(_) => all_ok = false,
            }
        }
        if all_ok && out == FeedOut::Arrow(k) && p.reconcile() {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add("J1-逐段到达完整10/10", ok10 == 10, "三字节序列十轮逐段喂入全重组");

    // 跨读分段重组 10/10
    let mut split10 = 0u32;
    for i in 0..10u32 {
        let keys = [ArrowKey::Up, ArrowKey::Down, ArrowKey::Right, ArrowKey::Left];
        let k = keys[(i % 4) as usize];
        let sq = arrow_seq(k);
        let mut p = EscParser::new();
        let parts = (i % 3) + 1;
        if p.feed_split(&sq, parts as usize) == Ok(FeedOut::Arrow(k)) {
            split10 = split10.wrapping_add(1);
        }
    }
    s.add("J2-跨读重组10/10", split10 == 10, "1~3 段跨读重组全对");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮样本", good == 1000, "千轮方向键重组全对");

    // 前两段保持挂起
    let mut p2 = EscParser::new();
    let seg_ok = p2.feed(ESC) == Ok(FeedOut::Pending)
        && p2.feed(CSI) == Ok(FeedOut::Pending)
        && p2.state == ST_CSI;
    s.add("J4-分段挂起", seg_ok, "前两字节逐段保持 Pending");

    // 普通字符直通
    let mut p3 = EscParser::new();
    let _ = p3.feed(b'a');
    let _ = p3.feed(b'b');
    let plain_ok = p3.plain == 2 && p3.state == ST_GROUND;
    s.add("J5-普通字符直通", plain_ok, "非 ESC 字节不进状态机");

    // 错误码分段
    let codes = [ES_EBADF, ES_EINVAL, ES_ESEQ, ES_EOVER, ES_ELOSE, ES_ECLOSED];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J6-错误码分段", seg_ok, "六码全部落在 0x1d8a0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J7-错误码互异", distinct, "六码两两互异");

    // 账目对平
    let mut p4 = EscParser::new();
    let _ = p4.feed_split(&arrow_seq(ArrowKey::Up), 2);
    let _ = p4.feed(b'x');
    s.add(
        "J8-账目对平",
        p4.arrows == 1 && p4.plain == 1,
        "方向键 1 + 普通字符 1 与账面一致",
    );

    // 残缺后状态复位
    let mut p5 = EscParser::new();
    let _ = p5.feed(ESC);
    let _ = p5.feed(b'Q');
    let reset_ok = p5.state == ST_GROUND && p5.n_eseq == 1;
    s.add("J9-残缺复位", reset_ok, "残缺序列报错后状态归零");

    // fold 稳定
    let h1 = p4.fold_all();
    let h2 = p4.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = p4.render();
    s.add(
        "J11-渲染非空",
        r.starts_with("esc:") && r.contains("arrows=1"),
        "摘要含状态机与重组账",
    );

    // 连续两键
    let mut p6 = EscParser::new();
    let a = p6.feed_split(&arrow_seq(ArrowKey::Left), 1);
    let b = p6.feed_split(&arrow_seq(ArrowKey::Right), 3);
    let two_ok = a == Ok(FeedOut::Arrow(ArrowKey::Left)) && b == Ok(FeedOut::Arrow(ArrowKey::Right)) && p6.arrows == 2;
    s.add("J12-连续两键", two_ok, "两键连续重组互不干扰");

    // 动态判据
    s.add_dyn(
        "J13-重组账",
        p6.arrows == 2 && p6.reconcile(),
        &crate::alloc::format!("arrows={} plain={}", p6.arrows, p6.plain),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-错误账",
        p5.n_eseq == 1 && p5.n_eover == 0,
        &crate::alloc::format!("eseq={} eover={}", p5.n_eseq, p5.n_eover),
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
    fn unx10969_escseq_parse_flow() {
        let mut p = EscParser::new();
        assert_eq!(p.feed(ESC), Ok(FeedOut::Pending));
        assert_eq!(p.feed(CSI), Ok(FeedOut::Pending));
        assert_eq!(p.feed(b'A'), Ok(FeedOut::Arrow(ArrowKey::Up)));
        assert!(p.reconcile());
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10969_checks_green() {
        let s = run_unx10969_checks();
        assert!(s.all_passed(), "UNX-F10969 判据未全绿");
    }
}
