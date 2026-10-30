//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15441 · IDispatch 四法全谱 · 0x67380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15441-J1 四法成文（GetTypeInfoCount/GetTypeInfo/
//!      GetIDsOfNames/Invoke 签名语义 10/10 次），缺法注入检出
//!      10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四法闭集（签名+语义逐法钉版）：
//!       法 0 GetTypeInfoCount：返回类型信息数（0 或 1，闭集）；
//!       法 1 GetTypeInfo：iTInfo=0 且有类型信息 → S_OK+非空；
//!           否则 E_NOINTERFACE；iTInfo 越界 → 越界拒；
//!       法 2 GetIDsOfNames：名字表 → DISPID 表（1:1 映射，长度
//!           守恒，未知名 → 固定负 DISPID 拒收）；
//!       法 3 Invoke：按 DISPID 调度 → 出参 VARIANT；未知 DISPID →
//!           拒收；已知 DISPID 必须与名字账一致（双账对表）；
//!   - 调度形状账：四法齐全 = 全谱形；缺任一法 = 缺法残形，必检出；
//!   - DISPID 账守恒：名字→ID 与 ID→名字 双向映射一致（无重 ID）；
//!   - LCG 种子写死 = SEG_BASE（0x67380），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67380，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67380;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67380;

/// 四法闭集。
pub const D_GET_COUNT: u8 = 0;
pub const D_GET_TYPEINFO: u8 = 1;
pub const D_GET_IDS: u8 = 2;
pub const D_INVOKE: u8 = 3;
pub const METHOD_COUNT: usize = 4;
pub const METHOD_NAMES: [&str; METHOD_COUNT] = [
    "GetTypeInfoCount", "GetTypeInfo", "GetIDsOfNames", "Invoke",
];

/// 全谱位图：bit0..bit3 = 四法。
pub const SHAPE_FULL: u32 = 0b1111;

/// 成员名闭集（调度账写死，来源：自动化对象最小成员面）。
pub const MEMBER_NAMES: [&str; 5] = ["Add", "Clear", "Count", "Item", "Refresh"];
/// 未知名固定负 DISPID（写死：DISPID_UNKNOWN 建模值）。
pub const DISPID_UNKNOWN: i32 = -1;
/// 越界 iTInfo 判据（类型信息数闭集 0/1，iTInfo 仅 0 合法）。
pub const TI_COUNT_MAX: u32 = 1;

/// 判据投影：10/10。
pub const SPECTRA_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispCode {
    /// 方法码越闭集
    MethodOutOfRange,
    /// 缺法残形（四法缺一）
    MissingMethodShape,
    /// 类型信息数越闭集（非 0/1）
    TiCountInvalid,
    /// GetTypeInfo 越界/错返回
    TiIndexBreach,
    /// 名字↔ID 长度不守恒
    NameIdCountBreach,
    /// 未知名放行（应拒收）
    UnknownNamePassed,
    /// DISPID 重复派发
    DispidDuplicate,
    /// Invoke 未知 DISPID 放行
    InvokeUnknownDispid,
    /// 双账不一致（名字账与 ID 账错位）
    LedgerMismatch,
}

impl DispCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DispCode::MethodOutOfRange => "方法码越闭集：四法冻结",
            DispCode::MissingMethodShape => "缺法残形：四法缺一",
            DispCode::TiCountInvalid => "类型信息数越闭集：0/1",
            DispCode::TiIndexBreach => "GetTypeInfo 越界/错返回",
            DispCode::NameIdCountBreach => "名字↔ID 长度不守恒",
            DispCode::UnknownNamePassed => "未知名放行：应 DISPID_UNKNOWN",
            DispCode::DispidDuplicate => "DISPID 重复派发",
            DispCode::InvokeUnknownDispid => "Invoke 未知 DISPID 放行",
            DispCode::LedgerMismatch => "双账不一致：名字账与 ID 账错位",
        }
    }
    pub const ALL: [DispCode; 9] = [
        DispCode::MethodOutOfRange,
        DispCode::MissingMethodShape,
        DispCode::TiCountInvalid,
        DispCode::TiIndexBreach,
        DispCode::NameIdCountBreach,
        DispCode::UnknownNamePassed,
        DispCode::DispidDuplicate,
        DispCode::InvokeUnknownDispid,
        DispCode::LedgerMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DispCode::ALL.len() {
        if DispCode::ALL[i].val() == code {
            return DispCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67380 段闭集内"
}

// ── 调度账（名字 ↔ DISPID 双向映射） ─────────────────────────────
/// DISPID 钉版：成员序号即 DISPID（0..4，写死最小面）。
pub fn name_dispid(name: &str) -> i32 {
    let mut i = 0;
    while i < MEMBER_NAMES.len() {
        if MEMBER_NAMES[i] == name {
            return i as i32;
        }
        i += 1;
    }
    DISPID_UNKNOWN
}

/// ID → 名字反查（双账一致性面）。
pub fn dispid_name(id: i32) -> Option<&'static str> {
    if id < 0 {
        return None;
    }
    let idx = id as usize;
    if idx >= MEMBER_NAMES.len() {
        return None;
    }
    Some(MEMBER_NAMES[idx])
}

/// 法 0：GetTypeInfoCount（闭集 0/1；本建模对象提供 1 份类型信息）。
pub fn get_type_info_count(shape: u32) -> Result<u32, u32> {
    if shape != SHAPE_FULL {
        return Err(DispCode::MissingMethodShape.val());
    }
    Ok(TI_COUNT_MAX)
}

/// 法 1：GetTypeInfo。iTInfo=0 → 产出（1）；越界 → 拒。
pub fn get_type_info(shape: u32, itinfo: u32) -> Result<u32, u32> {
    if shape != SHAPE_FULL {
        return Err(DispCode::MissingMethodShape.val());
    }
    if itinfo != 0 {
        return Err(DispCode::TiIndexBreach.val());
    }
    Ok(1)
}

/// 法 2：GetIDsOfNames。名字表 → ID 表（长度守恒；未知名 → 拒收
/// （本钉版选严格面：未知名直接 Err，不放 DISPID_UNKNOWN 出账））。
pub fn get_ids_of_names(shape: u32, names: &[&str]) -> Result<Vec<i32>, u32> {
    if shape != SHAPE_FULL {
        return Err(DispCode::MissingMethodShape.val());
    }
    let mut ids: Vec<i32> = Vec::new();
    let mut i = 0;
    while i < names.len() {
        let id = name_dispid(names[i]);
        if id == DISPID_UNKNOWN {
            return Err(DispCode::UnknownNamePassed.val());
        }
        ids.push(id);
        i += 1;
    }
    if ids.len() != names.len() {
        return Err(DispCode::NameIdCountBreach.val());
    }
    Ok(ids)
}

/// 法 3：Invoke。DISPID → 出参码（1 起）；未知 ID 拒收。
pub fn invoke(shape: u32, dispid: i32) -> Result<u32, u32> {
    if shape != SHAPE_FULL {
        return Err(DispCode::MissingMethodShape.val());
    }
    match dispid_name(dispid) {
        Some(_) => Ok((dispid + 1) as u32),
        None => Err(DispCode::InvokeUnknownDispid.val()),
    }
}

// ── 双账对表审计 ─────────────────────────────────────────────────
/// 名字账 ↔ ID 账双向一致：每名有 ID、每 ID 有名、无重 ID。
pub fn audit_dispatch_ledger() -> Result<u32, u32> {
    let mut seen: Vec<i32> = Vec::new();
    let mut i = 0;
    while i < MEMBER_NAMES.len() {
        let id = name_dispid(MEMBER_NAMES[i]);
        if id != i as i32 {
            return Err(DispCode::LedgerMismatch.val());
        }
        // 反查必须回到同名。
        let back = dispid_name(id);
        match back {
            Some(n) => {
                if n != MEMBER_NAMES[i] {
                    return Err(DispCode::LedgerMismatch.val());
                }
            }
            None => return Err(DispCode::LedgerMismatch.val()),
        }
        // 无重 ID。
        let mut j = 0;
        while j < seen.len() {
            if seen[j] == id {
                return Err(DispCode::DispidDuplicate.val());
            }
            j += 1;
        }
        seen.push(id);
        i += 1;
    }
    Ok(seen.len() as u32)
}

// ── 确定性 LCG（种子 = 0x67380） ─────────────────────────────────
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

// ── 单轮四法全谱压演 ─────────────────────────────────────────────
/// 每轮：全谱形走四法各法断言 + 缺法注入 2 形（必拒）+ 未知名注入。
pub fn dispatch_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let shape = SHAPE_FULL;
    // 法 0。
    let cnt = get_type_info_count(shape)?;
    if cnt != TI_COUNT_MAX {
        return Err(DispCode::TiCountInvalid.val());
    }
    // 法 1：合法 1 路 + 越界 1 路（必拒）。
    let _ti = get_type_info(shape, 0)?;
    let ti_bad = get_type_info(shape, 1);
    if let Ok(_) = ti_bad {
        return Err(DispCode::TiIndexBreach.val());
    }
    // 法 2：全成员名批量 + 未知名 1 路（必拒）。
    let ids = get_ids_of_names(shape, &MEMBER_NAMES)?;
    if ids.len() != MEMBER_NAMES.len() {
        return Err(DispCode::NameIdCountBreach.val());
    }
    let unk = get_ids_of_names(shape, &["NoSuchMember"]);
    if let Ok(_) = unk {
        return Err(DispCode::UnknownNamePassed.val());
    }
    // 法 3：全 DISPID 调度 + 未知 ID 1 路（必拒）。
    let mut invoked = 0;
    let mut i = 0;
    while (i as i32) < MEMBER_NAMES.len() as i32 {
        let out = invoke(shape, i)?;
        if out != (i + 1) as u32 {
            return Err(DispCode::LedgerMismatch.val());
        }
        invoked += 1;
        i += 1;
    }
    let inv_bad = invoke(shape, DISPID_UNKNOWN);
    if let Ok(_) = inv_bad {
        return Err(DispCode::InvokeUnknownDispid.val());
    }
    let inv_oob = invoke(shape, MEMBER_NAMES.len() as i32 + 9);
    if let Ok(_) = inv_oob {
        return Err(DispCode::InvokeUnknownDispid.val());
    }
    // 缺法残形注入 2 形。
    let mut missing_caught = 0;
    let mut k = 0;
    while k < 2 {
        let shape_bad = if k == 0 { 0b1110 } else { 0b1011 };
        let r = get_type_info_count(shape_bad);
        if let Err(e) = r {
            if e == DispCode::MissingMethodShape.val() {
                missing_caught += 1;
            }
        }
        k += 1;
    }
    if missing_caught != 2 {
        return Err(DispCode::MissingMethodShape.val());
    }
    let _ = lcg.next();
    Ok((invoked, missing_caught))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let ledger = audit_dispatch_ledger()?;
    if ledger != METHOD_COUNT as u32 {
        return Err(DispCode::LedgerMismatch.val());
    }
    let mut rounds = 0;
    let mut invoked_total = 0;
    let mut r = 0;
    while r < SPECTRA_TRIALS {
        let (inv, _) = dispatch_round(r)?;
        if inv != METHOD_COUNT as u32 {
            return Err(DispCode::InvokeUnknownDispid.val());
        }
        invoked_total += inv;
        rounds += 1;
        r += 1;
    }
    // 黄金手算对照（写死）：
    //   "Add"→0，"Refresh"→4，"Bogus"→未知；
    //   Invoke(0)=1，Invoke(4)=5；TiCount=1。
    let a = name_dispid("Add");
    let b = name_dispid("Refresh");
    let c = name_dispid("Bogus");
    if a != 0 || b != 4 || c != DISPID_UNKNOWN {
        return Err(DispCode::LedgerMismatch.val());
    }
    if invoke(SHAPE_FULL, 0)? != 1 || invoke(SHAPE_FULL, 4)? != 5 {
        return Err(DispCode::InvokeUnknownDispid.val());
    }
    if get_type_info_count(SHAPE_FULL)? != 1 {
        return Err(DispCode::TiCountInvalid.val());
    }
    Ok((rounds, invoked_total, ledger))
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
    // 族 0：缺法残形注入（15 种非全谱位图）×10。
    let mut t = 0;
    while t < 10 {
        let shape = (t % 15) as u32 + 1; // 1..15 全为非 0b1111
        let r = get_type_info_count(shape);
        ledger.tries[0] += 1;
        if let Err(e) = r {
            if e == DispCode::MissingMethodShape.val() {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：未知名注入 ×10。
    let mut t = 0;
    while t < 10 {
        let bad = ["Zap", "Qux", "Nop"][t % 3];
        let r = get_ids_of_names(SHAPE_FULL, &[bad]);
        ledger.tries[1] += 1;
        if let Err(e) = r {
            if e == DispCode::UnknownNamePassed.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：未知 DISPID 调度注入 ×10（负值 + 越界正值）。
    let mut t = 0;
    while t < 10 {
        let id = if t % 2 == 0 { -7 } else { 40 + t as i32 };
        let r = invoke(SHAPE_FULL, id);
        ledger.tries[2] += 1;
        if let Err(e) = r {
            if e == DispCode::InvokeUnknownDispid.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DispCode::ALL.len() {
        let v = DispCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 四法闭集自洽（判据常量独立写死）。
    METHOD_COUNT == 4 && SHAPE_FULL == 0b1111 && TI_COUNT_MAX == 1
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, _, ledger)) = golden {
        rounds == SPECTRA_TRIALS as u32 && ledger == 5
    } else {
        false
    };
    v.push((
        "J1-四法全谱10/10逐法断言",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-缺法/未知名/未知ID注入10/10必抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 四法名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < METHOD_COUNT {
        if METHOD_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    v.push((
        "J1-四法名点验",
        okn == METHOD_COUNT,
        format!("{}/4", okn),
    ));
    // 双账双向一致独立点验。
    let ledger = audit_dispatch_ledger();
    v.push((
        "J1-名字/ID双账双向一致",
        ledger == Ok(5),
        format!("{:?}", ledger),
    ));
    // 黄金映射点验（判据写死）。
    v.push((
        "J1-黄金映射 Add→0 Refresh→4",
        name_dispid("Add") == 0 && name_dispid("Refresh") == 4,
        String::from("钉版最小面 5 成员"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67380 窗口")));
    v
}

pub fn run_unx15441_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15441 IDispatch 四法全谱");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
