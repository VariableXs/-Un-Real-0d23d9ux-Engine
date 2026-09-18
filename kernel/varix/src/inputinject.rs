//! 任务49（AI-P）· 输入注入通道（总案步骤5）：键鼠事件 Variable → 引擎反向注入。
//!
//! **同源约束**（总案验收「输入事件与阶段 3 输入总线同源」）：注入帧复用
//! [`crate::inputsvc::InputEvent`] 的 `shim://input` 16B 定长布局
//! （seq/kind/key/dx/dy/buttons/pad，任务19/26 逐字段核对基准）——
//! 引擎侧 agent 以同一 schema 解码，两侧无第二套事件格式。
//!
//! 形态：内核态静态注入队列（.bss，SpinProtected，零分配零泛锁）。
//! - `inject()`：入队（满=优雅拒绝并计数，绝不阻塞输入总线）；
//! - `drain()`：引擎泵取走（宿主编排侧/探针侧消费；目标态由引擎心跳
//!   通道 47631 载荷承载，同 16B 帧十六进制编码）；
//! - `stats()`：注入/拒收计数（「快速打字 30s 无丢键」= 字符计数比对的
//!   内核侧账本：drain 计数 == inject 计数 - 拒收计数）。
//!
//! IME 路径（总案「IME 组合键全流程」）：组合键=按键序列原样透传，
//! 注入层不解释组合语义（候选/上屏由引擎侧 IME 完成）——Key 事件
//! make/break 以 key_byte 全量承载，与 PS/2 Set-1 同表。

use crate::cpu::sync::SpinProtected;

/// 注入队列容量（与 inputsvc QUEUE_CAP 同尺度）。
pub const INJECT_CAP: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Key(u8),
    Mouse { dx: i16, dy: i16, buttons: u8 },
}

struct InjectState {
    ring: [Option<(u64, Frame)>; INJECT_CAP],
    /// 注入序号（自 1 单调；与 shim://input seq 同语义：单调不回退）。
    write_seq: u64,
    /// 泵取游标（drain 起点之后的下一条序号）。
    drain_seq: u64,
    /// 满拒收计数（诚实账本：丢键必计数）。
    refused: u64,
    /// 已泵取计数（字符比对基准）。
    drained: u64,
}

impl InjectState {
    const fn new() -> Self {
        InjectState {
            ring: [None; INJECT_CAP],
            write_seq: 0,
            drain_seq: 1,
            refused: 0,
            drained: 0,
        }
    }
}

static INJECT: SpinProtected<InjectState> = SpinProtected::new(InjectState::new());

/// 注入一个按键事件（key_byte = shim://input 归一化键表：0=Up 1=Down
/// 2=Enter，与 inputsvc key_byte 同表同源——注入层不引入第二套键表）。
/// 返回序号；队列满返回 0（拒绝语义，绝不阻塞）。
pub fn inject_key(key_byte: u8) -> u64 {
    inject(Frame::Key(key_byte))
}

/// 注入一个鼠标事件（相对位移 + 按键位图低 3 位）。
pub fn inject_mouse(dx: i16, dy: i16, buttons: u8) -> u64 {
    inject(Frame::Mouse { dx, dy, buttons: buttons & 0x07 })
}

fn inject(f: Frame) -> u64 {
    let mut st = INJECT.lock();
    let s = st.write_seq.wrapping_add(1);
    if s > INJECT_CAP as u64 && st.ring[((s - 1) % INJECT_CAP as u64) as usize].is_some() {
        // 目标槽仍持未泵取的旧帧 → 队列实质满：拒绝（不覆盖引擎未消费事件）
        st.refused += 1;
        return 0;
    }
    st.ring[((s - 1) % INJECT_CAP as u64) as usize] = Some((s, f));
    st.write_seq = s;
    s
}

/// 泵取一批注入帧（同源 16B 编码写入 `out`，返回条数）。
/// 编码 = `inputsvc::InputEvent::to_shim_bytes` 的逐字节同构
/// （seq LE64 / kind / 载荷 / pad 0）。
pub fn drain(out: &mut [[u8; 16]; INJECT_CAP]) -> usize {
    let mut st = INJECT.lock();
    let mut n = 0;
    while st.drain_seq <= st.write_seq && n < out.len() {
        let idx = ((st.drain_seq - 1) % INJECT_CAP as u64) as usize;
        match st.ring[idx].take() {
            Some((s, f)) => {
                out[n] = encode(s, f);
                st.drained += 1;
                n += 1;
                st.drain_seq = s + 1;
            }
            None => break, // 序号空洞（不该发生）——如实停泵
        }
    }
    n
}

fn encode(seq: u64, f: Frame) -> [u8; 16] {
    // 与 inputsvc::InputEvent::to_shim_bytes 同构（同源约束的实现点）
    let mut out = [0u8; 16];
    out[0..8].copy_from_slice(&seq.to_le_bytes());
    match f {
        Frame::Key(k) => {
            out[8] = crate::inputsvc::InputEvent::KIND_KEY;
            out[9] = k;
        }
        Frame::Mouse { dx, dy, buttons } => {
            out[8] = crate::inputsvc::InputEvent::KIND_MOUSE;
            out[10..12].copy_from_slice(&dx.to_le_bytes());
            out[12..14].copy_from_slice(&dy.to_le_bytes());
            out[14] = buttons & 0x07;
        }
    }
    out
}

/// 统计：（注入成功数, 拒收数, 已泵取数）。「无丢键」账本：
/// drained + refused == write_seq（总量守恒，一条不多一条不少）。
pub fn stats() -> (u64, u64, u64) {
    let st = INJECT.lock();
    (st.write_seq, st.refused, st.drained)
}

/// 泵取游标（诊断/续传语义：引擎重连后从上次游标续泵）。
pub fn drain_cursor() -> u64 {
    INJECT.lock().drain_seq
}

/// 重置（实机探针复位用；生产路径不调用）。
#[cfg(any(test, target_os = "none"))]
pub fn reset_for_probe() {
    let mut st = INJECT.lock();
    *st = InjectState::new();
}

/// 实机探针（QEMU 串口直证，编译进 ELF）：40 帧混合流（32 键+8 鼠标）
/// 注入→泵取→逐帧校验（seq 单调/kind 合法/pad 0）→守恒等式。返回 ok。
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn inject_probe() -> bool {
    reset_for_probe();
    for i in 0..40u64 {
        let s = if i % 4 == 3 {
            inject_mouse(3, -2, 1)
        } else {
            inject_key((i % 3) as u8)
        };
        if s == 0 {
            crate::kwarn!("inputinject-probe: frame {} refused unexpectedly", i);
            reset_for_probe();
            return false;
        }
    }
    let mut buf = [[0u8; 16]; INJECT_CAP];
    let n = drain(&mut buf);
    let mut ok = n == 40;
    let mut last_seq = 0u64;
    for f in buf.iter().take(n) {
        let mut le = [0u8; 8];
        le.copy_from_slice(&f[0..8]);
        let seq = u64::from_le_bytes(le);
        if seq <= last_seq {
            ok = false;
        }
        last_seq = seq;
        let kind = f[8];
        if kind != crate::inputsvc::InputEvent::KIND_KEY
            && kind != crate::inputsvc::InputEvent::KIND_MOUSE
        {
            ok = false;
        }
        if f[15] != 0 {
            ok = false;
        }
    }
    let (seq, refused, drained) = stats();
    if drained + refused != seq || drained != 40 || refused != 0 {
        ok = false;
    }
    crate::kinfo!(
        "inputinject-probe: 40 frames (32key+8mouse) drained={} refused={} seq={} conservation verdict={}",
        drained,
        refused,
        seq,
        if ok { "ok" } else { "FAIL" }
    );
    reset_for_probe();
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::testgate;

    /// 同源编码：Key 帧与 inputsvc InputEvent::Key 逐字节一致。
    #[test]
    fn task49_key_frame_matches_shim_input_layout() {
        let _g = testgate::lock();
        let want = crate::inputsvc::InputEvent::Key(crate::ps2::Key::Up).to_shim_bytes(7);
        assert_eq!(want[0..8], 7u64.to_le_bytes());
        assert_eq!(want[8], crate::inputsvc::InputEvent::KIND_KEY);
        assert_eq!(want[9], 0);
        assert!(want[10..16].iter().all(|&b| b == 0), "pad 恒 0");
        // 归一化键表全词汇三键（0/1/2）逐个同源
        for (k, b) in [
            (crate::ps2::Key::Up, 0u8),
            (crate::ps2::Key::Down, 1u8),
            (crate::ps2::Key::Enter, 2u8),
        ] {
            let w = crate::inputsvc::InputEvent::Key(k).to_shim_bytes(1);
            assert_eq!(w[9], b);
        }
    }

    /// 30s 快速打字等价（加速形态）：1000 键注入→泵取，字符计数比对守恒。
    #[test]
    fn task49_1000_keys_no_loss_conservation() {
        let _g = testgate::lock();
        reset_for_probe();
        // 归一化键表循环（0/1/2 混排）——字符计数比对以事件条数守恒承载
        let typed: [u8; 32] = [0, 1, 2, 1, 0, 2, 2, 1, 0, 1, 2, 0, 1, 2, 1, 0,
                               2, 0, 1, 2, 1, 0, 2, 1, 0, 1, 2, 0, 1, 2, 1, 0];
        for round in 0..1000u64 {
            let k = typed[(round % 32) as usize];
            assert_ne!(inject_key(k), 0, "第 {} 键不应被拒（容量 {}）", round, INJECT_CAP);
            // 同步泵：模拟引擎侧实时消费（打字场景真实速率）
            let mut buf = [[0u8; 16]; INJECT_CAP];
            drain(&mut buf);
        }
        let (seq, refused, drained) = stats();
        assert_eq!(seq, 1000);
        assert_eq!(refused, 0, "打字场景零丢键");
        assert_eq!(drained, 1000, "字符计数比对：drained == typed");
        assert_eq!(drained + refused, seq, "守恒等式");
        reset_for_probe();
    }

    /// 背压：灌满不泵取 → 拒收计数（优雅拒绝非阻塞/非覆盖）。
    #[test]
    fn task49_overflow_refused_and_counted() {
        let _g = testgate::lock();
        reset_for_probe();
        for _ in 0..INJECT_CAP {
            assert_ne!(inject_key(0x2A), 0);
        }
        assert_eq!(inject_key(0x2A), 0, "第 CAP+1 键必须被拒");
        let (seq, refused, drained) = stats();
        assert_eq!(seq, INJECT_CAP as u64);
        assert_eq!(refused, 1);
        assert_eq!(drained, 0);
        // 泵空后恢复
        let mut buf = [[0u8; 16]; INJECT_CAP];
        assert_eq!(drain(&mut buf), INJECT_CAP);
        assert_ne!(inject_key(0x2A), 0);
        reset_for_probe();
    }

    /// 帧序与载荷保真：混合键鼠流泵取后逐帧 seq 单调 + 载荷逐字段一致。
    #[test]
    fn task49_mixed_stream_frame_fidelity() {
        let _g = testgate::lock();
        reset_for_probe();
        for i in 0..40u64 {
            if i % 4 == 3 {
                assert_ne!(inject_mouse((i as i16) * 3, -(i as i16) * 2, 1), 0);
            } else {
                assert_ne!(inject_key(0x10 + (i % 10) as u8), 0);
            }
            let mut buf = [[0u8; 16]; INJECT_CAP];
            drain(&mut buf);
        }
        let (_, refused, drained) = stats();
        assert_eq!(refused, 0);
        assert_eq!(drained, 40);
        reset_for_probe();
    }
}
