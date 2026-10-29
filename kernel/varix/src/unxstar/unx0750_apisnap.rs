//! UNX-F0750 · 移交包接口快照（六冻结接口终版字段表） · 诊断段 0x5cc0
//! 判据原文摘录：UNX-F0750-J1 快照账齐（六接口字段表终版 + 与 F0736 回归账
//! 一致 100%）且字段零变更断言过、表哈希封存；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 快照即终版：六冻结接口的字段表以"终版行"入快照账，行含
//!    接口号/字段名哈希/类型码/序号四列，账成后只读。
//! 2. 与 F0736 回归账一致 100%：快照行集与回归裁决行集逐 (接口,字段)
//!    对双向核对，双向命中即一致；快照有回归无（幽灵行）或回归有快照无
//!    （缺行）都判不一致。
//! 3. 字段零变更断言：终版表与冻结基线（B37 定版 schema）逐位相等，
//!    任何字段增删改即断言失败。
//! 4. 表哈希封存：六表行集聚合哈希封存，封存后快照不可写。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5cc0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5cc0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_ROW_DUP: u16 = SEG_BASE + 1; // (接口,字段) 重复行
const E_ROW_FULL: u16 = SEG_BASE + 2; // 快照行账已满
const E_IFACE_BAD: u16 = SEG_BASE + 3; // 接口号越界（1..=6）
const E_TY_BAD: u16 = SEG_BASE + 4; // 类型码越界（1..=6）
const E_SEQ_BAD: u16 = SEG_BASE + 5; // 字段序号越界（0 或超接口字段数）
const E_HASH_ZERO: u16 = SEG_BASE + 6; // 字段名哈希为 0
const E_GHOST: u16 = SEG_BASE + 7; // 幽灵行：快照有而回归账无
const E_MISSING_ROW: u16 = SEG_BASE + 8; // 缺行：回归账有而快照无
const E_FIELD_DRIFT: u16 = SEG_BASE + 9; // 字段变更检出（类型或序漂移）
const E_TABLE_LACK: u16 = SEG_BASE + 10; // 六表未齐（某接口零行）
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 12; // 快照已封存，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 13; // 快照账为空
const E_BASE_UNSET: u16 = SEG_BASE + 14; // 冻结基线未装载

/// 接口数与类型码闭集上界（与 F0736 同口径）。
pub const IFACES: usize = 6;
pub const TY_MAX: u8 = 6;

/// 快照行账定容（六表 × 每表 ≤6 字段 = 36，留容 48）。
pub const MAX_ROWS: usize = 48;

/// 回归账行（F0736 对侧）定容。
pub const MAX_REG_ROWS: usize = 48;

// ── FNV-1a 64（行哈希与封存口径）──────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ROW_DUP => "(接口,字段) 重复行：一表一字段一行",
        E_ROW_FULL => "快照行账已满 48 行",
        E_IFACE_BAD => "接口号越界：六冻结接口 1..=6",
        E_TY_BAD => "类型码越界：1..=6 闭集",
        E_SEQ_BAD => "字段序号越界：1 起且不超该表字段数",
        E_HASH_ZERO => "字段名哈希为 0：哈希必须真实采集",
        E_GHOST => "幽灵行：快照有而 F0736 回归账无",
        E_MISSING_ROW => "缺行：F0736 回归账有而快照无",
        E_FIELD_DRIFT => "字段变更检出：类型或序与冻结基线不符",
        E_TABLE_LACK => "六表未齐：某接口零行",
        E_BAL => "守恒破裂：行数账不平",
        E_SEALED => "快照已封存：封存后只读",
        E_EMPTY => "快照账为空",
        E_BASE_UNSET => "冻结基线未装载：先载 B37 定版 schema",
        _ => "未知诊断码",
    }
}

// ── 快照行（终版字段表行）────────────────────────────────────
/// 一条终版字段表行：四列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapRow {
    /// 列一：接口号（1..=6）。
    pub iface: u8,
    /// 列二：字段名哈希（FNV-1a 64）。
    pub name_hash: u64,
    /// 列三：类型码（1..=6）。
    pub ty: u8,
    /// 列四：字段序号（1 起，表内连续）。
    pub seq: u8,
}

impl SnapRow {
    /// 行静态合法性。
    pub fn well_formed(&self) -> bool {
        self.iface >= 1 && self.iface as usize <= IFACES
            && self.name_hash != 0
            && self.ty >= 1 && self.ty <= TY_MAX
            && self.seq >= 1
    }

    /// 行键（接口 × 字段）。
    pub fn key(&self) -> (u8, u64) {
        (self.iface, self.name_hash)
    }
}

// ── 回归账行（F0736 对侧视图）─────────────────────────────────
/// F0736 回归账的一行投影：接口 + 字段 + 裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegRow {
    /// 接口号。
    pub iface: u8,
    /// 字段名哈希。
    pub name_hash: u64,
    /// 类型码。
    pub ty: u8,
    /// 字段序号。
    pub seq: u8,
    /// 回归裁决（1=过，0=败）。
    pub ok: u8,
}

// ── 冻结基线（B37 定版 schema 的快照侧投影）────────────────────
/// 冻结基线表：六接口各字段 (名哈希, 类型) 的定版序列。
pub struct FrozenBaseline {
    /// 每接口字段序列（iface-1 下标起）。
    tables: [[Option<(u64, u8)>; 8]; IFACES],
    /// 每接口字段数。
    counts: [usize; IFACES],
    /// 是否已装载。
    loaded: bool,
}

impl FrozenBaseline {
    /// 空基线。
    pub fn empty() -> FrozenBaseline {
        FrozenBaseline {
            tables: [[None; 8]; IFACES],
            counts: [0; IFACES],
            loaded: false,
        }
    }

    /// 装载一接口的字段序列。
    pub fn load_iface(&mut self, iface: u8, fields: &[(u64, u8)]) -> Result<(), u16> {
        if self.loaded {
            return Err(E_SEALED);
        }
        if iface == 0 || iface as usize > IFACES {
            return Err(E_IFACE_BAD);
        }
        if fields.is_empty() || fields.len() > 8 {
            return Err(E_SEQ_BAD);
        }
        let idx = (iface - 1) as usize;
        let mut i = 0usize;
        while i < fields.len() {
            if fields[i].0 == 0 {
                return Err(E_HASH_ZERO);
            }
            if fields[i].1 == 0 || fields[i].1 > TY_MAX {
                return Err(E_TY_BAD);
            }
            self.tables[idx][i] = Some(fields[i]);
            i += 1;
        }
        self.counts[idx] = fields.len();
        Ok(())
    }

    /// 完成装载（六表齐才许封）。
    pub fn finish_load(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < IFACES {
            if self.counts[i] == 0 {
                return Err(E_TABLE_LACK);
            }
            i += 1;
        }
        self.loaded = true;
        Ok(())
    }

    /// 是否已装载。
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// 读取接口字段数。
    pub fn count_of(&self, iface: u8) -> Result<usize, u16> {
        if iface == 0 || iface as usize > IFACES {
            return Err(E_IFACE_BAD);
        }
        Ok(self.counts[(iface - 1) as usize])
    }

    /// 读取接口第 seq 字段（seq 1 起）。
    pub fn field_of(&self, iface: u8, seq: u8) -> Result<(u64, u8), u16> {
        let n = self.count_of(iface)?;
        if seq == 0 || seq as usize > n {
            return Err(E_SEQ_BAD);
        }
        match self.tables[(iface - 1) as usize][(seq - 1) as usize] {
            Some(f) => Ok(f),
            None => Err(E_SEQ_BAD),
        }
    }
}

// ── 接口快照账（主体）────────────────────────────────────────
/// 移交包接口快照账：终版行集 → 基线零变更核 → 回归账一致核 → 表哈希封存。
pub struct ApiSnapshot {
    rows: [Option<SnapRow>; MAX_ROWS],
    row_count: usize,
    /// 冻结基线。
    baseline: FrozenBaseline,
    /// 每接口行计数（六表齐核用）。
    per_iface: [u32; IFACES],
    /// 是否封存。
    sealed: bool,
    /// 表哈希（封存后非零）。
    sealed_hash: u64,
}

impl ApiSnapshot {
    /// 开快照账。
    pub fn new() -> ApiSnapshot {
        ApiSnapshot {
            rows: [None; MAX_ROWS],
            row_count: 0,
            baseline: FrozenBaseline::empty(),
            per_iface: [0; IFACES],
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 装载冻结基线（透明转发到基线表）。
    pub fn load_baseline_iface(&mut self, iface: u8, fields: &[(u64, u8)]) -> Result<(), u16> {
        self.baseline.load_iface(iface, fields)
    }

    /// 完成基线装载。
    pub fn finish_baseline(&mut self) -> Result<(), u16> {
        self.baseline.finish_load()
    }

    /// 入快照行：行合法 + (接口,字段) 唯一 + 与基线零变更核（类型与序逐位对）。
    pub fn put_row(&mut self, iface: u8, name_hash: u64, ty: u8, seq: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.row_count >= MAX_ROWS {
            return Err(E_ROW_FULL);
        }
        if !self.baseline.is_loaded() {
            return Err(E_BASE_UNSET);
        }
        let row = SnapRow { iface, name_hash, ty, seq };
        if !row.well_formed() {
            return Err(E_ROW_DUP);
        }
        // (接口,字段) 唯一。
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                if r.key() == row.key() {
                    return Err(E_ROW_DUP);
                }
            }
            i += 1;
        }
        // 与基线零变更核：基线同序字段必须同名同型。
        let (b_hash, b_ty) = self.baseline.field_of(iface, seq)?;
        if b_hash != name_hash || b_ty != ty {
            return Err(E_FIELD_DRIFT);
        }
        self.rows[self.row_count] = Some(row);
        self.row_count += 1;
        self.per_iface[(iface - 1) as usize] += 1;
        Ok(())
    }

    /// 六表齐核：每接口至少一行。
    pub fn tables_complete(&self) -> bool {
        let mut i = 0usize;
        while i < IFACES {
            if self.per_iface[i] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// 与 F0736 回归账一致核：逐回归行在快照中找同键同行，
    /// 逐快照行在回归账中找同键 —— 双向覆盖 + 类型序一致。
    pub fn consistency_with_regression(&self, reg: &[RegRow]) -> Result<u32, u16> {
        if self.row_count == 0 || reg.is_empty() {
            return Err(E_EMPTY);
        }
        let mut matched = 0u32;
        // 正向：回归行 → 快照行。
        let mut i = 0usize;
        while i < reg.len() {
            let r = &reg[i];
            if r.iface == 0 || r.iface as usize > IFACES {
                return Err(E_IFACE_BAD);
            }
            let mut hit = false;
            let mut j = 0usize;
            while j < self.row_count {
                if let Some(s) = &self.rows[j] {
                    if s.iface == r.iface
                        && s.name_hash == r.name_hash
                        && s.ty == r.ty
                        && s.seq == r.seq
                    {
                        hit = true;
                    }
                }
                j += 1;
            }
            if !hit {
                return Err(E_MISSING_ROW);
            }
            matched += 1;
            i += 1;
        }
        // 反向：快照行 → 回归行（幽灵行检测）。
        i = 0;
        while i < self.row_count {
            let s = self.rows[i].ok_or(E_GHOST)?;
            let mut hit = false;
            let mut j = 0usize;
            while j < reg.len() {
                if reg[j].iface == s.iface
                    && reg[j].name_hash == s.name_hash
                    && reg[j].seq == s.seq
                {
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return Err(E_GHOST);
            }
            i += 1;
        }
        // 一致性 100%：匹配数 == 双侧行数。
        if matched as usize != reg.len() || self.row_count != reg.len() {
            return Err(E_BAL);
        }
        Ok(matched)
    }

    /// 字段零变更断言：全行重核基线（防绕过 put_row 的直写）。
    pub fn zero_drift_assert(&self) -> Result<(), u16> {
        if !self.baseline.is_loaded() {
            return Err(E_BASE_UNSET);
        }
        if self.row_count == 0 {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.row_count {
            let s = self.rows[i].ok_or(E_FIELD_DRIFT)?;
            let (b_hash, b_ty) = self.baseline.field_of(s.iface, s.seq)?;
            if b_hash != s.name_hash || b_ty != s.ty {
                return Err(E_FIELD_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }

    /// 守恒恒等式：行数 == 逐接口行数之和。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < IFACES {
            sum += self.per_iface[i];
            i += 1;
        }
        sum == self.row_count as u32
    }

    /// 表哈希封存：六表齐 + 零变更 + 守恒 → 聚合哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if !self.tables_complete() {
            return Err(E_TABLE_LACK);
        }
        self.zero_drift_assert()?;
        if !self.conserved() {
            return Err(E_BAL);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                buf.push(r.iface);
                buf.push(r.ty);
                buf.push(r.seq);
                buf.extend_from_slice(r.name_hash.to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 表哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 每接口行数快照。
    pub fn per_iface_rows(&self) -> [u32; IFACES] {
        self.per_iface
    }

    /// 快照行渲染：四列逐行。
    pub fn snapshot_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 移交包接口快照 · 六表终版 ===".to_string());
        let mut i = 0usize;
        while i < self.row_count {
            if let Some(r) = &self.rows[i] {
                let mut line = String::from("IF");
                line.push_str(&itoa(r.iface as u64));
                line.push_str(".F");
                line.push_str(&itoa(r.seq as u64));
                line.push_str("|哈希 ");
                line.push_str(&hex64(r.name_hash));
                line.push_str("|类型 ");
                line.push_str(&itoa(r.ty as u64));
                lines.push(line);
            }
            i += 1;
        }
        lines
    }
}

/// u64 转 16 位十六进制字符串（确定性小写）。
fn hex64(v: u64) -> String {
    let digits = b"0123456789abcdef";
    let mut s = String::new();
    let mut i = 0usize;
    while i < 16 {
        let shift = 60 - (i as u32) * 4;
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(digits[nib] as char);
        i += 1;
    }
    s
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── B37 定版 schema 样本（六接口字段序列，写死）────────────────
/// 构建定版基线样本（与 F0736 冻结接口同构，哈希为字段名的确定性哈希）。
pub fn standard_baseline() -> Result<FrozenBaseline, u16> {
    let mut fb = FrozenBaseline::empty();
    fb.load_iface(1, &[(fnv1a64(b"batch_no"), 1), (fnv1a64(b"entry_count"), 1), (fnv1a64(b"ledger_hash"), 2)])?;
    fb.load_iface(2, &[(fnv1a64(b"artifact"), 5), (fnv1a64(b"fp_hash"), 2), (fnv1a64(b"version"), 5)])?;
    fb.load_iface(3, &[(fnv1a64(b"rule_id"), 1), (fnv1a64(b"enabled"), 3), (fnv1a64(b"hit_count"), 2)])?;
    fb.load_iface(4, &[(fnv1a64(b"section_no"), 1), (fnv1a64(b"payload"), 4)])?;
    fb.load_iface(5, &[(fnv1a64(b"topic"), 5), (fnv1a64(b"entry_id"), 1)])?;
    fb.load_iface(6, &[(fnv1a64(b"total_lines"), 2), (fnv1a64(b"state_code"), 1), (fnv1a64(b"verified"), 3)])?;
    fb.finish_load()?;
    Ok(fb)
}

/// 从基线生成回归账行样本（F0736 对侧，全过裁决）。
pub fn regression_rows(fb: &FrozenBaseline) -> Result<Vec<RegRow>, u16> {
    let mut out: Vec<RegRow> = Vec::new();
    let mut f = 1u8;
    while f as usize <= IFACES {
        let n = fb.count_of(f)?;
        let mut s = 1u8;
        while s as usize <= n {
            let (h, t) = fb.field_of(f, s)?;
            out.push(RegRow {
                iface: f,
                name_hash: h,
                ty: t,
                seq: s,
                ok: 1,
            });
            s += 1;
        }
        f += 1;
    }
    Ok(out)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：六表终版 + 回归一致 100% + 零变更 + 封存 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let fb = standard_baseline()?;
    let reg = regression_rows(&fb)?;
    if reg.len() != 16 {
        return Err(SEG_BASE);
    }
    let mut snap = ApiSnapshot::new();
    // 基线装载（六表）。
    let mut f = 1u8;
    while f as usize <= IFACES {
        let n = fb.count_of(f)?;
        let mut fields: Vec<(u64, u8)> = Vec::new();
        let mut s = 1u8;
        while s as usize <= n {
            fields.push(fb.field_of(f, s)?);
            s += 1;
        }
        snap.load_baseline_iface(f, &fields)?;
        f += 1;
    }
    snap.finish_baseline()?;
    // 入快照行（逐回归行）。
    let mut i = 0usize;
    while i < reg.len() {
        snap.put_row(reg[i].iface, reg[i].name_hash, reg[i].ty, reg[i].seq)?;
        i += 1;
    }
    if !snap.tables_complete() || snap.row_count() != 16 {
        return Err(SEG_BASE);
    }
    let matched = snap.consistency_with_regression(&reg)?;
    if matched != 16 {
        return Err(SEG_BASE);
    }
    snap.zero_drift_assert()?;
    if !snap.conserved() {
        return Err(SEG_BASE);
    }
    let h = snap.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封存后写拒绝。
    match snap.put_row(1, fnv1a64(b"new_field"), 1, 4) {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：类型漂移行拒绝。
    let mut snap2 = ApiSnapshot::new();
    let fb2 = standard_baseline()?;
    let mut f = 1u8;
    while f as usize <= IFACES {
        let n = fb2.count_of(f)?;
        let mut fields: Vec<(u64, u8)> = Vec::new();
        let mut s = 1u8;
        while s as usize <= n {
            fields.push(fb2.field_of(f, s)?);
            s += 1;
        }
        snap2.load_baseline_iface(f, &fields)?;
        f += 1;
    }
    snap2.finish_baseline()?;
    match snap2.put_row(1, fnv1a64(b"ledger_hash"), 1, 3) {
        Err(c) if c == E_FIELD_DRIFT => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：幽灵行（回归账缺行）。
    let mut reg3 = regression_rows(&fb)?;
    reg3.pop();
    let mut snap3 = ApiSnapshot::new();
    let mut f = 1u8;
    while f as usize <= IFACES {
        let n = fb.count_of(f)?;
        let mut fields: Vec<(u64, u8)> = Vec::new();
        let mut s = 1u8;
        while s as usize <= n {
            fields.push(fb.field_of(f, s)?);
            s += 1;
        }
        snap3.load_baseline_iface(f, &fields)?;
        f += 1;
    }
    snap3.finish_baseline()?;
    let mut j = 0usize;
    while j < reg3.len() {
        snap3.put_row(reg3[j].iface, reg3[j].name_hash, reg3[j].ty, reg3[j].seq)?;
        j += 1;
    }
    match snap3.consistency_with_regression(&reg3) {
        Err(c) if c == E_BAL || c == E_MISSING_ROW => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
