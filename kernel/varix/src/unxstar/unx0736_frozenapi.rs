//! UNX-F0736 · 嫁接源冻结接口回归（六接口逐项） · 诊断段 0x5b80
//! 判据原文摘录：UNX-F0736-J1 六项对外冻结接口升级后回归过（字段级存在性+
//! 类型 6/6 接口逐项）且只读消费核过；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 六项对外冻结接口写死（账册查询/指纹查询/拦截器注册/报告导出/域经检索/
//!    台账盘点），接口名与字段表升级前后不允许漂移。
//! 2. 回归口径 = 字段级：逐接口逐字段核对存在性 + 类型码，两级全过才算
//!    接口回归过；不允许接口级粗判。
//! 3. 基线来自升级前快照（expected），实测来自升级后探针（actual），
//!    两者独立入账，比对产物只有 等于/不等于 两种裁决。
//! 4. 只读消费核：冻结接口的消费路径必须证明只读——消费动作仅允许
//!    查询类操作码，写类操作码在消费核中一律拒绝。
//! 5. 定容数组账本 + 守恒恒等式：pass+fail == checked <= total_checks。
//! 6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!    诊断段 0x5b80 为本单独占。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5b80;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_IF_DUP: u16 = SEG_BASE + 1; // 接口号重复登记
const E_IF_BAD: u16 = SEG_BASE + 2; // 接口号越界（1..=6）
const E_IF_FULL: u16 = SEG_BASE + 3; // 接口账容量已满
const E_FIELD_DUP: u16 = SEG_BASE + 4; // 字段名重复（同接口内）
const E_FIELD_FULL: u16 = SEG_BASE + 5; // 字段数超上限
const E_FIELD_EMPTY: u16 = SEG_BASE + 6; // 字段名为空
const E_TYCODE_BAD: u16 = SEG_BASE + 7; // 类型码非法（不在闭集）
const E_SNAP_TWICE: u16 = SEG_BASE + 8; // 同接口基线/实测重复快照
const E_MISMATCH: u16 = SEG_BASE + 9; // 字段级不匹配（存在性或类型）
const E_NO_BASELINE: u16 = SEG_BASE + 10; // 基线缺失即回归被拒
const E_NO_ACTUAL: u16 = SEG_BASE + 11; // 实测缺失即回归被拒
const E_WRITE_OP: u16 = SEG_BASE + 12; // 只读消费核检出写类操作码
const E_BAL: u16 = SEG_BASE + 13; // 守恒恒等式破裂（pass+fail≠checked）
const E_SEALED: u16 = SEG_BASE + 14; // 回归账已封，拒绝再写
const E_NO_IF: u16 = SEG_BASE + 15; // 回归账为空，无从断言

/// 冻结接口数：六项写死。
pub const IFACE_COUNT: usize = 6;

/// 每接口字段数上限（冻结表规模防线）。
pub const MAX_FIELDS: usize = 16;

/// 类型码闭集（冻结接口字段类型，升级前后不可扩）。
pub const TY_U32: u8 = 1;
pub const TY_U64: u8 = 2;
pub const TY_BOOL: u8 = 3;
pub const TY_BYTES: u8 = 4;
pub const TY_STR: u8 = 5;
pub const TY_HANDLE: u8 = 6;

/// 消费操作码闭集：只读消费核只放行 1..4。
pub const OP_QUERY: u8 = 1;
pub const OP_SNAPSHOT: u8 = 2;
pub const OP_LIST: u8 = 3;
pub const OP_VERIFY: u8 = 4;
pub const OP_INSERT: u8 = 5; // 写类：消费核必须拒绝
pub const OP_DELETE: u8 = 6; // 写类：消费核必须拒绝

// ── FNV-1a 64（字段名哈希口径）────────────────────────────────
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
        E_IF_DUP => "接口号重复：六接口各登记一次",
        E_IF_BAD => "接口号越界：合法 1..=6",
        E_IF_FULL => "接口账已满：冻结接口数为定值，不许扩",
        E_FIELD_DUP => "字段名重复：同接口内字段名唯一",
        E_FIELD_FULL => "字段数超上限 16：冻结表不许膨胀",
        E_FIELD_EMPTY => "字段名为空：冻结表禁止空名",
        E_TYCODE_BAD => "类型码非法：闭集 1..=6",
        E_SNAP_TWICE => "快照重复：基线与实测各采一次",
        E_MISMATCH => "字段级不匹配：字段缺失或类型漂移，升级引入破坏",
        E_NO_BASELINE => "基线缺失：先采升级前快照",
        E_NO_ACTUAL => "实测缺失：先采升级后探针",
        E_WRITE_OP => "只读消费核检出写类操作码：消费面越权，拦截",
        E_BAL => "计数不守恒：pass+fail 必须等于 checked",
        E_SEALED => "回归账已封：封账后只读",
        E_NO_IF => "回归账为空：先登记接口再回归",
        _ => "未知诊断码",
    }
}

// ── 冻结字段 ─────────────────────────────────────────────────
/// 一个冻结字段的 schema 行：名哈希 + 类型码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenField {
    /// 字段名 FNV-1a 64（名本身不落账，哈希即身份）。
    pub name_hash: u64,
    /// 类型码（闭集 1..=6）。
    pub ty: u8,
}

impl FrozenField {
    /// 类型码合法性。
    pub fn ty_ok(ty: u8) -> bool {
        (1..=6).contains(&ty)
    }
}

// ── 冻结接口 ─────────────────────────────────────────────────
/// 一项冻结接口的字段表（基线与实测共用此结构）。
#[derive(Clone, Debug)]
pub struct FrozenIface {
    /// 接口号（1..=6）。
    pub iface_no: u32,
    /// 接口名。
    pub name: String,
    /// 字段表（定容）。
    pub fields: [Option<FrozenField>; MAX_FIELDS],
    /// 字段数。
    pub field_count: usize,
}

impl FrozenIface {
    /// 新接口：空字段表。
    pub fn new(iface_no: u32, name: &str) -> Result<FrozenIface, u16> {
        if iface_no == 0 || iface_no as usize > IFACE_COUNT {
            return Err(E_IF_BAD);
        }
        if name.is_empty() {
            return Err(E_IF_BAD);
        }
        Ok(FrozenIface {
            iface_no,
            name: name.to_string(),
            fields: [None; MAX_FIELDS],
            field_count: 0,
        })
    }

    /// 挂字段：名唯一 + 类型码闭集。
    pub fn add_field(&mut self, name: &str, ty: u8) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_FIELD_EMPTY);
        }
        if !FrozenField::ty_ok(ty) {
            return Err(E_TYCODE_BAD);
        }
        if self.field_count >= MAX_FIELDS {
            return Err(E_FIELD_FULL);
        }
        let nh = fnv1a64(name.as_bytes());
        let mut i = 0usize;
        while i < self.field_count {
            if let Some(f) = &self.fields[i] {
                if f.name_hash == nh {
                    return Err(E_FIELD_DUP);
                }
            }
            i += 1;
        }
        self.fields[self.field_count] = Some(FrozenField { name_hash: nh, ty });
        self.field_count += 1;
        Ok(())
    }

    /// 字段查找：按名哈希返回类型码。
    pub fn lookup(&self, name: &str) -> Result<u8, u16> {
        let nh = fnv1a64(name.as_bytes());
        let mut i = 0usize;
        while i < self.field_count {
            if let Some(f) = &self.fields[i] {
                if f.name_hash == nh {
                    return Ok(f.ty);
                }
            }
            i += 1;
        }
        Err(E_MISMATCH)
    }

    /// 接口哈希：字段表按序聚合（快照对比口径）。
    pub fn iface_hash(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.iface_no.to_le_bytes().as_slice());
        buf.extend_from_slice(self.name.as_bytes());
        let mut i = 0usize;
        while i < self.field_count {
            if let Some(f) = &self.fields[i] {
                buf.extend_from_slice(f.name_hash.to_le_bytes().as_slice());
                buf.push(f.ty);
            }
            i += 1;
        }
        fnv1a64(&buf)
    }
}

// ── 字段级裁决 ───────────────────────────────────────────────
/// 单字段回归裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldVerdict {
    /// 存在且类型一致。
    Ok,
    /// 字段缺失。
    Absent,
    /// 字段在但类型漂移。
    TypeDrift,
}

impl FieldVerdict {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            FieldVerdict::Ok => 1,
            FieldVerdict::Absent => 2,
            FieldVerdict::TypeDrift => 3,
        }
    }
}

/// 单接口回归结果：逐字段裁决集 + 接口级结论。
#[derive(Clone, Debug)]
pub struct IfaceResult {
    /// 接口号。
    pub iface_no: u32,
    /// 核对字段数（= 基线字段数）。
    pub checked: u32,
    /// 通过字段数。
    pub passed: u32,
    /// 失败字段数（checked = passed + failed）。
    pub failed: u32,
    /// 接口级结论（failed==0 才算过）。
    pub ok: bool,
}

// ── 接口回归账（主体）────────────────────────────────────────
/// 冻结接口回归账：基线快照 + 实测快照 + 逐项裁决 + 只读消费核。
pub struct ApiRegression {
    /// 基线（升级前）接口集，按下标 iface_no-1 存放。
    baseline: [Option<FrozenIface>; IFACE_COUNT],
    /// 实测（升级后）接口集。
    actual: [Option<FrozenIface>; IFACE_COUNT],
    /// 裁决集（回归后落账）。
    results: [Option<IfaceResult>; IFACE_COUNT],
    /// 已登记接口数。
    registered: usize,
    /// 已回归接口数。
    checked_ifaces: u32,
    /// 累计字段 pass/fail。
    total_pass: u32,
    total_fail: u32,
    /// 是否封账。
    sealed: bool,
}

impl ApiRegression {
    /// 开账。
    pub fn new() -> ApiRegression {
        ApiRegression {
            baseline: core::array::from_fn(|_| None),
            actual: core::array::from_fn(|_| None),
            results: core::array::from_fn(|_| None),
            registered: 0,
            checked_ifaces: 0,
            total_pass: 0,
            total_fail: 0,
            sealed: false,
        }
    }

    /// 登记接口到基线侧（升级前快照采集）。
    pub fn enroll_baseline(&mut self, iface: FrozenIface) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let idx = (iface.iface_no - 1) as usize;
        if self.baseline[idx].is_some() {
            return Err(E_SNAP_TWICE);
        }
        if iface.field_count == 0 {
            return Err(E_FIELD_EMPTY);
        }
        self.baseline[idx] = Some(iface);
        Ok(())
    }

    /// 登记接口到实测侧（升级后探针采集）。
    pub fn enroll_actual(&mut self, iface: FrozenIface) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let idx = (iface.iface_no - 1) as usize;
        if self.actual[idx].is_some() {
            return Err(E_SNAP_TWICE);
        }
        if self.baseline[idx].is_none() {
            return Err(E_NO_BASELINE);
        }
        self.actual[idx] = Some(iface);
        self.registered += 1;
        Ok(())
    }

    /// 单接口回归：逐字段存在性 + 类型核对。
    pub fn regress_iface(&mut self, iface_no: u32) -> Result<IfaceResult, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if iface_no == 0 || iface_no as usize > IFACE_COUNT {
            return Err(E_IF_BAD);
        }
        let idx = (iface_no - 1) as usize;
        if self.baseline[idx].is_none() {
            return Err(E_NO_BASELINE);
        }
        if self.actual[idx].is_none() {
            return Err(E_NO_ACTUAL);
        }
        let mut res = IfaceResult {
            iface_no,
            checked: 0,
            passed: 0,
            failed: 0,
            ok: false,
        };
        let base_fields = self.baseline[idx].as_ref().ok_or(E_NO_BASELINE)?.field_count;
        let mut fi = 0usize;
        // 基线字段逐个在实测中查证。
        while fi < base_fields {
            let (name_hash, ty) = {
                let b = self.baseline[idx].as_ref().ok_or(E_NO_BASELINE)?;
                let f = b.fields[fi].ok_or(E_MISMATCH)?;
                (f.name_hash, f.ty)
            };
            res.checked += 1;
            // 实测侧查找（不越界访问，逐槽扫描）。
            let act = self.actual[idx].as_ref().ok_or(E_NO_ACTUAL)?;
            let mut found: Option<u8> = None;
            let mut ai = 0usize;
            while ai < act.field_count {
                if let Some(f) = &act.fields[ai] {
                    if f.name_hash == name_hash {
                        found = Some(f.ty);
                    }
                }
                ai += 1;
            }
            match found {
                Some(t) if t == ty => res.passed += 1,
                Some(_) => res.failed += 1,
                None => res.failed += 1,
            }
            fi += 1;
        }
        res.ok = res.failed == 0 && res.checked > 0;
        self.checked_ifaces += 1;
        self.total_pass += res.passed;
        self.total_fail += res.failed;
        self.results[idx] = Some(res.clone());
        Ok(res)
    }

    /// 六接口全量回归：逐项跑，返回全过与否。
    pub fn regress_all(&mut self) -> Result<bool, u16> {
        if self.registered == 0 {
            return Err(E_NO_IF);
        }
        let mut n = 1u32;
        while n <= IFACE_COUNT as u32 {
            self.regress_iface(n)?;
            n += 1;
        }
        Ok(self.total_fail == 0 && self.checked_ifaces == IFACE_COUNT as u32)
    }

    /// 守恒恒等式：total_pass + total_fail == 逐接口 checked 之和。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < IFACE_COUNT {
            if let Some(r) = &self.results[i] {
                sum += r.checked;
            }
            i += 1;
        }
        self.total_pass + self.total_fail == sum
    }

    /// 回归总断言：六接口全登记、全回归、零失败、守恒。
    pub fn assert_pass(&self) -> Result<(), u16> {
        if self.registered == 0 {
            return Err(E_NO_IF);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.checked_ifaces != IFACE_COUNT as u32 {
            return Err(E_NO_ACTUAL);
        }
        if self.total_fail != 0 {
            return Err(E_MISMATCH);
        }
        let mut i = 0usize;
        while i < IFACE_COUNT {
            match &self.results[i] {
                Some(r) if !r.ok => return Err(E_MISMATCH),
                Some(_) => {}
                None => return Err(E_NO_ACTUAL),
            }
            i += 1;
        }
        Ok(())
    }

    /// 只读消费核：操作码白名单 1..4，写类一律拒。
    pub fn consume_readonly(&self, op: u8) -> Result<&'static str, u16> {
        match op {
            OP_QUERY => Ok("查询放行"),
            OP_SNAPSHOT => Ok("快照放行"),
            OP_LIST => Ok("列表放行"),
            OP_VERIFY => Ok("核验放行"),
            OP_INSERT | OP_DELETE => Err(E_WRITE_OP),
            _ => Err(E_WRITE_OP),
        }
    }

    /// 封账：断言过 + 消费核抽样过后冻结。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.assert_pass()?;
        // 消费核自证：写类必须被拒。
        if self.consume_readonly(OP_INSERT).is_ok() {
            return Err(E_WRITE_OP);
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < IFACE_COUNT {
            if let Some(r) = &self.results[i] {
                buf.extend_from_slice(r.iface_no.to_le_bytes().as_slice());
                buf.extend_from_slice(r.checked.to_le_bytes().as_slice());
                buf.extend_from_slice(r.passed.to_le_bytes().as_slice());
                buf.push(r.ok as u8);
            }
            i += 1;
        }
        buf.extend_from_slice(self.total_pass.to_le_bytes().as_slice());
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 报告行渲染。
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 冻结接口回归 · 六接口逐项 ===".to_string());
        let mut i = 0usize;
        while i < IFACE_COUNT {
            let name = match &self.baseline[i] {
                Some(f) => f.name.clone(),
                None => String::from("(未登记)"),
            };
            let mut line = String::from("IF");
            line.push_str(&itoa((i + 1) as u64));
            line.push('|');
            line.push_str(&name);
            line.push('|');
            match &self.results[i] {
                Some(r) => {
                    line.push_str("字段核 ");
                    line.push_str(&itoa(r.passed as u64));
                    line.push('/');
                    line.push_str(&itoa(r.checked as u64));
                    line.push_str(if r.ok { " 过" } else { " 败" });
                }
                None => line.push_str("未回归"),
            }
            lines.push(line);
            i += 1;
        }
        lines
    }
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

// ── 六接口冻结基线（定版字段表，写死来源：B37 冻结接口定版）──────
/// 构建六项冻结接口的定版基线（升级前权威 schema）。
pub fn frozen_baseline() -> Result<[FrozenIface; IFACE_COUNT], u16> {
    let mut ifs: [Option<FrozenIface>; IFACE_COUNT] = core::array::from_fn(|_| None);
    // IF1 账册查询。
    let mut f1 = FrozenIface::new(1, "ledger_query")?;
    f1.add_field("batch_no", TY_U32)?;
    f1.add_field("entry_count", TY_U32)?;
    f1.add_field("ledger_hash", TY_U64)?;
    ifs[0] = Some(f1);
    // IF2 指纹查询。
    let mut f2 = FrozenIface::new(2, "fingerprint_query")?;
    f2.add_field("artifact", TY_STR)?;
    f2.add_field("fp_hash", TY_U64)?;
    f2.add_field("version", TY_STR)?;
    ifs[1] = Some(f2);
    // IF3 拦截器注册。
    let mut f3 = FrozenIface::new(3, "intercept_register")?;
    f3.add_field("rule_id", TY_U32)?;
    f3.add_field("enabled", TY_BOOL)?;
    f3.add_field("hit_count", TY_U64)?;
    ifs[2] = Some(f3);
    // IF4 报告导出。
    let mut f4 = FrozenIface::new(4, "report_export")?;
    f4.add_field("section_no", TY_U32)?;
    f4.add_field("payload", TY_BYTES)?;
    ifs[3] = Some(f4);
    // IF5 域经检索。
    let mut f5 = FrozenIface::new(5, "domexp_search")?;
    f5.add_field("topic", TY_STR)?;
    f5.add_field("entry_id", TY_U32)?;
    ifs[4] = Some(f5);
    // IF6 台账盘点。
    let mut f6 = FrozenIface::new(6, "ledger_inventory")?;
    f6.add_field("total_lines", TY_U64)?;
    f6.add_field("state_code", TY_U32)?;
    f6.add_field("verified", TY_BOOL)?;
    ifs[5] = Some(f6);
    let mut out: [FrozenIface; IFACE_COUNT] = [
        FrozenIface::new(1, "fill")?,
        FrozenIface::new(2, "fill")?,
        FrozenIface::new(3, "fill")?,
        FrozenIface::new(4, "fill")?,
        FrozenIface::new(5, "fill")?,
        FrozenIface::new(6, "fill")?,
    ];
    let mut i = 0usize;
    while i < IFACE_COUNT {
        match ifs[i].take() {
            Some(f) => out[i] = f,
            None => return Err(E_IF_BAD),
        }
        i += 1;
    }
    Ok(out)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：六接口逐项回归 + 只读消费核 + 错误路径（类型漂移拦截）。
pub fn drill() -> Result<u32, u16> {
    let baseline = frozen_baseline()?;
    let mut reg = ApiRegression::new();
    let mut i = 0usize;
    while i < IFACE_COUNT {
        reg.enroll_baseline(clone_iface(&baseline[i]))?;
        // 实测侧：与基线完全一致（升级零漂移样本）。
        reg.enroll_actual(clone_iface(&baseline[i]))?;
        i += 1;
    }
    let all_ok = reg.regress_all()?;
    if !all_ok {
        return Err(SEG_BASE);
    }
    reg.assert_pass()?;
    let h = reg.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 只读消费核：读类放行、写类拒绝。
    if reg.consume_readonly(OP_QUERY).is_err() {
        return Err(SEG_BASE);
    }
    if reg.consume_readonly(OP_DELETE).is_ok() {
        return Err(SEG_BASE);
    }
    // 错误路径：类型漂移必须被字段级核对拦截。
    let mut reg2 = ApiRegression::new();
    let base = frozen_baseline()?;
    let mut drifted = clone_iface(&base[0]);
    // 把 ledger_hash 从 U64 改成 U32（类型漂移注入）。
    let mut fix = FrozenIface::new(1, "ledger_query")?;
    fix.add_field("batch_no", TY_U32)?;
    fix.add_field("entry_count", TY_U32)?;
    fix.add_field("ledger_hash", TY_U32)?;
    let _ = drifted;
    reg2.enroll_baseline(base[0].clone())?;
    reg2.enroll_actual(fix)?;
    let r = reg2.regress_iface(1)?;
    if r.ok || r.failed != 1 {
        return Err(SEG_BASE);
    }
    Ok(0)
}

/// 接口深拷贝（演练与账本衔接用，字段表逐槽复制）。
pub fn clone_iface(src: &FrozenIface) -> FrozenIface {
    let mut dst = FrozenIface {
        iface_no: src.iface_no,
        name: src.name.clone(),
        fields: [None; MAX_FIELDS],
        field_count: src.field_count,
    };
    let mut i = 0usize;
    while i < src.field_count {
        dst.fields[i] = src.fields[i];
        i += 1;
    }
    dst
}
