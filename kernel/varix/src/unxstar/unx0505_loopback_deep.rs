//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0505 串口环回演练（MCR loopback） · 0x76280段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定）：
//!   环回模式收发闭环：注入 64 字节序全收 64/64（顺序保持），退出环回
//!   后线态恢复断言，环回账只追加。
//! 设计要点：
//!   1. 16550 MCR 位账：DTR=bit0 / RTS=bit1 / OUT1=bit2 / OUT2=bit3 /
//!      LOOP=bit4（MCR_LOOP_BIT 冻结），环回位（bit4）置位/复位两态
//!      状态机（在线 ↔ 环回），重复置位与未置位退出均显性拒；
//!   2. 环回直通语义：LOOP=1 期间 send 直落接收账（外部 TX 线挂起），
//!      LOOP=0 期间收发显性拒（NotInLoop）；
//!   3. 收发闭环判据：注入 64 字节序（LCG 种子写死 = SEG_BASE，全字节
//!      域含控制字符）全收 64/64 且逐字节顺序保持；
//!   4. 线态恢复断言：进入环回前快照 DTR|RTS 两位，退出环回后逐位对拍
//!      复原，漂移即 LineLost 显性红；
//!   5. 环回账只追加：进入/注入/退出逐事件落账，行号连续递增，无删改
//!      路径，账长单调不减；
//!   6. 注入三路（顺序错乱/丢字节/线态漂移）×10 轮必抓；接收序 FNV
//!      指纹封账复算一致。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76280，AI-VX7-1104 批7 O 组独占区）。
pub const SEG_BASE: u32 = 0x76280;

/// 注入字节数（64 冻结，判据口径）。
pub const INJECT_BYTES: usize = 64;
/// MCR 环回位（bit4，16550 手册口径）。
pub const MCR_LOOP_BIT: u8 = 4;
/// MCR DTR 位（bit0）。
pub const MCR_DTR_BIT: u8 = 0;
/// MCR RTS 位（bit1）。
pub const MCR_RTS_BIT: u8 = 1;
/// 线态位掩码（DTR|RTS，退出环回须复原的位账）。
pub const LINE_MASK: u8 = (1 << MCR_DTR_BIT) | (1 << MCR_RTS_BIT);
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入路数（顺序错乱/丢字节/线态漂移 三路）。
pub const INJ_ROUTES: usize = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopCode {
    /// 重复进入环回：LOOP 位已置位
    AlreadyLoop,
    /// 未入环回：线态下收发/退出越权
    NotInLoop,
    /// 顺序错乱：环回序被换序（注入必抓）
    OrderBroken,
    /// 丢字节：全收数不足 64/64（注入必抓）
    ByteLoss,
    /// 线态未复原：退出环回后 DTR/RTS 位账与快照失配
    LineLost,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl LoopCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LoopCode::AlreadyLoop => "重复进入环回：LOOP 位已置位，建议先退出再进入",
            LoopCode::NotInLoop => "未入环回收发：LOOP=0 时收发越权，建议先 enter_loopback",
            LoopCode::OrderBroken => "顺序错乱：环回序被换序，收发必须逐字节保序",
            LoopCode::ByteLoss => "丢字节：全收数不足 64/64，建议核对 RBR 就绪账",
            LoopCode::LineLost => "线态未复原：退出环回后 DTR/RTS 须与进入前快照逐位相等",
            LoopCode::NotReproducible => "双跑不一致：同种子注入序必须同结果，建议固定种子",
        }
    }
    pub const ALL: [LoopCode; 6] = [
        LoopCode::AlreadyLoop,
        LoopCode::NotInLoop,
        LoopCode::OrderBroken,
        LoopCode::ByteLoss,
        LoopCode::LineLost,
        LoopCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LoopCode::ALL.len() {
        if let Some(c) = LoopCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76280 段闭集内"
}

// ── FNV-1a 32 位指纹（接收序封账复算） ──────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    h
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 注入字节序（确定性：全字节域，含控制字符）。
pub fn inject_sequence(seed: u64) -> Vec<u8> {
    let mut rng = Lcg::new(seed);
    let mut seq: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < INJECT_BYTES {
        seq.push(rng.below(256) as u8);
        i += 1;
    }
    seq
}

// ── 环回两态状态机（LOOP 位 置位/复位） ─────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopPhase {
    /// 在线（LOOP=0）
    Line,
    /// 环回（LOOP=1）
    Loop,
}

// ── 环回账行（只追加） ──────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopRow {
    pub seq: u64,
    pub text: String,
}

/// 串口环回演练机。
#[derive(Clone, Debug)]
pub struct LoopbackRig {
    /// MCR 位账。
    pub mcr: u8,
    /// 进入环回前 DTR|RTS 快照。
    snapshot: u8,
    /// 接收账（环回直通落账）。
    pub rx: Vec<u8>,
    /// 两态状态机。
    pub phase: LoopPhase,
    /// 环回账（只追加）。
    pub rows: Vec<LoopRow>,
    row_seq: u64,
    /// 注入记账 tries/caught。
    pub inj_tries: [u32; INJ_ROUTES],
    pub inj_caught: [u32; INJ_ROUTES],
}

impl LoopbackRig {
    pub fn new() -> LoopbackRig {
        // 开场线态：DTR|RTS 置位（非平凡快照，退出对拍才有内容）。
        LoopbackRig {
            mcr: LINE_MASK,
            snapshot: 0,
            rx: Vec::new(),
            phase: LoopPhase::Line,
            rows: Vec::new(),
            row_seq: 0,
            inj_tries: [0; INJ_ROUTES],
            inj_caught: [0; INJ_ROUTES],
        }
    }

    fn push_row(&mut self, text: &str) {
        self.row_seq += 1;
        self.rows.push(LoopRow { seq: self.row_seq, text: String::from(text) });
    }

    /// MCR 环回位读（bit4）。
    pub fn loop_bit_set(&self) -> bool {
        self.mcr & (1 << MCR_LOOP_BIT) != 0
    }

    /// 线态位账读（DTR|RTS）。
    pub fn line_bits(&self) -> u8 {
        self.mcr & LINE_MASK
    }

    /// 进入环回：快照线态 + 置 LOOP 位（重复进入显性拒）。
    pub fn enter_loopback(&mut self) -> Result<(), u32> {
        if self.phase == LoopPhase::Loop || self.loop_bit_set() {
            return Err(LoopCode::AlreadyLoop.val());
        }
        self.snapshot = self.line_bits();
        self.mcr |= 1 << MCR_LOOP_BIT;
        self.phase = LoopPhase::Loop;
        self.push_row("进入环回·快照线态入账");
        Ok(())
    }

    /// 环回期发送：直落接收账（顺序保持）；线态下显性拒。
    pub fn send_byte(&mut self, b: u8) -> Result<(), u32> {
        if self.phase != LoopPhase::Loop || !self.loop_bit_set() {
            return Err(LoopCode::NotInLoop.val());
        }
        self.rx.push(b);
        Ok(())
    }

    /// 环回期收对账：全收 INJECT_BYTES/INJECT_BYTES 且顺序保持。
    pub fn recv_verify(&self, sent: &[u8]) -> Result<u32, u32> {
        if sent.len() != INJECT_BYTES {
            return Err(LoopCode::ByteLoss.val());
        }
        if self.rx.len() != INJECT_BYTES {
            return Err(LoopCode::ByteLoss.val());
        }
        let mut i = 0;
        while i < INJECT_BYTES {
            let (a, b) = match (self.rx.get(i), sent.get(i)) {
                (Some(a), Some(b)) => (*a, *b),
                _ => return Err(LoopCode::ByteLoss.val()),
            };
            if a != b {
                return Err(LoopCode::OrderBroken.val());
            }
            i += 1;
        }
        Ok(INJECT_BYTES as u32)
    }

    /// 注入 64 字节闭环演练：全发全收 64/64 顺序保持，落账。
    pub fn inject_round(&mut self, seed: u64) -> Result<u32, u32> {
        if self.phase != LoopPhase::Loop {
            return Err(LoopCode::NotInLoop.val());
        }
        let sent = inject_sequence(seed);
        let mut i = 0;
        while i < sent.len() {
            let b = match sent.get(i) {
                Some(&b) => b,
                None => break,
            };
            self.send_byte(b)?;
            i += 1;
        }
        let n = self.recv_verify(&sent)?;
        // 控制字符覆盖记账（判据口径：注入含控制字符）。
        let ctrl = sent.iter().filter(|&&b| b < 0x20 || b == 0x7F).count();
        self.push_row(&format!("注入{}字节·全收{}/{}·控制字符{}", INJECT_BYTES, n, INJECT_BYTES, ctrl));
        Ok(n)
    }

    /// 退出环回：复位 LOOP 位 + 线态复原断言（漂移即红）。
    pub fn exit_loopback(&mut self) -> Result<(), u32> {
        if self.phase != LoopPhase::Loop || !self.loop_bit_set() {
            return Err(LoopCode::NotInLoop.val());
        }
        self.mcr &= !(1 << MCR_LOOP_BIT);
        let restored = self.line_bits() == self.snapshot;
        self.phase = LoopPhase::Line;
        if restored {
            self.push_row("退出环回·线态复原·绿");
            Ok(())
        } else {
            self.push_row("退出环回·线态漂移·红");
            Err(LoopCode::LineLost.val())
        }
    }

    /// 环回账只追加守卫：行号连续递增、账长单调不减。
    pub fn ledger_append_only_ok(&self) -> bool {
        let mut prev: u64 = 0;
        for row in &self.rows {
            if row.seq != prev + 1 {
                return false;
            }
            prev = row.seq;
        }
        true
    }

    /// 接收序指纹（封账复算面）。
    pub fn rx_fingerprint(&self) -> u32 {
        fnv1a(&self.rx)
    }

    /// 注入路演练：人为构造违约（换序/丢字节/线态漂移），必被对拍抓。
    pub fn inject_fault(&mut self, seed: u64, route: usize) -> Result<bool, u32> {
        if route >= INJ_ROUTES {
            return Err(LoopCode::NotInLoop.val());
        }
        match self.inj_tries.get_mut(route) {
            Some(t) => *t += 1,
            None => return Err(LoopCode::NotInLoop.val()),
        }
        // 干净闭环基线：进入环回并全量注入（每轮清收账，轮间不漂移）。
        self.enter_loopback()?;
        self.rx.clear();
        let sent = inject_sequence(seed);
        let mut i = 0;
        while i < sent.len() {
            let b = match sent.get(i) {
                Some(&b) => b,
                None => break,
            };
            let _ = self.send_byte(b);
            i += 1;
        }
        let caught = match route {
            0 => {
                // 顺序错乱注入：首字节异或 0xFF 后对拍必炸 OrderBroken
                // （异或保证必变值，不依赖随机序巧合）。
                let a = match self.rx.get(0) {
                    Some(&v) => v,
                    None => return Err(LoopCode::ByteLoss.val()),
                };
                if let Some(s0) = self.rx.get_mut(0) {
                    *s0 = a ^ 0xFF;
                }
                match self.recv_verify(&sent) {
                    Err(e) => e == LoopCode::OrderBroken.val(),
                    Ok(_) => false,
                }
            }
            1 => {
                // 丢字节注入：移除一收字节后对拍必炸 ByteLoss。
                if !self.rx.is_empty() {
                    let _ = self.rx.remove(self.rx.len() - 1);
                }
                match self.recv_verify(&sent) {
                    Err(e) => e == LoopCode::ByteLoss.val(),
                    Ok(_) => false,
                }
            }
            _ => {
                // 线态漂移注入：环回期内翻掉 DTR 位，退出必炸 LineLost；
                // 注入后线态现场复原（避免跨轮位账漂移）。
                let pre = self.line_bits();
                self.mcr &= !(1 << MCR_DTR_BIT);
                let hit = match self.exit_loopback() {
                    Err(e) => e == LoopCode::LineLost.val(),
                    Ok(_) => false,
                };
                self.mcr = (self.mcr & !LINE_MASK) | pre;
                hit
            }
        };
        if caught {
            if let Some(c) = self.inj_caught.get_mut(route) {
                *c += 1;
            }
            self.push_row("注入·必抓");
        } else {
            self.push_row("注入·逃逸（红线）");
        }
        // 收尾（线态漂移路已在分支内退出）。
        if self.phase == LoopPhase::Loop {
            let _ = self.exit_loopback();
        }
        Ok(caught)
    }
}

/// 真机演练种子（冻结 = SEG_BASE）。
pub fn machine_seed() -> u64 {
    SEG_BASE as u64
}

// ── 黄金全链：进入→注入64/64保序→退出线态复原→三路注入全抓 ──────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rig = LoopbackRig::new();
    rig.enter_loopback()?;
    let n = rig.inject_round(machine_seed())?;
    // 控制字符覆盖（注入序须含控制字符）。
    let sent = inject_sequence(machine_seed());
    let ctrl = sent.iter().filter(|&&b| b < 0x20 || b == 0x7F).count();
    let fp0 = rig.rx_fingerprint();
    rig.exit_loopback()?;
    // 退出后线态：LOOP=0 且 DTR|RTS 复原。
    let line_ok = (!rig.loop_bit_set() && rig.line_bits() == LINE_MASK) as u32;
    // 注入三路 10 轮全抓。
    let mut caught = 0;
    let mut route = 0;
    while route < INJ_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            if rig.inject_fault(machine_seed(), route)? {
                caught += 1;
            }
            r += 1;
        }
        route += 1;
    }
    let closed = (n == INJECT_BYTES as u32) as u32;
    Ok((closed + line_ok + (if ctrl > 0 { 1 } else { 0 }), caught, fp0))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金链 3 面 + 30 路注入全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 30, _)) => Ok(()),
        _ => Err(LoopCode::NotReproducible.val()),
    }
}

/// 序 B：两态守卫——线态收发拒、重复进入拒、未入环回退出拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut rig = LoopbackRig::new();
    if rig.send_byte(0x41).is_ok() {
        return Err(LoopCode::NotReproducible.val());
    }
    if rig.exit_loopback().is_ok() {
        return Err(LoopCode::NotReproducible.val());
    }
    rig.enter_loopback()?;
    if rig.enter_loopback().is_ok() {
        return Err(LoopCode::NotReproducible.val());
    }
    let _ = rig.exit_loopback();
    Ok(())
}

/// 序 C：保序探针——序号 0..64 逐字节递增序环回不乱序。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut rig = LoopbackRig::new();
    rig.enter_loopback()?;
    let mut sent: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < INJECT_BYTES {
        sent.push((i % 256) as u8);
        i += 1;
    }
    for &b in &sent {
        rig.send_byte(b)?;
    }
    let n = rig.recv_verify(&sent)?;
    let fp1 = rig.rx_fingerprint();
    let fp2 = rig.rx_fingerprint();
    rig.exit_loopback()?;
    if n == INJECT_BYTES as u32 && fp1 == fp2 {
        Ok(())
    } else {
        Err(LoopCode::OrderBroken.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(LoopCode::NotReproducible.val());
    }
    let s1 = inject_sequence(machine_seed());
    let s2 = inject_sequence(machine_seed());
    if s1 != s2 {
        return Err(LoopCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < LoopCode::ALL.len() {
        if let Some(c) = LoopCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// J1：64 字节全收 64/64 且顺序保持。
fn closed_loop_ok() -> bool {
    let mut rig = LoopbackRig::new();
    if rig.enter_loopback().is_err() {
        return false;
    }
    match rig.inject_round(machine_seed()) {
        Ok(n) => n == INJECT_BYTES as u32,
        Err(_) => false,
    }
}

/// J1：注入序含控制字符（全字节域覆盖）。
fn ctrl_cover_ok() -> bool {
    let sent = inject_sequence(machine_seed());
    if sent.len() != INJECT_BYTES {
        return false;
    }
    let ctrl = sent.iter().filter(|&&b| b < 0x20 || b == 0x7F).count();
    ctrl > 0
}

/// J1：退出环回后线态恢复（DTR/RTS 位账复原）。
fn line_restore_ok() -> bool {
    let mut rig = LoopbackRig::new();
    if rig.enter_loopback().is_err() {
        return false;
    }
    let snap = rig.line_bits();
    let _ = rig.inject_round(machine_seed());
    match rig.exit_loopback() {
        Ok(()) => rig.line_bits() == snap && !rig.loop_bit_set(),
        Err(_) => false,
    }
}

/// J1：环回账只追加。
fn loop_ledger_ok() -> bool {
    let mut rig = LoopbackRig::new();
    let _ = rig.enter_loopback();
    let l0 = rig.rows.len();
    let _ = rig.inject_round(machine_seed());
    let l1 = rig.rows.len();
    let _ = rig.exit_loopback();
    let l2 = rig.rows.len();
    l0 == 1 && l1 == 2 && l2 == 3 && rig.ledger_append_only_ok()
}

// ── 判据面（extra_checks：全 add_dyn） ──────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J0-段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X} 窗口 0x75000..=0x781C0 且 0x40 对齐", SEG_BASE),
    ));
    v.push((
        "J1-MCR位冻结",
        MCR_LOOP_BIT == 4 && MCR_DTR_BIT == 0 && MCR_RTS_BIT == 1 && LINE_MASK == 0x03,
        String::from("LOOP=bit4 DTR=bit0 RTS=bit1（16550 口径）"),
    ));
    v.push((
        "J1-收发闭环64/64",
        closed_loop_ok(),
        format!("注入{}字节全收且顺序保持", INJECT_BYTES),
    ));
    v.push((
        "J1-控制字符覆盖",
        ctrl_cover_ok(),
        String::from("LCG 全字节域含控制字符"),
    ));
    v.push((
        "J1-线态恢复",
        line_restore_ok(),
        String::from("退出环回后 DTR/RTS 与快照逐位相等"),
    ));
    v.push((
        "J1-环回账只追加",
        loop_ledger_ok(),
        String::from("进入/注入/退出逐事件落账 行号连续"),
    ));
    // 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((faces, caught, _)) => *faces == 3 && *caught == 30,
            Err(_) => false,
        },
        match &g {
            Ok((faces, caught, _)) => format!("faces={} caught={}/30", faces, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 注入三路 10 轮。
    let mut rig = LoopbackRig::new();
    let mut tries = 0;
    let mut caught = 0;
    let mut inj_ok = true;
    let mut route = 0;
    while route < INJ_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            tries += 1;
            match rig.inject_fault(machine_seed(), route) {
                Ok(true) => caught += 1,
                _ => inj_ok = false,
            }
            r += 1;
        }
        route += 1;
    }
    v.push((
        "J1-注入10/10",
        inj_ok && caught == tries,
        format!("t/c {}/{} 三路（换序/丢字节/线态漂移）", caught, tries),
    ));
    // 两态守卫探针。
    v.push((
        "J2-两态守卫探针",
        probe_seq_b().is_ok(),
        String::from("线态收发拒/重复进入拒/未入环回退出拒"),
    ));
    // 保序探针。
    v.push((
        "J2-保序探针",
        probe_seq_c().is_ok(),
        String::from("0..64 递增序环回逐字节保序"),
    ));
    // 黄金链序 A。
    v.push(("J2-探针序A", probe_seq_a().is_ok(), String::from("3面+30抓")));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 与注入序两跑同值"),
    ));
    // 指纹复算。
    let fp_g = match &g {
        Ok((_, _, fp)) => *fp,
        Err(_) => 0,
    };
    let fp_re = fnv1a(&inject_sequence(machine_seed()));
    v.push((
        "J3-指纹复算",
        fp_g == fp_re,
        format!("封账=0x{:08X} 复算=0x{:08X}", fp_g, fp_re),
    ));
    // 诊断码闭集。
    v.push((
        "J3-诊断码闭集",
        detail(SEG_BASE + 0x3F).starts_with("未知") && detail(LoopCode::LineLost.val()) == LoopCode::LineLost.detail(),
        String::from("闭集查表命中 + 窗口外未知"),
    ));
    v
}

pub fn run_unx0505_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0505 串口环回演练（MCR loopback）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chain_all_green() {
        assert_eq!(golden_run().map(|(f, c, _)| (f, c)), Ok((3, 30)));
    }

    #[test]
    fn inject_64_all_received_in_order() {
        let mut rig = LoopbackRig::new();
        assert!(rig.enter_loopback().is_ok());
        let sent = inject_sequence(machine_seed());
        assert_eq!(sent.len(), INJECT_BYTES);
        for &b in &sent {
            assert!(rig.send_byte(b).is_ok());
        }
        assert_eq!(rig.recv_verify(&sent), Ok(INJECT_BYTES as u32));
        assert_eq!(rig.rx.len(), 64);
    }

    #[test]
    fn order_preserved_byte_wise() {
        let mut rig = LoopbackRig::new();
        let _ = rig.enter_loopback();
        let sent = inject_sequence(machine_seed());
        for &b in &sent {
            let _ = rig.send_byte(b);
        }
        let mut i = 0;
        while i < INJECT_BYTES {
            let (a, b) = match (rig.rx.get(i), sent.get(i)) {
                (Some(a), Some(b)) => (*a, *b),
                _ => (0, 0),
            };
            assert_eq!(a, b, "第 {} 字节须保序", i);
            i += 1;
        }
    }

    #[test]
    fn line_state_restored_after_exit() {
        let mut rig = LoopbackRig::new();
        let snap = rig.line_bits();
        assert!(rig.enter_loopback().is_ok());
        assert!(rig.loop_bit_set());
        let _ = rig.inject_round(machine_seed());
        assert!(rig.exit_loopback().is_ok());
        assert!(!rig.loop_bit_set());
        assert_eq!(rig.line_bits(), snap, "DTR/RTS 位账须复原");
        assert_eq!(rig.line_bits(), LINE_MASK);
    }

    #[test]
    fn two_state_machine_guards() {
        let mut rig = LoopbackRig::new();
        assert_eq!(rig.send_byte(0x41), Err(LoopCode::NotInLoop.val()));
        assert_eq!(rig.exit_loopback(), Err(LoopCode::NotInLoop.val()));
        assert!(rig.enter_loopback().is_ok());
        assert_eq!(rig.enter_loopback(), Err(LoopCode::AlreadyLoop.val()));
        assert!(rig.exit_loopback().is_ok());
    }

    #[test]
    fn all_injection_routes_caught() {
        let mut rig = LoopbackRig::new();
        let mut route = 0;
        while route < INJ_ROUTES {
            let mut r = 0;
            while r < JUDGE_ROUNDS {
                match rig.inject_fault(machine_seed(), route) {
                    Ok(true) => {}
                    other => panic!("注入路{}轮{} 必抓，实得 {:?}", route, r, other),
                }
                r += 1;
            }
            route += 1;
        }
        let mut i = 0;
        while i < INJ_ROUTES {
            let (t, c) = match (rig.inj_tries.get(i), rig.inj_caught.get(i)) {
                (Some(t), Some(c)) => (*t, *c),
                _ => (0, 0),
            };
            assert_eq!(c, t, "路{}全抓", i);
            assert_eq!(t, JUDGE_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn loop_ledger_append_only() {
        let mut rig = LoopbackRig::new();
        let _ = rig.enter_loopback();
        let _ = rig.inject_round(machine_seed());
        let _ = rig.exit_loopback();
        assert!(rig.ledger_append_only_ok());
        assert_eq!(rig.rows.len(), 3);
        let mut expect = 1u64;
        for row in &rig.rows {
            assert!(!row.text.is_empty());
            assert_eq!(row.seq, expect);
            expect += 1;
        }
    }

    #[test]
    fn double_run_identical() {
        assert!(repro_check().is_ok());
        let s1 = inject_sequence(machine_seed());
        let s2 = inject_sequence(machine_seed());
        assert_eq!(s1, s2);
        assert_eq!(fnv1a(&s1), fnv1a(&s2), "指纹确定性");
    }

    #[test]
    fn sequence_contains_control_bytes() {
        let sent = inject_sequence(machine_seed());
        let ctrl = sent.iter().filter(|&&b| b < 0x20 || b == 0x7F).count();
        assert!(ctrl > 0, "注入序须含控制字符（判据口径）");
    }

    #[test]
    fn segment_window_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut i = 0;
        while i < LoopCode::ALL.len() {
            if let Some(c) = LoopCode::ALL.get(i) {
                let v = c.val();
                assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知"));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }
}
