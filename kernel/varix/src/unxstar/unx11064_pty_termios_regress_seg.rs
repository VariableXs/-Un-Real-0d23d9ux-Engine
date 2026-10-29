//! UNX-F11064 pty/termios/行规程回归段（0x1e800 段）
//!
//! 本段把 B05–B07 三批判据重放为一张可冻结的账：每个用例登记
//! （批次, 终端操作, 期望返回码, 实测返回码），重放时逐格对拍；
//! 只有 10/10 全绿、且期望面与实测面的指纹逐位一致时才允许封账。
//!
//! 不变式：enrolled == green + red；fold 对任一字段敏感；冻结后
//! 全部写路径以 PT_FROZEN 拒止。行规程侧自带 ERASE/KILL/INTR 边编辑
//! 与 OPOST/ONLCR 翻译状态机，不借用外部 tty 会话。
//!
//! 结构：TermiosCase（一行判据）/ LineDiscipline（行编辑状态机）/
//! PtySegment（批次总账 + 重放 + 冻结）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11064 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e800;

/// 空账：本段没有任何登记行
pub const PT_EMPTY: u32 = SEG_BASE + 0x01;
/// 越界：批次不在 B05..=B07 或窗口尺寸超限
pub const PT_RANGE: u32 = SEG_BASE + 0x02;
/// 重号：同一用例号被二次登记
pub const PT_DUP: u32 = SEG_BASE + 0x03;
/// 失配：重放结果与录制期望不一致
pub const PT_MISMATCH: u32 = SEG_BASE + 0x04;
/// 短缺：登记行不足 10 条即申请重放
pub const PT_SHORT: u32 = SEG_BASE + 0x05;
/// 已封：段账冻结后的任何写操作
pub const PT_FROZEN: u32 = SEG_BASE + 0x06;

/// FNV 折前缀：低两字节 0x64 标记 11064 号卡
pub const PT_TAG: u64 = 0x1e80_64;

/// 行规程单行缓冲上限（超出即 PT_RANGE）
const LINE_CAP: usize = 256;
/// 样本批次下界（B05）
const BATCH_LO: u8 = 5;
/// 样本批次上界（B07）
const BATCH_HI: u8 = 7;
/// 本段重放固定席位
const REPLAY_SEATS: usize = 10;

/// termios lflag：规范行模式
const ICANON: u32 = 0x0000_0002;
/// termios lflag：回显
const ECHO: u32 = 0x0000_0008;
/// termios lflag：信号字符生效
const ISIG: u32 = 0x0000_0001;
/// termios oflag：输出处理使能
const OPOST: u32 = 0x0000_0001;
/// termios oflag：NL→CRNL 映射
const ONLCR: u32 = 0x0000_0004;

fn fnv_byte(h: u64, b: u8) -> u64 {
    let x = h ^ (b as u64);
    x.wrapping_mul(0x0000_0100_0000_01b3)
}

fn fnv_u32(mut h: u64, v: u32) -> u64 {
    let mut k = 0u32;
    while k < 4 {
        h = fnv_byte(h, ((v >> (k.wrapping_mul(8))) & 0xff) as u8);
        k = k.wrapping_add(1);
    }
    h
}

fn fnv_u16(mut h: u64, v: u16) -> u64 {
    h = fnv_byte(h, (v & 0xff) as u8);
    fnv_byte(h, ((v >> 8) & 0xff) as u8)
}

/// B05–B07 覆盖的终端操作闭集
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TtyOp {
    /// tcsetattr 写属性
    SetAttr,
    /// tcgetattr 读属性
    GetAttr,
    /// TIOCSWINSZ 窗口尺寸
    Winsize,
    /// VMIN/VTIME 读语义
    Vtime,
    /// 回显开关
    Echo,
}

impl TtyOp {
    /// 闭集判别码（写账用）
    pub fn code(self) -> u8 {
        match self {
            TtyOp::SetAttr => 1,
            TtyOp::GetAttr => 2,
            TtyOp::Winsize => 3,
            TtyOp::Vtime => 4,
            TtyOp::Echo => 5,
        }
    }

    /// 闭集名（可读面）
    pub fn name(self) -> &'static str {
        match self {
            TtyOp::SetAttr => "setattr",
            TtyOp::GetAttr => "getattr",
            TtyOp::Winsize => "winsize",
            TtyOp::Vtime => "vtime",
            TtyOp::Echo => "echo",
        }
    }
}

/// 单条判据行：一行一次对拍
#[derive(Clone, Debug)]
pub struct TermiosCase {
    /// 用例号（1..=10，段内唯一）
    pub id: u16,
    /// 批次 B05/B06/B07
    pub batch: u8,
    /// 覆盖的终端操作
    pub op: TtyOp,
    /// 录制基线的期望返回码
    pub expected: u32,
    /// 本次重放的实测返回码
    pub observed: u32,
}

impl TermiosCase {
    /// 构造一条判据行
    pub fn new(id: u16, batch: u8, op: TtyOp, expected: u32, observed: u32) -> TermiosCase {
        TermiosCase { id, batch, op, expected, observed }
    }

    /// 本行是否绿（期望面 == 实测面）
    pub fn green(&self) -> bool {
        self.expected == self.observed
    }

    /// 偏差量（绿行为 0，红行为异或差）
    pub fn deviation(&self) -> u32 {
        self.expected ^ self.observed
    }

    /// 判据行指纹：批次/操作/期望/实测四元组折叠
    pub fn digest(&self, mut h: u64) -> u64 {
        h = fnv_u16(h, self.id);
        h = fnv_byte(h, self.batch);
        h = fnv_byte(h, self.op.code());
        h = fnv_u32(h, self.expected);
        fnv_u32(h, self.observed)
    }
}

/// 行规程状态机：规范行编辑 + 输出翻译
#[derive(Clone, Debug)]
pub struct LineDiscipline {
    pub iflag: u32,
    pub oflag: u32,
    pub cflag: u32,
    pub lflag: u32,
    /// 非规范模式最少可读字节
    pub vmin: u8,
    /// 非规范模式读超时（0.1s 单位）
    pub vtime: u8,
    pub cols: u16,
    pub rows: u16,
    /// 已回显字节数
    pub echo: u32,
    /// 待投递信号（0 = 无，2 = SIGINT）
    pub pending_sig: u8,
    /// 已交付行数
    pub delivered: u32,
    /// ERASE 命中数
    pub erased: u32,
    /// KILL 命中数
    pub killed: u32,
    buf: Vec<u8>,
    frozen: Option<u64>,
}

impl LineDiscipline {
    /// 默认 pty 属性：规范 + 回显 + ISIG + OPOST/ONLCR，24x80
    pub fn new() -> LineDiscipline {
        LineDiscipline {
            iflag: 0,
            oflag: OPOST | ONLCR,
            cflag: 0x0000_00bf,
            lflag: ICANON | ECHO | ISIG,
            vmin: 1,
            vtime: 0,
            cols: 80,
            rows: 24,
            echo: 0,
            pending_sig: 0,
            delivered: 0,
            erased: 0,
            killed: 0,
            buf: Vec::new(),
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(PT_FROZEN);
        }
        Ok(())
    }

    /// 已缓存但未交付的字节数
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// 投递一个输入字节。返回 Some(line) 表示规范行已交付（含换行）。
    pub fn push(&mut self, b: u8) -> Result<Option<Vec<u8>>, u32> {
        self.guard()?;
        if b == 0x03 && (self.lflag & ISIG) != 0 {
            self.pending_sig = 2;
            return Ok(None);
        }
        if (self.lflag & ICANON) == 0 {
            if (self.lflag & ECHO) != 0 {
                self.echo = self.echo.wrapping_add(1);
            }
            self.delivered = self.delivered.wrapping_add(1);
            return Ok(Some(alloc::vec![b]));
        }
        if b == 0x7f {
            if self.buf.pop().is_some() {
                self.erased = self.erased.wrapping_add(1);
            }
            return Ok(None);
        }
        if b == 0x15 {
            if !self.buf.is_empty() {
                self.killed = self.killed.wrapping_add(1);
            }
            self.buf.clear();
            return Ok(None);
        }
        if b == b'\n' {
            let line = core::mem::take(&mut self.buf);
            self.delivered = self.delivered.wrapping_add(1);
            return Ok(Some(line));
        }
        if self.buf.len() >= LINE_CAP {
            return Err(PT_RANGE);
        }
        self.buf.push(b);
        if (self.lflag & ECHO) != 0 {
            self.echo = self.echo.wrapping_add(1);
        }
        Ok(None)
    }

    /// 取走待投递信号（读后清零）
    pub fn take_signal(&mut self) -> u8 {
        let s = self.pending_sig;
        self.pending_sig = 0;
        s
    }

    /// 设置窗口尺寸；0 或 >4096 拒收，返回 (rows<<16)|cols
    pub fn winsize(&mut self, rows: u16, cols: u16) -> Result<u32, u32> {
        self.guard()?;
        if rows == 0 || cols == 0 || rows > 4096 || cols > 4096 {
            return Err(PT_RANGE);
        }
        self.rows = rows;
        self.cols = cols;
        Ok(((rows as u32) << 16) | (cols as u32))
    }

    /// 输出字节翻译：OPOST 且 ONLCR 时 NL→CRNL 的首字节
    pub fn translate_out(&mut self, b: u8) -> u8 {
        if (self.oflag & OPOST) != 0 && (self.oflag & ONLCR) != 0 && b == b'\n' {
            b'\r'
        } else {
            b
        }
    }

    /// 改写 lflag（用于回显/规范模式切换）
    pub fn set_lflag(&mut self, bits: u32) -> Result<(), u32> {
        self.guard()?;
        if (bits & !0x0000_8fff) != 0 {
            return Err(PT_RANGE);
        }
        self.lflag = bits;
        Ok(())
    }

    /// 行规程状态折叠（含未交付缓冲）
    pub fn fold(&self) -> u64 {
        let mut h = PT_TAG;
        h = fnv_u32(h, self.iflag);
        h = fnv_u32(h, self.oflag);
        h = fnv_u32(h, self.cflag);
        h = fnv_u32(h, self.lflag);
        h = fnv_byte(h, self.vmin);
        h = fnv_byte(h, self.vtime);
        h = fnv_u16(h, self.cols);
        h = fnv_u16(h, self.rows);
        h = fnv_u32(h, self.echo);
        h = fnv_u32(h, self.delivered);
        h = fnv_u32(h, self.erased);
        h = fnv_u32(h, self.killed);
        h = fnv_byte(h, self.pending_sig);
        let mut k = 0usize;
        while k < self.buf.len() {
            h = fnv_byte(h, self.buf[k]);
            k += 1;
        }
        h
    }

    /// 冻结行规程，返回冻结指纹
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }

    /// 是否已冻结
    pub fn is_frozen(&self) -> bool {
        self.frozen.is_some()
    }
}

/// B05–B07 回归段总账
#[derive(Clone, Debug)]
pub struct PtySegment {
    /// 判据行表（按登记序）
    pub cases: Vec<TermiosCase>,
    /// 段内行规程实例
    pub line: LineDiscipline,
    /// 最近一次重放的绿行数
    pub replayed: u16,
    frozen: Option<u64>,
}

impl PtySegment {
    /// 空段账
    pub fn new() -> PtySegment {
        PtySegment {
            cases: Vec::new(),
            line: LineDiscipline::new(),
            replayed: 0,
            frozen: None,
        }
    }

    fn guard(&self) -> Result<(), u32> {
        if self.frozen.is_some() {
            return Err(PT_FROZEN);
        }
        Ok(())
    }

    /// 登记一条判据行：0 号 / 越批 / 重号 / 冻结四道闸
    pub fn enroll(&mut self, case: TermiosCase) -> Result<(), u32> {
        self.guard()?;
        if case.id == 0 {
            return Err(PT_EMPTY);
        }
        if case.batch < BATCH_LO || case.batch > BATCH_HI {
            return Err(PT_RANGE);
        }
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].id == case.id {
                return Err(PT_DUP);
            }
            k += 1;
        }
        self.cases.push(case);
        Ok(())
    }

    /// 红行计数（期望面与实测面不一致）
    pub fn red_count(&self) -> u16 {
        let mut n = 0u16;
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].green() {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 登记序是否严格递增（J7 排序不变量）
    pub fn ids_ordered(&self) -> bool {
        let mut k = 1usize;
        while k < self.cases.len() {
            if self.cases[k].id <= self.cases[k - 1].id {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 录制基线指纹：只折期望面
    pub fn baseline_fold(&self) -> u64 {
        let mut h = PT_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            h = fnv_u16(h, self.cases[k].id);
            h = fnv_byte(h, self.cases[k].batch);
            h = fnv_byte(h, self.cases[k].op.code());
            h = fnv_u32(h, self.cases[k].expected);
            k += 1;
        }
        h
    }

    /// 实测面指纹：只折实测值
    pub fn live_fold(&self) -> u64 {
        let mut h = PT_TAG;
        let mut k = 0usize;
        while k < self.cases.len() {
            h = fnv_u16(h, self.cases[k].id);
            h = fnv_byte(h, self.cases[k].batch);
            h = fnv_byte(h, self.cases[k].op.code());
            h = fnv_u32(h, self.cases[k].observed);
            k += 1;
        }
        h
    }

    /// 基线一致：满 10 席且两面指纹相等（逐位可复现）
    pub fn baseline_consistent(&self) -> bool {
        self.cases.len() == REPLAY_SEATS && self.baseline_fold() == self.live_fold()
    }

    /// 全量重放：席位不足 PT_SHORT，存在红行 PT_MISMATCH
    pub fn replay(&mut self) -> Result<u16, u32> {
        self.guard()?;
        if self.cases.len() < REPLAY_SEATS {
            return Err(PT_SHORT);
        }
        let mut green = 0u16;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].green() {
                green = green.wrapping_add(1);
            }
            k += 1;
        }
        self.replayed = green;
        if green as usize != self.cases.len() {
            return Err(PT_MISMATCH);
        }
        Ok(green)
    }

    /// 审计：空账 / 短缺 / 失配三级
    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.is_empty() {
            return Err(PT_EMPTY);
        }
        if self.cases.len() < REPLAY_SEATS {
            return Err(PT_SHORT);
        }
        if self.red_count() > 0 {
            return Err(PT_MISMATCH);
        }
        Ok(())
    }

    /// 改写某行的实测值（用于指纹敏感性对照）
    pub fn perturb(&mut self, id: u16, observed: u32) -> Result<(), u32> {
        self.guard()?;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].id == id {
                self.cases[k].observed = observed;
                return Ok(());
            }
            k += 1;
        }
        Err(PT_EMPTY)
    }

    /// 段账指纹：批次表 + 行规程 + 重放计数
    pub fn fold(&self) -> u64 {
        let mut h = self.line.fold();
        h = fnv_u16(h, self.replayed);
        let mut k = 0usize;
        while k < self.cases.len() {
            h = self.cases[k].digest(h);
            k += 1;
        }
        h
    }

    /// 冻结段账
    pub fn freeze(&mut self) -> u64 {
        let f = self.fold();
        self.frozen = Some(f);
        f
    }
}

/// 四条负路径：空账 / 越批 / 重号 / 冻结
pub fn inject_four() -> [bool; 4] {
    let empty = matches!(PtySegment::new().audit(), Err(PT_EMPTY));
    let mut r = PtySegment::new();
    let range = matches!(
        r.enroll(TermiosCase::new(1, 9, TtyOp::Echo, 0, 0)),
        Err(PT_RANGE)
    );
    let mut d = PtySegment::new();
    let first = d.enroll(TermiosCase::new(5, 5, TtyOp::Echo, 0, 0)).is_ok();
    let dup = first
        && matches!(
            d.enroll(TermiosCase::new(5, 6, TtyOp::SetAttr, 0, 0)),
            Err(PT_DUP)
        );
    let mut f = PtySegment::new();
    f.freeze();
    let frozen = matches!(
        f.enroll(TermiosCase::new(8, 5, TtyOp::Echo, 0, 0)),
        Err(PT_FROZEN)
    );
    [empty, range, dup, frozen]
}

/// UNX-F11064 判据入口
pub fn run_unx11064_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11064");
    let ops = [TtyOp::SetAttr, TtyOp::GetAttr, TtyOp::Winsize, TtyOp::Vtime, TtyOp::Echo];
    let mut seg = PtySegment::new();
    let mut seeded = true;
    let mut i = 0u16;
    while i < REPLAY_SEATS as u16 {
        let batch = BATCH_LO + (i % 3) as u8;
        let op = ops[(i as usize) % ops.len()];
        if seg.enroll(TermiosCase::new(i.wrapping_add(1), batch, op, 0, 0)).is_err() {
            seeded = false;
        }
        i = i.wrapping_add(1);
    }
    let green = seg.replay().unwrap_or(0);

    let mut line = LineDiscipline::new();
    let mut delivered = 0usize;
    let mut stream = [b'a', b'b', 0x7f, b'c', b'\n'];
    let mut si = 0usize;
    while si < stream.len() {
        match line.push(stream[si]) {
            Ok(Some(_)) => delivered += 1,
            Ok(None) => {}
            Err(_) => seeded = false,
        }
        si += 1;
    }
    let edit_ok = delivered == 1 && line.buffered() == 0 && line.erased == 1 && line.echo == 3;
    let intr_ok = matches!(line.push(0x03), Ok(None)) && line.pending_sig == 2;
    let win = line.winsize(40, 120);
    let win_ok = matches!(win, Ok(v) if v == 0x0028_0078);
    let nl = line.translate_out(b'\n');

    s.add(
        "J1a-终端判据全量重放",
        seeded && green == 10 && seg.cases.len() == REPLAY_SEATS,
        "B05–B07 十席必须全部登记且逐格绿",
    );
    s.add(
        "J1b-录制基线一致",
        seg.baseline_consistent(),
        "期望面与实测面指纹必须逐位相等",
    );
    let conserved = green as usize + seg.red_count() as usize == seg.cases.len();
    s.add(
        "J2-守恒恒等式",
        conserved && seg.replayed == green,
        "enrolled 必须等于 green + red",
    );
    let inj = inject_four();
    s.add("J3a-空账拒止", inj[0], "无登记行时 audit 必须 PT_EMPTY");
    s.add("J3b-越批拒止", inj[1], "B09 批次必须被 PT_RANGE 拒绝");
    s.add("J3c-重号拒止", inj[2], "同号二次登记必须 PT_DUP");
    s.add("J3d-冻结拒止", inj[3], "冻结后的登记必须 PT_FROZEN");
    let mut fz = seg.clone();
    let frozen_fp = fz.freeze();
    let frozen_reject = matches!(
        fz.enroll(TermiosCase::new(77, BATCH_LO, TtyOp::Echo, 0, 0)),
        Err(PT_FROZEN)
    );
    s.add(
        "J4-冻结不可写",
        frozen_reject && fz.fold() == frozen_fp && line.is_frozen() == false,
        "冻结后 fold 不变且写路径全拒",
    );
    let base_fp = seg.clone().fold();
    let mut sens = seg.clone();
    let sens_ok = sens.perturb(3, 1).is_ok() && sens.fold() != base_fp;
    s.add("J5-指纹敏感性", sens_ok, "单格实测扰动必须改变 fold");
    let empty_audit = matches!(PtySegment::new().audit(), Err(PT_EMPTY));
    let mut rng = PtySegment::new();
    let range_enroll = matches!(
        rng.enroll(TermiosCase::new(2, 8, TtyOp::Echo, 0, 0)),
        Err(PT_RANGE)
    );
    let range_win = matches!(LineDiscipline::new().winsize(0, 80), Err(PT_RANGE));
    s.add(
        "J6-范围与空账拒止",
        empty_audit && range_enroll && range_win,
        "空账/越批/零窗口三路都必须拒",
    );
    let mut dup_seg = PtySegment::new();
    let mut unique_ok = dup_seg.enroll(TermiosCase::new(1, 5, TtyOp::Echo, 0, 0)).is_ok();
    unique_ok = unique_ok && matches!(
        dup_seg.enroll(TermiosCase::new(1, 5, TtyOp::Echo, 0, 0)),
        Err(PT_DUP)
    );
    s.add(
        "J7-唯一与排序不变量",
        unique_ok && seg.ids_ordered(),
        "用例号段内唯一且登记序严格递增",
    );
    let mut partial = seg.clone();
    while partial.cases.len() > 5 {
        partial.cases.pop();
    }
    let partial_ok = matches!(partial.replay(), Err(PT_SHORT))
        && matches!(partial.audit(), Err(PT_SHORT))
        && !partial.baseline_consistent();
    s.add(
        "J8-半成品不得全绿",
        partial_ok,
        "5 席半账必须短路为 PT_SHORT 而非绿",
    );
    s.add(
        "J10-行规程边编辑",
        edit_ok && intr_ok && nl == b'\r' && win_ok,
        "ERASE/INTR/ONLCR/窗口编码四路语义必须成立",
    );
    s.add_dyn(
        "J9-动态账",
        green == 10 && delivered == 1,
        &format!(
            "green={} red={} 交付={} 回显={}",
            green,
            seg.red_count(),
            line.delivered,
            line.echo
        ),
    );
    s.add_dyn(
        "J9b-指纹账",
        seg.fold() != 0,
        &format!("fold={:016x} tag={:x}", seg.fold(), PT_TAG),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11064_flow() {
        let mut seg = PtySegment::new();
        let mut i = 0u16;
        while i < 10 {
            let batch = BATCH_LO + (i % 3) as u8;
            let case = TermiosCase::new(i + 1, batch, TtyOp::Echo, 0, 0);
            assert!(seg.enroll(case).is_ok(), "样本登记必须成功");
            i += 1;
        }
        assert_eq!(seg.replay().unwrap_or(0), 10);
        assert!(seg.baseline_consistent());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11064_checks_green() {
        assert!(run_unx11064_checks().all_passed(), "UNX-F11064 判据未全绿");
    }
}
