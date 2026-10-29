//! UNX-F11085 pty vim 全屏长稳段（录制对照万轮）（0x1eaa0 段）
//!
//! pty 全屏回放维护两块同尺寸屏幕：live 与录制镜像 mirror。每帧对两屏施加
//! 同一编辑脚本并逐帧比较 digest，1000 帧汇总成一个录制窗口，窗口链把上一窗
//! 的末指纹接到下一窗的首指纹。万轮循环只保留 10 个窗口记录与标量累计，绝不
//! 逐帧 push。不变式：frames=10000、mismatches=0，10 窗 matched 之和等于帧数，
//! 窗口链首尾连续。
//!
//! 结构：屏幕模型（行列/单元/digest）+ 录制窗口（帧数/匹配数/链首尾）+ 回放账
//! （逐帧对照、窗口链、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11085 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1eaa0;

/// 空账 —— 尚未回放任何帧。
pub const PV_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— 屏幕行列越界/尺寸非法，或编辑坐标越界。
pub const PV_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同一录制窗口重复回放。
pub const PV_DUP: u32 = SEG_BASE + 0x03;
/// 帧失配 —— live 与 mirror 的 digest 不一致。
pub const PV_FRAME: u32 = SEG_BASE + 0x04;
/// 链漂移 —— 窗口指纹链断裂或帧总数不足。
pub const PV_DRIFT: u32 = SEG_BASE + 0x05;
/// 封账后回放 —— 回放账冻结后仍推进帧。
pub const PV_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（pty vim 回放账）。
pub const PV_TAG: u64 = 0x1ea_85;

/// 全屏行数。
pub const PV_ROWS: u8 = 12;
/// 全屏列数。
pub const PV_COLS: u8 = 24;
/// 窗口数。
pub const PV_WINDOWS: usize = 10;
/// 每窗帧数。
pub const PV_FRAMES_PER_WINDOW: u32 = 1_000;
/// 总帧数。
pub const PV_FRAMES: u32 = 10_000;

/// 一块字符屏幕。
#[derive(Clone, Debug)]
pub struct ScreenModel {
    pub rows: u8,
    pub cols: u8,
    pub cells: Vec<u8>,
}

impl ScreenModel {
    /// 建立屏幕；行列必须为正且不超过 32×80。
    pub fn new(rows: u8, cols: u8) -> Result<ScreenModel, u32> {
        if rows == 0 || cols == 0 || rows > 32 || cols > 80 {
            return Err(PV_RANGE);
        }
        let total = (rows as usize).wrapping_mul(cols as usize);
        let mut cells = Vec::with_capacity(total);
        let mut i = 0usize;
        while i < total {
            cells.push(b' ');
            i += 1;
        }
        Ok(ScreenModel { rows, cols, cells })
    }

    fn index(&self, row: u8, col: u8) -> Option<usize> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        Some((row as usize).wrapping_mul(self.cols as usize).wrapping_add(col as usize))
    }

    /// 写一个字符单元。
    pub fn put(&mut self, row: u8, col: u8, ch: u8) -> Result<(), u32> {
        if ch == 0 {
            return Err(PV_RANGE);
        }
        match self.index(row, col) {
            Some(i) => {
                self.cells[i] = ch;
                Ok(())
            }
            None => Err(PV_RANGE),
        }
    }

    /// 读一个字符单元。
    pub fn get(&self, row: u8, col: u8) -> Result<u8, u32> {
        match self.index(row, col) {
            Some(i) => Ok(self.cells[i]),
            None => Err(PV_RANGE),
        }
    }

    /// 全屏上滚一行，底行清空。
    pub fn scroll_up(&mut self) {
        let cols = self.cols as usize;
        let rows = self.rows as usize;
        let mut r = 0usize;
        while r + 1 < rows {
            let dst = r.wrapping_mul(cols);
            let src = (r + 1).wrapping_mul(cols);
            let mut c = 0usize;
            while c < cols {
                self.cells[dst + c] = self.cells[src + c];
                c += 1;
            }
            r += 1;
        }
        let last = rows.wrapping_sub(1).wrapping_mul(cols);
        let mut c = 0usize;
        while c < cols {
            self.cells[last + c] = b' ';
            c += 1;
        }
    }

    /// 屏幕 FNV 指纹。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = PV_TAG;
        h = (h ^ self.rows as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.cols as u64).wrapping_mul(0x100000001b3);
        let mut i = 0usize;
        while i < self.cells.len() {
            h = (h ^ self.cells[i] as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }

    /// 字符和校验。
    pub fn checksum(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            sum = sum.wrapping_add(self.cells[i] as u32);
            i += 1;
        }
        sum
    }
}

/// 一个录制窗口的帧账。
#[derive(Clone, Debug)]
pub struct FrameWindow {
    pub epoch: u16,
    pub frames: u32,
    pub matched: u32,
    pub mismatches: u32,
    pub first_chain: u64,
    pub last_chain: u64,
    pub checksum: u32,
}

impl FrameWindow {
    /// 窗口内逐帧对平。
    pub fn clean(&self) -> bool {
        self.frames != 0 && self.matched == self.frames && self.mismatches == 0
    }
}

/// pty vim 回放总账。
#[derive(Clone, Debug)]
pub struct VimReplay {
    pub live: ScreenModel,
    pub mirror: ScreenModel,
    pub windows: Vec<FrameWindow>,
    pub frames: u32,
    pub mismatches: u32,
    pub chain: u64,
    pub genesis: u64,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl VimReplay {
    pub fn new(rows: u8, cols: u8) -> Result<VimReplay, u32> {
        let live = ScreenModel::new(rows, cols)?;
        let mirror = ScreenModel::new(rows, cols)?;
        let genesis = live.digest() ^ mirror.digest();
        Ok(VimReplay {
            live,
            mirror,
            windows: Vec::new(),
            frames: 0,
            mismatches: 0,
            chain: genesis,
            genesis,
            frozen: false,
            root: None,
        })
    }

    fn epoch_at(&self, epoch: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.windows.len() {
            if self.windows[i].epoch == epoch {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 对两屏施加同一编辑。
    pub fn draw(&mut self, row: u8, col: u8, ch: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(PV_FROZEN);
        }
        self.live.put(row, col, ch)?;
        self.mirror.put(row, col, ch)?;
        Ok(())
    }

    /// 推进一帧：编辑、滚动、双屏 digest 对照、链累加。
    fn advance_frame(&mut self, serial: u32) -> bool {
        let rows = self.live.rows;
        let cols = self.live.cols;
        let row = (serial % rows as u32) as u8;
        let col = ((serial.wrapping_mul(7)) % cols as u32) as u8;
        let ch = b'a'.wrapping_add((serial % 26) as u8);
        let _ = self.draw(row, col, ch);
        if serial % 50 == 0 {
            self.live.scroll_up();
            self.mirror.scroll_up();
        }
        let same = self.live.digest() == self.mirror.digest();
        if !same {
            self.mismatches = self.mismatches.wrapping_add(1);
        }
        self.chain = (self.chain ^ self.live.digest()).wrapping_mul(0x100000001b3);
        self.frames = self.frames.wrapping_add(1);
        same
    }

    /// 回放一个千帧窗口。
    pub fn run_window(&mut self, epoch: u16, frames: u32) -> Result<FrameWindow, u32> {
        if self.frozen {
            return Err(PV_FROZEN);
        }
        if epoch >= PV_WINDOWS as u16 || frames == 0 || frames > PV_FRAMES_PER_WINDOW {
            return Err(PV_RANGE);
        }
        if self.epoch_at(epoch).is_some() {
            return Err(PV_DUP);
        }
        let first_chain = self.chain;
        let start_mismatch = self.mismatches;
        let mut matched = 0u32;
        let mut f = 0u32;
        while f < frames {
            let serial = (epoch as u32).wrapping_mul(PV_FRAMES_PER_WINDOW)
                .wrapping_add(f);
            if self.advance_frame(serial) {
                matched = matched.wrapping_add(1);
            }
            f = f.wrapping_add(1);
        }
        let win = FrameWindow {
            epoch,
            frames,
            matched,
            mismatches: self.mismatches.wrapping_sub(start_mismatch),
            first_chain,
            last_chain: self.chain,
            checksum: self.live.checksum(),
        };
        self.windows.push(win.clone());
        Ok(win)
    }

    /// 回放全部十窗。
    pub fn run_all(&mut self) -> Result<u64, u32> {
        let mut e = 0u16;
        while e < PV_WINDOWS as u16 {
            self.run_window(e, PV_FRAMES_PER_WINDOW)?;
            e = e.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 单帧即时对照；供异常注入直接调用。
    pub fn compare_frame(&self) -> Result<bool, u32> {
        if self.frames == 0 {
            return Err(PV_EMPTY);
        }
        if self.live.digest() != self.mirror.digest() {
            return Err(PV_FRAME);
        }
        if self.live.checksum() != self.mirror.checksum() {
            return Err(PV_FRAME);
        }
        Ok(true)
    }

    /// 窗口指纹链连续。
    pub fn chain_ok(&self) -> bool {
        if self.windows.len() != PV_WINDOWS {
            return false;
        }
        let mut prev = self.genesis;
        let mut i = 0usize;
        while i < self.windows.len() {
            if self.windows[i].first_chain != prev {
                return false;
            }
            prev = self.windows[i].last_chain;
            i += 1;
        }
        true
    }

    /// 万轮回放全平。
    pub fn all_frames_matched(&self) -> bool {
        if self.frames != PV_FRAMES || self.mismatches != 0 || self.windows.len() != 10 {
            return false;
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.windows.len() {
            if !self.windows[i].clean() {
                return false;
            }
            sum = sum.wrapping_add(self.windows[i].frames);
            i += 1;
        }
        sum == PV_FRAMES && self.chain_ok() && self.live.digest() == self.mirror.digest()
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.frames == 0 {
            return Err(PV_EMPTY);
        }
        if self.live.digest() != self.mirror.digest() || self.mismatches != 0 {
            return Err(PV_FRAME);
        }
        if self.frames != PV_FRAMES || !self.chain_ok() {
            return Err(PV_DRIFT);
        }
        Ok(())
    }

    /// FNV-1a：两屏指纹、帧数、失配数、链与十窗记录全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PV_TAG;
        h = (h ^ self.live.digest()).wrapping_mul(0x100000001b3);
        h = (h ^ self.mirror.digest()).wrapping_mul(0x100000001b3);
        h = (h ^ self.frames as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.mismatches as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.chain).wrapping_mul(0x100000001b3);
        let mut i = 0usize;
        while i < self.windows.len() {
            let w = &self.windows[i];
            h = (h ^ w.epoch as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.frames as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.matched as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.mismatches as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.last_chain).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }

    /// 冻回放账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 回放摘要。
    pub fn summary(&self) -> String {
        let label = "ptyvim".to_string();
        format!("{}|frames={}|windows={}|mismatch={}|checksum={}", label,
            self.frames, self.windows.len(), self.mismatches, self.live.checksum())
    }
}

/// 四路坏样本：屏幕尺寸越界、窗口重号、镜像帧失配、冻结后回放。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    got[0] = ScreenModel::new(0, 0).err() == Some(PV_RANGE);

    let mut b = match VimReplay::new(4, 8) {
        Ok(v) => v,
        Err(_) => return got,
    };
    let _ = b.run_window(0, 10);
    got[1] = b.run_window(0, 10).err() == Some(PV_DUP);

    let mut c = match VimReplay::new(4, 8) {
        Ok(v) => v,
        Err(_) => return got,
    };
    let _ = c.run_window(0, 10);
    let _ = c.mirror.put(0, 0, b'X');
    got[2] = c.compare_frame().err() == Some(PV_FRAME);

    let mut d = match VimReplay::new(4, 8) {
        Ok(v) => v,
        Err(_) => return got,
    };
    let _ = d.run_window(0, 10);
    let _ = d.freeze();
    got[3] = d.run_window(1, 10).err() == Some(PV_FROZEN);
    got
}

/// pty vim 万轮判据。
pub fn run_unx11085_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11085");

    let mut b = match VimReplay::new(PV_ROWS, PV_COLS) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let run = b.run_all();

    // J1 两半：万轮回放零漂，逐帧对照账对平。
    let j1a = b.mismatches == 0 && b.frames == 10_000;
    let j1b = b.all_frames_matched() && b.live.digest() == b.mirror.digest();
    s.add("J1-万轮回放零漂", j1a, "10000 帧 live/mirror 无一次失配");
    s.add("J1-逐帧对照对平", j1b, "十窗 matched 之和等于 10000 帧");

    // J2 窗口链连续且每窗千帧。
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < b.windows.len() {
        sum = sum.wrapping_add(b.windows[i].frames);
        i += 1;
    }
    let j2 = b.chain_ok() && sum == 10_000 && b.windows.len() == 10;
    s.add("J2-窗口链守恒", j2, "10 窗首尾指纹相接且帧数和为万");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-尺寸注入", inj[0], "零尺寸屏幕必拒");
    s.add("J3-重号注入", inj[1], "同窗二次回放必拒");
    s.add("J3-失配注入", inj[2], "镜像 digest 偏差必抓");
    s.add("J3-冻结注入", inj[3], "冻结后回放必拒");

    // J4 冻结：根稳定且 draw/run_window 双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.draw(0, 0, b'Z').err() == Some(PV_FROZEN)
        && f.run_window(0, 1).err() == Some(PV_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后绘制与回放均拒");

    // J5 指纹敏感：单窗末链变化即改根。
    let mut c = b.clone();
    c.windows[6].last_chain ^= 0x55;
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单窗末链变化即改变回放根");

    // J6 空账/越界/链漂移。
    let mut e = match VimReplay::new(4, 8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let empty = e.compare_frame();
    let range = e.draw(4, 0, b'a');
    let _ = e.run_window(0, 10);
    e.windows[0].first_chain ^= 1;
    let j6 = empty.err() == Some(PV_EMPTY) && range.err() == Some(PV_RANGE)
        && !e.chain_ok();
    s.add("J6-空账链漂", j6, "空帧、越界坐标、断链均被抓住");

    // J7 窗口唯一有序：epoch 0..9 严格递增。
    let mut ordered = true;
    let mut k = 0usize;
    while k < b.windows.len() {
        if b.windows[k].epoch as usize != k {
            ordered = false;
        }
        k += 1;
    }
    s.add("J7-窗口有序", ordered, "10 个录制窗口编号 0..9");

    // J8 负语义：九窗不得判万轮全绿。
    let mut partial = match VimReplay::new(4, 8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut pe = 0u16;
    while pe < 9 {
        let _ = partial.run_window(pe, 1_000);
        pe = pe.wrapping_add(1);
    }
    let j8 = !partial.all_frames_matched() && partial.frames == 9_000
        && partial.audit().err() == Some(PV_DRIFT);
    s.add("J8-九窗非绿", j8, "九千帧不得冒充万轮零漂");

    // J9 动态帧账。
    s.add_dyn("J9-帧账动态", j1a,
        &format!("frames={} mismatch={} chain={:x}", b.frames, b.mismatches, b.chain));

    // J10 屏幕原子性：同一字符两屏同步。
    let mut probe = match VimReplay::new(4, 8) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = probe.draw(1, 1, b'q');
    let j10 = probe.live.get(1, 1).ok() == Some(b'q')
        && probe.mirror.get(1, 1).ok() == Some(b'q')
        && probe.live.digest() == probe.mirror.digest();
    s.add("J10-双屏同步", j10, "单字符写入两屏一致");

    // J11 总审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("ptyvim|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11085_flow() {
        let mut b = match VimReplay::new(6, 12) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(b.compare_frame().err() == Some(PV_EMPTY));
        assert!(b.draw(0, 0, b'a').is_ok());
        assert!(b.draw(6, 0, b'a').err() == Some(PV_RANGE));
        assert!(b.run_window(0, 100).is_ok());
        assert!(b.run_window(0, 100).err() == Some(PV_DUP));
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.run_window(1, 100).err() == Some(PV_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11085_checks_green() {
        assert!(run_unx11085_checks().all_passed(), "UNX-F11085 判据未全绿");
    }
}
