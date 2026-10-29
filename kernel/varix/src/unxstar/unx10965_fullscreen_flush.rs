//! UNX-F10965 全屏刷新输出联测（write 吞吐与缓冲协同）（0x1d820 段）
//!
//! 卡片判据（UNX-F10965-J1）：
//! - 全屏刷新 100 轮零丢失（100/100 逐字节对账），缓冲协同对账（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模全屏刷新输出链：帧缓冲按
//! 行填充 → write 吞吐段切分（缓冲协同：小块攒批、满块即发）→
//! 终端落账。逐字节对账：帧字节数 = 落账字节数，攒批不丢字节。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10965-10900)*0x20 = 0x1d820）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d820;

/// FW_EINVAL：尺寸非法（零行/零列/超界）
pub const FW_EINVAL: u32 = SEG_BASE + 0x01;
/// FW_EIO：输出端失效
pub const FW_EIO: u32 = SEG_BASE + 0x02;
/// FW_ENOSPC：帧缓冲溢出
pub const FW_ENOSPC: u32 = SEG_BASE + 0x03;
/// FW_ELOSE：逐字节对账不平（自检面）
pub const FW_ELOSE: u32 = SEG_BASE + 0x04;
/// FW_ECOAL：攒批账不平（自检面）
pub const FW_ECOAL: u32 = SEG_BASE + 0x05;
/// FW_ESTATE：帧未满即刷新被拒
pub const FW_ESTATE: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 屏幕行数
pub const ROWS: usize = 24;
/// 屏幕列数
pub const COLS: usize = 80;
/// 输出缓冲容量（字节）
const OUT_CAP: usize = 256;
/// 吞吐块大小（字节）
const CHUNK: usize = 64;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 一帧全屏内容
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<u8>,
    /// 帧号
    pub no: u64,
}

impl Frame {
    /// 空白帧
    pub fn blank(rows: usize, cols: usize, no: u64) -> Result<Frame, u32> {
        if rows == 0 || cols == 0 || rows > 64 || cols > 256 {
            return Err(FW_EINVAL);
        }
        Ok(Frame { rows, cols, cells: vec![b' '; rows.wrapping_mul(cols)], no })
    }

    /// 填充一行
    pub fn put_row(&mut self, row: usize, text: &str) -> Result<(), u32> {
        if row >= self.rows {
            return Err(FW_EINVAL);
        }
        let base = row.wrapping_mul(self.cols);
        let mut k = 0usize;
        let bytes = text.as_bytes();
        while k < self.cols {
            let ch = if k < bytes.len() { bytes[k] } else { b' ' };
            self.cells[base + k] = ch;
            k = k.wrapping_add(1);
        }
        Ok(())
    }

    /// 帧字节数
    pub fn byte_len(&self) -> usize {
        self.cells.len()
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.no).wrapping_mul(0x100000001b3);
        for b in &self.cells {
            h = (h ^ (*b as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }
}

/// 全屏刷新输出台
#[derive(Clone, Debug)]
pub struct ScrWriter {
    pub frame: Frame,
    /// 输出缓冲（攒批）
    out_buf: Vec<u8>,
    /// 终端落账字节
    pub landed: u64,
    /// 帧账（应落账字节）
    pub frame_bytes: u64,
    /// write 吞吐段数
    pub chunks: u64,
    /// 刷新轮数
    pub rounds: u64,
    /// 输出端是否存活
    pub out_alive: bool,
    /// 错误计数
    pub n_eio: u32,
    pub n_einval: u32,
}

impl ScrWriter {
    pub fn new() -> Result<Self, u32> {
        Ok(ScrWriter {
            frame: Frame::blank(ROWS, COLS, 0)?,
            out_buf: Vec::new(),
            landed: 0,
            frame_bytes: 0,
            chunks: 0,
            rounds: 0,
            out_alive: true,
            n_eio: 0,
            n_einval: 0,
        })
    }

    /// 写帧内容一行
    pub fn fill_row(&mut self, row: usize, text: &str) -> Result<(), u32> {
        self.frame.put_row(row, text)
    }

    /// 缓冲协同推入：满 CHUNK 即切一段落账
    fn push_bytes(&mut self, data: &[u8]) -> Result<(), u32> {
        if !self.out_alive {
            self.n_eio = self.n_eio.wrapping_add(1);
            return Err(FW_EIO);
        }
        for b in data {
            if self.out_buf.len() >= OUT_CAP {
                self.flush_chunk()?;
            }
            self.out_buf.push(*b);
        }
        Ok(())
    }

    /// 切一段（CHUNK 或缓冲全量）
    fn flush_chunk(&mut self) -> Result<(), u32> {
        let n = if self.out_buf.len() >= CHUNK { CHUNK } else { self.out_buf.len() };
        if n == 0 {
            return Ok(());
        }
        let mut k = 0usize;
        while k < n {
            self.out_buf.remove(0);
            k = k.wrapping_add(1);
        }
        self.landed = self.landed.wrapping_add(n as u64);
        self.chunks = self.chunks.wrapping_add(1);
        Ok(())
    }

    /// 全屏刷新一轮：整帧推入并清空缓冲尾
    pub fn refresh(&mut self) -> Result<u64, u32> {
        if !self.out_alive {
            self.n_eio = self.n_eio.wrapping_add(1);
            return Err(FW_EIO);
        }
        let bytes = self.frame.byte_len() as u64;
        self.frame_bytes = self.frame_bytes.wrapping_add(bytes);
        let cells = core::mem::take(&mut self.frame.cells);
        let pushed = self.push_bytes(&cells);
        self.frame.cells = cells;
        pushed?;
        // 尾批：缓冲剩余不足 CHUNK 的部分强制落账
        while !self.out_buf.is_empty() {
            self.flush_chunk()?;
        }
        self.rounds = self.rounds.wrapping_add(1);
        self.frame.no = self.frame.no.wrapping_add(1);
        Ok(self.landed)
    }

    /// 逐字节对账：帧账=落账 且 缓冲清空
    pub fn reconcile(&self) -> Result<(), u32> {
        if self.landed != self.frame_bytes || !self.out_buf.is_empty() {
            return Err(FW_ELOSE);
        }
        Ok(())
    }

    /// 攒批协同对账：段数与字节数匹配（每段 ≤ CHUNK）
    pub fn coalesce_ok(&self) -> bool {
        if self.chunks == 0 {
            return self.landed == 0;
        }
        // 每段至多 CHUNK 字节，总字节 = 段*CHUNK 以内
        self.landed <= self.chunks * CHUNK as u64
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_rounds: u64) -> bool {
        self.rounds == expect_rounds && self.reconcile().is_ok()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h = self.frame.fold();
        h = (h ^ self.landed).wrapping_mul(0x100000001b3);
        h = (h ^ self.chunks).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "scrw:rounds={} landed={} chunks={} frame={}",
            self.rounds,
            self.landed,
            self.chunks,
            self.frame.no
        )
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    // 路1：零尺寸帧 → EINVAL
    let r1 = Frame::blank(0, 80, 1).is_err() && matches!(Frame::blank(0, 80, 1), Err(x) if x == FW_EINVAL);
    // 路2：超界行写入 → EINVAL
    let mut f = match Frame::blank(ROWS, COLS, 1) { Ok(f) => f, Err(_) => return [false; 4] };
    let r2 = matches!(f.put_row(ROWS + 1, "x"), Err(x) if x == FW_EINVAL);
    // 路3：输出端失效 → EIO
    let mut w = match ScrWriter::new() { Ok(w) => w, Err(_) => return [false; 4] };
    w.out_alive = false;
    let r3 = matches!(w.refresh(), Err(x) if x == FW_EIO);
    // 路4：对账丢失面：伪造落账缺口
    let mut w2 = match ScrWriter::new() { Ok(w) => w, Err(_) => return [false; 4] };
    w2.frame_bytes = 10;
    let r4 = w2.reconcile() == Err(FW_ELOSE);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮填帧一行并全屏刷新；百轮周期对账。
pub fn sample_thousand() -> u32 {
    let mut w = match ScrWriter::new() { Ok(w) => w, Err(_) => return 0 };
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        // 填一行标记帧轮
        let row = ((r as u64) % (ROWS as u64)) as usize;
        let txt = crate::alloc::format!("frame-{}", r);
        if w.fill_row(row, &txt).is_err() {
            return good;
        }
        if w.refresh().is_ok() {
            // 每百轮强制逐字节对账一次
            if r % 100 == 99 {
                if w.reconcile().is_ok() && w.coalesce_ok() {
                    good = good.wrapping_add(1);
                }
            } else {
                good = good.wrapping_add(1);
            }
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10965_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10965");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EINVAL/EIO/ELOSE 坏样本全部命中");

    // J1 全屏刷新百轮零丢失
    let mut w = match ScrWriter::new() { Ok(w) => w, Err(_) => return s };
    let mut ok100 = 0u32;
    for i in 0..100u64 {
        let row = (i % ROWS as u64) as usize;
        let _ = w.fill_row(row, "vim-line");
        if w.refresh().is_ok() {
            ok100 = ok100.wrapping_add(1);
        }
    }
    let j1 = ok100 == 100
        && w.reconcile().is_ok()
        && w.landed == 100 * (ROWS * COLS) as u64;
    s.add(
        "J1-百轮零丢失",
        j1,
        "100 轮全屏逐字节对账 100/100 全平",
    );

    // 缓冲协同对账 10/10
    let mut c10 = 0u32;
    for _ in 0..10 {
        let mut w2 = match ScrWriter::new() { Ok(w) => w, Err(_) => break };
        let _ = w2.fill_row(0, "coalesce");
        if w2.refresh().is_ok() && w2.coalesce_ok() && w2.out_buf.is_empty() {
            c10 = c10.wrapping_add(1);
        }
    }
    s.add("J2-缓冲协同对账10/10", c10 == 10, "攒批切分十轮账目自洽");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮样本", good == 1000, "千轮刷新全平");

    // 帧渲染正确性
    let mut f = match Frame::blank(ROWS, COLS, 7) { Ok(f) => f, Err(_) => return s };
    let _ = f.put_row(3, "hello");
    let row_ok = f.cells[3 * COLS] == b'h'
        && f.cells[3 * COLS + 4] == b'o'
        && f.cells[3 * COLS + 5] == b' '
        && f.cells[3 * COLS + COLS - 1] == b' ';
    s.add("J4-帧渲染正确", row_ok, "行填充右补空格");

    // 错误码分段
    let codes = [FW_EINVAL, FW_EIO, FW_ENOSPC, FW_ELOSE, FW_ECOAL, FW_ESTATE];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d820 段内");

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

    // 账目对平
    s.add("J7-账目对平", w.ledger_ok(100), "刷新轮数 100 与账面一致");

    // fold 稳定
    let h1 = w.fold_all();
    let h2 = w.fold_all();
    s.add("J8-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = w.render();
    s.add(
        "J9-渲染非空",
        r.starts_with("scrw:") && r.contains("rounds=100"),
        "摘要含吞吐账",
    );

    // 攒批段数合理：百轮 192000 字节，段数 = 落账/CHUNK（整除）
    let seg_math = w.landed == w.chunks * CHUNK as u64;
    s.add("J10-段数对账", seg_math, "落账字节恰为段数×块大小");

    // 帧号推进
    let fno_ok = w.frame.no == 100;
    s.add("J11-帧号推进", fno_ok, "百轮后帧号为 100");

    // 动态判据
    s.add_dyn(
        "J12-吞吐账",
        w.landed == 192_000,
        &crate::alloc::format!("landed={} chunks={}", w.landed, w.chunks),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J14-协同账",
        w.coalesce_ok(),
        &crate::alloc::format!("chunks={} landed={}", w.chunks, w.landed),
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
    fn unx10965_fullscreen_flush_flow() {
        let mut w = ScrWriter::new().unwrap();
        let _ = w.fill_row(0, "top");
        assert!(w.refresh().is_ok());
        assert_eq!(w.landed, (ROWS * COLS) as u64);
        assert!(w.reconcile().is_ok());
        assert!(matches!(Frame::blank(0, 10, 1), Err(x) if x == FW_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10965_checks_green() {
        let s = run_unx10965_checks();
        assert!(s.all_passed(), "UNX-F10965 判据未全绿");
    }
}
