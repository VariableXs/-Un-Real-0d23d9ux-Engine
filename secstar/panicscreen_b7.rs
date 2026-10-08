//! F173 panic 屏幕 · 批次七深化（v7）——QR 定位协商、字库回退账、
//! 串口回显解析、panic 频次直方。零堆、no_std。

use crate::checks::CheckSet;

/// QR 逻辑尺寸（模块数——版本 1 = 21×21，恰合 25 行屏）。
pub const QR_MODULES: u32 = 21;
/// 屏宽（列）。
pub const SCREEN_COLS: u32 = 80;
/// 屏高（行）。
pub const SCREEN_ROWS: u32 = 25;
/// QR 安静区（模块——屏内紧凑布局取 2）。
pub const QR_QUIET_ZONE: u32 = 2;
/// 字库回退链深度。
pub const FONT_FALLBACK_DEPTH: usize = 3;
/// 回显行缓冲长。
pub const ECHO_LINE_LEN: usize = 32;
/// 直方桶数（按模块分桶）。
pub const HISTO_BUCKETS: usize = 15;

/// QR 定位协商：屏内找一块放得下 QR+安静区的矩形（右下角优先——
/// 文字在左上，二维码不遮错误信息）。
/// 返回 Some((x, y)) 左上角；放不下 → None（诚实拒绝不硬塞）。
pub fn qr_place() -> Option<(u32, u32)> {
    let total = QR_MODULES + QR_QUIET_ZONE * 2;
    // 文字区占左上 60×10——QR 必须完全避开。
    let text_w = 50u32;
    let text_h = 10u32;
    // 右下角策略：x = COLS - total, y = ROWS - total。
    let x = SCREEN_COLS.checked_sub(total)?;
    let y = SCREEN_ROWS.checked_sub(total)?;
    // 与文字区重叠检查（矩形相交判定）。
    let overlap = x < text_w && y < text_h;
    if overlap {
        return None;
    }
    Some((x, y))
}

/// 字库回退账：目标字形缺失时逐级回退（主字库 → 备选 → 块面兜底）。
/// 返回 Some(级别)：0=主 1=备 2=块面；链尽头 → None（不可渲染诚实上报）。
pub fn font_fallback(has_primary: bool, has_fallback: bool) -> Option<usize> {
    if has_primary {
        Some(0)
    } else if has_fallback {
        Some(1)
    } else {
        Some(2) // 块面兜底永远在——除非调用方明确连块面都没有
    }
}

/// 串口回显解析：行格式 "ACK <seq>" / "NAK <seq>"——返回 (是否确认, 序号)。
/// 坏行 → None（协议外字节不猜）。
pub fn parse_echo(line: &[u8]) -> Option<(bool, u16)> {
    if line.len() < 5 {
        return None;
    }
    let (tag, rest) = line.split_at(3);
    if rest[0] != b' ' {
        return None;
    }
    let ack = match tag {
        b"ACK" => true,
        b"NAK" => false,
        _ => return None,
    };
    let digits = &rest[1..];
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let mut seq: u16 = 0;
    for &d in digits {
        if !d.is_ascii_digit() {
            return None;
        }
        seq = seq.checked_mul(10)?.checked_add((d - b'0') as u16)?;
    }
    Some((ack, seq))
}

/// 回显重发账：NAK 的序号入重发队列（≤8），重复 NAK 同序号不重复入队。
#[derive(Clone, Copy)]
pub struct RetransmitQueue {
    seqs: [Option<u16>; 8],
    pub n: usize,
    pub dup_naks: u32,
}

impl RetransmitQueue {
    pub const fn new() -> RetransmitQueue {
        RetransmitQueue { seqs: [None; 8], n: 0, dup_naks: 0 }
    }

    pub fn on_nak(&mut self, seq: u16) -> bool {
        for i in 0..self.n {
            if self.seqs[i] == Some(seq) {
                self.dup_naks += 1;
                return false;
            }
        }
        if self.n >= 8 {
            return false;
        }
        self.seqs[self.n] = Some(seq);
        self.n += 1;
        true
    }

    pub fn pop(&mut self) -> Option<u16> {
        if self.n == 0 {
            return None;
        }
        let v = self.seqs[0];
        for i in 1..self.n {
            self.seqs[i - 1] = self.seqs[i];
        }
        self.seqs[self.n - 1] = None;
        self.n -= 1;
        v
    }

    /// ACK 收货：从队列移除（确认即清——重发账不欠账）。
    pub fn on_ack(&mut self, seq: u16) -> bool {
        for i in 0..self.n {
            if self.seqs[i] == Some(seq) {
                for j in i..self.n - 1 {
                    self.seqs[j] = self.seqs[j + 1];
                }
                self.seqs[self.n - 1] = None;
                self.n -= 1;
                return true;
            }
        }
        false
    }
}

/// panic 频次直方：按模块 id 分桶计数（桶 0..14），环形不回卷——满桶封顶。
#[derive(Clone, Copy)]
pub struct PanicHistogram {
    buckets: [u16; HISTO_BUCKETS],
    pub total: u32,
}

impl PanicHistogram {
    pub const fn new() -> PanicHistogram {
        PanicHistogram { buckets: [0; HISTO_BUCKETS], total: 0 }
    }

    pub fn record(&mut self, module_id: u8) {
        if module_id as usize >= HISTO_BUCKETS {
            return; // 越界模块不进直方（归因面守门）
        }
        if self.buckets[module_id as usize] < u16::MAX {
            self.buckets[module_id as usize] += 1;
            self.total += 1;
        }
    }

    pub fn count_of(&self, module_id: u8) -> Option<u16> {
        if module_id as usize >= HISTO_BUCKETS {
            return None;
        }
        Some(self.buckets[module_id as usize])
    }

    /// 最热模块（并列取低位——确定性）。
    pub fn hottest(&self) -> Option<u8> {
        if self.total == 0 {
            return None;
        }
        let mut best = 0usize;
        for i in 1..HISTO_BUCKETS {
            if self.buckets[i] > self.buckets[best] {
                best = i;
            }
        }
        Some(best as u8)
    }

    /// 守恒：桶和 == 总数（账平判据）。
    pub fn conserved(&self) -> bool {
        let sum: u32 = self.buckets.iter().map(|&b| b as u32).sum();
        sum == self.total
    }
}

#[inline(never)]
pub fn run_panicscreen_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F173-b7");

    // 1) QR 放置：右下角、避开文字区（矩形几何直核）。
    let (qx, qy) = qr_place().unwrap_or((0, 0));
    let total = QR_MODULES + QR_QUIET_ZONE * 2;
    cs.add(
        "qr_place_bottom_right",
        qr_place().is_some() && qx == SCREEN_COLS - total && qy == SCREEN_ROWS - total,
        "",
    );

    // 2) QR 完整在屏：x+total ≤ COLS 且 y+total ≤ ROWS（越界 = 布局违约）。
    cs.add(
        "qr_fits_screen",
        qx + total <= SCREEN_COLS && qy + total <= SCREEN_ROWS,
        "",
    );

    // 3) 字库回退链：主在用主、主缺用备、全缺块面兜底（兜底永不缺席）。
    cs.add(
        "font_fallback_chain",
        font_fallback(true, true) == Some(0)
            && font_fallback(false, true) == Some(1)
            && font_fallback(false, false) == Some(2),
        "",
    );

    // 4) 回显解析：ACK 5、NAK 12、坏标签 None、非数字 None、溢出 None。
    cs.add(
        "echo_parse",
        parse_echo(b"ACK 5") == Some((true, 5))
            && parse_echo(b"NAK 12") == Some((false, 12))
            && parse_echo(b"XXX 5").is_none()
            && parse_echo(b"ACK x").is_none()
            && parse_echo(b"ACK 65535") == Some((true, 65_535))
            && parse_echo(b"ACK 70000").is_none(),
        "",
    );

    // 5) 回显短行拒：4 字节行不够 "TAG d" 结构（协议面守门）。
    cs.add("echo_short_rejects", parse_echo(b"ACK").is_none() && parse_echo(b"").is_none(), "");

    // 6) 重发账：NAK 3/5/7 入队 FIFO 弹出、重复 NAK 5 计 dup 不重入。
    let mut q = RetransmitQueue::new();
    q.on_nak(3);
    q.on_nak(5);
    q.on_nak(7);
    let dup = q.on_nak(5);
    let p1 = q.pop();
    let p2 = q.pop();
    let p3 = q.pop();
    cs.add(
        "retransmit_fifo_dedup",
        !dup && q.dup_naks == 1 && p1 == Some(3) && p2 == Some(5) && p3 == Some(7),
        "",
    );

    // 7) ACK 清账：NAK 9 后 ACK 9 → 队列空、二次 ACK false（不欠账不虚收）。
    let mut q2 = RetransmitQueue::new();
    q2.on_nak(9);
    let got = q2.on_ack(9);
    let again = q2.on_ack(9);
    cs.add("retransmit_ack_clears", got && !again && q2.n == 0, "");

    // 8) 直方记账：模块 3×2 次、模块 5×1 次 → 计数与守恒。
    let mut h = PanicHistogram::new();
    h.record(3);
    h.record(3);
    h.record(5);
    cs.add(
        "histo_counts",
        h.count_of(3) == Some(2) && h.count_of(5) == Some(1) && h.conserved() && h.total == 3,
        "",
    );

    // 9) 最热模块：模块 3 最热（并列低位确定性——同热取小 id）。
    h.record(5);
    cs.add("histo_hottest", h.hottest() == Some(3), "");

    // 10) 直方越界模块拒：id 20 ≥ 桶数 → 不进账（归因不串）。
    h.record(20);
    cs.add("histo_oob_rejected", h.count_of(20).is_none() && h.total == 4 && h.conserved(), "");

    // 11) 直方空账：无 panic → hottest None（不编造热点）。
    cs.add("histo_empty_honest", PanicHistogram::new().hottest().is_none(), "");

    // 12) 常量自洽：QR 21+4=25 恰合 25 行、QR 区避开 50 宽文字区、桶 15。
    let (x, y) = qr_place().unwrap();
    cs.add(
        "b7_layout_premise",
        total == 25 && x >= 50 && y + total <= SCREEN_ROWS && HISTO_BUCKETS == 15,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_seq_boundaries() {
        // 序号边界：0 合法、65535 恰好合法、65536 溢出拒。
        assert_eq!(parse_echo(b"ACK 0"), Some((true, 0)));
        assert_eq!(parse_echo(b"ACK 65535"), Some((true, 65_535)));
        assert!(parse_echo(b"ACK 65536").is_none());
    }

    #[test]
    fn retransmit_full_queue_honest() {
        // 8 满容：第 9 个 NAK 诚实拒（重发账不是无底洞）。
        let mut q = RetransmitQueue::new();
        for s in 0..8u16 {
            assert!(q.on_nak(s));
        }
        assert!(!q.on_nak(100));
        assert_eq!(q.n, 8);
    }

    #[test]
    fn histogram_deterministic_tie() {
        // 并列最热：两桶同计数 → 取低位 id（同输入同输出——确定性）。
        let mut h = PanicHistogram::new();
        h.record(4);
        h.record(9);
        assert_eq!(h.hottest(), Some(4));
        h.record(9);
        assert_eq!(h.hottest(), Some(9));
    }
}
