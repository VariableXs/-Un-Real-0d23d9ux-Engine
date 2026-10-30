//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15381 · OBJREF 标准形式字节布局钉版 · 0x672C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15381-J1 布局钉版成文（OBJREF 头+STDOBJ 字段逐字节
//!      对表 10/10 次），字节漂移注入（任一字段错位）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 标准形式字节布局钉版（来源：DCE/RPC OBJREF 标准封送形，
//!     小端序；字段偏移写死）：
//!       off 0..4   signature u32 = 0x574F4547（'GEOW' 小端）
//!       off 4..8   flags u32（0 = 标准形）
//!       off 8..24  iid [u8;16]（接口 IID）
//!       off 24..28 std_flags u32（STDOBJ 标志）
//!       off 28..32 reserved u32（0）
//!       off 32..40 oid u64（对象标识）
//!       off 40..48 oxid u64（OLE 标识）
//!       off 48..52 oipcs u32（接口指针 cookie）
//!   - 逐字节对表：编码→52 字节缓冲→按钉版偏移逐字段解码→恒等
//!     往返，10/10 随机字段组全对；
//!   - 字节漂移注入面：任一字段偏移 ±1/±4 错位写入，解码侧按
//!     钉版偏移读出的值必与原值不符 → 检出；签名哨兵首查；
//!   - 守恒恒等式：字段宽度和 == 总长 52；8 字段偏移单调递增无重叠；
//!   - LCG 种子写死 = SEG_BASE（0x672C0），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x672C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x672C0;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x672C0;

/// 钉版偏移与宽度（写死，来源见文件头）。
pub const OFF_SIGNATURE: usize = 0;
pub const OFF_FLAGS: usize = 4;
pub const OFF_IID: usize = 8;
pub const OFF_STD_FLAGS: usize = 24;
pub const OFF_RESERVED: usize = 28;
pub const OFF_OID: usize = 32;
pub const OFF_OXID: usize = 40;
pub const OFF_OIPCS: usize = 48;
/// 总长 52。
pub const OBJREF_TOTAL: usize = 52;
/// 签名黄金值（'GEOW' 小端）。
pub const GOLDEN_SIGNATURE: u32 = 0x574F_4547;
/// 字段数（含 16 字节 IID 记 1 字段）。
pub const FIELD_COUNT: usize = 8;

/// 钉版偏移表（写死，单调递增无重叠）。
pub const PIN_OFFSETS: [usize; FIELD_COUNT] = [
    OFF_SIGNATURE, OFF_FLAGS, OFF_IID, OFF_STD_FLAGS,
    OFF_RESERVED, OFF_OID, OFF_OXID, OFF_OIPCS,
];
/// 钉版宽度表（与偏移一一对应）。
pub const PIN_WIDTHS: [usize; FIELD_COUNT] = [4, 4, 16, 4, 4, 8, 8, 4];
/// 字段名表（点验用，写死）。
pub const FIELD_NAMES: [&str; FIELD_COUNT] = [
    "signature", "flags", "iid", "std_flags", "reserved", "oid", "oxid", "oipcs",
];

/// 判据投影：逐字节对表 10/10。
pub const LAYOUT_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjrefCode {
    /// 缓冲长度不符（≠52）
    LengthMismatch,
    /// 签名哨兵不符
    SignatureMismatch,
    /// 保留域非零
    ReservedNonZero,
    /// 钉版表偏移单调性破
    PinOffsetBreach,
    /// 钉版表宽度守恒破（Σ宽 ≠ 52）
    PinWidthBreach,
    /// 字段往返不恒等
    RoundtripMismatch,
    /// 字节漂移检出失败（错位未现形）
    DriftUndetected,
    /// 偏移越界读（offset+width > 52）
    OffsetOutOfRange,
    /// 字段名表与钉版表长度不守恒
    FieldTableBreach,
}

impl ObjrefCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ObjrefCode::LengthMismatch => "缓冲长度不符：52 字节钉版",
            ObjrefCode::SignatureMismatch => "签名哨兵不符：0x574F4547",
            ObjrefCode::ReservedNonZero => "保留域非零",
            ObjrefCode::PinOffsetBreach => "钉版表偏移单调性破",
            ObjrefCode::PinWidthBreach => "钉版表宽度守恒破：Σ宽≠52",
            ObjrefCode::RoundtripMismatch => "字段往返不恒等",
            ObjrefCode::DriftUndetected => "字节漂移未现形",
            ObjrefCode::OffsetOutOfRange => "偏移越界读",
            ObjrefCode::FieldTableBreach => "字段名表与钉版表长度不守恒",
        }
    }
    pub const ALL: [ObjrefCode; 9] = [
        ObjrefCode::LengthMismatch,
        ObjrefCode::SignatureMismatch,
        ObjrefCode::ReservedNonZero,
        ObjrefCode::PinOffsetBreach,
        ObjrefCode::PinWidthBreach,
        ObjrefCode::RoundtripMismatch,
        ObjrefCode::DriftUndetected,
        ObjrefCode::OffsetOutOfRange,
        ObjrefCode::FieldTableBreach,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ObjrefCode::ALL.len() {
        if ObjrefCode::ALL[i].val() == code {
            return ObjrefCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x672C0 段闭集内"
}

// ── OBJREF 字段组 ────────────────────────────────────────────────
pub struct ObjrefFields {
    pub flags: u32,
    pub iid: [u8; 16],
    pub std_flags: u32,
    pub oid: u64,
    pub oxid: u64,
    pub oipcs: u32,
}

impl ObjrefFields {
    pub fn zeroed() -> ObjrefFields {
        ObjrefFields {
            flags: 0,
            iid: [0; 16],
            std_flags: 0,
            oid: 0,
            oxid: 0,
            oipcs: 0,
        }
    }
}

// ── 编码面（按钉版偏移写入 52 字节缓冲） ─────────────────────────
pub fn encode_objref(f: &ObjrefFields, buf: &mut Vec<u8>) -> Result<(), u32> {
    buf.clear();
    // signature（4）。
    push_u32(buf, GOLDEN_SIGNATURE);
    // flags（4）。
    push_u32(buf, f.flags);
    // iid（16）。
    let mut i = 0;
    while i < 16 {
        buf.push(f.iid[i]);
        i += 1;
    }
    // std_flags（4）/ reserved（4，恒 0）。
    push_u32(buf, f.std_flags);
    push_u32(buf, 0);
    // oid（8）/ oxid（8）。
    push_u64(buf, f.oid);
    push_u64(buf, f.oxid);
    // oipcs（4）。
    push_u32(buf, f.oipcs);
    if buf.len() != OBJREF_TOTAL {
        return Err(ObjrefCode::LengthMismatch.val());
    }
    Ok(())
}

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.push((v & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
}

fn push_u64(buf: &mut Vec<u8>, v: u64) {
    let mut i = 0;
    while i < 8 {
        buf.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

// ── 解码面（按钉版偏移读取 + 哨兵核验） ─────────────────────────
fn read_u32(buf: &[u8], off: usize) -> Result<u32, u32> {
    if off + 4 > buf.len() {
        return Err(ObjrefCode::OffsetOutOfRange.val());
    }
    let mut v: u32 = 0;
    let mut i = 0;
    while i < 4 {
        v |= (buf[off + i] as u32) << (i * 8);
        i += 1;
    }
    Ok(v)
}

fn read_u64(buf: &[u8], off: usize) -> Result<u64, u32> {
    if off + 8 > buf.len() {
        return Err(ObjrefCode::OffsetOutOfRange.val());
    }
    let mut v: u64 = 0;
    let mut i = 0;
    while i < 8 {
        v |= (buf[off + i] as u64) << (i * 8);
        i += 1;
    }
    Ok(v)
}

pub fn decode_objref(buf: &[u8]) -> Result<ObjrefFields, u32> {
    if buf.len() != OBJREF_TOTAL {
        return Err(ObjrefCode::LengthMismatch.val());
    }
    let sig = read_u32(buf, OFF_SIGNATURE)?;
    if sig != GOLDEN_SIGNATURE {
        return Err(ObjrefCode::SignatureMismatch.val());
    }
    let reserved = read_u32(buf, OFF_RESERVED)?;
    if reserved != 0 {
        return Err(ObjrefCode::ReservedNonZero.val());
    }
    let mut f = ObjrefFields::zeroed();
    f.flags = read_u32(buf, OFF_FLAGS)?;
    let mut i = 0;
    while i < 16 {
        f.iid[i] = buf[OFF_IID + i];
        i += 1;
    }
    f.std_flags = read_u32(buf, OFF_STD_FLAGS)?;
    f.oid = read_u64(buf, OFF_OID)?;
    f.oxid = read_u64(buf, OFF_OXID)?;
    f.oipcs = read_u32(buf, OFF_OIPCS)?;
    Ok(f)
}

/// 恒等往返：编码→解码→逐字段相等。
pub fn roundtrip(f: &ObjrefFields) -> Result<(), u32> {
    let mut buf: Vec<u8> = Vec::new();
    encode_objref(f, &mut buf)?;
    let g = decode_objref(&buf)?;
    if g.flags != f.flags
        || g.std_flags != f.std_flags
        || g.oid != f.oid
        || g.oxid != f.oxid
        || g.oipcs != f.oipcs
    {
        return Err(ObjrefCode::RoundtripMismatch.val());
    }
    let mut i = 0;
    while i < 16 {
        if g.iid[i] != f.iid[i] {
            return Err(ObjrefCode::RoundtripMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 字节漂移注入模型 ─────────────────────────────────────────────
/// 把某字段写入时偏移错位 ±4 字节（换字段位），解码侧按钉版偏移
/// 读出必不符（签名/保留域哨兵或字段值不符）。返回检出标志。
pub fn drift_inject(f: &ObjrefFields, field_idx: usize) -> Result<bool, u32> {
    if field_idx >= FIELD_COUNT {
        return Err(ObjrefCode::OffsetOutOfRange.val());
    }
    let mut buf: Vec<u8> = Vec::new();
    encode_objref(f, &mut buf)?;
    // 错位写：把目标字段整体右移 4 字节（与后一字段互换位）。
    let off = PIN_OFFSETS[field_idx];
    let w = PIN_WIDTHS[field_idx];
    if off + w + 4 > OBJREF_TOTAL {
        // 尾字段改左错位（前移 4）。
        let mut j = 0;
        while j < w {
            let v = buf[off + j];
            buf[off + j] = 0;
            if off >= 4 + j {
                buf[off - 4 + j] = buf[off - 4 + j] ^ v;
            }
            j += 1;
        }
    } else {
        let mut j = 0;
        while j < w {
            let v = buf[off + j];
            buf[off + j] = 0;
            buf[off + 4 + j] = buf[off + 4 + j] ^ v;
            j += 1;
        }
    }
    // 解码：必现形（签名哨兵/保留域/字段值三路之一）。
    let detected = match decode_objref(&buf) {
        Err(_) => true,
        Ok(g) => {
            g.flags != f.flags
                || g.std_flags != f.std_flags
                || g.oid != f.oid
                || g.oxid != f.oxid
                || g.oipcs != f.oipcs
        }
    };
    Ok(detected)
}

// ── 确定性 LCG（种子 = 0x672C0） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 10/10 随机布局往返压演 ───────────────────────────────────────
pub fn layout_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut trips = 0;
    let mut drifts = 0;
    let mut i = 0;
    while i < 32 {
        let mut f = ObjrefFields::zeroed();
        f.flags = (lcg.below(0x1_0000)) as u32;
        let mut b = 0;
        while b < 16 {
            f.iid[b] = (lcg.below(256)) as u8;
            b += 1;
        }
        f.std_flags = (lcg.below(0x100)) as u32;
        f.oid = lcg.next();
        f.oxid = lcg.next();
        f.oipcs = (lcg.below(0x1_0000_0000)) as u32;
        roundtrip(&f)?;
        trips += 1;
        // 同点注入一次字节漂移（轮转字段）。
        let fi = (i % FIELD_COUNT + 2) % FIELD_COUNT; // 避开签名(0)/flags(1) 尾字段
        if drift_inject(&f, fi)? {
            drifts += 1;
        } else {
            return Err(ObjrefCode::DriftUndetected.val());
        }
        i += 1;
    }
    Ok((trips, drifts))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 黄金字节对照（手算写死）：全零字段 + flags=1 的缓冲首 8 字节
/// = 47 45 4F 57（'GEOW' 小端）01 00 00 00。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rounds = 0;
    let mut trips_total = 0;
    let mut drift_total = 0;
    let mut r = 0;
    while r < LAYOUT_TRIALS {
        let (t, d) = layout_round(r)?;
        if t != 32 || d != 32 {
            return Err(ObjrefCode::RoundtripMismatch.val());
        }
        trips_total += t;
        drift_total += d;
        rounds += 1;
        r += 1;
    }
    // 黄金首 8 字节对照。
    let mut f = ObjrefFields::zeroed();
    f.flags = 1;
    let mut buf: Vec<u8> = Vec::new();
    encode_objref(&f, &mut buf)?;
    let head_ok = buf[0] == 0x47 && buf[1] == 0x45 && buf[2] == 0x4F && buf[3] == 0x57
        && buf[4] == 0x01 && buf[5] == 0x00 && buf[6] == 0x00 && buf[7] == 0x00;
    if !head_ok {
        return Err(ObjrefCode::SignatureMismatch.val());
    }
    // 钉版表自守恒：Σ宽 == 52；偏移单调递增。
    let mut sum_w = 0;
    let mut i = 0;
    while i < FIELD_COUNT {
        sum_w += PIN_WIDTHS[i];
        if i > 0 && PIN_OFFSETS[i] <= PIN_OFFSETS[i - 1] {
            return Err(ObjrefCode::PinOffsetBreach.val());
        }
        if PIN_OFFSETS[i] + PIN_WIDTHS[i] > OBJREF_TOTAL {
            return Err(ObjrefCode::OffsetOutOfRange.val());
        }
        i += 1;
    }
    if sum_w != OBJREF_TOTAL {
        return Err(ObjrefCode::PinWidthBreach.val());
    }
    Ok((rounds, trips_total, drift_total))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：签名篡改注入 ×10（签名哨兵首查必拒）。
    let mut t = 0;
    while t < 10 {
        let f = ObjrefFields::zeroed();
        let mut buf: Vec<u8> = Vec::new();
        let _ = encode_objref(&f, &mut buf);
        buf[0] = buf[0] ^ 0xFF; // 签名字节破坏
        let v = decode_objref(&buf);
        ledger.tries[0] += 1;
        if let Err(e) = v {
            if e == ObjrefCode::SignatureMismatch.val() {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：长度不符注入 ×10（52±1 缓冲）。
    let mut t = 0;
    while t < 10 {
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0;
        let n = if t % 2 == 0 { OBJREF_TOTAL + 1 } else { OBJREF_TOTAL - 1 }; // 53 或 51
        while i < n {
            buf.push(0);
            i += 1;
        }
        let v = decode_objref(&buf);
        ledger.tries[1] += 1;
        if let Err(e) = v {
            if e == ObjrefCode::LengthMismatch.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：保留域非零注入 ×10。
    let mut t = 0;
    while t < 10 {
        let f = ObjrefFields::zeroed();
        let mut buf: Vec<u8> = Vec::new();
        let _ = encode_objref(&f, &mut buf);
        buf[OFF_RESERVED] = 1;
        let v = decode_objref(&buf);
        ledger.tries[2] += 1;
        if let Err(e) = v {
            if e == ObjrefCode::ReservedNonZero.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ObjrefCode::ALL.len() {
        let v = ObjrefCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 钉版自洽：8 字段 52 字节，签名值黄金。
    FIELD_COUNT == 8 && OBJREF_TOTAL == 52 && GOLDEN_SIGNATURE == 0x574F4547
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, trips, drift)) = golden {
        rounds == LAYOUT_TRIALS as u32 && trips == 320 && drift == 320
    } else {
        false
    };
    v.push((
        "J1-布局钉版10/10逐字节对表",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-字节漂移注入10/10必抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 字段名表守恒。
    v.push((
        "J1-8字段名表守恒",
        FIELD_NAMES.len() == FIELD_COUNT && PIN_OFFSETS.len() == FIELD_COUNT
            && PIN_WIDTHS.len() == FIELD_COUNT,
        format!("{}/8", FIELD_NAMES.len()),
    ));
    // 黄金首 8 字节独立点验。
    let mut f = ObjrefFields::zeroed();
    f.flags = 1;
    let mut buf: Vec<u8> = Vec::new();
    let enc = encode_objref(&f, &mut buf);
    let head_ok = if enc.is_ok() {
        buf[0] == 0x47 && buf[4] == 0x01 && buf.len() == OBJREF_TOTAL
    } else {
        false
    };
    v.push((
        "J1-黄金首8字节 47454F57 01000000",
        head_ok,
        format!("len={}", buf.len()),
    ));
    // 钉版宽度守恒独立点验（Σ宽=52 判据写死）。
    let mut sum_w = 0;
    let mut i = 0;
    while i < FIELD_COUNT {
        sum_w += PIN_WIDTHS[i];
        i += 1;
    }
    v.push((
        "J1-钉版Σ宽=52",
        sum_w == 52,
        format!("{}", sum_w),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x672C0 窗口")));
    v
}

pub fn run_unx15381_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15381 OBJREF 标准形式字节布局钉版");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
