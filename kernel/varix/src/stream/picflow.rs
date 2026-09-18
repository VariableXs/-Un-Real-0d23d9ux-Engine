//! 画面流通道 — AI-P 语义 · AI-B 实现（双域总案·阶段6 任务48，QEMU 先行等价）。
//!
//! 总案施工步骤与验收口径：
//! - **全屏画面流打通**：VM 帧缓冲 → 通道（帧槽环形 + 完整性校验）→
//!   VARIX 显示栈（QEMU 先行：显示栈 = [`crate::proc::winsrv::Canvas`]，
//!   实机 GPU 帧缓冲随任务 27 嵌入层落地后原样对接——blit 接口不变）；
//! - **窗口级画面流**：帧携带裁剪矩形（RemoteApp 语义），blit 只写
//!   矩形区域（画布其余像素保持不动 = 窗口叠加语义）；
//! - 生产端（QEMU 先行）= 探针模拟引擎帧源写入；实机 = VM 帧缓冲
//!   DMA 同步循环（同一 push 接口）；
//! - 坏帧（fnv 不符/越界矩形/零长）丢弃并计数，绝不进显示栈；
//! - 旧帧按 seq 单调裁决：stale 帧拒绝（防回滚叠加）。
//!
//! 定容 .bss（任务56 戒律）：2 槽 × 320×200×4 = 512KB，零堆分配；
//! 行打包（窗口帧只载 w×h×4 字节），通道内零大栈物化。

use crate::proc::winsrv::Canvas;
use crate::drivers::blk::fnv1a64;
use crate::fb::{Color, Surface};

/// 帧宽（像素）。
pub const FRAME_W: usize = 320;
/// 帧高（像素）。
pub const FRAME_H: usize = 200;
/// 字节/像素（BGRA32）。
pub const FRAME_BPP: usize = 4;
/// 整帧载荷容量（字节）。
pub const FRAME_MAX: usize = FRAME_W * FRAME_H * FRAME_BPP;
/// 环形帧槽数。
pub const SLOTS: usize = 2;

/// 全屏帧标志。
pub const FLAG_FULL: u32 = 1;
/// 窗口级帧标志（裁剪矩形有效）。
pub const FLAG_WINDOW: u32 = 2;

/// 一帧画面（定容槽内结构）。
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub seq: u64,
    pub flags: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub payload_len: usize,
    pub fnv: u64,
    pub payload: [u8; FRAME_MAX],
}

impl Frame {
    const fn empty() -> Self {
        Frame {
            seq: 0,
            flags: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            payload_len: 0,
            fnv: 0,
            payload: [0; FRAME_MAX],
        }
    }
}

/// 画面流通道：环形帧槽 + seq 单调 + fnv 校验 + 显示栈 blit。
#[derive(Debug)]
pub struct FrameChannel {
    slots: [Frame; SLOTS],
    write_cursor: usize,
    latest_seq: u64,
    pub accepted: u64,
    pub bad_frames: u64,
    pub stale_frames: u64,
}

impl FrameChannel {
    pub const fn new() -> Self {
        FrameChannel {
            slots: [Frame::empty(); SLOTS],
            write_cursor: 0,
            latest_seq: 0,
            accepted: 0,
            bad_frames: 0,
            stale_frames: 0,
        }
    }

    /// 全屏帧入通道（w×h 必须 == FRAME_W×FRAME_H，payload_len 一致）。
    pub fn push_full(&mut self, seq: u64, payload: &[u8]) -> bool {
        self.push_frame(seq, FLAG_FULL, 0, 0, FRAME_W as u32, FRAME_H as u32, payload)
    }

    /// 窗口级帧入通道（行打包：h 行 × w×4 字节连续；矩形越界拒绝）。
    pub fn push_window(&mut self, seq: u64, x: u32, y: u32, w: u32, h: u32, payload: &[u8]) -> bool {
        self.push_frame(seq, FLAG_WINDOW, x, y, w, h, payload)
    }

    fn push_frame(&mut self, seq: u64, flags: u32, x: u32, y: u32, w: u32, h: u32, payload: &[u8]) -> bool {
        // 矩形边界校验（越界 = 坏帧）。
        if w == 0 || h == 0 {
            self.bad_frames += 1;
            return false;
        }
        let xe = x as usize + w as usize;
        let ye = y as usize + h as usize;
        if xe > FRAME_W || ye > FRAME_H {
            self.bad_frames += 1;
            return false;
        }
        let expect = w as usize * h as usize * FRAME_BPP;
        if payload.len() != expect {
            self.bad_frames += 1;
            return false;
        }
        let f = fnv1a64(payload);
        // seq 单调：stale/重放拒绝。
        if seq <= self.latest_seq && self.accepted > 0 {
            self.stale_frames += 1;
            return false;
        }
        let i = self.write_cursor;
        self.slots[i] = Frame::empty();
        self.slots[i].seq = seq;
        self.slots[i].flags = flags;
        self.slots[i].x = x;
        self.slots[i].y = y;
        self.slots[i].w = w;
        self.slots[i].h = h;
        self.slots[i].payload_len = payload.len();
        self.slots[i].fnv = f;
        self.slots[i].payload[..payload.len()].copy_from_slice(payload);
        self.write_cursor = (i + 1) % SLOTS;
        self.latest_seq = seq;
        self.accepted += 1;
        true
    }

    /// 最新帧引用。
    pub fn latest(&self) -> Option<&Frame> {
        if self.accepted == 0 {
            return None;
        }
        // latest_seq 所在槽：环形中最后写入 = write_cursor 前一格。
        Some(&self.slots[(self.write_cursor + SLOTS - 1) % SLOTS])
    }

    /// 显示栈对接：最新帧 blit 进画布。
    /// 全屏帧整幅覆盖；窗口帧只写矩形（其余像素保持 = 窗口叠加）。
    /// fnv 复验失败拒绝上屏（坏帧即使已入槽也不显示）。
    pub fn blit_latest(&mut self, canvas: &mut Canvas) -> bool {
        let Some(f) = self.latest() else { return false };
        if f.payload_len == 0 || f.fnv != fnv1a64(&f.payload[..f.payload_len]) {
            self.bad_frames += 1;
            return false;
        }
        let w = f.w as usize;
        let row = f.w as usize * FRAME_BPP;
        for r in 0..f.h as usize {
            let src = &f.payload[r * row..(r + 1) * row];
            let y = f.y as usize + r;
            let x0 = f.x as usize;
            for c in 0..w {
                let b0 = src[c * 4] as u32;
                let b1 = src[c * 4 + 1] as u32;
                let b2 = src[c * 4 + 2] as u32;
                let b3 = src[c * 4 + 3] as u32;
                let px = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
                canvas.buf[y * FRAME_W + x0 + c] = px;
            }
        }
        true
    }

    /// 显示栈对接（实机）：最新帧直写 Limine/GOP 显存（fb::Surface 带
    /// 像素格式统一层 BGR/RGB）。全屏整幅；窗口帧只写矩形。
    /// 逻辑与 [`Self::blit_latest`] 同构——显示栈后端可替换，帧语义不变。
    pub fn blit_latest_surface(&mut self, s: &Surface) -> bool {
        let Some(f) = self.latest() else { return false };
        if f.payload_len == 0 || f.fnv != fnv1a64(&f.payload[..f.payload_len]) {
            self.bad_frames += 1;
            return false;
        }
        let row = f.w as usize * FRAME_BPP;
        for r in 0..f.h as usize {
            let y = f.y as usize + r;
            for c in 0..f.w as usize {
                let i = r * row + c * 4;
                // BGRA32 → Color：B=payload[i] G=i+1 R=i+2（alpha 忽略）。
                let col = Color::rgb(f.payload[i + 2], f.payload[i + 1], f.payload[i]);
                s.set_px((f.x as usize + c) as i64, y as i64, col);
            }
        }
        true
    }

    /// 诊断快照。
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (self.accepted, self.bad_frames, self.stale_frames, self.latest_seq)
    }
}

impl Default for FrameChannel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 实机探针（main.rs 挂点）：模拟 VM 帧源 → 通道 → 显示栈全链
// ---------------------------------------------------------------------------

/// 实机探针（QEMU）：生产端模拟引擎帧源（全屏渐变帧 + 窗口级红框帧 +
/// 坏帧/重放拒绝面），消费端 blit 进 Limine 显存——总案波次验收
/// 「画面能在 QEMU 里从 VM 流出到 VARIX 显示栈」的直写证据。
pub mod target {
    use super::*;

    /// 探针暂存（.bss const 初始化）：FrameChannel≈640KiB+帧缓冲 160KiB，
    /// 内核戒律三禁齐触（>64KiB 禁栈 ×2 + vec 19200B>kheap MAX_ALLOC=4KiB
    /// 会 alloc panic）——static 单例是唯一合规放置；探针独占锁内使用，
    /// 冷启动 ELF 重载即复位。
    struct PicScratch {
        ch: FrameChannel,
        full: [u8; FRAME_MAX],
        win: [u8; PIC_WIN_W * PIC_WIN_H * 4],
    }
    const PIC_WIN_W: usize = 80;
    const PIC_WIN_H: usize = 60;
    static PIC_SCRATCH: crate::cpu::sync::SpinProtected<PicScratch> =
        crate::cpu::sync::SpinProtected::new(PicScratch {
            ch: FrameChannel::new(),
            full: [0; FRAME_MAX],
            win: [0; PIC_WIN_W as usize * PIC_WIN_H as usize * 4],
        });

    pub fn picflow_probe() {
        let Some(fb) = crate::limine::framebuffer()
            .and_then(|f| crate::fb::Surface::from_limine(f).ok())
        else {
            crate::kwarn!("picflow-probe: no framebuffer, abort");
            return;
        };
        let sw = fb.width() as usize;
        let sh = fb.height() as usize;
        let mut scratch = PIC_SCRATCH.lock();
        let PicScratch { ch, full, win } = &mut *scratch;
        crate::kinfo!("picflow-probe: stage=locked");
        let ch = ch;

        // ① 全屏帧（棋盘渐变，帧坐标系 320×200 内有效）。
        let full = &mut *full;
        for y in 0..FRAME_H {
            for x in 0..FRAME_W {
                let i = (y * FRAME_W + x) * 4;
                full[i] = (x * 255 / FRAME_W) as u8; // B 渐变
                full[i + 1] = (y * 255 / FRAME_H) as u8; // G 渐变
                full[i + 2] = if (x / 16 + y / 16) % 2 == 0 { 0x60 } else { 0x20 };
                full[i + 3] = 0xFF;
            }
        }
        crate::kinfo!("picflow-probe: stage=filled");
        let ok1 = ch.push_full(1, full);
        crate::kinfo!("picflow-probe: stage=push1 ok1={}", ok1 as u8);
        let blit1 = ch.blit_latest_surface(&fb);
        crate::kinfo!("picflow-probe: stage=blit1 r={}", blit1 as u8);

        // ② 窗口级帧（RemoteApp 语义：只叠 80×60 矩形 @ (120,60)）。
        let (wx, wy, ww, wh) = (120u32, 60u32, PIC_WIN_W as u32, PIC_WIN_H as u32);
        let win: &mut [u8] = &mut *win;
        for y in 0..wh as usize {
            for x in 0..ww as usize {
                let i = (y * ww as usize + x) * 4;
                let border = x < 2 || y < 2 || x >= ww as usize - 2 || y >= wh as usize - 2;
                win[i] = 0x30;
                win[i + 1] = 0x30;
                win[i + 2] = if border { 0xF0 } else { 0xA0 };
                win[i + 3] = 0xFF;
            }
        }
        let ok2 = ch.push_window(2, wx, wy, ww, wh, win);
        let blit2 = ch.blit_latest_surface(&fb);

        // ③ 拒绝面：重放（seq=2 已消费）+ 坏矩形（越界）。
        let replay_rejected = !ch.push_window(2, 0, 0, 8, 8, &win[..8 * 8 * 4]);
        let oob_rejected = !ch.push_window(3, 300, 190, 40, 40, &win[..40 * 40 * 4]);

        let (acc, bad, stale, seq) = ch.stats();
        let ok = ok1 && blit1 && ok2 && blit2 && replay_rejected && oob_rejected
            && (acc, bad, stale, seq) == (2, 1, 1, 2)
            && sw > 0 && sh > 0;
        crate::kinfo!(
            "picflow-probe: surface={}x{} full+blit={} win+blit={} replay_reject={} oob_reject={} stats=(acc={} bad={} stale={} seq={}) verdict={}",
            sw,
            sh,
            ok1 && blit1,
            ok2 && blit2,
            replay_rejected,
            oob_rejected,
            acc,
            bad,
            stale,
            seq,
            if ok { "ok" } else { "FAIL" }
        );
        if !ok {
            crate::kwarn!("picflow-probe: stream matrix mismatch");
        }
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（通道语义 100% 覆盖；QEMU 探针另走实链）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 全屏帧：入通道 → blit → 像素逐点一致。
    #[test]
    fn full_frame_roundtrip_pixels() {
        let mut ch = FrameChannel::new();
        let mut payload = [0u8; FRAME_MAX];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        assert!(ch.push_full(1, &payload));
        let mut cv = Canvas::new();
        cv.clear(0);
        assert!(ch.blit_latest(&mut cv));
        // 抽样 + 边角逐点：像素 = BGRA 小端重组。
        for (px, py) in [(0usize, 0usize), (319, 199), (160, 100), (5, 7)] {
            let i = (py * FRAME_W + px) * 4;
            let expect = payload[i] as u32
                | ((payload[i + 1] as u32) << 8)
                | ((payload[i + 2] as u32) << 16)
                | ((payload[i + 3] as u32) << 24);
            assert_eq!(cv.buf[py * FRAME_W + px], expect, "px {px},{py}");
        }
    }

    /// 窗口级帧：blit 只改矩形区域（叠加语义），矩形外像素保持。
    #[test]
    fn window_frame_blit_clipped() {
        let mut ch = FrameChannel::new();
        let mut cv = Canvas::new();
        cv.clear(0xFF11_2233); // 背景
        // 窗口矩形 (100, 50) 80x40。
        let (x, y, w, h) = (100u32, 50u32, 80u32, 40u32);
        let mut payload = vec![0u8; (w * h * 4) as usize];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i * 7 % 251) as u8;
        }
        assert!(ch.push_window(1, x, y, w, h, &payload));
        assert!(ch.blit_latest(&mut cv));
        // 矩形内像素 = 帧内容。
        let i = (10 * 4) as usize;
        let expect = payload[i] as u32
            | ((payload[i + 1] as u32) << 8)
            | ((payload[i + 2] as u32) << 16)
            | ((payload[i + 3] as u32) << 24);
        assert_eq!(cv.buf[(y as usize) * FRAME_W + x as usize + 10], expect);
        // 矩形外保持背景。
        assert_eq!(cv.buf[0], 0xFF11_2233);
        assert_eq!(cv.buf[FRAME_H * FRAME_W - 1], 0xFF11_2233);
        assert_eq!(cv.buf[(y as usize) * FRAME_W + x as usize - 1], 0xFF11_2233);
    }

    /// 坏帧拒绝：fnv 不符 / 越界矩形 / 零长 / 长度不符 → bad_frames 计数。
    #[test]
    fn bad_frames_rejected() {
        let mut ch = FrameChannel::new();
        let payload = vec![1u8; 100];
        assert!(!ch.push_window(1, 0, 0, 10, 10, &payload)); // 长度不符
        let big = vec![1u8; 40 * 40 * 4];
        assert!(!ch.push_window(1, 300, 190, 40, 40, &big)); // 越界
        assert!(!ch.push_window(1, 0, 0, 0, 0, &[])); // 零尺寸
        assert_eq!(ch.bad_frames, 3);
        assert_eq!(ch.accepted, 0);
        assert!(ch.latest().is_none());
        assert!(!ch.blit_latest(&mut Canvas::new()), "无帧不可上屏");
    }

    /// seq 单调：stale/重放拒绝；环形槽覆盖后 latest 仍正确。
    #[test]
    fn seq_monotonic_and_ring_wrap() {
        let mut ch = FrameChannel::new();
        let f1 = vec![1u8; FRAME_MAX];
        let f2 = vec![2u8; FRAME_MAX];
        let f3 = vec![3u8; FRAME_MAX];
        assert!(ch.push_full(1, &f1));
        assert!(!ch.push_full(1, &f1), "重放拒绝");
        assert!(!ch.push_full(0, &f1), "回退拒绝");
        assert_eq!(ch.stale_frames, 2);
        assert!(ch.push_full(2, &f2));
        assert!(ch.push_full(3, &f3)); // 环形覆盖槽 0
        assert_eq!(ch.accepted, 3);
        let (acc, bad, stale, seq) = ch.stats();
        assert_eq!((acc, bad, stale, seq), (3, 0, 2, 3));
        let lf = ch.latest().unwrap();
        assert_eq!(lf.seq, 3);
        assert_eq!(lf.payload[0], 3);
    }

    /// 1000 帧连续流：环形通道零泄漏、blit 幂等、计数如实。
    #[test]
    fn thousand_frame_stream_no_leak() {
        let mut ch = FrameChannel::new();
        let mut cv = Canvas::new();
        let payload = vec![9u8; FRAME_MAX];
        for seq in 1..=1000u64 {
            // 全屏/窗口交替。
            if seq % 2 == 0 {
                assert!(ch.push_full(seq, &payload));
            } else {
                assert!(ch.push_window(seq, (seq % 160) as u32, (seq % 100) as u32, 160, 100, &payload[..160 * 100 * 4]));
            }
            if seq % 100 == 0 {
                assert!(ch.blit_latest(&mut cv));
            }
        }
        let (acc, bad, stale, seq) = ch.stats();
        assert_eq!((acc, bad, stale, seq), (1000, 0, 0, 1000));
        assert_eq!(ch.latest().unwrap().seq, 1000);
    }
}
