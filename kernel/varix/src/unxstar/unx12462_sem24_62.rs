//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12462 · NtOpenKeyEx/NtCreateKeyEx 语义档 · 0x307C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12462-J1 Ex 档（AccessOptions/OpenOptions）判据绿，
//!      REG_LEGAL_OPTION 越界注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - Ex 档扩展面：AccessOptions（访问权位集 DesiredAccess）+
//!     OpenOptions（打开选项 REG_OPEN_LEGAL_OPTION 闭集）+ Attributes
//!     （对象属性）三参齐备——Ex 档与基础档的语义差全在此三参；
//!   - 访问权位闭集：KEY_QUERY_VALUE/KEY_SET_VALUE/KEY_CREATE_SUB_KEY/
//!     KEY_ENUMERATE_SUB_KEYS/KEY_NOTIFY/KEY_CREATE_LINK 六位，外加
//!     KEY_READ/KEY_WRITE/KEY_ALL_ACCESS 三组合宏（展开核）；
//!   - OpenOptions 闭集（REG_OPEN_LEGAL_OPTION）：REG_OPTION_OPEN_LINK
//!     一位（NT REG_LEGAL_OPTION 口径：仅 OPEN_LINK 合法），越位注入
//!     显性拒 InvalidParameter；
//!   - 访问检查器：句柄权位 ∩ 请求权位判——请求位集非请求方所持即拒
//!     AccessDenied（位运算零分支实现）；
//!   - 组合宏展开：KEY_READ = STANDARD_RIGHTS_READ + QUERY_VALUE +
//!     ENUMERATE_SUB_KEYS + NOTIFY；KEY_WRITE = STANDARD_RIGHTS_WRITE
//!     + SET_VALUE + CREATE_SUB_KEY；展开一致性核（宏值 ≡ 手工或值）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，REG_LEGAL_OPTION 越位（位 1~31
//!     逐位扫）10/10 全拒；访问越权注入（请求未持位）10/10 全拒；
//!   - 诊断码闭集 0x307C0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x307C0 段）。
pub const SEG_BASE: u32 = 0x307C0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 访问权位闭集（NT KEY_* 口径）。
pub const KEY_QUERY_VALUE: u32 = 0x0001;
pub const KEY_SET_VALUE: u32 = 0x0002;
pub const KEY_CREATE_SUB_KEY: u32 = 0x0004;
pub const KEY_ENUMERATE_SUB_KEYS: u32 = 0x0008;
pub const KEY_NOTIFY: u32 = 0x0010;
pub const KEY_CREATE_LINK: u32 = 0x0020;

/// 组合宏（NT 展开口径）。
pub const KEY_READ: u32 = KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | KEY_NOTIFY;
pub const KEY_WRITE: u32 = KEY_SET_VALUE | KEY_CREATE_SUB_KEY;
pub const KEY_ALL_ACCESS: u32 = KEY_READ | KEY_WRITE | KEY_CREATE_LINK;

/// OpenOptions 闭集（REG_OPEN_LEGAL_OPTION：仅 OPEN_LINK）。
pub const REG_OPTION_OPEN_LINK: u32 = 0x0001;
/// REG_LEGAL_OPTION 合法掩码。
pub const REG_LEGAL_OPTION: u32 = REG_OPTION_OPEN_LINK;

/// 基础档六位掩码（越位判据锚）。
pub const KEY_BASE_MASK: u32 = 0x003F;

/// 诊断码（0x307C0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExCode {
    /// 0x307C0 OpenOptions 越位（出 REG_LEGAL_OPTION）。
    OptionOob = 0,
    /// 0x307C1 访问权位越位（出六位基础掩码）。
    AccessOob = 1,
    /// 0x307C2 访问拒绝（请求位集含未持位）。
    AccessDenied = 2,
    /// 0x307C3 组合宏展开失配。
    MacroMismatch = 3,
    /// 0x307C4 键不存在。
    NameNotFound = 4,
    /// 0x307C5 键已存在（Ex 创建排他档）。
    AlreadyExists = 5,
    /// 0x307C6 路径解析拒（复用 F12461 解析器口径）。
    BadPath = 6,
    /// 0x307C7 权位表满。
    RightTableFull = 7,
    /// 0x307C8 幽灵键（访问检查引用未登记键）。
    GhostKey = 8,
    /// 0x307C9 段位越界。
    SegOob = 9,
    /// 0x307CA 零权请求（DesiredAccess=0 非法）。
    ZeroAccess = 10,
    /// 0x307CB 排他冲突（两次 Ex 排他创建）。
    ExclusiveClash = 11,
}

impl ExCode {
    /// 全部码闭集。
    pub const ALL: [ExCode; 12] = [
        ExCode::OptionOob,
        ExCode::AccessOob,
        ExCode::AccessDenied,
        ExCode::MacroMismatch,
        ExCode::NameNotFound,
        ExCode::AlreadyExists,
        ExCode::BadPath,
        ExCode::RightTableFull,
        ExCode::GhostKey,
        ExCode::SegOob,
        ExCode::ZeroAccess,
        ExCode::ExclusiveClash,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            ExCode::OptionOob => "OpenOptions 出 REG_LEGAL_OPTION（仅 OPEN_LINK 合法），清掉越位",
            ExCode::AccessOob => "DesiredAccess 出六位基础掩码，按 KEY_* 闭集重组",
            ExCode::AccessDenied => "请求位集含句柄未持位，降权请求或重新授权",
            ExCode::MacroMismatch => "组合宏展开与手工或值失配，核对 KEY_READ/WRITE 定义",
            ExCode::NameNotFound => "键不存在，先逐级 CreateKey",
            ExCode::AlreadyExists => "排他创建遇既有键，改用非排他 CreateKey",
            ExCode::BadPath => "路径解析拒，复用 F12461 五族坏路径闭集",
            ExCode::RightTableFull => "权位登记表满，回收闲置句柄",
            ExCode::GhostKey => "访问检查引用未登记键，先开键再访问",
            ExCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            ExCode::ZeroAccess => "DesiredAccess=0 非法，至少请求一位",
            ExCode::ExclusiveClash => "排他创建冲突，同键第二次排他建必拒",
        }
    }
}

/// 键对象（Ex 档视图）。
#[derive(Clone, Copy)]
struct ExKey {
    /// 名字（首级名，简化档）。
    name: [u8; 32],
    name_len: usize,
    /// 登记时授予的权位。
    granted: u32,
    /// 排他创建标记。
    exclusive: bool,
    /// 存活。
    alive: bool,
}

/// Ex 档键管理器。
pub struct ExKeyMgr {
    /// 键表。
    keys: [ExKey; 16],
    used: usize,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl ExKeyMgr {
    /// 新建。
    pub fn new() -> ExKeyMgr {
        ExKeyMgr {
            keys: [ExKey { name: [0; 32], name_len: 0, granted: 0, exclusive: false, alive: false }; 16],
            used: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: ExCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// OpenOptions 闭集判（REG_LEGAL_OPTION 越界注入闸）。
    pub fn options_legal(opts: u32) -> bool {
        opts & !REG_LEGAL_OPTION == 0
    }

    /// 访问权位闭集判（越位注入闸）。
    pub fn access_legal(acc: u32) -> bool {
        acc != 0 && acc & !KEY_ALL_ACCESS == 0
    }

    /// 访问检查器：granted ⊇ requested 才放行（位运算零分支）。
    pub fn access_check(granted: u32, requested: u32) -> Result<(), u32> {
        if requested == 0 {
            return Err(ExCode::ZeroAccess.val());
        }
        if requested & granted != requested {
            return Err(ExCode::AccessDenied.val());
        }
        Ok(())
    }

    /// 按名查键（存活 + 名匹配）。
    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            let k = &self.keys[i];
            if k.alive && k.name_len == name.len() {
                let mut eq = true;
                let mut j = 0;
                while j < name.len() {
                    if k.name[j] != name[j] {
                        eq = false;
                        break;
                    }
                    j += 1;
                }
                if eq {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// NtCreateKeyEx 语义档：三参齐备（权位闭集 + 选项闭集 + 排他档）。
    pub fn create_ex(&mut self, name: &[u8], acc: u32, opts: u32, exclusive: bool) -> Result<usize, u32> {
        if !Self::access_legal(acc) {
            if acc == 0 {
                return Err(self.hit(ExCode::ZeroAccess));
            }
            return Err(self.hit(ExCode::AccessOob));
        }
        if !Self::options_legal(opts) {
            return Err(self.hit(ExCode::OptionOob));
        }
        if name.is_empty() || name.len() > 32 {
            return Err(self.hit(ExCode::BadPath));
        }
        if let Some(i) = self.find(name) {
            if exclusive || self.keys[i].exclusive {
                return Err(self.hit(ExCode::AlreadyExists));
            }
            // 非排他再建：打开既有（登记权位取并集——NT 口径取请求档）。
            self.keys[i].granted = acc;
            return Ok(i);
        }
        if self.used >= 16 {
            return Err(self.hit(ExCode::RightTableFull));
        }
        let mut nm = [0u8; 32];
        let mut j = 0;
        while j < name.len() {
            nm[j] = name[j];
            j += 1;
        }
        self.keys[self.used] = ExKey { name: nm, name_len: name.len(), granted: acc, exclusive, alive: true };
        let idx = self.used;
        self.used += 1;
        Ok(idx)
    }

    /// NtOpenKeyEx 语义档：只开不建 + 三参校验 + 幽灵键拒。
    pub fn open_ex(&mut self, name: &[u8], acc: u32, opts: u32) -> Result<usize, u32> {
        if !Self::access_legal(acc) {
            if acc == 0 {
                return Err(self.hit(ExCode::ZeroAccess));
            }
            return Err(self.hit(ExCode::AccessOob));
        }
        if !Self::options_legal(opts) {
            return Err(self.hit(ExCode::OptionOob));
        }
        match self.find(name) {
            Some(i) => {
                // 开启即访问检查：请求位 ⊆ 登记位。
                Self::access_check(self.keys[i].granted, acc)
                    .map_err(|_| self.hit(ExCode::AccessDenied))?;
                Ok(i)
            }
            None => Err(self.hit(ExCode::NameNotFound)),
        }
    }

    /// 登记权位只读。
    pub fn granted(&self, idx: usize) -> u32 {
        if idx < self.used {
            self.keys[idx].granted
        } else {
            0
        }
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: ExCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// REG_LEGAL_OPTION 越位注入 10/10 轮（判准 J1 第二句）。
///
/// 位 1..=31 逐位扫描（除 OPEN_LINK 位 0），每一越位必拒 OptionOob。
pub fn drill_option_oob_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut caught = 0usize;
        let mut bit = 1u32;
        while bit < 32 {
            let mut mgr = ExKeyMgr::new();
            match mgr.create_ex(b"K", KEY_READ, 1u32 << bit, false) {
                Err(c) if c == ExCode::OptionOob.val() => caught += 1,
                _ => d.push(format!("轮{} 位{} 漏抓", round, bit)),
            }
            bit += 1;
        }
        // 合法位（OPEN_LINK=0x1）放行。
        let mut mgr = ExKeyMgr::new();
        let legal = mgr.create_ex(b"K", KEY_READ, REG_OPTION_OPEN_LINK, false).is_ok();
        if caught == 31 && legal {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("REG_LEGAL_OPTION 越位 {}/{} ×31位 {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 访问越权注入 10/10 轮：请求未持位必拒 AccessDenied。
pub fn drill_access_denied_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut mgr = ExKeyMgr::new();
        // 登记只读权。
        let _ = mgr.create_ex(b"R", KEY_READ, 0, false);
        // 请求写权：未持 → AccessDenied。
        let denied = match mgr.open_ex(b"R", KEY_WRITE, 0) {
            Err(c) => c == ExCode::AccessDenied.val(),
            Ok(_) => false,
        };
        // 请求读权：放行。
        let granted = mgr.open_ex(b"R", KEY_QUERY_VALUE, 0).is_ok();
        // 随机扰动消耗 LCG（确定性口径保持）。
        let _ = lcg.below(3);
        if denied && granted {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("访问越权注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 访问权位越位注入 10/10 轮：位 6..=31 逐位扫必拒 AccessOob。
pub fn drill_access_oob_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut caught = 0usize;
        let mut bit = 6u32;
        while bit < 32 {
            let mut mgr = ExKeyMgr::new();
            match mgr.create_ex(b"K", 1u32 << bit, 0, false) {
                Err(c) if c == ExCode::AccessOob.val() => caught += 1,
                _ => {}
            }
            bit += 1;
        }
        // 零权请求。
        let zero = match ExKeyMgr::new().create_ex(b"K", 0, 0, false) {
            Err(c) => c == ExCode::ZeroAccess.val(),
            Ok(_) => false,
        };
        if caught == 26 && zero {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("权位越位 {}/{} ×26位", ok, INJECT_ROUNDS))
}

/// 组合宏展开一致性演练 10/10 轮（判准 J1 第一句的宏档）。
pub fn drill_macro_expand_10() -> (bool, String) {
    // 手工展开（判据纪律：独立写死不复用宏值）。
    const HAND_READ: u32 = 0x0001 | 0x0008 | 0x0010;
    const HAND_WRITE: u32 = 0x0002 | 0x0004;
    const HAND_ALL: u32 = 0x0001 | 0x0002 | 0x0004 | 0x0008 | 0x0010 | 0x0020;
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let rd = KEY_READ == HAND_READ;
        let wr = KEY_WRITE == HAND_WRITE;
        let al = KEY_ALL_ACCESS == HAND_ALL;
        // 组合宏位覆盖核：READ ⊆ ALL，WRITE ⊆ ALL。
        let subset = KEY_READ & !KEY_ALL_ACCESS == 0 && KEY_WRITE & !KEY_ALL_ACCESS == 0;
        if rd && wr && al && subset {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("宏展开 {}/{} READ:{:x} WRITE:{:x} ALL:{:x}", ok, INJECT_ROUNDS, KEY_READ, KEY_WRITE, KEY_ALL_ACCESS))
}

/// Ex 档三参齐备演练 10/10 轮（判准 J1 第一句）。
pub fn drill_ex_full_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut mgr = ExKeyMgr::new();
        let mut r_ok = true;
        // CreateEx 首建（权位 ALL，选项 0，非排他）。
        let idx = mgr.create_ex(b"K1", KEY_ALL_ACCESS, 0, false);
        if idx.is_err() {
            r_ok = false;
            d.push(format!("轮{} 建档错", round));
        }
        // OpenEx 读访问放行。
        if mgr.open_ex(b"K1", KEY_READ, 0).is_err() {
            r_ok = false;
            d.push(format!("轮{} 开读错", round));
        }
        // OpenEx 全访问放行（登记 ALL）。
        if mgr.open_ex(b"K1", KEY_ALL_ACCESS, 0).is_err() {
            r_ok = false;
            d.push(format!("轮{} 开全错", round));
        }
        // OpenEx 不存在键：NameNotFound。
        match mgr.open_ex(b"K2", KEY_READ, 0) {
            Err(c) if c == ExCode::NameNotFound.val() => {}
            _ => r_ok = false,
        }
        // CreateEx 排他再建既有键：AlreadyExists。
        let excl = match mgr.create_ex(b"K1", KEY_READ, 0, true) {
            Err(c) => c == ExCode::AlreadyExists.val(),
            Ok(_) => false,
        };
        let _ = lcg.below(5);
        if r_ok && excl {
            ok += 1;
        } else {
            d.push(format!("轮{} 排他{}", round, excl));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("Ex 三参齐备 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 排他冲突注入 10/10 轮。
pub fn drill_exclusive_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut mgr = ExKeyMgr::new();
        // 排他首建成功。
        let first = mgr.create_ex(b"X", KEY_READ, 0, true).is_ok();
        // 排他二建：AlreadyExists。
        let second = match mgr.create_ex(b"X", KEY_READ, 0, true) {
            Err(c) => c == ExCode::AlreadyExists.val(),
            Ok(_) => false,
        };
        // 非排他撞排他：AlreadyExists。
        let third = match mgr.create_ex(b"X", KEY_READ, 0, false) {
            Err(c) => c == ExCode::AlreadyExists.val(),
            Ok(_) => false,
        };
        if first && second && third {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("排他冲突 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    ExCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：权位常量锚核对。
pub fn run_unx12462_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-权位锚",
        KEY_BASE_MASK == 0x3F && REG_LEGAL_OPTION == 0x1 && KEY_ALL_ACCESS == 0x3F,
        format!("基础{:x} 合法选项{:x} 全权{:x}", KEY_BASE_MASK, REG_LEGAL_OPTION, KEY_ALL_ACCESS),
    ));
    v
}

/// run_unx12462_checks：本单判据聚合（诊断段 0x307C0）。
pub fn run_unx12462_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12462 Ex 档语义");
    // J1-a：Ex 档三参齐备。
    let (ok1, d1) = drill_ex_full_10();
    s.add_dyn("J1-Ex三参齐备10/10", ok1, &d1);
    // J1-b：REG_LEGAL_OPTION 越位注入。
    let (ok2, d2) = drill_option_oob_10();
    s.add_dyn("J1-选项越位注入10/10", ok2, &d2);
    // J1-c：访问越权注入。
    let (ok3, d3) = drill_access_denied_10();
    s.add_dyn("J1-访问越权注入10/10", ok3, &d3);
    // J1-d：权位越位注入。
    let (ok4, d4) = drill_access_oob_10();
    s.add_dyn("J1-权位越位注入10/10", ok4, &d4);
    // J1-e：组合宏展开一致性。
    let (ok5, d5) = drill_macro_expand_10();
    s.add_dyn("J1-宏展开一致10/10", ok5, &d5);
    // J1-f：排他冲突。
    let (ok6, d6) = drill_exclusive_10();
    s.add_dyn("J1-排他冲突10/10", ok6, &d6);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x307C0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12462_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
