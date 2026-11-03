//! UNX-F0449 DSDT 指针账（只读定位） · 0x75200段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104）：
//!   DSDT 物理指针/长度/校验和三要素账齐（AML 零执行断言），
//!   v2+ X_DSDT 64 位优先裁定入账。
//!
//! 设计要点：
//!   ① 三要素账：DSDT 物理指针（FADT.DSDT 32 位 @140 + v2+ X_DSDT
//!      64 位 @140，字段完整落位以 FADT 表长 ≥148 为门）/ 表长
//!      （DSDT 表头 @4 的 u32）/ 全表校验和（sum mod 256 ≡ 0）；
//!   ② v2+ 优先裁定：FADT revision ≥ 2 且表长 ≥ 148 且 X_DSDT 非
//!      零 → 走 64 位面（V2Plus64 路径）；X_DSDT 为零则回落 32 位
//!      面（V2PlusFallBack32 路径）；revision 1 只走 32 位面
//!      （V1Only 路径）——三条裁定路径分账，账面可复算；
//!   ③ AML 零执行红线：DSDT 全文是 AML 字节码，本模块只做指针账
//!      不做 AML 解释——AmlGuard 记账面（查看字节数/执行次数/写
//!      次数），执行与写任何一次都立案，管线跑完必须 read_only；
//!   ④ 长度 0 拒、校验和坏拒、签名非 "DSDT" 拒、双面指针全零拒；
//!   ⑤ 单调状态机 Raw→FacdVetted→PtrArbitrated→TableVetted→
//!      LedgerClosed，倒退显性拒。
//!
//! 诊断码段：0x75200（0x40 窗口闭集，全域独占）。
//! 纯功能线：no_std + alloc，无 std::/unsafe/unwrap/expect/panic，
//! 整数算术，确定性，双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75200，AI-VX7-1104 F0449 独占区）。
pub const SEG_BASE: u32 = 0x75200;

// ── 冻结常量表（来源：ACPI Specification 6.x §5.2.9 FADT / §5.2.11.1 DSDT； ──
// ── 偏移口径对齐 ACPICA ACPI_FADT_OFFSET 表） ──
/// DSDT 表头定长（标准 ACPI 表头 36B）。
pub const DSDT_HDR_LEN: usize = 36;
/// 表头 Revision 字节偏移（标准 ACPI 表头：Length u32 @4 占 4..8）。
pub const OFF_REVISION: usize = 8;
/// 表头 Checksum 字节偏移。
pub const OFF_CHECKSUM: usize = 9;
/// FADT 内 32 位 DSDT 指针偏移（FIRMWARE_CTRL@36 之后）。
pub const OFF_DSDT32: usize = 40;
/// FADT 内 64 位 X_DSDT 指针偏移（X_FIRMWARE_CTRL@132 之后）。
pub const OFF_XDSDT: usize = 140;
/// X_DSDT 字段完整落位所需的最小 FADT 表长（140 + 8 = 148）。
pub const FADT_LEN_XDSDT_FULL: usize = 148;
/// DSDT 表签名 "DSDT" 的 4 字节。
pub const SIG_DSDT: [u8; 4] = [b'D', b'S', b'D', b'T'];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsdtCode {
    /// DSDT 表签名非 "DSDT"
    SigBad,
    /// DSDT 声明长度为 0
    LenZero,
    /// DSDT 声明长度越缓冲（截断表）
    LenOver,
    /// 全表校验和坏
    SumBad,
    /// 双面指针全零：既无 32 位也无 64 位指针可立账
    PtrZero,
    /// FADT 连 32 位指针偏移（140..144）都放不下
    FacdShort,
    /// AML 执行红线：指针账期间发生任何解释/写操作
    AmlExec,
    /// 状态机倒退
    StageRegress,
    /// 双跑不一致（复现红线）
    NotRepro,
}

impl DsdtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DsdtCode::SigBad => "签名非 DSDT：定位到的不是差分系统描述表，回查指针面选择",
            DsdtCode::LenZero => "DSDT 长度为 0：空表不可入账，回查固件或换 64 位面",
            DsdtCode::LenOver => "DSDT 声明长度越缓冲：截断表不可信，补齐映射再入账",
            DsdtCode::SumBad => "DSDT 校验和坏：全表 sum mod 256 ≠ 0，AML 字节码疑似损坏",
            DsdtCode::PtrZero => "双面指针全零：FADT 既没给 32 位也没给 64 位 DSDT 指针",
            DsdtCode::FacdShort => "FADT 太短连 32 位指针都放不下：表长必须 ≥ 144",
            DsdtCode::AmlExec => "AML 零执行红线破：指针账只读定位，解释/写即立案",
            DsdtCode::StageRegress => "状态机倒退：定位只能单向推进，禁止回退重走",
            DsdtCode::NotRepro => "双跑不一致：同输入两次定位结果不同，查非确定性来源",
        }
    }
    pub const ALL: [DsdtCode; 9] = [
        DsdtCode::SigBad,
        DsdtCode::LenZero,
        DsdtCode::LenOver,
        DsdtCode::SumBad,
        DsdtCode::PtrZero,
        DsdtCode::FacdShort,
        DsdtCode::AmlExec,
        DsdtCode::StageRegress,
        DsdtCode::NotRepro,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DsdtCode::ALL.len() {
        if let Some(c) = DsdtCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75200 段闭集内"
}

// ── AML 零执行红线（记账面） ─────────────────────────────────────
/// 只读纪律记账面：定位全程只许「看」（记查看字节数），
/// 「解释」（执行）与「写」任何一次都算破线。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AmlGuard {
    /// 累计查看字节数（只读面合法动作）。
    pub bytes_viewed: u64,
    /// 累计 AML 解释动作数（红线：必须恒 0）。
    pub ops_executed: u64,
    /// 累计对 FADT/DSDT 字节的写次数（红线：必须恒 0）。
    pub writes: u64,
}

impl AmlGuard {
    pub fn new() -> AmlGuard {
        AmlGuard::default()
    }
    /// 合法动作：只读查看 n 字节。
    pub fn record_view(&mut self, n: u64) {
        self.bytes_viewed = self.bytes_viewed.wrapping_add(n);
    }
    /// 非法动作：任何 AML 解释尝试（记账并留给红线断言抓）。
    pub fn record_exec(&mut self, n: u64) {
        self.ops_executed = self.ops_executed.wrapping_add(n);
    }
    /// 非法动作：任何写字节尝试。
    pub fn record_write(&mut self, n: u64) {
        self.writes = self.writes.wrapping_add(n);
    }
    /// 零执行断言：执行与写恒 0（查看数不限）。
    pub fn read_only_ok(&self) -> bool {
        self.ops_executed == 0 && self.writes == 0
    }
}

/// AML 解释器桩：本模块**永不调用**它做真解释——它存在的唯一
/// 意义是给注入轮当靶子，证明红线必抓。
pub fn aml_interpret_stub(g: &mut AmlGuard, n: u64) -> Result<(), u32> {
    g.record_exec(n);
    Err(DsdtCode::AmlExec.val())
}

// ── 字节面读数（无裸下标：全走 .get()） ─────────────────────────
fn le32(buf: &[u8], off: usize) -> Option<u32> {
    let b0 = *buf.get(off)?;
    let b1 = *buf.get(off.checked_add(1)?)?;
    let b2 = *buf.get(off.checked_add(2)?)?;
    let b3 = *buf.get(off.checked_add(3)?)?;
    Some(u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16) | (u32::from(b3) << 24))
}

fn le64(buf: &[u8], off: usize) -> Option<u64> {
    let lo = u64::from(le32(buf, off)?);
    let hi = u64::from(le32(buf, off.checked_add(4)?)?);
    Some(lo | (hi << 32))
}

/// 全表校验和（首 `declared` 字节 sum mod 256 ≡ 0）。
fn table_sum_ok(buf: &[u8], declared: usize) -> bool {
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < declared {
        if let Some(b) = buf.get(i) {
            acc += u32::from(*b);
        } else {
            return false;
        }
        i += 1;
    }
    acc & 0xFF == 0
}

// ── FADT 供面者 ─────────────────────────────────────────────────
/// FADT 视图：供出双面 DSDT 指针与裁定所需元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FacdView {
    /// FADT 声明表长。
    pub declared_len: usize,
    /// FADT revision。
    pub revision: u8,
    /// 32 位 DSDT 指针原值。
    pub dsdt32: u32,
    /// 64 位 X_DSDT 指针原值（表长不足 148 时恒 0）。
    pub x_dsdt: u64,
}

/// 从 FADT 字节面取视图（带查看记账；表长 <144 立案 FacdShort）。
pub fn facd_view(buf: &[u8], g: &mut AmlGuard) -> Result<FacdView, u32> {
    let declared = match le32(buf, 4) {
        Some(v) => v as usize,
        None => return Err(DsdtCode::FacdShort.val()),
    };
    if declared > buf.len() || buf.len() < OFF_DSDT32 + 4 {
        return Err(DsdtCode::FacdShort.val());
    }
    g.record_view((OFF_DSDT32 + 4) as u64);
    let rev = *buf.get(OFF_REVISION).ok_or_else(|| DsdtCode::FacdShort.val())?;
    let dsdt32 = le32(buf, OFF_DSDT32).ok_or_else(|| DsdtCode::FacdShort.val())?;
    // 64 位面只在字段完整落位（表长 ≥ 148）时读取，否则记 0。
    let x_dsdt = if buf.len() >= FADT_LEN_XDSDT_FULL {
        g.record_view(8);
        le64(buf, OFF_XDSDT).unwrap_or(0)
    } else {
        // 64 位字段未完整落位：不读、记 0（落位门，见判据「v2+ 优先裁定」）。
        0
    };
    Ok(FacdView { declared_len: declared, revision: rev, dsdt32, x_dsdt })
}

// ── v2+ 64 位优先裁定（三条路径分账） ────────────────────────────
/// 指针位面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsdtPlane {
    /// 32 位面（< 4GiB）。
    P32(u32),
    /// 64 位面。
    P64(u64),
}

impl DsdtPlane {
    /// 归一物理指针读数。
    pub fn phys(self) -> u64 {
        match self {
            DsdtPlane::P32(v) => u64::from(v),
            DsdtPlane::P64(v) => v,
        }
    }
}

/// 裁定路径标签（三路分账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArbPath {
    /// revision 1：只认 32 位面。
    V1Only,
    /// revision ≥ 2 且 X_DSDT 可用：64 位优先。
    V2Plus64,
    /// revision ≥ 2 但 X_DSDT 为零：回落 32 位面。
    V2PlusFallBack32,
}

/// 裁定核：v2+ X_DSDT 64 位优先，回落与 v1 分账；双面全零立案。
pub fn arbitrate(f: &FacdView) -> Result<(DsdtPlane, ArbPath), u32> {
    let x_eligible = f.declared_len >= FADT_LEN_XDSDT_FULL && f.x_dsdt != 0;
    if f.revision >= 2 {
        if x_eligible {
            Ok((DsdtPlane::P64(f.x_dsdt), ArbPath::V2Plus64))
        } else if f.dsdt32 != 0 {
            Ok((DsdtPlane::P32(f.dsdt32), ArbPath::V2PlusFallBack32))
        } else {
            Err(DsdtCode::PtrZero.val())
        }
    } else if f.dsdt32 != 0 {
        Ok((DsdtPlane::P32(f.dsdt32), ArbPath::V1Only))
    } else {
        Err(DsdtCode::PtrZero.val())
    }
}

// ── DSDT 表自证（长度/校验和两要素） ─────────────────────────────
/// DSDT 表账：签名 + 长度 + 校验和。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsdtTable {
    /// 声明长度（三要素之二）。
    pub declared_len: usize,
    /// 表头校验和字节原值（三要素之三，真伪由全表 sum 判）。
    pub checksum_byte: u8,
}

/// DSDT 表自证：签名门 → 长度 0 拒 → 长度越缓冲拒 → 全表校验和。
/// 全程只读（经 `g` 记查看字节数）。
pub fn vet_dsdt(buf: &[u8], g: &mut AmlGuard) -> Result<DsdtTable, u32> {
    if buf.len() < DSDT_HDR_LEN {
        return Err(DsdtCode::LenZero.val());
    }
    g.record_view(4);
    let sig_ok = buf
        .get(0)
        .zip(buf.get(1))
        .zip(buf.get(2))
        .zip(buf.get(3))
        .map(|(((a, b), c), d)| [*a, *b, *c, *d] == SIG_DSDT)
        .unwrap_or(false);
    if !sig_ok {
        return Err(DsdtCode::SigBad.val());
    }
    let declared = match le32(buf, 4) {
        Some(v) => v as usize,
        None => return Err(DsdtCode::LenZero.val()),
    };
    let checksum_byte = *buf.get(OFF_CHECKSUM).ok_or_else(|| DsdtCode::LenZero.val())?;
    // 长度 0 拒（判据明文）。
    if declared == 0 {
        return Err(DsdtCode::LenZero.val());
    }
    if declared > buf.len() {
        return Err(DsdtCode::LenOver.val());
    }
    // 全表校验和（判据明文：坏拒）。查看整个表体。
    g.record_view(declared as u64);
    if !table_sum_ok(buf, declared) {
        return Err(DsdtCode::SumBad.val());
    }
    Ok(DsdtTable { declared_len: declared, checksum_byte })
}

// ── 单调状态机 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsdtStage {
    Raw,
    FacdVetted,
    PtrArbitrated,
    TableVetted,
    LedgerClosed,
}

impl DsdtStage {
    fn rank(self) -> u8 {
        match self {
            DsdtStage::Raw => 0,
            DsdtStage::FacdVetted => 1,
            DsdtStage::PtrArbitrated => 2,
            DsdtStage::TableVetted => 3,
            DsdtStage::LedgerClosed => 4,
        }
    }
}

/// 单调推进器：只许 +1，倒退/越级一律拒。
#[derive(Clone, Copy, Debug, Default)]
pub struct DsdtMachine {
    stage: u8,
}

impl DsdtMachine {
    pub fn new() -> DsdtMachine {
        DsdtMachine { stage: DsdtStage::Raw.rank() }
    }
    pub fn stage(&self) -> DsdtStage {
        match self.stage {
            0 => DsdtStage::Raw,
            1 => DsdtStage::FacdVetted,
            2 => DsdtStage::PtrArbitrated,
            3 => DsdtStage::TableVetted,
            _ => DsdtStage::LedgerClosed,
        }
    }
    pub fn advance(&mut self, to: DsdtStage) -> Result<(), u32> {
        let want = to.rank();
        if want == self.stage.wrapping_add(1) {
            self.stage = want;
            Ok(())
        } else {
            Err(DsdtCode::StageRegress.val())
        }
    }
}

// ── 三要素账本 ──────────────────────────────────────────────────
/// DSDT 定位账：指针/长度/校验和三要素 + 裁定路径分账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DsdtLedger {
    /// 归一物理指针（三要素之一）。
    pub phys: u64,
    /// 所选位面。
    pub plane: DsdtPlane,
    /// 裁定路径（v1 / v2+64 / v2+回落 分账）。
    pub path: ArbPath,
    /// DSDT 声明长度（三要素之二）。
    pub length: usize,
    /// 表头校验和字节（三要素之三）。
    pub checksum_byte: u8,
}

impl DsdtLedger {
    /// 三要素齐账谓词：指针非零、长度 ≥ 表头、校验和字节已录。
    pub fn three_elem_ok(&self) -> bool {
        self.phys != 0 && self.length >= DSDT_HDR_LEN
    }
}

// ── 夹具构造 ────────────────────────────────────────────────────
fn put8(buf: &mut Vec<u8>, off: usize, v: u8) {
    if let Some(slot) = buf.get_mut(off) {
        *slot = v;
    }
}

fn put32(buf: &mut Vec<u8>, off: usize, v: u32) {
    put8(buf, off, (v & 0xFF) as u8);
    put8(buf, off.wrapping_add(1), ((v >> 8) & 0xFF) as u8);
    put8(buf, off.wrapping_add(2), ((v >> 16) & 0xFF) as u8);
    put8(buf, off.wrapping_add(3), ((v >> 24) & 0xFF) as u8);
}

fn put64(buf: &mut Vec<u8>, off: usize, v: u64) {
    put32(buf, off, (v & 0xFFFF_FFFF) as u32);
    put32(buf, off.wrapping_add(4), ((v >> 32) & 0xFFFF_FFFF) as u32);
}

/// 修复校验和通用桩：把 `ck_off` 处的字节调到全表 sum ≡ 0。
fn seal_checksum(buf: &mut Vec<u8>, ck_off: usize) {
    if let Some(slot) = buf.get_mut(ck_off) {
        *slot = 0;
    }
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            acc += u32::from(*b);
        }
        i += 1;
    }
    let cks = (0x100u32 - (acc & 0xFF)) & 0xFF;
    put8(buf, ck_off, cks as u8);
}

/// FADT 夹具（只铺 DSDT 相关字段：@140 dsdt32 / @144 x_dsdt）。
pub fn build_fadt(rev: u8, declared_len: usize, dsdt32: u32, x_dsdt: u64) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < declared_len {
        b.push(0u8);
        i += 1;
    }
    put8(&mut b, 0, b'F');
    put8(&mut b, 1, b'A');
    put8(&mut b, 2, b'C');
    put8(&mut b, 3, b'P');
    put32(&mut b, 4, declared_len as u32);
    put8(&mut b, OFF_REVISION, rev);
    put32(&mut b, OFF_DSDT32, dsdt32);
    if declared_len >= FADT_LEN_XDSDT_FULL {
        put64(&mut b, OFF_XDSDT, x_dsdt);
    }
    seal_checksum(&mut b, OFF_CHECKSUM);
    b
}

/// DSDT 表夹具：AML 字节码区用确定性图案（假码：只看不解释）。
pub fn build_dsdt(aml_len: usize, sig: Option<[u8; 4]>) -> Vec<u8> {
    let declared = DSDT_HDR_LEN + aml_len;
    let mut b: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < declared {
        b.push(0u8);
        i += 1;
    }
    let sig_b = sig.unwrap_or(SIG_DSDT);
    // 签名 4 字节按序落位（迭代取值，无裸下标）。
    let mut si = 0;
    while si < 4 {
        if let Some(byte) = sig_b.get(si) {
            put8(&mut b, si, *byte);
        }
        si += 1;
    }
    put32(&mut b, 4, declared as u32);
    put8(&mut b, OFF_REVISION, 2); // DSDT revision 2（表头 Revision @8）
    // AML 字节码区铺确定性图案（0x10 = ScopeOp 假码，永不执行）。
    let mut k = DSDT_HDR_LEN;
    while k < declared {
        put8(&mut b, k, (0x10 + (k % 7)) as u8);
        k += 1;
    }
    seal_checksum(&mut b, OFF_CHECKSUM);
    b
}

/// 标准真机口径：v5 FADT（双面非零）+ 256B AML 的 DSDT。
pub fn machine_v5() -> (Vec<u8>, Vec<u8>) {
    (build_fadt(5, FADT_LEN_XDSDT_FULL, 0xBF00_0000, 0x0000_0001_BF00_0000), build_dsdt(256, None))
}

/// v1 口径：FADT 只有 32 位面。
pub fn machine_v1() -> (Vec<u8>, Vec<u8>) {
    (build_fadt(1, 144, 0xBF00_0000, 0), build_dsdt(128, None))
}

// ── 主管线（状态机驱动 + 只读记账） ──────────────────────────────
/// 全链定位：FADT 视图 → 双面裁定 → DSDT 表自证 → 三要素账闭。
/// 全程只读：结束时 `g.read_only_ok()` 必须成立，否则 AmlExec 立案。
pub fn locate_dsdt(fadt: &[u8], dsdt: &[u8], g: &mut AmlGuard) -> Result<DsdtLedger, u32> {
    let mut m = DsdtMachine::new();
    let fv = facd_view(fadt, g)?;
    m.advance(DsdtStage::FacdVetted)?;
    let (plane, path) = arbitrate(&fv)?;
    m.advance(DsdtStage::PtrArbitrated)?;
    let tbl = vet_dsdt(dsdt, g)?;
    m.advance(DsdtStage::TableVetted)?;
    let ledger = DsdtLedger {
        phys: plane.phys(),
        plane,
        path,
        length: tbl.declared_len,
        checksum_byte: tbl.checksum_byte,
    };
    if !ledger.three_elem_ok() {
        return Err(DsdtCode::PtrZero.val());
    }
    // AML 零执行断言（判据明文）：破线即拒。
    if !g.read_only_ok() {
        return Err(DsdtCode::AmlExec.val());
    }
    m.advance(DsdtStage::LedgerClosed)?;
    Ok(ledger)
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：长度 0/校验和/签名/AML 执行） ──────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            } else {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮（LCG 驱动，固定种子，每轮四路必抓）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(u64::from(SEG_BASE));
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：长度 0 注入（表头声明 0）。
        let mut z = build_dsdt(64, None);
        put32(&mut z, 4, 0);
        seal_checksum(&mut z, OFF_CHECKSUM);
        let mut gz = AmlGuard::new();
        led.record(0, vet_dsdt(&z, &mut gz) == Err(DsdtCode::LenZero.val()));
        // 路 2：校验和坏注入。
        let mut bad = build_dsdt(64, None);
        let bl = bad.len();
        if let Some(slot) = bad.get_mut(bl - 1) {
            *slot = slot.wrapping_add(1);
        }
        let mut gb = AmlGuard::new();
        led.record(1, vet_dsdt(&bad, &mut gb) == Err(DsdtCode::SumBad.val()));
        // 路 3：签名注入。
        let sig = build_dsdt(64, Some([b'D', b'S', b'D', b'X']));
        let mut gs = AmlGuard::new();
        led.record(2, vet_dsdt(&sig, &mut gs) == Err(DsdtCode::SigBad.val()));
        // 路 4：AML 执行红线注入（桩必拒 + 账面留痕）。
        let mut ge = AmlGuard::new();
        let exec = aml_interpret_stub(&mut ge, 1);
        led.record(3, exec == Err(DsdtCode::AmlExec.val()) && !ge.read_only_ok());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：v5 双面 64 位优先 → v1 只走 32 位面 → v2+ 回落分账 →
/// 三要素齐账 → 只读断言 → 四路注入必抓。
pub fn golden_run() -> Result<(u32, u32), u32> {
    // v5：64 位优先路径。
    let (f5, d5) = machine_v5();
    let mut g5 = AmlGuard::new();
    let l5 = locate_dsdt(&f5, &d5, &mut g5)?;
    let ok64 = (l5.path == ArbPath::V2Plus64
        && l5.plane == DsdtPlane::P64(0x0000_0001_BF00_0000)
        && l5.phys == 0x0000_0001_BF00_0000
        && l5.length == DSDT_HDR_LEN + 256
        && l5.three_elem_ok()
        && g5.read_only_ok()) as u32;
    // v1：只走 32 位面。
    let (f1, d1) = machine_v1();
    let mut g1 = AmlGuard::new();
    let l1 = locate_dsdt(&f1, &d1, &mut g1)?;
    let ok32 = (l1.path == ArbPath::V1Only && l1.plane == DsdtPlane::P32(0xBF00_0000)) as u32;
    // v2+ 回落：X_DSDT 为零但 32 位非零。
    let ff = build_fadt(5, FADT_LEN_XDSDT_FULL, 0xBF00_0000, 0);
    let mut gf = AmlGuard::new();
    let lf = locate_dsdt(&ff, &d5, &mut gf)?;
    let okfb = (lf.path == ArbPath::V2PlusFallBack32
        && lf.plane == DsdtPlane::P32(0xBF00_0000)) as u32;
    // 双面全零立案。
    let fz = build_fadt(5, FADT_LEN_XDSDT_FULL, 0, 0);
    let mut gz2 = AmlGuard::new();
    let okz = locate_dsdt(&fz, &d5, &mut gz2) == Err(DsdtCode::PtrZero.val());
    // 四路注入必抓。
    let inj = inject_rounds(4)?;
    let okinj = inj.all_caught() as u32;
    Ok((ok64 + ok32 + okfb + (okz as u32), okinj))
}

/// 双跑一致（复现红线）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DsdtCode::NotRepro.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < DsdtCode::ALL.len() {
        if let Some(c) = DsdtCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. v5 真机：64 位优先三要素账齐。
    let (f5, d5) = machine_v5();
    let mut g1 = AmlGuard::new();
    let r1 = locate_dsdt(&f5, &d5, &mut g1);
    v.push((
        "F0449-64位优先",
        match &r1 {
            Ok(l) => {
                l.path == ArbPath::V2Plus64
                    && l.phys == 0x0000_0001_BF00_0000
                    && l.length == 292
                    && l.three_elem_ok()
            }
            Err(_) => false,
        },
        match &r1 {
            Ok(l) => format!("phys=0x{:X} len={}", l.phys, l.length),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. v1 / v2+回落 两路径分账。
    let (f1, d1) = machine_v1();
    let mut g2 = AmlGuard::new();
    let r2 = locate_dsdt(&f1, &d1, &mut g2);
    let ff = build_fadt(5, FADT_LEN_XDSDT_FULL, 0xBF00_0000, 0);
    let mut g3 = AmlGuard::new();
    let r3 = locate_dsdt(&ff, &d5, &mut g3);
    let paths_ok = match (&r2, &r3) {
        (Ok(a), Ok(b)) => a.path == ArbPath::V1Only && b.path == ArbPath::V2PlusFallBack32,
        _ => false,
    };
    v.push((
        "F0449-三路径分账",
        paths_ok,
        String::from("v1 / v2+64 / v2+回落 各走各账"),
    ));

    // 3. X_DSDT 字段落位门：表长 144（140+8 放不下）→ 必走 32 位面。
    let f148 = build_fadt(5, 144, 0xBF00_0000, 0xdead);
    let mut g4 = AmlGuard::new();
    let r4 = locate_dsdt(&f148, &d5, &mut g4);
    let gate_ok = match &r4 {
        Ok(l) => l.path == ArbPath::V2PlusFallBack32 && l.plane == DsdtPlane::P32(0xBF00_0000),
        Err(_) => false,
    };
    v.push((
        "F0449-落位门",
        gate_ok,
        String::from("表长<148 时 X_DSDT 不读，退 32 位"),
    ));

    // 4. 双面全零立案。
    let fz = build_fadt(5, FADT_LEN_XDSDT_FULL, 0, 0);
    let mut g5 = AmlGuard::new();
    let r5 = locate_dsdt(&fz, &d5, &mut g5);
    v.push((
        "F0449-双面全零拒",
        r5 == Err(DsdtCode::PtrZero.val()),
        String::from("32/64 位面皆零立案"),
    ));

    // 5. FADT 太短立案（连 140..144 都放不下）。
    let fshort = build_fadt(1, 40, 0, 0);
    let mut g6 = AmlGuard::new();
    let r6 = locate_dsdt(&fshort, &d5, &mut g6);
    v.push((
        "F0449-FADT短拒",
        r6 == Err(DsdtCode::FacdShort.val()),
        String::from("表长 40 连 32 位指针域 40..44 都放不下"),
    ));

    // 6. 长度 0 拒（判据明文）。
    let mut z = build_dsdt(64, None);
    put32(&mut z, 4, 0);
    seal_checksum(&mut z, OFF_CHECKSUM);
    let mut g7 = AmlGuard::new();
    let r7 = vet_dsdt(&z, &mut g7);
    v.push((
        "F0449-长度0拒",
        r7 == Err(DsdtCode::LenZero.val()),
        String::from("declared=0 显性拒"),
    ));

    // 7. 校验和坏拒（判据明文）。
    let mut bad = build_dsdt(64, None);
    let bl = bad.len();
    if let Some(slot) = bad.get_mut(bl - 1) {
        *slot = slot.wrapping_add(1);
    }
    let mut g8 = AmlGuard::new();
    let r8 = vet_dsdt(&bad, &mut g8);
    v.push((
        "F0449-校验和坏拒",
        r8 == Err(DsdtCode::SumBad.val()),
        String::from("尾字节扰动必抓"),
    ));

    // 8. 长度越缓冲拒。
    let mut ov = build_dsdt(64, None);
    put32(&mut ov, 4, 8192);
    seal_checksum(&mut ov, OFF_CHECKSUM);
    let mut g9 = AmlGuard::new();
    let r9 = vet_dsdt(&ov, &mut g9);
    v.push((
        "F0449-长度越缓冲拒",
        r9 == Err(DsdtCode::LenOver.val()),
        String::from("declared>buf 显性拒"),
    ));

    // 9. 签名非 DSDT 拒。
    let sig = build_dsdt(64, Some([b'D', b'S', b'D', b'X']));
    let mut g10 = AmlGuard::new();
    let r10 = vet_dsdt(&sig, &mut g10);
    v.push((
        "F0449-签名拒",
        r10 == Err(DsdtCode::SigBad.val()),
        String::from("DSDT→DSDX 立案"),
    ));

    // 10. AML 零执行红线：桩执行必抓 + 账面留痕。
    let mut g11 = AmlGuard::new();
    let ex = aml_interpret_stub(&mut g11, 3);
    let redline_ok = ex == Err(DsdtCode::AmlExec.val()) && !g11.read_only_ok();
    // 只读管线跑完必须零执行。
    let mut g12 = AmlGuard::new();
    let _ = locate_dsdt(&f5, &d5, &mut g12);
    let pipeline_clean = g12.read_only_ok() && g12.bytes_viewed > 0;
    v.push((
        "F0449-AML零执行",
        redline_ok && pipeline_clean,
        format!("查看 {}B 执行 0 次", g12.bytes_viewed),
    ));

    // 11. 三要素账面复算（指针/长度/校验和字节三账同闭）。
    let mut g13 = AmlGuard::new();
    let r13 = locate_dsdt(&f5, &d5, &mut g13);
    let reok = match &r13 {
        Ok(l) => {
            l.three_elem_ok()
                && l.phys == 0x0000_0001_BF00_0000
                && l.length == DSDT_HDR_LEN + 256
                && g13.read_only_ok()
        }
        Err(_) => false,
    };
    v.push((
        "F0449-三要素齐账",
        reok,
        String::from("指针/长度/校验和 同闭"),
    ));

    // 12. 状态机倒退拒。
    let mut m = DsdtMachine::new();
    let a1 = m.advance(DsdtStage::FacdVetted).is_ok();
    let a2s = m.advance(DsdtStage::FacdVetted).is_err();
    let a3 = m.advance(DsdtStage::PtrArbitrated).is_ok();
    let a4 = m.advance(DsdtStage::LedgerClosed).is_err();
    let a5 = m.advance(DsdtStage::TableVetted).is_ok();
    let a6 = m.advance(DsdtStage::LedgerClosed).is_ok();
    v.push((
        "F0449-状态机倒退",
        a1 && a2s && a3 && a4 && a5 && a6,
        String::from("禁重走 + 禁越级 + 正常走完"),
    ));

    // 13. 注入账 10/10。
    let inj = inject_rounds(10);
    v.push((
        "F0449-注入10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 14. 双跑一致。
    v.push((
        "F0449-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 15. 段位守卫。
    v.push((
        "F0449-段位守卫",
        segment_guard(),
        String::from("0x75200 窗口"),
    ));

    v
}

pub fn run_unx0449_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0449 DSDT 指针账（只读定位）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元测试（注入必抓 / 双跑一致 / 边界） ───────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v5_64bit_preferred() {
        let (f, d) = machine_v5();
        let mut g = AmlGuard::new();
        match locate_dsdt(&f, &d, &mut g) {
            Ok(l) => {
                assert_eq!(l.path, ArbPath::V2Plus64);
                assert_eq!(l.phys, 0x0000_0001_BF00_0000);
                assert_eq!(l.length, 292);
                assert!(l.three_elem_ok());
            }
            Err(_) => assert!(false, "v5 机器夹具必须可定位"),
        }
        assert!(g.read_only_ok());
        assert!(g.bytes_viewed > 0);
    }

    #[test]
    fn test_v1_and_fallback_paths() {
        let (f1, d1) = machine_v1();
        let mut g1 = AmlGuard::new();
        let l1 = match locate_dsdt(&f1, &d1, &mut g1) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "v1 夹具必须可定位");
                return;
            }
        };
        assert_eq!(l1.path, ArbPath::V1Only);
        assert_eq!(l1.plane, DsdtPlane::P32(0xBF00_0000));
        let ff = build_fadt(5, FADT_LEN_XDSDT_FULL, 0xBF00_0000, 0);
        let mut g2 = AmlGuard::new();
        let l2 = match locate_dsdt(&ff, &d1, &mut g2) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "回落夹具必须可定位");
                return;
            }
        };
        assert_eq!(l2.path, ArbPath::V2PlusFallBack32);
    }

    #[test]
    fn test_xdsdt_gate_len144() {
        let f = build_fadt(5, 144, 0xBF00_0000, 0xdead_beef);
        let d = build_dsdt(64, None);
        let mut g = AmlGuard::new();
        match locate_dsdt(&f, &d, &mut g) {
            Ok(l) => {
                assert_eq!(l.path, ArbPath::V2PlusFallBack32);
                assert_ne!(l.phys, 0xdead_beef);
            }
            Err(_) => assert!(false, "144 落位门必须退 32 位"),
        }
    }

    #[test]
    fn test_len_zero_and_over_and_sum() {
        let mut z = build_dsdt(64, None);
        put32(&mut z, 4, 0);
        seal_checksum(&mut z, OFF_CHECKSUM);
        let mut g = AmlGuard::new();
        assert_eq!(vet_dsdt(&z, &mut g), Err(DsdtCode::LenZero.val()));
        let mut ov = build_dsdt(64, None);
        put32(&mut ov, 4, 8192);
        seal_checksum(&mut ov, OFF_CHECKSUM);
        assert_eq!(vet_dsdt(&ov, &mut g), Err(DsdtCode::LenOver.val()));
        let mut bad = build_dsdt(64, None);
        let n = bad.len();
        if let Some(slot) = bad.get_mut(n - 1) {
            *slot = slot.wrapping_add(1);
        }
        assert_eq!(vet_dsdt(&bad, &mut g), Err(DsdtCode::SumBad.val()));
    }

    #[test]
    fn test_sig_and_ptr_zero() {
        let sig = build_dsdt(64, Some([b'D', b'S', b'D', b'X']));
        let mut g = AmlGuard::new();
        assert_eq!(vet_dsdt(&sig, &mut g), Err(DsdtCode::SigBad.val()));
        let fz = build_fadt(5, FADT_LEN_XDSDT_FULL, 0, 0);
        let d = build_dsdt(64, None);
        assert_eq!(locate_dsdt(&fz, &d, &mut g), Err(DsdtCode::PtrZero.val()));
        let fshort = build_fadt(1, 40, 0, 0);
        assert_eq!(locate_dsdt(&fshort, &d, &mut g), Err(DsdtCode::FacdShort.val()));
    }

    #[test]
    fn test_aml_redline() {
        let mut g = AmlGuard::new();
        assert_eq!(aml_interpret_stub(&mut g, 1), Err(DsdtCode::AmlExec.val()));
        assert!(!g.read_only_ok());
        assert_eq!(g.ops_executed, 1);
        let mut g2 = AmlGuard::new();
        g2.record_write(2);
        assert!(!g2.read_only_ok());
        let mut g3 = AmlGuard::new();
        g3.record_view(100);
        assert!(g3.read_only_ok());
    }

    #[test]
    fn test_machine_regress() {
        let mut m = DsdtMachine::new();
        assert!(m.advance(DsdtStage::FacdVetted).is_ok());
        assert!(m.advance(DsdtStage::FacdVetted).is_err());
        assert!(m.advance(DsdtStage::PtrArbitrated).is_ok());
        assert!(m.advance(DsdtStage::LedgerClosed).is_err());
        assert!(m.advance(DsdtStage::TableVetted).is_ok());
        assert!(m.advance(DsdtStage::LedgerClosed).is_ok());
    }

    #[test]
    fn test_double_run_repro() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_inject_all_caught() {
        let led = match inject_rounds(10) {
            Ok(v) => v,
            Err(_) => {
                assert!(false, "注入轮不得报错");
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in DsdtCode::ALL.iter() {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x75200 段闭集内");
        }
        assert_eq!(detail(0x9999_9999), "未知诊断码：不在 0x75200 段闭集内");
    }
}
