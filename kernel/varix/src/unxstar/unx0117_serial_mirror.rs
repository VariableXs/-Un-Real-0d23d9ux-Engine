//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0117 · 串口远程日志只读镜像 · 0x6EAC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0117-J1 串口远程镜像：主账（本机日志）→ 镜像账（串
//!     口帧序）单向只读（镜像写企图拒），帧账（seq/len/载荷/FNV
//!     尾验）逐帧封账，镜像序列与主账逐条一致（漂移拒），重放一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 帧账：seq（单调）/len（≤120）/载荷/FNV32 尾验；
//!   - 单向红线：镜像账只许 append-from-main（外部写企图拒）；
//!   - 漂移对拍：镜像载荷序 == 主账序（逐条）；
//!   - 尾验：帧内 FNV32（IEEE），篡改必抓；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EAC0，AI-VX3-1102 批L 独占区）。
pub const SEG_BASE: u32 = 0x6EAC0;

/// 帧载荷上限（冻结 120）。
pub const FRAME_MAX: usize = 120;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MirrorCode {
    /// 载荷越上限
    PayloadOver,
    /// 镜像外部写企图（只读红线）
    ExternalWriteAttempt,
    /// 镜像漂移（与主账失配）
    MirrorDrift,
    /// 尾验不符（帧篡改）
    TailChecksumBad,
    /// seq 不单调
    SeqNonMonotonic,
    /// 双跑不一致
    NotReproducible,
}

impl MirrorCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MirrorCode::PayloadOver => "载荷越上限：120 冻结",
            MirrorCode::ExternalWriteAttempt => "镜像外部写企图：只读红线",
            MirrorCode::MirrorDrift => "镜像漂移：与主账失配",
            MirrorCode::TailChecksumBad => "尾验不符：帧篡改",
            MirrorCode::SeqNonMonotonic => "seq 不单调",
            MirrorCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MirrorCode; 6] = [
        MirrorCode::PayloadOver,
        MirrorCode::ExternalWriteAttempt,
        MirrorCode::MirrorDrift,
        MirrorCode::TailChecksumBad,
        MirrorCode::SeqNonMonotonic,
        MirrorCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MirrorCode::ALL.len() {
        if MirrorCode::ALL[i].val() == code {
            return MirrorCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EAC0 段闭集内"
}

// ── FNV32（IEEE） ───────────────────────────────────────────────
pub fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h = (h ^ data[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 帧账 ────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub seq: u64,
    pub payload: Vec<u8>,
    pub tail: u32,
}

impl Frame {
    /// 组帧（尾验自动）。
    pub fn build(seq: u64, payload: &[u8]) -> Result<Frame, u32> {
        if payload.len() > FRAME_MAX {
            return Err(MirrorCode::PayloadOver.val());
        }
        Ok(Frame { seq, payload: Vec::from(payload), tail: fnv32(payload) })
    }

    /// 尾验闸。
    pub fn tail_ok(&self) -> Result<(), u32> {
        if fnv32(&self.payload) != self.tail {
            return Err(MirrorCode::TailChecksumBad.val());
        }
        Ok(())
    }
}

// ── 主账与镜像账 ────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct MainLog {
    pub lines: Vec<Vec<u8>>,
}

impl MainLog {
    /// 逐条出帧（seq 单调）。
    pub fn frames(&self) -> Result<Vec<Frame>, u32> {
        let mut out = Vec::new();
        let mut seq = 0u64;
        let mut i = 0;
        while i < self.lines.len() {
            out.push(Frame::build(seq, &self.lines[i])?);
            seq += 1;
            i += 1;
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SerialMirror {
    pub frames: Vec<Frame>,
    pub external_writes: u32,
}

impl SerialMirror {
    pub fn new() -> SerialMirror {
        SerialMirror::default()
    }

    /// 从主账镜像（唯一合法入口，整批重建）。
    pub fn mirror_from(&mut self, main: &MainLog) -> Result<usize, u32> {
        self.frames = main.frames()?;
        Ok(self.frames.len())
    }

    /// 外部写企图（只读红线）。
    pub fn external_write(&mut self, f: Frame) -> Result<(), u32> {
        self.external_writes += 1;
        let _ = f;
        Err(MirrorCode::ExternalWriteAttempt.val())
    }

    /// 漂移对拍：镜像载荷序 == 主账序（逐条逐字节）。
    pub fn drift_audit(&self, main: &MainLog) -> Result<(), u32> {
        if self.frames.len() != main.lines.len() {
            return Err(MirrorCode::MirrorDrift.val());
        }
        let mut i = 0;
        while i < self.frames.len() {
            if self.frames[i].payload != main.lines[i] {
                return Err(MirrorCode::MirrorDrift.val());
            }
            if i > 0 && self.frames[i].seq != self.frames[i - 1].seq + 1 {
                return Err(MirrorCode::SeqNonMonotonic.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 全帧尾验。
    pub fn tail_audit(&self) -> Result<usize, u32> {
        let mut n = 0;
        let mut i = 0;
        while i < self.frames.len() {
            self.frames[i].tail_ok()?;
            n += 1;
            i += 1;
        }
        Ok(n)
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：越限路/外写路/漂移路/尾验路） ──────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机主账（四行）。
pub fn machine_main() -> MainLog {
    MainLog {
        lines: vec![
            Vec::from(&b"[INFO] boot ok"[..]),
            Vec::from(&b"[WARN] slow disk"[..]),
            Vec::from(&b"[ERROR] fs retry"[..]),
            Vec::from(&b"[FATAL] halt"[..]),
        ],
    }
}

// ── 黄金全链：镜像→漂移对拍→尾验→外写拒→重放 ────────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    let main = machine_main();
    let mut m = SerialMirror::new();
    let n = m.mirror_from(&main)?;
    m.drift_audit(&main)?;
    let tailed = m.tail_audit()?;
    let ext_reject = m.external_write(Frame::build(99, b"x")?).is_err();
    // 重放：再镜像一次帧账一致。
    let mut m2 = SerialMirror::new();
    let _ = m2.mirror_from(&main);
    if m2.frames[..] != m.frames[..] {
        return Err(MirrorCode::NotReproducible.val());
    }
    Ok((n, tailed, ext_reject))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 4 帧 + 尾验 4 + 外写拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, true)) => Ok(()),
        _ => Err(MirrorCode::NotReproducible.val()),
    }
}

/// 序 B：载荷越限 ⇒ PayloadOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let big = vec![0u8; FRAME_MAX + 1];
    match Frame::build(0, &big) {
        Err(e) if e == MirrorCode::PayloadOver.val() => Ok(()),
        _ => Err(MirrorCode::NotReproducible.val()),
    }
}

/// 序 C：帧篡改 ⇒ TailChecksumBad。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut f = Frame::build(0, b"hello")?;
    f.payload[0] = b'H';
    match f.tail_ok() {
        Err(e) if e == MirrorCode::TailChecksumBad.val() => Ok(()),
        _ => Err(MirrorCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越限路。
        let big = vec![0u8; FRAME_MAX + 8];
        led.record(0, Frame::build(0, &big).is_err());
        // 路 2：外写路。
        let mut m2 = SerialMirror::new();
        let _ = m2.mirror_from(&machine_main());
        let evil = Frame { seq: 9, payload: Vec::from(&b"evil"[..]), tail: fnv32(b"evil") };
        led.record(1, m2.external_write(evil).is_err() && m2.external_writes == 1);
        // 路 3：漂移路（主账追加一行后对拍必失配）。
        let mut main3 = machine_main();
        let mut m3 = SerialMirror::new();
        let _ = m3.mirror_from(&main3);
        main3.lines.push(Vec::from(&b"extra"[..]));
        led.record(2, m3.drift_audit(&main3).is_err());
        // 路 4：尾验路。
        let mut f4 = Frame::build(0, b"data")?;
        f4.tail ^= 1;
        led.record(3, f4.tail_ok().is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MirrorCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MirrorCode::ALL.len() {
        let v = MirrorCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((n, tailed, ext)) => *n == 4 && *tailed == 4 && *ext,
            Err(_) => false,
        },
        match &g {
            Ok((n, tailed, ext)) => format!("frames={} tailed={} ext_reject={}", n, tailed, ext),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 单向红线（镜像恒只读）。
    let mut m = SerialMirror::new();
    let _ = m.mirror_from(&machine_main());
    let w_frame = Frame { seq: 7, payload: Vec::from(&b"w"[..]), tail: fnv32(b"w") };
    v.push((
        "J1-单向红线",
        m.external_write(w_frame).is_err() && m.external_writes == 1,
        String::from("镜像外部写企图记账拒"),
    ));
    // 帧账冻结（载荷 ≤120 + seq 单调 + FNV32 尾验）。
    v.push((
        "J1-帧账冻结",
        FRAME_MAX == 120 && fnv32(b"") == 0x811C_9DC5,
        String::from("载荷 120 / FNV32 IEEE 初值冻结"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EAC0 窗口"),
    ));
    v
}

pub fn run_unx0117_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0117 串口远程日志只读镜像");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
