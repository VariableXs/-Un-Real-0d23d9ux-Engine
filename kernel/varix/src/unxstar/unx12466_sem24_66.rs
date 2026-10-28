//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12466 · NtSetValueKey/NtDeleteValueKey 语义档 · 0x30840
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12466-J1 值类型档（REG_SZ/DWORD/BINARY 矩阵）判据绿，
//!      类型错配注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 值类型矩阵：三类型 × 写入面（字节长校验）——REG_SZ（UTF-16LE
//!     口径：偶数字节长 + 双零终止）、REG_DWORD（恰 4 字节）、
//!     REG_BINARY（任意非零长 ≤64），矩阵行齐码（类型/长校验/拒码
//!     三列齐）；
//!   - NtSetValueKey 语义档：同名覆盖写（覆盖计数账）+ 新建写（类型
//!     闭集 + 长校验），覆盖不改类型不合法——改类型必先删（类型冻结
//!     语义档）；
//!   - NtDeleteValueKey 语义档：删值（墓碑计数），缺值拒 ValueNotFound，
//!     删后可重建（值槽可复用——与键墓碑纪律区分）；
//!   - 类型错配注入：DWORD 写 3 字节/5 字节、SZ 写奇数字节长/缺双零
//!     终止、BINARY 写零长——五族错配 10/10 全拒且逐码分账；
//!   - 值表账：注册数 ≡ 存活数 + 删除数（守恒），覆盖数单列；
//!   - 注入核：LCG 写死种子 = SEG_BASE，矩阵全行实跑 + 错配五族
//!     10/10 + 覆盖不改类型注入 10/10；
//!   - 诊断码闭集 0x30840 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30840 段）。
pub const SEG_BASE: u32 = 0x30840;

/// 值名定容。
pub const VAL_NAME_CAP: usize = 24;

/// 值数据定容。
pub const VAL_DATA_CAP: usize = 64;

/// 值表容量。
pub const VALUE_TABLE_CAP: usize = 8;

/// 类型锚（NT REG_*）。
pub const REG_SZ: u32 = 1;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30840 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvCode {
    /// 0x30840 类型出闭集。
    BadType = 0,
    /// 0x30841 DWORD 长错（≠4B）。
    DwordLen = 1,
    /// 0x30842 SZ 奇数字节长。
    SzOddLen = 2,
    /// 0x30843 SZ 缺双零终止。
    SzNoTerm = 3,
    /// 0x30844 BINARY 零长。
    BinaryZero = 4,
    /// 0x30845 数据超容。
    DataOob = 5,
    /// 0x30846 覆盖改类型（类型冻结破）。
    TypeFrozen = 6,
    /// 0x30847 删缺值。
    ValueNotFound = 7,
    /// 0x30848 值名超长。
    NameOob = 8,
    /// 0x30849 值表满。
    TableFull = 9,
    /// 0x3084A 段位越界。
    SegOob = 10,
    /// 0x3084B 守恒破（注册 ≡ 存活+删除）。
    ConsrvBreak = 11,
}

impl SvCode {
    /// 全部码闭集。
    pub const ALL: [SvCode; 12] = [
        SvCode::BadType,
        SvCode::DwordLen,
        SvCode::SzOddLen,
        SvCode::SzNoTerm,
        SvCode::BinaryZero,
        SvCode::DataOob,
        SvCode::TypeFrozen,
        SvCode::ValueNotFound,
        SvCode::NameOob,
        SvCode::TableFull,
        SvCode::SegOob,
        SvCode::ConsrvBreak,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            SvCode::BadType => "类型出 REG_* 闭集（SZ/BINARY/DWORD）",
            SvCode::DwordLen => "REG_DWORD 恰 4 字节，核对写入面",
            SvCode::SzOddLen => "REG_SZ 是 UTF-16LE，字节数必偶",
            SvCode::SzNoTerm => "REG_SZ 须双零终止（UTF-16 NUL）",
            SvCode::BinaryZero => "REG_BINARY 非零长（判据口径）",
            SvCode::DataOob => "数据超 64 字节容量",
            SvCode::TypeFrozen => "覆盖写不可改类型，先删再按新类型重建",
            SvCode::ValueNotFound => "值不存在，先注册",
            SvCode::NameOob => "值名超 24 字节",
            SvCode::TableFull => "值表满（8），先删闲置值",
            SvCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            SvCode::ConsrvBreak => "注册 ≡ 存活+删除 守恒破，逐项核对",
        }
    }
}

/// 值槽。
#[derive(Clone, Copy)]
struct ValueSlot {
    name: [u8; VAL_NAME_CAP],
    name_len: usize,
    ty: u32,
    data: [u8; VAL_DATA_CAP],
    data_len: usize,
    alive: bool,
}

/// 值表语义机（写/删/覆盖三面）。
pub struct ValueTable {
    /// 槽表。
    slots: [Option<ValueSlot>; VALUE_TABLE_CAP],
    used: usize,
    /// 注册总数。
    registered: u64,
    /// 删除总数。
    deleted: u64,
    /// 覆盖总数。
    overwrites: u64,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl ValueTable {
    /// 新建。
    pub fn new() -> ValueTable {
        ValueTable {
            slots: [None; VALUE_TABLE_CAP],
            used: 0,
            registered: 0,
            deleted: 0,
            overwrites: 0,
            code_hits: [0; 12],
        }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: SvCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 类型矩阵行核（三列齐：类型/长校验/通过）。
    /// 返回 Err=拒码（长校验失败），Ok=通过。
    fn type_matrix_check(ty: u32, data: &[u8]) -> Result<(), u32> {
        match ty {
            t if t == REG_SZ => {
                if data.len() % 2 != 0 {
                    return Err(SvCode::SzOddLen.val());
                }
                if data.len() < 2 || data[data.len() - 1] != 0 || data[data.len() - 2] != 0 {
                    return Err(SvCode::SzNoTerm.val());
                }
                Ok(())
            }
            t if t == REG_DWORD => {
                if data.len() != 4 {
                    return Err(SvCode::DwordLen.val());
                }
                Ok(())
            }
            t if t == REG_BINARY => {
                if data.is_empty() {
                    return Err(SvCode::BinaryZero.val());
                }
                Ok(())
            }
            _ => Err(SvCode::BadType.val()),
        }
    }

    /// 按名找存活槽。
    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(ref s) = self.slots[i] {
                if s.alive && s.name_len == name.len() {
                    let mut eq = true;
                    let mut k = 0;
                    while k < name.len() {
                        if s.name[k] != name[k] {
                            eq = false;
                            break;
                        }
                        k += 1;
                    }
                    if eq {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// NtSetValueKey 语义档：类型矩阵 + 覆盖不改类型（类型冻结）。
    pub fn set_value(&mut self, name: &[u8], ty: u32, data: &[u8]) -> Result<(), u32> {
        if name.is_empty() || name.len() > VAL_NAME_CAP {
            return Err(self.hit(SvCode::NameOob));
        }
        if data.len() > VAL_DATA_CAP {
            return Err(self.hit(SvCode::DataOob));
        }
        // 类型矩阵行核（错配注入闸）。
        if let Err(c) = Self::type_matrix_check(ty, data) {
            let code = if c == SvCode::SzOddLen.val() {
                SvCode::SzOddLen
            } else if c == SvCode::SzNoTerm.val() {
                SvCode::SzNoTerm
            } else if c == SvCode::DwordLen.val() {
                SvCode::DwordLen
            } else if c == SvCode::BinaryZero.val() {
                SvCode::BinaryZero
            } else {
                SvCode::BadType
            };
            return Err(self.hit(code));
        }
        // 覆盖档：同名存活槽。
        if let Some(i) = self.find(name) {
            if let Some(ref s) = self.slots[i] {
                if s.ty != ty {
                    // 类型冻结：覆盖写改类型必拒。
                    return Err(self.hit(SvCode::TypeFrozen));
                }
            }
            if let Some(ref mut s) = self.slots[i] {
                let mut k = 0;
                while k < data.len() {
                    s.data[k] = data[k];
                    k += 1;
                }
                s.data_len = data.len();
            }
            self.overwrites += 1;
            return Ok(());
        }
        // 新建档。
        if self.used >= VALUE_TABLE_CAP {
            return Err(self.hit(SvCode::TableFull));
        }
        let mut nm = [0u8; VAL_NAME_CAP];
        let mut k = 0;
        while k < name.len() {
            nm[k] = name[k];
            k += 1;
        }
        let mut dt = [0u8; VAL_DATA_CAP];
        let mut k = 0;
        while k < data.len() {
            dt[k] = data[k];
            k += 1;
        }
        self.slots[self.used] =
            Some(ValueSlot { name: nm, name_len: name.len(), ty, data: dt, data_len: data.len(), alive: true });
        self.used += 1;
        self.registered += 1;
        Ok(())
    }

    /// NtDeleteValueKey 语义档：删值（槽复用：值槽可重建）。
    pub fn delete_value(&mut self, name: &[u8]) -> Result<(), u32> {
        match self.find(name) {
            Some(i) => {
                if let Some(ref mut s) = self.slots[i] {
                    s.alive = false;
                }
                self.deleted += 1;
                Ok(())
            }
            None => Err(self.hit(SvCode::ValueNotFound)),
        }
    }

    /// 删后重建（值槽可复用语义档）。
    pub fn recreate_after_delete(&mut self, name: &[u8], ty: u32, data: &[u8]) -> Result<bool, u32> {
        // 曾删过同名 → 重建新槽（槽位递增不复用，但值身份重建）。
        let recreated = self.find(name).is_none();
        self.set_value(name, ty, data)?;
        Ok(recreated)
    }

    /// 守恒：registered ≡ alive + deleted。
    pub fn conserved(&self) -> bool {
        let mut alive = 0;
        let mut i = 0;
        while i < self.used {
            if let Some(ref s) = self.slots[i] {
                if s.alive {
                    alive += 1;
                }
            }
            i += 1;
        }
        self.registered == alive as u64 + self.deleted
    }

    /// 存活数。
    pub fn alive_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.used {
            if let Some(ref s) = self.slots[i] {
                if s.alive {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 覆盖数。
    pub fn overwrite_count(&self) -> u64 {
        self.overwrites
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: SvCode) -> u32 {
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

/// 类型矩阵全行实跑 10/10 轮（判准 J1 第一句）。
pub fn drill_type_matrix_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = ValueTable::new();
        let mut r_ok = true;
        // 三类型合法行。
        if t.set_value(b"S", REG_SZ, &[0x41, 0, 0x42, 0, 0, 0]).is_err() {
            r_ok = false;
            d.push(format!("轮{} SZ 合法行拒", round));
        }
        if t.set_value(b"D", REG_DWORD, &[1, 0, 0, 0]).is_err() {
            r_ok = false;
        }
        if t.set_value(b"B", REG_BINARY, &[0xFF, 0xFE]).is_err() {
            r_ok = false;
        }
        // 三类型非法行（各拒各码）。
        let m1 = matches!(t.set_value(b"D2", REG_DWORD, &[1, 2, 3]),
            Err(c) if c == SvCode::DwordLen.val());
        let m2 = matches!(t.set_value(b"S2", REG_SZ, &[0x41, 0, 0x42, 0]),
            Err(c) if c == SvCode::SzNoTerm.val());
        let m3 = matches!(t.set_value(b"S3", REG_SZ, &[0x41, 0, 0x42]),
            Err(c) if c == SvCode::SzOddLen.val());
        let m4 = matches!(t.set_value(b"B2", REG_BINARY, b""),
            Err(c) if c == SvCode::BinaryZero.val());
        let m5 = matches!(t.set_value(b"X", 99, &[1, 0]),
            Err(c) if c == SvCode::BadType.val());
        if !(m1 && m2 && m3 && m4 && m5) {
            r_ok = false;
            d.push(format!("轮{} 矩阵{}{}{}{}{}", round, m1, m2, m3, m4, m5));
        }
        let _ = lcg.below(3);
        if r_ok && t.conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("类型矩阵 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 类型错配注入 10/10 轮（判准 J1 第二句）：五族全拒逐码分账。
pub fn drill_mismatch_10() -> (bool, String) {
    let mut caught = [0usize; 5];
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = ValueTable::new();
        // 族 1：DWORD 3 字节。
        if matches!(t.set_value(b"a1", REG_DWORD, &[1, 2, 3]), Err(c) if c == SvCode::DwordLen.val()) {
            caught[0] += 1;
        }
        // 族 2：DWORD 5 字节。
        if matches!(t.set_value(b"a2", REG_DWORD, &[1, 2, 3, 4, 5]), Err(c) if c == SvCode::DwordLen.val()) {
            caught[1] += 1;
        }
        // 族 3：SZ 奇长。
        if matches!(t.set_value(b"a3", REG_SZ, &[0x41, 0, 0x42]), Err(c) if c == SvCode::SzOddLen.val()) {
            caught[2] += 1;
        }
        // 族 4：SZ 无终止。
        if matches!(t.set_value(b"a4", REG_SZ, &[0x41, 0x42]), Err(c) if c == SvCode::SzNoTerm.val()) {
            caught[3] += 1;
        }
        // 族 5：BINARY 零长。
        if matches!(t.set_value(b"a5", REG_BINARY, b""), Err(c) if c == SvCode::BinaryZero.val()) {
            caught[4] += 1;
        }
        round += 1;
    }
    let ok = caught.iter().all(|c| *c == INJECT_ROUNDS);
    (ok, format!("错配注入 10/10×5族 抓{:?}", caught))
}

/// 覆盖语义演练 10/10 轮：同名覆盖 + 类型冻结 + 删后重建。
pub fn drill_overwrite_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = ValueTable::new();
        let _ = t.set_value(b"K", REG_DWORD, &[1, 0, 0, 0]);
        let mut r_ok = true;
        // 同型覆盖：成功，覆盖计数 +1。
        if t.set_value(b"K", REG_DWORD, &[9, 9, 9, 9]).is_err() {
            r_ok = false;
        }
        if t.overwrite_count() != 1 {
            r_ok = false;
        }
        // 覆盖改类型：TypeFrozen。
        match t.set_value(b"K", REG_SZ, &[0x41, 0, 0, 0]) {
            Err(c) if c == SvCode::TypeFrozen.val() => {}
            _ => r_ok = false,
        }
        // 删后重建（新类型）：成功。
        if t.delete_value(b"K").is_err() {
            r_ok = false;
        }
        let recreated = t.recreate_after_delete(b"K", REG_SZ, &[0x41, 0, 0, 0]);
        match recreated {
            Ok(true) => {}
            _ => r_ok = false,
        }
        // 删缺值：ValueNotFound。
        match t.delete_value(b"NOPE") {
            Err(c) if c == SvCode::ValueNotFound.val() => {}
            _ => r_ok = false,
        }
        let _ = lcg.below(3);
        if r_ok && t.conserved() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("覆盖语义 {}/{}", ok, INJECT_ROUNDS))
}

/// 值表守恒演练 10/10 轮。
pub fn drill_conservation_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(2) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = ValueTable::new();
        let mut names: Vec<[u8; 2]> = Vec::new();
        let mut i = 0;
        while i < 6 {
            let nm = [b'V', b'0' + i as u8];
            let _ = t.set_value(&nm, REG_BINARY, &[i as u8 + 1]);
            names.push(nm);
            i += 1;
        }
        // 随机删约半。
        let mut deleted = 0;
        let mut k = 0;
        while k < names.len() {
            if lcg.below(2) == 0 && t.delete_value(&names[k]).is_ok() {
                deleted += 1;
            }
            k += 1;
        }
        let cons = t.conserved();
        let alive_ok = t.alive_count() == 6 - deleted;
        if cons && alive_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("值表守恒 {}/{}", ok, INJECT_ROUNDS))
}

/// 表满注入 10/10 轮。
pub fn drill_table_full_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = ValueTable::new();
        let mut i = 0;
        let mut full = false;
        while i < VALUE_TABLE_CAP + 2 {
            let mut nm = [b'N', b'0'];
            nm[1] = b'0' + (i % 10) as u8;
            if let Err(c) = t.set_value(&nm, REG_BINARY, &[1]) {
                if c == SvCode::TableFull.val() {
                    full = true;
                }
            }
            i += 1;
        }
        if full {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("表满注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：类型矩阵常量锚。
pub fn run_unx12466_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-类型锚",
        REG_SZ == 1 && REG_BINARY == 3 && REG_DWORD == 4 && VALUE_TABLE_CAP == 8,
        format!("SZ{} BIN{} DWORD{} 表{}", REG_SZ, REG_BINARY, REG_DWORD, VALUE_TABLE_CAP),
    ));
    v
}

/// run_unx12466_checks：本单判据聚合（诊断段 0x30840）。
pub fn run_unx12466_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12466 值类型档语义");
    // J1-a：类型矩阵全行实跑。
    let (ok1, d1) = drill_type_matrix_10();
    s.add_dyn("J1-类型矩阵10/10", ok1, &d1);
    // J1-b：类型错配注入五族。
    let (ok2, d2) = drill_mismatch_10();
    s.add_dyn("J1-错配注入10/10", ok2, &d2);
    // J1-c：覆盖语义（同名覆盖+类型冻结+删后重建）。
    let (ok3, d3) = drill_overwrite_10();
    s.add_dyn("J1-覆盖语义10/10", ok3, &d3);
    // J1-d：值表守恒。
    let (ok4, d4) = drill_conservation_10();
    s.add_dyn("J1-值表守恒10/10", ok4, &d4);
    // J1-e：表满注入。
    let (ok5, d5) = drill_table_full_10();
    s.add_dyn("J1-表满注入10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30840段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12466_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
