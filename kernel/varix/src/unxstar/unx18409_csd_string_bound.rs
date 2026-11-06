//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18409 · szCSDVersion 服务包字符串语义
//! · 0x7E200 段 · 批 01 · 配额 260
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18409-J1 字符串拷贝边界（128 WCHAR）越界注入 10/10 检出，
//!   空串/缺省路径覆盖。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 缓冲冻结：szCSDVersion = [WCHAR; 128]，NUL 收尾——可用正文至多
//!     127 槽（末槽留 NUL），len==128 即越界必拒；
//!   - 越界注入核：128..137 十档轮换 × 10 轮 = 100/100 全拒（显式
//!     CsdOverrun，绝不截断静默）；
//!   - 代理对闸：WCHAR ∈ [0xD800, 0xDFFF]（UTF-16 代理半对）→ 拒
//!     （无配对半的孤立代理是坏标量）；
//!   - 空串/缺省路径：空输入 → 存单 NUL、解码空串（合法）；残留清洁
//!     性质：拷贝前缓冲有陈旧垃圾，拷贝后 NUL 之后必须全零（无残渣）；
//!   - 幂等：同串重复拷贝缓冲逐槽一致；往返：copy → decode → 再 copy
//!     逐槽一致；
//!   - 状态机：CopyGate 两态 Open → Sealed 单向；封印后拷贝被拒；
//!   - 账本：CopyLedger（尝试/成/越界拒/代理拒/空路径，守恒）；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] CsCode 闭集（10 码）+ ALL + detail() + detail(code)
//!   [数据结构] CsdBuffer（128 WCHAR 定容）+ 残留清洁核算
//!   [解析器]   copy_csd / decode_csd / csd_roundtrip
//!   [状态机]   GatePhase 两态单向 + CopyGate
//!   [账本]     CopyLedger（守恒核算）
//!   [注入核]   越界十档 ×10 轮 + 代理对 ×10 轮 + 封印后拷贝
//!   [收口]     golden_run + run_unx18409_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E200，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E200;

// ── 缓冲冻结常量 ────────────────────────────────────────────────
/// szCSDVersion 容量（128 WCHAR，winnt.h 口径）。
pub const CSD_CAP: usize = 128;
/// 正文上限（推导：128 槽 - 1 个 NUL 收尾槽 = 127）。
pub const CSD_BODY_MAX: usize = 127;
/// 代理半对下界。
pub const SURROGATE_LO: u16 = 0xD800;
/// 代理半对上界。
pub const SURROGATE_HI: u16 = 0xDFFF;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 越界注入档数（128..137 十档）。
pub const OVERRUN_VARIANTS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CsCode {
    /// 越界（源长 ≥ 128，NUL 无处安放）。
    CsdOverrun,
    /// 代理半对（孤立 surrogate）。
    WcharSurrogate,
    /// 解码缺 NUL（缓冲 128 槽无收尾）。
    DecodeNoTerminator,
    /// 残留污染（NUL 之后仍有非零残渣）。
    ResidueDirty,
    /// 往返失真（copy→decode→copy 不还原）。
    RoundtripBroken,
    /// 封印后拷贝被拒。
    SealedCopy,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 解码越界（防御，缓冲恒定长）。
    DecodeOverrun,
    /// 双跑不一致。
    NotReproducible,
    /// 拷贝账与缓冲态不符（账实对不上）。
    LedgerStateMismatch,
}

impl CsCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CsCode::CsdOverrun => "越界：源长 ≥128，NUL 无处安放",
            CsCode::WcharSurrogate => "代理半对：孤立 surrogate 拒收",
            CsCode::DecodeNoTerminator => "解码缺 NUL：128 槽无收尾",
            CsCode::ResidueDirty => "残留污染：NUL 之后有非零残渣",
            CsCode::RoundtripBroken => "往返失真：拷贝链不还原",
            CsCode::SealedCopy => "封印后拷贝被拒",
            CsCode::LedgerBroken => "账本守恒破坏",
            CsCode::DecodeOverrun => "解码越界：防御性拒绝",
            CsCode::NotReproducible => "双跑不一致：复现红线",
            CsCode::LedgerStateMismatch => "账实不符：拷贝账与缓冲态对不上",
        }
    }
    pub const ALL: [CsCode; 10] = [
        CsCode::CsdOverrun,
        CsCode::WcharSurrogate,
        CsCode::DecodeNoTerminator,
        CsCode::ResidueDirty,
        CsCode::RoundtripBroken,
        CsCode::SealedCopy,
        CsCode::LedgerBroken,
        CsCode::DecodeOverrun,
        CsCode::NotReproducible,
        CsCode::LedgerStateMismatch,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CsCode::ALL.len() {
        if CsCode::ALL[i].code() == code {
            return CsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E200 段闭集内"
}

// ── 数据结构（定容缓冲） ────────────────────────────────────────
/// szCSDVersion 定容缓冲（128 WCHAR，NUL 收尾）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CsdBuffer {
    pub buf: [u16; CSD_CAP],
}

impl CsdBuffer {
    /// 空缓冲（全零 = 空串）。
    pub const fn empty() -> CsdBuffer {
        CsdBuffer { buf: [0; CSD_CAP] }
    }
    /// 垃圾预填（残留测试用：全槽 0x4141）。
    pub fn junk() -> CsdBuffer {
        CsdBuffer { buf: [0x4141; CSD_CAP] }
    }
    /// 是否 NUL 收尾（扫描 128 槽）。
    pub fn terminated(&self) -> bool {
        let mut i = 0;
        while i < CSD_CAP {
            if self.buf[i] == 0 {
                return true;
            }
            i += 1;
        }
        false
    }
    /// 残留清洁：NUL 之后全零。
    pub fn residue_clean(&self) -> bool {
        // 先找 NUL 位；缺 NUL 时该性质不适用（由 terminated 闸单独判）。
        let mut nul = None;
        let mut i = 0;
        while i < CSD_CAP {
            if self.buf[i] == 0 {
                nul = Some(i);
                break;
            }
            i += 1;
        }
        let start = match nul {
            Some(k) => k,
            None => return false,
        };
        let mut j = start;
        while j < CSD_CAP {
            if self.buf[j] != 0 {
                return false;
            }
            j += 1;
        }
        true
    }
}

// ── 解析器核（拷贝 / 解码） ─────────────────────────────────────
/// 源校验：逐槽代理闸（孤立 surrogate 拒收）。
pub fn src_scalar_ok(src: &[u16]) -> Result<(), u32> {
    let mut i = 0;
    while i < src.len() {
        let w = src[i];
        if w >= SURROGATE_LO && w <= SURROGATE_HI {
            return Err(CsCode::WcharSurrogate.code());
        }
        i += 1;
    }
    Ok(())
}

/// 拷贝：src（WCHAR 串）→ 缓冲。len ≤ 127 过（含 0=空串）；
/// len ≥ 128 越界显式拒；拷贝后 NUL 之后全槽清零（无残留）。
/// 返回拷贝的正文槽数（= len）。
pub fn copy_csd(src: &[u16], buf: &mut [u16; CSD_CAP]) -> Result<usize, u32> {
    // 越界闸（先于任何写入，失败零副作用）。
    if src.len() > CSD_BODY_MAX {
        return Err(CsCode::CsdOverrun.code());
    }
    // 代理闸。
    src_scalar_ok(src)?;
    // 写正文。
    let mut i = 0;
    while i < src.len() {
        buf[i] = src[i];
        i += 1;
    }
    // NUL 收尾 + 残留清洁（正文之后全槽清零）。
    let mut j = src.len();
    while j < CSD_CAP {
        buf[j] = 0;
        j += 1;
    }
    Ok(src.len())
}

/// 解码：缓冲 → 串（扫描至 NUL，有界；缺 NUL 拒）。
pub fn decode_csd(buf: &[u16; CSD_CAP]) -> Result<String, u32> {
    let mut n = 0usize;
    while n < CSD_CAP {
        if buf[n] == 0 {
            break;
        }
        n += 1;
    }
    if n == CSD_CAP {
        return Err(CsCode::DecodeNoTerminator.code());
    }
    // 代理闸（缓冲内也要校验）。
    let mut i = 0;
    while i < n {
        let w = buf[i];
        if w >= SURROGATE_LO && w <= SURROGATE_HI {
            return Err(CsCode::WcharSurrogate.code());
        }
        i += 1;
    }
    // u16 → char（BMP 直映；代理已被闸掉）。
    let mut s = String::new();
    let mut k = 0;
    while k < n {
        match char::from_u32(buf[k] as u32) {
            Some(c) => s.push(c),
            None => return Err(CsCode::DecodeOverrun.code()),
        }
        k += 1;
    }
    Ok(s)
}

/// 往返：src → 拷贝 → 解码 → 再拷贝 → 逐槽一致。
pub fn csd_roundtrip(src: &[u16]) -> Result<(), u32> {
    let mut b1 = CsdBuffer::empty();
    copy_csd(src, &mut b1.buf)?;
    let s = decode_csd(&b1.buf)?;
    // 串回 WCHAR（BMP 直映：解码产物必为 BMP 字符序列）。
    let mut back: Vec<u16> = Vec::new();
    let mut cit = s.chars();
    loop {
        let c = match cit.next() {
            Some(c) => c,
            None => break,
        };
        let v = c as u32;
        if v > 0xFFFF {
            return Err(CsCode::RoundtripBroken.code());
        }
        back.push(v as u16);
    }
    let mut b2 = CsdBuffer::empty();
    copy_csd(&back, &mut b2.buf)?;
    if b1 != b2 {
        return Err(CsCode::RoundtripBroken.code());
    }
    Ok(())
}

// ── 账本 ────────────────────────────────────────────────────────
/// 拷贝账本（守恒：尝试 = 成 + 越界拒 + 代理拒 + 封印拒）。
#[derive(Clone, Copy, Debug)]
pub struct CopyLedger {
    pub tries: u32,
    pub ok: u32,
    pub overrun_rej: u32,
    pub surrogate_rej: u32,
    pub sealed_rej: u32,
    /// 空路径命中数（空串拷贝走缺省路径）。
    pub empty_path: u32,
}

impl CopyLedger {
    pub fn new() -> CopyLedger {
        CopyLedger {
            tries: 0,
            ok: 0,
            overrun_rej: 0,
            surrogate_rej: 0,
            sealed_rej: 0,
            empty_path: 0,
        }
    }
    /// 守恒：尝试 = 成 + 越界拒 + 代理拒 + 封印拒。
    pub fn conserve(&self) -> bool {
        self.tries == self.ok + self.overrun_rej + self.surrogate_rej + self.sealed_rej
    }
}

// ── 状态机（CopyGate：Open → Sealed 单向） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatePhase {
    /// 开放拷贝。
    Open,
    /// 已封印（只读）。
    Sealed,
}

/// 拷贝闸门：开放期收拷贝账，封印后拷贝被拒。
pub struct CopyGate {
    phase: GatePhase,
    /// 当前缓冲（封印后只读）。
    pub buffer: CsdBuffer,
    pub ledger: CopyLedger,
}

impl CopyGate {
    pub fn new() -> CopyGate {
        CopyGate {
            phase: GatePhase::Open,
            buffer: CsdBuffer::empty(),
            ledger: CopyLedger::new(),
        }
    }
    pub fn phase(&self) -> GatePhase {
        self.phase
    }
    /// 拷贝一笔（Open 态；Sealed 态拒）。
    pub fn copy(&mut self, src: &[u16]) -> Result<usize, u32> {
        self.ledger.tries += 1;
        if self.phase != GatePhase::Open {
            self.ledger.sealed_rej += 1;
            return Err(CsCode::SealedCopy.code());
        }
        if src.is_empty() {
            self.ledger.empty_path += 1;
        }
        match copy_csd(src, &mut self.buffer.buf) {
            Ok(n) => {
                self.ledger.ok += 1;
                Ok(n)
            }
            Err(c) if c == CsCode::CsdOverrun.code() => {
                self.ledger.overrun_rej += 1;
                Err(CsCode::CsdOverrun.code())
            }
            Err(c) if c == CsCode::WcharSurrogate.code() => {
                self.ledger.surrogate_rej += 1;
                Err(CsCode::WcharSurrogate.code())
            }
            Err(c) => Err(c),
        }
    }
    /// 封印（Open → Sealed 单向；封印前缓冲必须 NUL 收尾且残留清洁）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != GatePhase::Open {
            return Err(CsCode::SealedCopy.code());
        }
        if !self.buffer.terminated() || !self.buffer.residue_clean() {
            return Err(CsCode::ResidueDirty.code());
        }
        if !self.ledger.conserve() {
            return Err(CsCode::LedgerBroken.code());
        }
        self.phase = GatePhase::Sealed;
        Ok(())
    }
}

// ── 注入核 ──────────────────────────────────────────────────────
/// 越界注入样本：len = 128..137 十档轮换（127+1 .. 127+10）。
pub fn overrun_sample(round: usize) -> Vec<u16> {
    let len = CSD_BODY_MAX + 1 + (round % OVERRUN_VARIANTS);
    let mut v = Vec::new();
    let mut i = 0;
    while i < len {
        // 正文全合法标量（'A' + 轮换），保证拒因是越界而非代理。
        v.push(0x41 + ((i + round) % 26) as u16);
        i += 1;
    }
    v
}

/// 越界注入 rounds 轮：十档 × 10 轮 = 100 全拒。
pub fn overrun_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut k = 0;
        while k < OVERRUN_VARIANTS {
            let sample = overrun_sample(k);
            let mut buf = CsdBuffer::junk().buf;
            match copy_csd(&sample, &mut buf) {
                Err(c) => {
                    if c != CsCode::CsdOverrun.code() {
                        return Err(CsCode::CsdOverrun.code());
                    }
                    rejected += 1;
                }
                Ok(_) => return Err(CsCode::CsdOverrun.code()),
            }
            k += 1;
        }
        r += 1;
    }
    Ok(rejected)
}

/// 代理注入 rounds 轮：孤立 surrogate 三档（下界/上界/中间）× rounds 轮，
/// 每轮三档全打（3 × 10 = 30 样本），逐档必拒且拒因必须是代理码。
pub fn surrogate_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        // 三档样本逐档打：下界 0xD800 / 上界 0xDFFF / 中间 0xDABC，
        // 三值全部落在 [0xD800, 0xDFFF] 闭区间内（含端点即闸）。
        let mut k = 0;
        while k < 3 {
            let w = match k {
                0 => SURROGATE_LO,
                1 => SURROGATE_HI,
                _ => 0xDABC,
            };
            let sample: Vec<u16> = vec![0x53CA, w, 0x0053]; // "叉?S"：中间埋代理
            let mut buf = CsdBuffer::empty().buf;
            match copy_csd(&sample, &mut buf) {
                Err(c) => {
                    if c != CsCode::WcharSurrogate.code() {
                        return Err(CsCode::WcharSurrogate.code());
                    }
                    rejected += 1;
                }
                Ok(_) => return Err(CsCode::WcharSurrogate.code()),
            }
            k += 1;
        }
        r += 1;
    }
    Ok(rejected)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut gate = CopyGate::new();
    // 常规串拷贝（"Service Pack 2" 的 WCHAR 投影，14 槽正文）。
    let sp2: Vec<u16> = vec![
        0x0053, 0x0065, 0x0072, 0x0076, 0x0069, 0x0063, 0x0065, 0x0020,
        0x0050, 0x0061, 0x0063, 0x006B, 0x0020, 0x0032,
    ];
    let n1 = gate.copy(&sp2)?;
    if n1 != 14 {
        return Err(CsCode::LedgerStateMismatch.code());
    }
    // 空串路径（缺省：无 SP）。
    let n0 = gate.copy(&[])?;
    if n0 != 0 {
        return Err(CsCode::LedgerStateMismatch.code());
    }
    // 满 body 边界（127 槽过）。
    let full: Vec<u16> = {
        let mut v = Vec::new();
        let mut i = 0;
        while i < CSD_BODY_MAX {
            v.push(0x41 + (i % 26) as u16);
            i += 1;
        }
        v
    };
    let nmax = gate.copy(&full)?;
    if nmax != CSD_BODY_MAX {
        return Err(CsCode::LedgerStateMismatch.code());
    }
    // 残留清洁：垃圾预填 + 短串拷贝 → NUL 后全零。
    let mut junkbuf = CsdBuffer::junk();
    copy_csd(&sp2, &mut junkbuf.buf)?;
    if !junkbuf.residue_clean() {
        return Err(CsCode::ResidueDirty.code());
    }
    // 往返：常规串 + 空串 + 满串。
    csd_roundtrip(&sp2)?;
    csd_roundtrip(&[])?;
    csd_roundtrip(&full)?;
    // 注入：越界 100 拒 + 代理 30 拒。
    let ovr = overrun_inject_rounds(JUDGE_ROUNDS)?;
    if ovr != OVERRUN_VARIANTS as u32 * JUDGE_ROUNDS {
        return Err(CsCode::CsdOverrun.code());
    }
    let sur = surrogate_inject_rounds(JUDGE_ROUNDS)?;
    if sur != 3 * JUDGE_ROUNDS {
        return Err(CsCode::WcharSurrogate.code());
    }
    // 封印（空串后缓冲 NUL 收尾 + 清洁）。
    if !gate.ledger.conserve() {
        return Err(CsCode::LedgerBroken.code());
    }
    gate.seal()?;
    if gate.phase() != GatePhase::Sealed {
        return Err(CsCode::SealedCopy.code());
    }
    if gate.copy(&sp2).is_ok() {
        return Err(CsCode::SealedCopy.code());
    }
    Ok((gate.ledger.ok, ovr, sur, gate.ledger.tries))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CsCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CsCode::ALL.len() {
        let v = CsCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：常规串拷贝 → 解码还原（"Service Pack 2"）。
pub fn probe_seq_a() -> Result<(), u32> {
    let sp2: Vec<u16> = vec![
        0x0053, 0x0065, 0x0072, 0x0076, 0x0069, 0x0063, 0x0065, 0x0020,
        0x0050, 0x0061, 0x0063, 0x006B, 0x0020, 0x0032,
    ];
    let mut b = CsdBuffer::empty();
    copy_csd(&sp2, &mut b.buf)?;
    let s = decode_csd(&b.buf)?;
    if s != "Service Pack 2" {
        return Err(CsCode::RoundtripBroken.code());
    }
    Ok(())
}

/// 探针 B：len=128 越界 → 必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = CsdBuffer::empty();
    let over = overrun_sample(0);
    copy_csd(&over, &mut b.buf).map(|_| ())
}

/// 探针 C：空串路径（空缓冲缺省 → 解码空串）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = CsdBuffer::junk();
    copy_csd(&[], &mut b.buf)?;
    let s = decode_csd(&b.buf)?;
    if !s.is_empty() {
        return Err(CsCode::ResidueDirty.code());
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 缓冲冻结：128 WCHAR / 正文 127。
    v.push((
        "J1-缓冲128正文127",
        CSD_CAP == 128 && CSD_BODY_MAX == 127,
        format!("cap={} body={}", CSD_CAP, CSD_BODY_MAX),
    ));

    // 2. 常规串拷贝→解码还原（"Service Pack 2"）。
    let pa = probe_seq_a();
    v.push(("J1-拷贝解码还原SP2", pa.is_ok(), format!("{:?}", pa)));

    // 3. 越界注入 10 档 × 10 轮 = 100 全拒。
    let ovr = overrun_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-越界注入100/100",
        match &ovr {
            Ok(n) => *n == 100,
            Err(_) => false,
        },
        format!("{:?}", ovr),
    ));

    // 4. 代理注入 3 档 × 10 轮 = 30 全拒。
    let sur = surrogate_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-代理注入30/30",
        match &sur {
            Ok(n) => *n == 30,
            Err(_) => false,
        },
        format!("{:?}", sur),
    ));

    // 5. 边界：127 过 / 128 拒（满与满+1）。
    let b_ok = {
        let full: Vec<u16> = {
            let mut v2 = Vec::new();
            let mut i = 0;
            while i < CSD_BODY_MAX {
                v2.push(0x41 + (i % 26) as u16);
                i += 1;
            }
            v2
        };
        let mut buf1 = CsdBuffer::empty();
        let ok127 = copy_csd(&full, &mut buf1.buf).is_ok();
        let mut buf2 = CsdBuffer::empty();
        let mut over = full.clone();
        over.push(0x42);
        let rej128 = match copy_csd(&over, &mut buf2.buf) {
            Err(c) => c == CsCode::CsdOverrun.code(),
            Ok(_) => false,
        };
        ok127 && rej128
    };
    v.push(("J1-127过128拒", b_ok, String::from("满/满+1 显式裁决")));

    // 6. 空串/缺省路径（垃圾缓冲 + 空拷贝 → 干净空串）。
    let pc = probe_seq_c();
    v.push(("J1-空串缺省路径", pc.is_ok(), format!("{:?}", pc)));

    // 7. 残留清洁（垃圾预填 + 短串 → NUL 后全零）。
    let residue_ok = {
        let mut jb = CsdBuffer::junk();
        let sp: Vec<u16> = vec![0x0041, 0x0042];
        let mut ok = copy_csd(&sp, &mut jb.buf).is_ok();
        ok = ok && jb.residue_clean();
        // 对照：垃圾未拷贝时 residue_clean 必为假（性质不平凡）。
        let dirty = !CsdBuffer::junk().residue_clean();
        ok && dirty
    };
    v.push(("J1-残留清洁性质", residue_ok, String::from("NUL 后全零")));

    // 8. 往返（常规串 + 空串 + 满串三路）。
    let rt_ok = {
        let sp2: Vec<u16> = vec![
            0x0053, 0x0065, 0x0072, 0x0076, 0x0069, 0x0063, 0x0065, 0x0020,
            0x0050, 0x0061, 0x0063, 0x006B, 0x0020, 0x0032,
        ];
        let mut ok = csd_roundtrip(&sp2).is_ok() && csd_roundtrip(&[]).is_ok();
        let full: Vec<u16> = {
            let mut v3 = Vec::new();
            let mut i = 0;
            while i < CSD_BODY_MAX {
                v3.push(0x41 + (i % 26) as u16);
                i += 1;
            }
            v3
        };
        ok = ok && csd_roundtrip(&full).is_ok();
        ok
    };
    v.push(("J1-往返三路", rt_ok, String::from("常规/空/满")));

    // 9. 幂等：同串重复拷贝缓冲逐槽一致。
    let idem = {
        let sp: Vec<u16> = vec![0x0041, 0x0042, 0x0043];
        let mut b1 = CsdBuffer::empty();
        let mut b2 = CsdBuffer::empty();
        let _ = copy_csd(&sp, &mut b1.buf);
        let _ = copy_csd(&sp, &mut b2.buf);
        let _ = copy_csd(&sp, &mut b2.buf);
        b1 == b2
    };
    v.push(("J1-重复拷贝幂等", idem, String::from("三拷同缓冲")));

    // 10. 封印闸（黄金链内含封印后拷贝拒）。
    let g = golden_run();
    let seal_ok = match &g {
        Ok((okc, _o, _s, _t)) => *okc == 3,
        Err(_) => false,
    };
    v.push(("J1-封印后拷贝拒", seal_ok, format!("{:?}", g)));

    // 11. 账守恒（黄金链 3 成 + 封印拒 1）。
    let cons_ok = match &g {
        Ok((_o1, _o2, _s, tries)) => *tries == 4,
        Err(_) => false,
    };
    v.push(("J1-账守恒(4试)", cons_ok, String::from("3成1封印拒")));

    // 12. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 14. 诊断码闭集（10 码全非空中文明细）。
    let all_ok = {
        let mut ok = CsCode::ALL.len() == 10;
        let mut i = 0;
        while i < CsCode::ALL.len() {
            if CsCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集10码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 14 条；前 8 条动态明细）。
pub fn run_unx18409_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18409 CSD 串拷贝边界");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18409_checks();
        assert!(set.all_passed(), "UNX-F18409 判据集应全绿");
    }

    /// 正常流："Service Pack 2" 拷贝→解码还原。
    #[test]
    fn test_probe_seq_a() {
        assert!(probe_seq_a().is_ok());
    }

    /// 注入：越界 10 档 × 10 轮 = 100 全拒。
    #[test]
    fn test_overrun_injection() {
        assert_eq!(overrun_inject_rounds(JUDGE_ROUNDS), Ok(100));
    }

    /// 注入：代理 3 档 × 10 轮 = 30 全拒。
    #[test]
    fn test_surrogate_injection() {
        assert_eq!(surrogate_inject_rounds(JUDGE_ROUNDS), Ok(30));
    }

    /// 边界：127 过 / 128 拒（CsdOverrun 码）。
    #[test]
    fn test_boundary_127_128() {
        assert!(probe_seq_b().is_err());
        let full: Vec<u16> = {
            let mut v = Vec::new();
            let mut i = 0;
            while i < CSD_BODY_MAX {
                v.push(0x41);
                i += 1;
            }
            v
        };
        let mut buf = CsdBuffer::empty();
        assert!(copy_csd(&full, &mut buf.buf).is_ok());
        let mut over = full.clone();
        over.push(0x42);
        match copy_csd(&over, &mut buf.buf) {
            Err(c) => assert_eq!(c, CsCode::CsdOverrun.code()),
            Ok(_) => assert!(false, "128 必须拒"),
        }
    }

    /// 空串路径：垃圾缓冲 + 空拷贝 → 干净空串。
    #[test]
    fn test_empty_path() {
        assert!(probe_seq_c().is_ok());
    }

    /// 残留清洁性质（含反例：垃圾缓冲本身不洁）。
    #[test]
    fn test_residue_clean() {
        let mut jb = CsdBuffer::junk();
        assert!(!jb.residue_clean(), "垃圾缓冲必须不洁（性质反例）");
        let one: Vec<u16> = vec![0x0041];
        assert!(copy_csd(&one, &mut jb.buf).is_ok());
        assert!(jb.residue_clean());
    }

    /// 幂等：同串重复拷贝缓冲逐槽一致。
    #[test]
    fn test_copy_idempotent() {
        let sp: Vec<u16> = vec![0x0041, 0x0042];
        let mut b1 = CsdBuffer::empty();
        let mut b2 = CsdBuffer::empty();
        let _ = copy_csd(&sp, &mut b1.buf);
        let _ = copy_csd(&sp, &mut b2.buf);
        let _ = copy_csd(&sp, &mut b2.buf);
        assert_eq!(b1, b2);
    }

    /// 封印：Open→Sealed 单向；封印后拷贝拒（SealedCopy 码）。
    #[test]
    fn test_seal_gate() {
        let mut gate = CopyGate::new();
        assert!(gate.seal().is_ok());
        let one: Vec<u16> = vec![0x0041];
        match gate.copy(&one) {
            Err(c) => assert_eq!(c, CsCode::SealedCopy.code()),
            Ok(_) => assert!(false, "封印后拷贝必须拒"),
        }
    }

    /// 复现红线：golden_run 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
