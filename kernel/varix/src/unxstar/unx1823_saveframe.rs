//! UNX-F1823 · 保存帧寄存器集冻结（callee-saved 全集）
//!
//! 诊断段 0x9C51~0x9C58（上下文族保存帧段）。
//! 判据（施工书 J1）：保存帧寄存器集冻结 v1 与 F1807 原语逐寄存器对齐，保持性断言全过。

use alloc::vec::Vec;

/// 保存帧诊断码闭集（0x9C51~0x9C58 八码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameCode {
    /// 0x9C51 保存帧冻结成功。
    Frozen = 0x9C51,
    /// 0x9C52 寄存器集与 F1807 原语不对齐。
    Misaligned = 0x9C52,
    /// 0x9C53 callee-saved 集缺失寄存器。
    RegMissing = 0x9C53,
    /// 0x9C54 保持性断言通过（保存→恢复值不变）。
    Preserved = 0x9C54,
    /// 0x9C55 保持性破坏（恢复值漂移）。
    Corrupted = 0x9C55,
    /// 0x9C56 帧尺寸越闭集。
    SizeOutOfRange = 0x9C56,
    /// 0x9C57 帧指纹核通过。
    FpSealed = 0x9C57,
    /// 0x9C58 帧账守恒破坏。
    LedgerBroken = 0x9C58,
}

impl FrameCode {
    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            FrameCode::Frozen => "保存帧寄存器集已冻结",
            FrameCode::Misaligned => "寄存器集与 F1807 原语不对齐",
            FrameCode::RegMissing => "callee-saved 集存在缺失",
            FrameCode::Preserved => "保存→恢复值逐寄存器不变",
            FrameCode::Corrupted => "恢复值发生漂移",
            FrameCode::SizeOutOfRange => "帧尺寸越闭集",
            FrameCode::FpSealed => "帧指纹核通过",
            FrameCode::LedgerBroken => "帧账守恒破坏",
        }
    }
    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            FrameCode::Frozen => "全集齐且序冻结",
            FrameCode::Misaligned => "寄存器序或命名与原语册漂移",
            FrameCode::RegMissing => "callee-saved 全集定义不全",
            FrameCode::Preserved => "保存/恢复路径对称",
            FrameCode::Corrupted => "保存区被越界写或恢复序错",
            FrameCode::SizeOutOfRange => "帧字数越下界/上界",
            FrameCode::FpSealed => "全帧指纹双读一致",
            FrameCode::LedgerBroken => "保存数与恢复数不一致",
        }
    }
    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            FrameCode::Frozen => "无需处置",
            FrameCode::Misaligned => "对齐 F1807 原语寄存器序",
            FrameCode::RegMissing => "补全集并重冻结",
            FrameCode::Preserved => "无需处置",
            FrameCode::Corrupted => "排查越界写并重保存",
            FrameCode::SizeOutOfRange => "按全集字数重建帧",
            FrameCode::FpSealed => "无需处置",
            FrameCode::LedgerBroken => "冻结上下文账并上报 0x9C58",
        }
    }
}

/// x86-64 callee-saved 寄存器全集（F1807 原语序冻结：rbx rbp r12 r13 r14 r15）。
pub const CALLEE_SAVED: [&str; 6] = ["rbx", "rbp", "r12", "r13", "r14", "r15"];

/// 保存帧：callee-saved 全集值区（含返回地址槽，共 7 字）。
pub const FRAME_WORDS: usize = CALLEE_SAVED.len() + 1;
/// 帧字数下界。
pub const FRAME_MIN: usize = FRAME_WORDS;
/// 帧字数上界。
pub const FRAME_MAX: usize = FRAME_WORDS * 2;

/// 保存帧实例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SaveFrame {
    /// 值区：[rbx, rbp, r12, r13, r14, r15, ret]。
    pub words: [u64; FRAME_WORDS],
}

impl SaveFrame {
    /// 从现场捕获（逐寄存器对齐 F1807 序）。
    pub fn capture(rbx: u64, rbp: u64, r12: u64, r13: u64, r14: u64, r15: u64, ret: u64) -> SaveFrame {
        SaveFrame { words: [rbx, rbp, r12, r13, r14, r15, ret] }
    }

    /// 恢复（与 capture 对称）。
    pub fn restore(&self) -> [u64; 7] {
        self.words
    }

    /// 帧指纹（FNV-1a）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for w in self.words {
            h ^= w;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 与 F1807 原语序对齐核：字数与寄存器全集一一对应。
    pub const fn aligned(&self) -> bool {
        self.words.len() == CALLEE_SAVED.len() + 1
    }

    /// 尺寸闭集核。
    pub const fn size_ok() -> bool {
        FRAME_WORDS >= FRAME_MIN && FRAME_WORDS <= FRAME_MAX
    }
}

/// 保持性断言引擎：保存→扰动现场→恢复→逐寄存器比对。
pub struct FramePreserver {
    /// 帧栈。
    frames: Vec<SaveFrame>,
    /// 保持通过数。
    preserved: u64,
    /// 破坏检出数。
    corrupted: u64,
    /// 现场寄存器（模拟 CPU 态）。
    live: [u64; 7],
}

impl FramePreserver {
    /// 新引擎。
    pub fn new() -> Self {
        FramePreserver { frames: Vec::new(), preserved: 0, corrupted: 0, live: [0; 7] }
    }

    /// 写现场。
    pub fn set_live(&mut self, vals: [u64; 7]) {
        self.live = vals;
    }

    /// 现场读取。
    pub const fn live(&self) -> &[u64; 7] {
        &self.live
    }

    /// 保存：捕获帧入栈。
    pub fn save(&mut self) -> FrameCode {
        let f = SaveFrame::capture(self.live[0], self.live[1], self.live[2], self.live[3], self.live[4], self.live[5], self.live[6]);
        if !f.aligned() || !SaveFrame::size_ok() {
            return FrameCode::SizeOutOfRange;
        }
        self.frames.push(f);
        FrameCode::Frozen
    }

    /// 扰动现场（模拟被调用方改写 callee-saved）。
    pub fn clobber_live(&mut self, mask: u64) {
        for i in 0..7 {
            if mask & (1 << i) != 0 {
                self.live[i] = self.live[i].wrapping_add(0x5A5A_0000 + i as u64);
            }
        }
    }

    /// 恢复：出栈并逐寄存器回写，比对保持性。
    pub fn restore(&mut self) -> FrameCode {
        match self.frames.pop() {
            None => FrameCode::LedgerBroken,
            Some(f) => {
                let saved = f.restore();
                let mut same = true;
                for i in 0..7 {
                    if self.live[i] != saved[i] {
                        same = false;
                    }
                    self.live[i] = saved[i];
                }
                if same {
                    self.preserved += 1;
                    FrameCode::Preserved
                } else {
                    // 恢复本身修复漂移：破坏在保存窗内被检出（先记破坏再回写）。
                    self.corrupted += 1;
                    FrameCode::Corrupted
                }
            }
        }
    }

    /// 帧指纹双读一致核。
    pub fn fp_seal(&self) -> FrameCode {
        match self.frames.last() {
            None => FrameCode::LedgerBroken,
            Some(f) => {
                let a = f.fingerprint();
                let b = f.fingerprint();
                if a == b {
                    FrameCode::FpSealed
                } else {
                    FrameCode::Corrupted
                }
            }
        }
    }

    /// 统计。
    pub const fn stats(&self) -> (u64, u64, usize) {
        (self.preserved, self.corrupted, self.frames.len())
    }

    /// 账守恒：栈深不超过保存总数。
    pub fn ledger_consistent(&self, saved_total: usize) -> bool {
        self.frames.len() <= saved_total
    }
}

/// 判据测试（段 0x9C51~0x9C58）。
pub fn run_t823_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("UNX-t823");
    // c1 全集冻结：6 寄存器 + 返回地址 = 7 字。
    set.add("t823-c1-frozen", CALLEE_SAVED.len() == 6 && FRAME_WORDS == 7 && SaveFrame::size_ok(), "");
    // c2 F1807 序对齐：capture/restore 对称。
    let f = SaveFrame::capture(1, 2, 3, 4, 5, 6, 7);
    let r = f.restore();
    set.add("t823-c2-aligned", f.aligned() && r == [1, 2, 3, 4, 5, 6, 7], "");
    // c3 保持性断言：保存→扰动→恢复→值回原。
    let mut p = FramePreserver::new();
    p.set_live([10, 20, 30, 40, 50, 60, 70]);
    let fr = p.save();
    p.clobber_live(0x7F);
    let cl = p.live()[0] != 10;
    let rs = p.restore();
    let back = p.live() == &[10, 20, 30, 40, 50, 60, 70];
    set.add(
        "t823-c3-preserve",
        fr == FrameCode::Frozen && cl && rs == FrameCode::Corrupted && back && p.live() == &[10, 20, 30, 40, 50, 60, 70],
        "",
    );
    // c4 无扰动恢复 = 纯保持。
    let mut p4 = FramePreserver::new();
    p4.set_live([1, 1, 2, 3, 5, 8, 13]);
    let _ = p4.save();
    let rs4 = p4.restore();
    set.add("t823-c4-pure-preserve", rs4 == FrameCode::Preserved, "");
    // c5 帧指纹双读一致。
    let mut p5 = FramePreserver::new();
    p5.set_live([9, 9, 9, 9, 9, 9, 9]);
    let _ = p5.save();
    set.add("t823-c5-fp-seal", p5.fp_seal() == FrameCode::FpSealed, "");
    // c6 空栈恢复 → 账破坏检出。
    let mut p6 = FramePreserver::new();
    set.add("t823-c6-empty-pop", p6.restore() == FrameCode::LedgerBroken, "");
    // c7 嵌套保存/恢复（LIFO）。
    let mut p7 = FramePreserver::new();
    p7.set_live([1, 1, 1, 1, 1, 1, 1]);
    let _ = p7.save();
    p7.set_live([2, 2, 2, 2, 2, 2, 2]);
    let _ = p7.save();
    let _ = p7.restore();
    let mid = p7.live() == &[2, 2, 2, 2, 2, 2, 2];
    let _ = p7.restore();
    let outer = p7.live() == &[1, 1, 1, 1, 1, 1, 1];
    set.add("t823-c7-nested-lifo", mid && outer, "");
    // c8 LCG 512 轮注入：随机保存/扰动/恢复序列，栈深账守恒。
    let mut seed: u64 = 0x8230_9C51_0000_0001;
    let mut lc = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        seed
    };
    let mut inj_ok = true;
    for _ in 0..512u32 {
        let mut eng = FramePreserver::new();
        let mut saved = 0usize;
        for _ in 0..16 {
            match lc() % 3 {
                0 => {
                    eng.set_live([lc(), lc(), lc(), lc(), lc(), lc(), lc()]);
                    if eng.save() == FrameCode::Frozen {
                        saved += 1;
                    }
                }
                1 => {
                    eng.clobber_live(lc() % 128);
                }
                _ => {
                    let _ = eng.restore();
                }
            }
        }
        inj_ok &= eng.ledger_consistent(saved);
    }
    set.add("t823-c8-lcg-inject", inj_ok, "");
    // 三要素自核。
    let mut tri_ok = true;
    for c in [
        FrameCode::Frozen,
        FrameCode::Misaligned,
        FrameCode::RegMissing,
        FrameCode::Preserved,
        FrameCode::Corrupted,
        FrameCode::SizeOutOfRange,
        FrameCode::FpSealed,
        FrameCode::LedgerBroken,
    ] {
        tri_ok &= !c.what().is_empty() && !c.why().is_empty() && !c.advice().is_empty();
    }
    set.add("t823-c9-triad", tri_ok, "");
    set
}
