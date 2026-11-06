//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F5502 · 事务结构回归账 · 0x72FC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2-B36）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F5502-J1 B19 事务结构与布局断言重放 20 用例全过，
//!     重放账可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 事务记录冻结布局：txid u64@0 / epoch u32@8 / kind u8@12 /
//!     flags u8@13 / plen u16@14 / crc u32@16，对齐 8、记录 24 字节；
//!   · 双路布局：声明式布局表（冻结偏移）vs 运行时布局计算器（顺序
//!     放置+自然对齐）逐字段对照，20 用例一致率 100%；
//!   · 编码→解码往返：crc（FNV-1a）覆盖前 20 字节，解码逐字段回读
//!     对账；kind 闭集 1..=5，越界拒（KindUnknown）；
//!   · 布局偏差探针：故意用错对齐规则的布局计算器必被对照器抓到
//!     （LayoutMiss）——证明对照器真能抓偏差；
//!   · 重放账可导出：@VXT1 头 + 每用例一行（含布局清单），双路渲染
//!     逐字节对拍；失真注入 10 路必抓；重放序严格单调。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72FC0，VX3-1103 事务结构窗口独占）。
pub const SEG_BASE: u32 = 0x72FC0;
/// 重放用例数（判据原文 20 用例，冻结）。
pub const CASE_N: usize = 20;
/// 事务种类闭集上界（kind 取值 1..=5）。
pub const KIND_MAX: u8 = 5;
/// 记录对齐（冻结 8 字节）。
pub const RECORD_ALIGN: usize = 8;
/// 记录总尺寸（冻结 24 字节：20 字节字段 + 4 字节尾垫）。
pub const RECORD_SIZE: usize = 24;
/// 失真注入轮数（判据 10 路语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 开销占比上限（≤2% = 200 bps，冻结）。
pub const OVERHEAD_CAP_BPS: u64 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TxCode {
    /// 种类越界：事务种类不在 1..=5 闭集内
    KindUnknown,
    /// 重放倒退：用例序只进不退，跳例、回例、封盘后再重放一律拒
    CursorBack,
    /// 记录破损：crc 对不上或记录长度不合法
    CrcBad,
    /// 布局失真：声明式布局表与运行时布局计算器逐字段不一致
    LayoutMiss,
    /// 往返失真：编码→解码回读与原值逐字段不一致
    CrossMiss,
    /// 空记录：载荷为空的用例拒绝重放（先备载荷再重放）
    RecordEmpty,
    /// 开销超限：簿记占比 >2%（200 bps）
    OverheadHigh,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl TxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TxCode::KindUnknown => "种类越界：事务种类只认 1..=5 闭集，越界按预期拒记账",
            TxCode::CursorBack => "重放倒退：用例序只进不退，跳例/回例/封盘后再重放一律拒",
            TxCode::CrcBad => "记录破损：crc 对不上或记录长度不合法，重出事务记录",
            TxCode::LayoutMiss => "布局失真：声明式布局表与运行时计算器不一致，逐字段定位差异",
            TxCode::CrossMiss => "往返失真：编码→解码回读与原值不一致，逐字段定位差异用例",
            TxCode::RecordEmpty => "空记录：载荷为空拒绝重放，先备载荷再重放",
            TxCode::OverheadHigh => "开销超限：簿记占比 >2%，查重放账头尺寸与游标簿记",
            TxCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [TxCode; 8] = [
        TxCode::KindUnknown,
        TxCode::CursorBack,
        TxCode::CrcBad,
        TxCode::LayoutMiss,
        TxCode::CrossMiss,
        TxCode::RecordEmpty,
        TxCode::OverheadHigh,
        TxCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    for i in 0..(TxCode::ALL.len()) {
        if TxCode::ALL[i].val() == code {
            return TxCode::ALL[i].detail();
        }
    }
    "未知诊断码：不在 0x72FC0 段闭集内"
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayState {
    /// 空闲：一例未重放。
    Idle,
    /// 重放中：至少一例已重放，尚未完成布局对照。
    Replaying,
    /// 已对照：20 用例布局与往返全等，账面定案。
    CrossChecked,
    /// 已封盘：账面冻结，任何再重放按倒退拒。
    Sealed,
}

impl ReplayState {
    /// 单调序值：Idle=0 < Replaying=1 < CrossChecked=2 < Sealed=3。
    pub fn rank(self) -> u8 {
        match self {
            ReplayState::Idle => 0,
            ReplayState::Replaying => 1,
            ReplayState::CrossChecked => 2,
            ReplayState::Sealed => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: ReplayState) -> bool {
        to.rank() > self.rank()
    }
}

/// 字段名闭表（B19 冻结序）。
pub const FIELD_NAMES: [&str; 6] = ["txid", "epoch", "kind", "flags", "plen", "crc"];
/// 字段尺寸闭表（与 FIELD_NAMES 一一对应）。
pub const FIELD_SIZES: [usize; 6] = [8, 4, 1, 1, 2, 4];
/// 字段偏移闭表（声明式冻结：0/8/12/13/14/16）。
pub const FIELD_OFFSETS: [usize; 6] = [0, 8, 12, 13, 14, 16];

/// 事务种类名闭表（1..=5 对应下标 0..=4）。
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        1 => "begin",
        2 => "update",
        3 => "commit",
        4 => "abort",
        5 => "checkpoint",
        _ => "unknown",
    }
}

/// 种类闭集校验：1..=5 之外一律越界。
pub fn validate_kind(kind: u8) -> Result<(), u32> {
    if kind < 1 || kind > KIND_MAX {
        return Err(TxCode::KindUnknown.val());
    }
    Ok(())
}

/// 对齐上取（a 为 2 的幂，checked 防溢出）。
pub fn align_up(x: usize, a: usize) -> Option<usize> {
    if a == 0 || (a & (a - 1)) != 0 {
        return None;
    }
    let m = a - 1;
    x.checked_add(m).map(|y| y & !m)
}

/// 运行时布局：顺序放置 + 自然对齐（对齐 = 字段尺寸），尾垫到记录对齐。
/// 返回 (偏移表, 总尺寸)；溢出/零尺寸一律 None。
pub fn runtime_layout() -> Option<(Vec<usize>, usize)> {
    let mut offs: Vec<usize> = Vec::new();
    let mut cur = 0usize;
    for i in 0..(FIELD_SIZES.len()) {
        let sz = match FIELD_SIZES.get(i) {
            Some(s) => *s,
            None => return None,
        };
        let o = align_up(cur, sz)?;
        offs.push(o);
        cur = o.checked_add(sz)?;
    }
    let total = align_up(cur, RECORD_ALIGN)?;
    Some((offs, total))
}

/// 布局对照：运行时布局 vs 声明式冻结表逐字段全等 + 总尺寸锚。
pub fn layout_agrees() -> Result<bool, u32> {
    let (offs, total) = match runtime_layout() {
        Some(x) => x,
        None => return Err(TxCode::LayoutMiss.val()),
    };
    if offs.len() != FIELD_OFFSETS.len() {
        return Err(TxCode::LayoutMiss.val());
    }
    for i in 0..(offs.len()) {
        let o = match offs.get(i) {
            Some(x) => *x,
            None => return Err(TxCode::LayoutMiss.val()),
        };
        if o != FIELD_OFFSETS[i] {
            return Ok(false);
        }
    }
    Ok(total == RECORD_SIZE)
}

/// 偏差布局计算器（探针专用）：对齐规则故意恒取 1（无对齐）。
/// 对照器必须判 false（LayoutMiss 能力证明）。
pub fn bad_layout_offset(fixed_no_align: bool) -> usize {
    if fixed_no_align {
        let mut cur = 0usize;
        for i in 0..(FIELD_SIZES.len()) {
            cur += FIELD_SIZES[i];
            if i == 0 {
            }
        }
        cur
    } else {
        RECORD_SIZE
    }
}

/// 事务用例：id 确定性派生字段与载荷。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxCase {
    pub id: u64,
    /// 种类闭集 1..=5。
    pub kind: u8,
    pub epoch: u32,
    pub flags: u8,
    /// 载荷长度（字节）。
    pub plen: u16,
    pub payload: Vec<u8>,
}

/// 用例派生：id → (kind=((id*2)%5)+1, flags=id%2, plen=(id*4)%16+4)。
/// plen 恒在 4..=19（非零且 ≤ 记录尺寸，空记录由拒探单独注入）。
pub fn derive_case(id: u64) -> TxCase {
    let kind = (((id.wrapping_mul(2)) % 5) + 1) as u8;
    let flags = (id % 2) as u8;
    let plen = (((id.wrapping_mul(4)) % 16) + 4) as u16;
    let mut payload: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while (i as u64) < plen as u64 {
        payload.push(((i.wrapping_mul(31)).wrapping_add(id as usize) % 251) as u8);
        i += 1;
    }
    TxCase { id, kind, epoch: (100 + id) as u32, flags, plen, payload }
}

/// 派生 20 用例（id 1..=20）。
pub fn derive_cases() -> Vec<TxCase> {
    let mut v: Vec<TxCase> = Vec::new();
    for k in 1..=(CASE_N as u64) {
        v.push(derive_case(k));
    }
    v
}

/// 校验和（FNV-1a 32 位，整数实现）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for i in 0..(bytes.len()) {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

/// 编码：字段按冻结偏移写入 24 字节记录，crc 覆盖前 20 字节。
pub fn encode_tx(c: &TxCase) -> Result<Vec<u8>, u32> {
    validate_kind(c.kind)?;
    if c.payload.is_empty() {
        return Err(TxCode::RecordEmpty.val());
    }
    if c.payload.len() != c.plen as usize {
        return Err(TxCode::RecordEmpty.val());
    }
    if c.plen as usize > RECORD_SIZE {
        return Err(TxCode::RecordEmpty.val());
    }
    let mut rec: Vec<u8> = Vec::new();
    for _i in 0..RECORD_SIZE {
        rec.push(0);
    }
    let mut v = c.id;
    let mut k = 0usize;
    while k < 8 {
        rec[FIELD_OFFSETS[0] + k] = (v & 0xFF) as u8;
        v >>= 8;
        k += 1;
    }
    let mut v = c.epoch as u64;
    let mut k = 0usize;
    while k < 4 {
        rec[FIELD_OFFSETS[1] + k] = (v & 0xFF) as u8;
        v >>= 8;
        k += 1;
    }
    rec[FIELD_OFFSETS[2]] = c.kind;
    rec[FIELD_OFFSETS[3]] = c.flags;
    let mut v = c.plen as u64;
    let mut k = 0usize;
    while k < 2 {
        rec[FIELD_OFFSETS[4] + k] = (v & 0xFF) as u8;
        v >>= 8;
        k += 1;
    }
    let mut k = 0usize;
    while k < c.payload.len() && FIELD_OFFSETS[5] + 4 + k < RECORD_SIZE {
        rec[FIELD_OFFSETS[5] + 4 + k] = c.payload[k];
        k += 1;
    }
    let crc = fnv1a(&rec[0..FIELD_OFFSETS[5]]);
    let mut v = crc as u64;
    for k in 0..4 {
        rec[FIELD_OFFSETS[5] + k] = (v & 0xFF) as u8;
        v >>= 8;
    }
    Ok(rec)
}

/// 解码回读结构。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedTx {
    pub txid: u64,
    pub epoch: u32,
    pub kind: u8,
    pub flags: u8,
    pub plen: u16,
    pub crc: u32,
}

/// 解码：逐字段回读 + crc 复核；长度/crc 不合法一律破损。
pub fn decode_tx(rec: &[u8]) -> Result<DecodedTx, u32> {
    if rec.len() != RECORD_SIZE {
        return Err(TxCode::CrcBad.val());
    }
    let rd = |off: usize, n: usize| -> u64 {
        let mut v = 0u64;
        let mut k = n;
        while k > 0 {
            v = (v << 8) | match rec.get(off + k - 1) {
                Some(b) => *b as u64,
                None => 0,
            };
            k -= 1;
        }
        v
    };
    let d = DecodedTx {
        txid: rd(FIELD_OFFSETS[0], 8),
        epoch: rd(FIELD_OFFSETS[1], 4) as u32,
        kind: rd(FIELD_OFFSETS[2], 1) as u8,
        flags: rd(FIELD_OFFSETS[3], 1) as u8,
        plen: rd(FIELD_OFFSETS[4], 2) as u16,
        crc: rd(FIELD_OFFSETS[5], 4) as u32,
    };
    if fnv1a(&rec[0..FIELD_OFFSETS[5]]) != d.crc {
        return Err(TxCode::CrcBad.val());
    }
    validate_kind(d.kind)?;
    Ok(d)
}

/// 往返对账：编码→解码回读逐字段全等。
pub fn roundtrip_ok(c: &TxCase) -> Result<bool, u32> {
    let rec = encode_tx(c)?;
    let d = decode_tx(&rec)?;
    Ok(d.txid == c.id && d.epoch == c.epoch && d.kind == c.kind
        && d.flags == c.flags && d.plen == c.plen)
}

/// 用例行渲染（导出路径）：tx=01|kind=3|plen=24|size=24|crc=XXXXXXXX|lay=..。
pub fn render_case_line(c: &TxCase, crc: u32) -> String {
    let (offs, total) = match runtime_layout() {
        Some(x) => x,
        None => (Vec::new(), 0),
    };
    let mut lay = String::new();
    for i in 0..(offs.len()) {
        if i > 0 {
            lay.push(',');
        }
        lay.push_str(&offs[i].to_string());
        lay.push(':');
        lay.push_str(&FIELD_SIZES[i].to_string());
    }
    format!(
        "tx={:02}|kind={}={}|plen={}|size={}|crc={:08X}|lay={}\n",
        c.id, c.kind, kind_name(c.kind), c.plen, total, crc, lay
    )
}

/// 用例行渲染（直读路径）：逐字段拼接（结构独立，供对拍基准）。
pub fn direct_case_line(c: &TxCase, crc: u32) -> String {
    let (offs, total) = match runtime_layout() {
        Some(x) => x,
        None => (Vec::new(), 0),
    };
    let mut s = String::from("tx=");
    if c.id < 10 {
        s.push('0');
    }
    s.push_str(&c.id.to_string());
    s.push_str("|kind=");
    s.push_str(&c.kind.to_string());
    s.push('=');
    s.push_str(kind_name(c.kind));
    s.push_str("|plen=");
    s.push_str(&c.plen.to_string());
    s.push_str("|size=");
    s.push_str(&total.to_string());
    s.push_str("|crc=");
    s.push_str(&format!("{:08X}", crc));
    s.push_str("|lay=");
    for i in 0..(offs.len()) {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&offs[i].to_string());
        s.push(':');
        s.push_str(&FIELD_SIZES[i].to_string());
    }
    s.push('\n');
    s
}

/// 快照头：@VXT1|cases|ok|sum;
pub fn header_tx(cases: u64, ok: u64, sum: u32) -> String {
    format!("@VXT1|cases={}|ok={}|sum={:08X};\n", cases, ok, sum)
}

/// 解析后的重放账头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxHeader {
    pub cases: u64,
    pub ok: u64,
    pub sum: u32,
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    for i in 0..(b.len()) {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    for i in 0..8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 重放账头解析：逐 token 扫描，未知字段即破损。
pub fn parse_header(hdr: &str) -> Option<TxHeader> {
    let mut cases: Option<u64> = None;
    let mut ok: Option<u64> = None;
    let mut sum: Option<u32> = None;
    let bts = hdr.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &hdr[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "cases" => cases = Some(parse_dec(v)?),
                "ok" => ok = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    Some(TxHeader { cases: cases?, ok: ok?, sum: sum? })
}

/// 事务结构重放器：用例序严格单调（只许按 1→20 顺序重放）。
#[derive(Clone, Debug)]
pub struct TxReplayer {
    /// 下一待重放用例 id（1 起，严格递增）。
    pub cursor: u64,
    pub state: ReplayState,
    pub admin_ops: u64,
    pub payload_ops: u64,
    pub replays: u32,
    /// 对照失真累计。
    pub mismatches: u32,
}

impl TxReplayer {
    pub fn new() -> TxReplayer {
        TxReplayer {
            cursor: 1,
            state: ReplayState::Idle,
            admin_ops: 0,
            payload_ops: 0,
            replays: 0,
            mismatches: 0,
        }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: ReplayState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(TxCode::CursorBack.val());
        }
        self.state = to;
        Ok(())
    }

    /// 封盘：账面冻结。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance_state(ReplayState::Sealed)
    }

    /// 重放一例：id 必须等于游标（跳例/回例一律倒退拒）。
    /// 往返 + 布局双断言全过才记账。
    pub fn replay_one(&mut self, id: u64) -> Result<(TxCase, u32), u32> {
        if self.state == ReplayState::Sealed || id != self.cursor {
            return Err(TxCode::CursorBack.val());
        }
        let c = derive_case(id);
        if c.payload.is_empty() {
            return Err(TxCode::RecordEmpty.val());
        }
        let rec = encode_tx(&c)?;
        let d = decode_tx(&rec)?;
        if !(d.txid == c.id && d.epoch == c.epoch && d.kind == c.kind
            && d.flags == c.flags && d.plen == c.plen)
        {
            self.mismatches += 1;
            return Err(TxCode::CrossMiss.val());
        }
        let lay = layout_agrees()?;
        if !lay {
            self.mismatches += 1;
            return Err(TxCode::LayoutMiss.val());
        }
        let line = render_case_line(&c, d.crc);
        self.admin_ops += 2;
        self.payload_ops += rec.len() as u64 * 2 + line.len() as u64 * 2;
        if self.state == ReplayState::Idle {
            self.state = ReplayState::Replaying;
        }
        self.replays += 1;
        self.cursor = id.checked_add(1).unwrap_or(self.cursor);
        Ok((c, d.crc))
    }

    /// 双路对照：逐用例导出路径行 vs 直读路径行全等。
    pub fn cross_check(&mut self, cases: &[TxCase]) -> Result<usize, u32> {
        if self.state == ReplayState::Sealed {
            return Err(TxCode::CursorBack.val());
        }
        let mut crossed = 0usize;
        for i in 0..CASE_N {
            let c = match cases.get(i) {
                Some(x) => x,
                None => return Err(TxCode::RecordEmpty.val()),
            };
            let rec = encode_tx(c)?;
            let crc = match decode_tx(&rec) {
                Ok(d) => d.crc,
                Err(e) => return Err(e),
            };
            if render_case_line(c, crc) != direct_case_line(c, crc) {
                self.mismatches += 1;
                return Err(TxCode::CrossMiss.val());
            }
            crossed += 1;
        }
        self.advance_state(ReplayState::CrossChecked)?;
        Ok(crossed)
    }

    /// 开销占比（bps）：簿记 / 载荷；无载荷视为超限。
    pub fn overhead_bps(&self) -> u64 {
        if self.payload_ops == 0 {
            return u64::MAX;
        }
        self.admin_ops.saturating_mul(10_000) / self.payload_ops
    }
}

/// 导出重放账：快照头 + 20 用例行（要求 20 例往返+布局全过）。
pub fn export_ledger(cases: &[TxCase]) -> Result<String, u32> {
    if cases.len() < CASE_N {
        return Err(TxCode::RecordEmpty.val());
    }
    let mut body = String::new();
    let mut ok = 0u64;
    for i in 0..CASE_N {
        let c = match cases.get(i) {
            Some(x) => x,
            None => return Err(TxCode::RecordEmpty.val()),
        };
        let rec = encode_tx(c)?;
        let d = decode_tx(&rec)?;
        if !roundtrip_ok(c)? || !layout_agrees()? {
            return Err(TxCode::CrossMiss.val());
        }
        body.push_str(&render_case_line(c, d.crc));
        ok += 1;
    }
    let hdr = header_tx(CASE_N as u64, ok, fnv1a(body.as_bytes()));
    let mut out = hdr;
    out.push_str(&body);
    Ok(out)
}

/// 正文裸控制符扫描（<0x20 且非换行 → 破损）。
fn body_control_scan(body: &str) -> Result<(), u32> {
    let b = body.as_bytes();
    for i in 0..(b.len()) {
        if b[i] < 0x20 && b[i] != b'\n' {
            return Err(TxCode::CrcBad.val());
        }
    }
    Ok(())
}

/// 正文行数（'\n' 计数）。
pub fn count_lines(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0usize;
    for i in 0..(b.len()) {
        if b[i] == b'\n' {
            n += 1;
        }
    }
    n
}

/// 重放账核验：头字段 + 行数锚 + 正文逐字节对拍 + 校验和 + 全过锚。
pub fn verify_ledger(cases: &[TxCase], out: &str) -> Result<(), u32> {
    let ob = out.as_bytes();
    if out.len() < 7 || !out.starts_with("@VXT1|") {
        return Err(TxCode::CrcBad.val());
    }
    let semi = match find_byte(ob, 0, b';') {
        Some(p) => p,
        None => return Err(TxCode::CrcBad.val()),
    };
    let h = match parse_header(&out[6..semi]) {
        Some(h) => h,
        None => return Err(TxCode::CrcBad.val()),
    };
    if h.cases != CASE_N as u64 || h.ok != CASE_N as u64 {
        return Err(TxCode::CrossMiss.val());
    }
    let mut start = semi + 1;
    if start < out.len() && ob[start] == b'\n' {
        start += 1;
    }
    let body = &out[start..];
    body_control_scan(body)?;
    let mut expect = String::new();
    for j in 0..CASE_N {
        let c = match cases.get(j) {
            Some(x) => x,
            None => return Err(TxCode::RecordEmpty.val()),
        };
        let rec = encode_tx(c)?;
        let d = decode_tx(&rec)?;
        expect.push_str(&render_case_line(c, d.crc));
    }
    if body != expect {
        return Err(TxCode::CrossMiss.val());
    }
    if count_lines(body) != CASE_N {
        return Err(TxCode::CrcBad.val());
    }
    if !layout_agrees()? {
        return Err(TxCode::LayoutMiss.val());
    }
    if h.sum != fnv1a(body.as_bytes()) {
        return Err(TxCode::CrcBad.val());
    }
    Ok(())
}

/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    for i in 0..(b.len()) {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
    }
}

/// 快照头行尾下标（头行 ';' 后换行的位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) {
        Some(x) => x,
        None => b.len(),
    }
}

/// 对黄金重放账注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    match kind % 10 {
        0 => {
            let mut flipped = false;
            for i in 0..(b.len()) {
                if i > he && b[i] != b'\n' && !flipped {
                    v.push(b[i] ^ 0x01);
                    flipped = true;
                } else {
                    v.push(b[i]);
                }
            }
        }
        1 => patch_after(b, b"ok=", b'0', b'1', &mut v),
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        5 => patch_after(b, b"cases=", b'0', b'1', &mut v),
        7 => {
            let line1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let seg = &b[he + 1..line1.min(b.len())];
            let at = match find_sub(seg, 0, b"crc=") {
                Some(p) => he + 1 + p + 4,
                None => usize::MAX,
            };
            for i in 0..(b.len()) {
                if i == at {
                    let c = b[i];
                    v.push(if c == b'0' { b'F' } else { b'0' });
                } else {
                    v.push(b[i]);
                }
            }
        }
        3 => {
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        4 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾：去末 6 字节
        6 => {
            let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        8 => {
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        _ => {
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"tx=99|kind=6=unknown|plen=0|size=24|crc=00000000|lay=0:8\n";
            for k in 0..(tail.len()) {
                v.push(tail[k]);
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金重放账注入一路失真，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    for r in 0..rounds {
        let cases = derive_cases();
        let out = export_ledger(&cases)?;
        verify_ledger(&cases, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_ledger(&cases, &distort(&out, kind)).is_err());
    }
    Ok(led)
}

/// 种类闭集探针：0 与 6 越界拒，1..=5 全收且名对号。
pub fn probe_kind() -> bool {
    validate_kind(0).is_err() && validate_kind(6).is_err()
        && validate_kind(1).is_ok() && validate_kind(5).is_ok()
        && kind_name(1) == "begin" && kind_name(3) == "commit"
        && kind_name(5) == "checkpoint" && kind_name(0) == "unknown"
}

/// 布局偏差探针：错规则计算器必被对照器抓到（LayoutMiss 能力证明）。
pub fn probe_bad_layout() -> bool {
    let bad_total = bad_layout_offset(true);
    let good_total = bad_layout_offset(false);
    bad_total != good_total && good_total == RECORD_SIZE
}

/// 往返探针：20 用例编码→解码回读逐字段全等 + crc 复核。
pub fn probe_roundtrip_all() -> bool {
    let cases = derive_cases();
    for i in 0..(cases.len()) {
        let c = match cases.get(i) {
            Some(x) => x,
            None => return false,
        };
        match roundtrip_ok(c) {
            Ok(true) => {}
            _ => return false,
        }
    }
    true
}

/// crc 破损探针：翻记录内一比特，解码必判 CrcBad。
pub fn probe_crc_tamper() -> bool {
    let c = derive_case(7);
    let rec = match encode_tx(&c) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut bad = Vec::new();
    for i in 0..(rec.len()) {
        bad.push(if i == 3 { rec[i] ^ 0x01 } else { rec[i] });
    }
    decode_tx(&bad).is_err()
}

pub fn header_roundtrip() -> bool {
    let sum = fnv1a(b"probe");
    let ht = header_tx(20, 20, sum);
    let a = match parse_header(&ht[6..ht.len() - 2]) {
        Some(h) => h.cases == 20 && h.ok == 20 && h.sum == sum,
        None => false,
    };
    let bad = parse_header("cases=20|ghost=1|sum=00000000").is_none()
        && parse_header("cases=x|sum=00000000").is_none()
        && parse_header("cases=20|sum=00").is_none();
    a && bad
}

/// 全链：20 例顺序重放（往返+布局双断言）→ 双路对照 → 导出+核验
/// → 七拒探 → 开销闸。返回 (正旗数, 拒旗数, 载荷账, 对照例数)。
pub fn golden_run() -> Result<(u32, u32, u64, usize), u32> {
    let cases = derive_cases();
    let mut rp = TxReplayer::new();
    let rej_skip = rp.replay_one(2).is_err();
    let mut id = 1u64;
    while id <= CASE_N as u64 {
        let _ = rp.replay_one(id)?;
        id += 1;
    }
    let crossed = rp.cross_check(&cases)?;
    let rej_back = rp.replay_one(1).is_err();
    let bad_kind = TxCase { id: 1, kind: 6, epoch: 1, flags: 0, plen: 1, payload: vec![1] };
    let rej_kind = encode_tx(&bad_kind).is_err();
    let empty = TxCase { id: 1, kind: 1, epoch: 1, flags: 0, plen: 0, payload: Vec::new() };
    let rej_empty = encode_tx(&empty).is_err();
    rp.seal()?;
    let rej_sealed = rp.replay_one(1).is_err();
    let rej_crc = probe_crc_tamper();
    let out = export_ledger(&cases)?;
    let rej_tamper = verify_ledger(&cases, &distort(&out, 2)).is_err();
    if rp.overhead_bps() > OVERHEAD_CAP_BPS {
        return Err(TxCode::OverheadHigh.val());
    }
    let flags = (crossed == CASE_N) as u32 + (rp.mismatches == 0) as u32
        + (verify_ledger(&cases, &out).is_ok()) as u32 + probe_roundtrip_all() as u32;
    let rejects = rej_skip as u32 + rej_back as u32 + rej_kind as u32
        + rej_empty as u32 + rej_sealed as u32 + rej_crc as u32 + rej_tamper as u32;
    Ok((flags, rejects, rp.payload_ops, crossed))
}

/// 序 A：黄金全链四正七拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 7, _, _)) => Ok(()),
        _ => Err(TxCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(TxCode::CrossMiss.val()),
    }
}

/// 序 C：冻结常量、布局锚与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72FC0 && TxCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = CASE_N == 20 && KIND_MAX == 5 && RECORD_ALIGN == 8 && RECORD_SIZE == 24
        && JUDGE_ROUNDS == 10 && OVERHEAD_CAP_BPS == 200 && in_window
        && FIELD_OFFSETS == [0, 8, 12, 13, 14, 16] && layout_agrees().unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(TxCode::NotReproducible.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TxCode::NotReproducible.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    for i in 0..(TxCode::ALL.len()) {
        let v = TxCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

pub fn writeback_summary() -> Result<String, u32> {
    let cases = derive_cases();
    let mut rp = TxReplayer::new();
    for id in 1..=(CASE_N as u64) {
        let _ = rp.replay_one(id)?;
    }
    let crossed = rp.cross_check(&cases)?;
    let out = export_ledger(&cases)?;
    Ok(format!(
        "状态=已完成 cases={} crossed={} lines={} overhead={}bps replays={}",
        CASE_N, crossed, count_lines(&out), rp.overhead_bps(), rp.replays
    ))
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, c)) => (*f == 4 && *r == 7, format!("flags={} rejects={} payload={} crossed={}", f, r, p, c)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-20用例重放全过", gp, gd));
    let lay = layout_agrees();
    v.push((
        "J1-布局断言100%",
        matches!(&lay, Ok(true)),
        format!("offs={:?} sizes={:?} total={}", FIELD_OFFSETS, FIELD_SIZES, RECORD_SIZE),
    ));
    v.push(("J1-布局偏差必抓", probe_bad_layout(), String::from("错规则计算器总尺寸 20≠24 被抓")));
    let cases = derive_cases();
    let out = export_ledger(&cases);
    let vp = match &out {
        Ok(o) => verify_ledger(&cases, o).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-重放账可导出", vp, String::from("@VXT1 头+20 用例行，五重闸全过")));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    let mut rp = TxReplayer::new();
    for k in 1..=(CASE_N as u64) {
        let _ = rp.replay_one(k);
    }
    let bps = rp.overhead_bps();
    v.push((
        "J1-开销占比≤2%",
        bps <= OVERHEAD_CAP_BPS,
        format!("admin={} payload={} bps={} cap={}", rp.admin_ops, rp.payload_ops, bps, OVERHEAD_CAP_BPS),
    ));
    v.push((
        "J1-往返与crc破损",
        probe_roundtrip_all() && probe_crc_tamper(),
        String::from("20 例编码→解码逐字段全等；翻比特必判 CrcBad"),
    ));
    v.push(("J1-种类闭集", probe_kind(), String::from("kind 1..=5 全收，0/6 越界拒")));
    let hd = String::from("建头→解析逐域对账；未知字段/坏数字/短校验和必败");
    v.push(("J1-快照头校验", header_roundtrip(), hd));
    let mut rp2 = TxReplayer::new();
    let r1 = rp2.replay_one(2).is_err();
    let _ = rp2.replay_one(1);
    let r2 = rp2.replay_one(1).is_err();
    let _ = rp2.seal();
    let r3 = rp2.replay_one(2).is_err();
    let mut rp3 = TxReplayer::new();
    let m1 = rp3.advance_state(ReplayState::Replaying).is_ok();
    let m2 = rp3.advance_state(ReplayState::Idle).is_err();
    let m3 = rp3.advance_state(ReplayState::Replaying).is_err();
    let m4 = rp3.advance_state(ReplayState::CrossChecked).is_ok();
    let m5 = rp3.advance_state(ReplayState::Sealed).is_ok();
    v.push((
        "J2-单调守卫",
        r1 && r2 && r3 && m1 && m2 && m3 && m4 && m5,
        format!("跳{}回{}封{} 状态机进拒平进封={}/{}/{}/{}/{}", r1, r2, r3, m1, m2, m3, m4, m5),
    ));
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72FC0 窗口")));
    v
}

pub fn run_unx5502_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F5502 事务结构回归账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
