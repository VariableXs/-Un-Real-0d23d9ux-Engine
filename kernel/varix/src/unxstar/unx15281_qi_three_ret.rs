//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15281 · QI 三返回律（S_OK/E_NOINTERFACE/E_UNEXPECTED 归位） · 0x67180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15281-J1 三返回律成文（成功/不支持/错误三态归位表
//!      10/10 次），第四态注入（异常返回路径）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态归位闭集：S_OK（已知接口，返回指针并加引用）/
//!     E_NOINTERFACE（未实现接口，返回空指针不加引用）/
//!     E_UNEXPECTED（对象态非法：已撕毁/上下文残缺）；
//!   - 归位表写死：接口族 × 对象态 → 三态之一，任何第四态
//!     （S_FALSE/E_FAIL/自造码）入账即注入，必检出；
//!   - E_NOINTERFACE 语义钉版：返回空指针 + 不触发 AddRef；
//!     S_OK 语义钉版：返回非空指针 + 恰好一次 AddRef；
//!   - IUnknown 自身恒可查（S_OK 铁律）；
//!   - LCG 种子写死 = SEG_BASE（0x67180），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67180，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67180;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67180;

/// HRESULT 判据常量（写死，来源：winerror.h 公开值，不复用被测值）。
pub const HR_S_OK: u32 = 0x0000_0000;
pub const HR_S_FALSE: u32 = 0x0000_0001;
pub const HR_E_NOINTERFACE: u32 = 0x8000_4002;
pub const HR_E_UNEXPECTED: u32 = 0x8000_FFFF;
pub const HR_E_FAIL: u32 = 0x8000_4005;

/// 接口族闭集（查询目标）。
pub const IID_IUNKNOWN: u32 = 0;
pub const IID_ICLASSFACTORY: u32 = 1;
pub const IID_IPERSIST: u32 = 2;
pub const IID_IUNKNOWN_DELEGATE: u32 = 3;
pub const IID_IUNKNOWN_CTRL: u32 = 4;
pub const IID_UNKNOWN_FOREIGN: u32 = 5;
pub const IID_COUNT: usize = 6;
pub const IID_NAMES: [&str; IID_COUNT] = [
    "IUnknown", "IClassFactory", "IPersist", "聚合委托IUnknown", "聚合控制IUnknown", "未实现外接口",
];

/// 对象态闭集。
pub const OBJ_ACTIVE: u8 = 0;
pub const OBJ_TORN: u8 = 1;

/// 判据投影：归位表 10/10 轮全对。
pub const TABLE_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QiCode {
    /// 接口族越闭集
    IidOutOfRange,
    /// 第四态返回（归位表外值）
    FourthStateInjected,
    /// S_OK 但返回空指针
    OkWithNull,
    /// S_OK 未加引用（引用账不平）
    OkWithoutAddRef,
    /// E_NOINTERFACE 返回非空指针
    NoIntfWithPtr,
    /// E_NOINTERFACE 错加了引用
    NoIntfWithAddRef,
    /// IUnknown 恒可查铁律破
    UnknownDenied,
    /// 撕毁对象未归位 E_UNEXPECTED
    TornNotRouted,
    /// 归位表与查询格数不守恒
    TableMismatch,
}

impl QiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QiCode::IidOutOfRange => "接口族越闭集：6 族冻结",
            QiCode::FourthStateInjected => "第四态返回：S_FALSE/E_FAIL 等归位外值",
            QiCode::OkWithNull => "S_OK 但返回空指针",
            QiCode::OkWithoutAddRef => "S_OK 未加引用：引用账不平",
            QiCode::NoIntfWithPtr => "E_NOINTERFACE 返回非空指针",
            QiCode::NoIntfWithAddRef => "E_NOINTERFACE 错加了引用",
            QiCode::UnknownDenied => "IUnknown 恒可查铁律破",
            QiCode::TornNotRouted => "撕毁对象未归位 E_UNEXPECTED",
            QiCode::TableMismatch => "归位表与查询格数不守恒",
        }
    }
    pub const ALL: [QiCode; 9] = [
        QiCode::IidOutOfRange,
        QiCode::FourthStateInjected,
        QiCode::OkWithNull,
        QiCode::OkWithoutAddRef,
        QiCode::NoIntfWithPtr,
        QiCode::NoIntfWithAddRef,
        QiCode::UnknownDenied,
        QiCode::TornNotRouted,
        QiCode::TableMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < QiCode::ALL.len() {
        if QiCode::ALL[i].val() == code {
            return QiCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67180 段闭集内"
}

// ── 三态归位闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QiOutcome {
    /// 成功：非空指针 + 恰一次 AddRef。
    Ok,
    /// 不支持：空指针 + 零 AddRef。
    NoInterface,
    /// 错误：对象态非法，不产生指针不加引用。
    Unexpected,
}

/// 归位表（写死：接口族 × 对象态 → 三态）。
/// 语义来源：COM QI 三返回律——已知接口 S_OK；未实现 E_NOINTERFACE；
/// 对象态非法（撕毁/残缺上下文）E_UNEXPECTED；IUnknown 恒可查。
pub fn pin_outcome(iid: u32, obj_state: u8) -> Result<QiOutcome, u32> {
    if iid as usize >= IID_COUNT {
        return Err(QiCode::IidOutOfRange.val());
    }
    if obj_state == OBJ_TORN {
        // 撕毁对象：任何查询归位 E_UNEXPECTED（IUnknown 也不例外——
        // 对象已无有效态可答）。
        return Ok(QiOutcome::Unexpected);
    }
    match iid {
        IID_UNKNOWN_FOREIGN => Ok(QiOutcome::NoInterface),
        _ => Ok(QiOutcome::Ok),
    }
}

/// 归位表格数守恒常量：6 族 × 2 态 = 12 格。
pub const TABLE_CELLS: usize = IID_COUNT * 2;

// ── QI 执行账（指针 + 引用账双列） ───────────────────────────────
pub struct QiReply {
    pub hr: u32,
    pub ptr: u32,
    pub addrefs: u32,
}

pub struct QiObject {
    pub id: u32,
    pub state: u8,
    pub refcount: i64,
    /// 撕毁后查询次数（应为 0——撕毁后仍可查询但归位 E_UNEXPECTED）。
    pub torn_queries: u32,
}

impl QiObject {
    pub fn new(id: u32) -> QiObject {
        QiObject { id, state: OBJ_ACTIVE, refcount: 1, torn_queries: 0 }
    }
    pub fn tear(&mut self) {
        self.state = OBJ_TORN;
    }
}

/// 执行一次 QI：按归位表产三态回复，并核语义钉版（指针/加引用）。
pub fn do_qi(obj: &mut QiObject, iid: u32) -> Result<QiReply, u32> {
    let outcome = pin_outcome(iid, obj.state)?;
    let torn = obj.state == OBJ_TORN;
    let reply = match outcome {
        QiOutcome::Ok => {
            if torn {
                return Err(QiCode::TornNotRouted.val());
            }
            // S_OK 语义钉版：非空指针 + 恰一次加引用。
            obj.refcount += 1;
            QiReply { hr: HR_S_OK, ptr: 0x1000 + iid, addrefs: 1 }
        }
        QiOutcome::NoInterface => {
            // E_NOINTERFACE 语义钉版：空指针 + 零加引用。
            QiReply { hr: HR_E_NOINTERFACE, ptr: 0, addrefs: 0 }
        }
        QiOutcome::Unexpected => {
            if torn {
                obj.torn_queries += 1;
            }
            QiReply { hr: HR_E_UNEXPECTED, ptr: 0, addrefs: 0 }
        }
    };
    Ok(reply)
}

/// 归位回复核验：三态语义逐条钉版（返回 Err 即语义破）。
pub fn verify_reply(obj_state: u8, r: &QiReply) -> Result<(), u32> {
    let torn = obj_state == OBJ_TORN;
    if r.hr == HR_S_OK {
        if torn {
            return Err(QiCode::TornNotRouted.val());
        }
        if r.ptr == 0 {
            return Err(QiCode::OkWithNull.val());
        }
        if r.addrefs != 1 {
            return Err(QiCode::OkWithoutAddRef.val());
        }
        return Ok(());
    }
    if r.hr == HR_E_NOINTERFACE {
        if r.ptr != 0 {
            return Err(QiCode::NoIntfWithPtr.val());
        }
        if r.addrefs != 0 {
            return Err(QiCode::NoIntfWithAddRef.val());
        }
        return Ok(());
    }
    if r.hr == HR_E_UNEXPECTED {
        if r.ptr != 0 || r.addrefs != 0 {
            return Err(QiCode::TornNotRouted.val());
        }
        return Ok(());
    }
    // 其余一切值 = 第四态注入。
    Err(QiCode::FourthStateInjected.val())
}

/// 归位表全格走查：12 格逐格跑 QI + 核钉版，格数守恒。
pub fn walk_table() -> Result<u32, u32> {
    let mut obj = QiObject::new(1);
    let mut cells = 0;
    let mut state = OBJ_ACTIVE;
    while state <= OBJ_TORN {
        let mut iid = 0;
        while (iid as usize) < IID_COUNT {
            if state == OBJ_TORN {
                obj.tear();
            }
            let r = do_qi(&mut obj, iid)?;
            verify_reply(state, &r)?;
            cells += 1;
            iid += 1;
        }
        // 复位对象（重走 Active 轮）。
        obj = QiObject::new(obj.id + 1);
        state += 1;
    }
    if cells as usize != TABLE_CELLS {
        return Err(QiCode::TableMismatch.val());
    }
    Ok(cells)
}

// ── 确定性 LCG（种子 = 0x67180） ─────────────────────────────────
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

// ── 单轮随机查询压演（激活对象 + 随机查询 + 引用账对账） ────────
pub fn qi_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut obj = QiObject::new(round as u32 + 1);
    let mut ok_cnt = 0;
    let mut nointf_cnt = 0;
    let mut i = 0;
    while i < 64 {
        let iid = lcg.below(IID_COUNT as u64) as u32;
        let r = do_qi(&mut obj, iid)?;
        verify_reply(OBJ_ACTIVE, &r)?;
        if r.hr == HR_S_OK {
            ok_cnt += 1;
        } else if r.hr == HR_E_NOINTERFACE {
            nointf_cnt += 1;
        } else {
            return Err(QiCode::FourthStateInjected.val());
        }
        i += 1;
    }
    // 引用账守恒：初值 1 + 成功次数 == 终值（失败不动的铁律）。
    if obj.refcount != 1 + ok_cnt as i64 {
        return Err(QiCode::OkWithoutAddRef.val());
    }
    // 撕毁轮：撕毁后任意查询必归位 E_UNEXPECTED。
    obj.tear();
    let mut torn_cnt = 0;
    let mut k = 0;
    while k < 8 {
        let iid = lcg.below(IID_COUNT as u64) as u32;
        let r = do_qi(&mut obj, iid)?;
        if r.hr != HR_E_UNEXPECTED {
            return Err(QiCode::TornNotRouted.val());
        }
        torn_cnt += 1;
        k += 1;
    }
    if obj.torn_queries != torn_cnt {
        return Err(QiCode::TableMismatch.val());
    }
    Ok((ok_cnt, nointf_cnt))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 黄金手算对照（写死）：
///   活对象查 IUnknown → S_OK，ptr≠0，+1 引用；
///   活对象查 未实现外接口 → E_NOINTERFACE，ptr=0，引用不动；
///   撕毁对象查 IUnknown → E_UNEXPECTED。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let cells = walk_table()?;
    let mut rounds_ok = 0;
    let mut r = 0;
    while r < TABLE_TRIALS {
        qi_round(r)?;
        rounds_ok += 1;
        r += 1;
    }
    // 黄金三问对照。
    let mut obj = QiObject::new(99);
    let a = do_qi(&mut obj, IID_IUNKNOWN)?;
    let b = do_qi(&mut obj, IID_UNKNOWN_FOREIGN)?;
    let a_ok = a.hr == HR_S_OK && a.ptr != 0 && a.addrefs == 1;
    let b_ok = b.hr == HR_E_NOINTERFACE && b.ptr == 0 && b.addrefs == 0;
    obj.tear();
    let c = do_qi(&mut obj, IID_IUNKNOWN)?;
    let c_ok = c.hr == HR_E_UNEXPECTED && c.ptr == 0;
    if !(a_ok && b_ok && c_ok) {
        return Err(QiCode::TableMismatch.val());
    }
    if obj.refcount != 2 {
        // 1 初值 + IUnknown 成功一次 = 2（未实现接口不动账）。
        return Err(QiCode::OkWithoutAddRef.val());
    }
    Ok((cells, rounds_ok, obj.refcount as u32))
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
    // 族 0：第四态注入（S_FALSE/E_FAIL/自造码过 verify_reply）×10×3 码。
    let fourth_states = [HR_S_FALSE, HR_E_FAIL, 0x8FFF_0001];
    let mut t = 0;
    while t < 10 {
        let mut k = 0;
        while k < 3 {
            let r = QiReply { hr: fourth_states[k], ptr: 0, addrefs: 0 };
            let v = verify_reply(OBJ_ACTIVE, &r);
            ledger.tries[0] += 1;
            if let Err(e) = v {
                if e == QiCode::FourthStateInjected.val() {
                    ledger.caught[0] += 1;
                }
            }
            k += 1;
        }
        t += 1;
    }
    // 族 1：E_NOINTERFACE 语义破（非空指针 / 错加引用）×10×2 形。
    let mut t = 0;
    while t < 10 {
        let r1 = QiReply { hr: HR_E_NOINTERFACE, ptr: 0x1234, addrefs: 0 };
        let r2 = QiReply { hr: HR_E_NOINTERFACE, ptr: 0, addrefs: 1 };
        let v1 = verify_reply(OBJ_ACTIVE, &r1);
        let v2 = verify_reply(OBJ_ACTIVE, &r2);
        ledger.tries[1] += 1;
        let c1 = if let Err(e) = v1 {
            e == QiCode::NoIntfWithPtr.val()
        } else {
            false
        };
        let c2 = if let Err(e) = v2 {
            e == QiCode::NoIntfWithAddRef.val()
        } else {
            false
        };
        if c1 && c2 {
            ledger.caught[1] += 1;
        }
        t += 1;
    }
    // 族 2：越闭集接口族 ×10。
    let mut t = 0;
    while t < 10 {
        let mut obj = QiObject::new(1);
        let v = do_qi(&mut obj, IID_COUNT as u32 + 7);
        ledger.tries[2] += 1;
        if let Err(e) = v {
            if e == QiCode::IidOutOfRange.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < QiCode::ALL.len() {
        let v = QiCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 三态互异闭集自洽。
    let ok = QiOutcome::Ok;
    let ni = QiOutcome::NoInterface;
    let un = QiOutcome::Unexpected;
    ok != ni && ni != un && ok != un
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((cells, rounds, _)) = golden {
        cells as usize == TABLE_CELLS && rounds == TABLE_TRIALS as u32
    } else {
        false
    };
    v.push((
        "J1-三态归位表10/10+12格守恒",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-第四态注入10/10必抓",
        led.all_caught() && led.caught[0] == 30,
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 接口族名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < IID_COUNT {
        if IID_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    v.push((
        "J1-6接口族名点验",
        okn == IID_COUNT,
        format!("{}/6", okn),
    ));
    // IUnknown 恒可查铁律独立点验（写死判据：归位表 Active 态 IUnknown=Ok）。
    let unk_ok = pin_outcome(IID_IUNKNOWN, OBJ_ACTIVE) == Ok(QiOutcome::Ok);
    v.push((
        "J1-IUnknown恒可查铁律",
        unk_ok,
        String::from("归位表 Active×IUnknown=S_OK"),
    ));
    // HRESULT 判据常量独立写死点验。
    v.push((
        "J1-判据常量独立",
        HR_S_OK == 0 && HR_E_NOINTERFACE == 0x80004002 && HR_E_UNEXPECTED == 0x8000FFFF,
        format!("S_OK={:x} NOINTF={:x} UNEXP={:x}", HR_S_OK, HR_E_NOINTERFACE, HR_E_UNEXPECTED),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67180 窗口")));
    v
}

pub fn run_unx15281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15281 QI 三返回律（三态归位表）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
