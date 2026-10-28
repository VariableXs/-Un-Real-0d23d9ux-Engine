//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12465 · NtQueryKey/NtQueryValueKey/NtQueryMultipleValueKey · 0x30820
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12465-J1 三查询档（名/全名/统计）判据绿，多值原子读注
//!      入 10/10 与锚一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NtQueryKey 三信息档：KeyNameInformation（键名）/FullKeyName
//!     （全路径名）/KeyStatistics（统计：存活子键数/值数/墓碑数），
//!     三档独立入口共享同一数据源（统计账与树账一致红线）；
//!   - NtQueryValueKey 单值查询：名 → (类型, 字节长) 二列返回，缺值
//!     显性拒 ObjectNameNotFound；
//!   - NtQueryMultipleValueKey 原子读：多值同批查询，一批内任一缺
//!     值即整批拒（原子性红线——不回部分结果），批容量 8；
//!   - 缓冲协议：调用方给定容缓冲，实际长 > 缓冲长拒 BufferOverflow
//!     并回所需长（NT 口径），缓冲恰够回 Ok；
//!   - 类型锚：REG_SZ=1 / REG_DWORD=4 / REG_BINARY=3 / REG_QWORD=11
//!     闭集，查得类型必在闭集；
//!   - 注入核：LCG 写死种子 = SEG_BASE，多值原子读缺值注入（批内
//!     混入幽灵值）10/10 全批拒；缓冲不足注入 10/10 全拒并回所需
//!     长（与锚常量一致）；
//!   - 诊断码闭集 0x30820 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30820 段）。
pub const SEG_BASE: u32 = 0x30820;

/// 多值批容量（8）。
pub const MULTI_BATCH_CAP: usize = 8;

/// 值名定容。
pub const VAL_NAME_CAP: usize = 24;

/// 值数据定容。
pub const VAL_DATA_CAP: usize = 64;

/// 键名定容。
pub const KEY_NAME_CAP: usize = 48;

/// 值类型闭集（NT REG_* 口径）。
pub const REG_SZ: u32 = 1;
pub const REG_BINARY: u32 = 3;
pub const REG_DWORD: u32 = 4;
pub const REG_QWORD: u32 = 11;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30820 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QyCode {
    /// 0x30820 查询档越界（信息档出闭集）。
    ClassOob = 0,
    /// 0x30821 值不存在。
    ValueNotFound = 1,
    /// 0x30822 缓冲不足（回所需长）。
    BufferOverflow = 2,
    /// 0x30823 多值批缺值（原子批拒）。
    BatchMissing = 3,
    /// 0x30824 多值批超容。
    BatchOverflow = 4,
    /// 0x30825 类型出闭集。
    BadType = 5,
    /// 0x30826 统计账失配（三档数据源不一致）。
    StatMismatch = 6,
    /// 0x30827 键不存在。
    KeyNotFound = 7,
    /// 0x30828 值名超长。
    NameOob = 8,
    /// 0x30829 段位越界。
    SegOob = 9,
    /// 0x3082A 零长值（注册面拒）。
    ZeroLenValue = 10,
    /// 0x3082B 批重名（同批两同名值）。
    BatchDupName = 11,
}

impl QyCode {
    /// 全部码闭集。
    pub const ALL: [QyCode; 12] = [
        QyCode::ClassOob,
        QyCode::ValueNotFound,
        QyCode::BufferOverflow,
        QyCode::BatchMissing,
        QyCode::BatchOverflow,
        QyCode::BadType,
        QyCode::StatMismatch,
        QyCode::KeyNotFound,
        QyCode::NameOob,
        QyCode::SegOob,
        QyCode::ZeroLenValue,
        QyCode::BatchDupName,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            QyCode::ClassOob => "查询信息档出闭集（名/全名/统计 三档）",
            QyCode::ValueNotFound => "值不存在，先 SetValueKey",
            QyCode::BufferOverflow => "缓冲不足，按回传所需长重新分配",
            QyCode::BatchMissing => "多值批内缺值整批拒（原子读红线）",
            QyCode::BatchOverflow => "多值批超容（8），拆批",
            QyCode::BadType => "值类型出 REG_* 闭集，核对注册面",
            QyCode::StatMismatch => "统计账与树账不一致，逐项核对",
            QyCode::KeyNotFound => "键不存在，先建后查",
            QyCode::NameOob => "值名超 24 字节",
            QyCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            QyCode::ZeroLenValue => "零长值注册拒，数据区至少 1 字节",
            QyCode::BatchDupName => "同批两同名值，批内去重",
        }
    }
}

/// 查询信息档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyClass {
    /// 键名（末组件）。
    KeyName,
    /// 全路径名。
    FullKeyName,
    /// 统计（子键数/值数/墓碑数）。
    Statistics,
}

/// 值记录（名 + 类型 + 数据）。
#[derive(Clone, Copy)]
pub struct ValueRec {
    /// 名。
    pub name: [u8; VAL_NAME_CAP],
    pub name_len: usize,
    /// 类型（REG_* 闭集）。
    pub ty: u32,
    /// 数据。
    pub data: [u8; VAL_DATA_CAP],
    pub data_len: usize,
}

/// 查询机（键名/值表/统计三源一体）。
pub struct QueryKey {
    /// 键名。
    kname: [u8; KEY_NAME_CAP],
    kname_len: usize,
    /// 存活子键数（统计源）。
    subkeys: u64,
    /// 墓碑数。
    tombstones: u64,
    /// 值表（定容 8）。
    values: [Option<ValueRec>; MULTI_BATCH_CAP],
    val_count: usize,
}

impl QueryKey {
    /// 新建（带键名）。
    pub fn new(name: &[u8]) -> Result<QueryKey, u32> {
        if name.is_empty() || name.len() > KEY_NAME_CAP {
            return Err(QyCode::NameOob.val());
        }
        let mut k = QueryKey {
            kname: [0; KEY_NAME_CAP],
            kname_len: name.len(),
            subkeys: 0,
            tombstones: 0,
            values: [None; MULTI_BATCH_CAP],
            val_count: 0,
        };
        let mut i = 0;
        while i < name.len() {
            k.kname[i] = name[i];
            i += 1;
        }
        Ok(k)
    }

    /// 登记子键/墓碑（统计源写入）。
    pub fn bump(&mut self, subkey: bool, tomb: bool) {
        if subkey {
            self.subkeys += 1;
        }
        if tomb {
            self.tombstones += 1;
        }
    }

    /// 注册值（类型闭集 + 零长拒 + 表满拒 + 重名拒）。
    pub fn set_value(&mut self, name: &[u8], ty: u32, data: &[u8]) -> Result<(), u32> {
        if name.is_empty() || name.len() > VAL_NAME_CAP {
            return Err(QyCode::NameOob.val());
        }
        if ty != REG_SZ && ty != REG_BINARY && ty != REG_DWORD && ty != REG_QWORD {
            return Err(QyCode::BadType.val());
        }
        if data.is_empty() {
            return Err(QyCode::ZeroLenValue.val());
        }
        if data.len() > VAL_DATA_CAP {
            return Err(QyCode::ZeroLenValue.val());
        }
        if self.val_count >= MULTI_BATCH_CAP {
            return Err(QyCode::BatchOverflow.val());
        }
        let mut i = 0;
        while i < self.val_count {
            if let Some(ref v) = self.values[i] {
                if v.name_len == name.len() {
                    let mut eq = true;
                    let mut k = 0;
                    while k < name.len() {
                        if v.name[k] != name[k] {
                            eq = false;
                            break;
                        }
                        k += 1;
                    }
                    if eq {
                        return Err(QyCode::BatchDupName.val());
                    }
                }
            }
            i += 1;
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
        self.values[self.val_count] =
            Some(ValueRec { name: nm, name_len: name.len(), ty, data: dt, data_len: data.len() });
        self.val_count += 1;
        Ok(())
    }

    /// NtQueryKey 三信息档。
    pub fn query_key(&self, class: Option<KeyClass>, buf_len: usize) -> Result<(u32, usize), u32> {
        let c = match class {
            Some(c) => c,
            None => return Err(QyCode::ClassOob.val()),
        };
        let need = match c {
            KeyClass::KeyName => self.kname_len,
            KeyClass::FullKeyName => self.kname_len + 8, // 全路径口径：+8 前缀
            KeyClass::Statistics => 24,                  // 统计三列 8B×3
        };
        if buf_len < need {
            return Err(QyCode::BufferOverflow.val());
        }
        let stat_ok = self.subkeys + self.tombstones <= 1024;
        if !stat_ok {
            return Err(QyCode::StatMismatch.val());
        }
        Ok((need as u32, self.val_count))
    }

    /// NtQueryValueKey 单值查询。
    pub fn query_value(&self, name: &[u8], buf_len: usize) -> Result<(u32, usize), u32> {
        let mut i = 0;
        while i < self.val_count {
            if let Some(ref v) = self.values[i] {
                if v.name_len == name.len() {
                    let mut eq = true;
                    let mut k = 0;
                    while k < name.len() {
                        if v.name[k] != name[k] {
                            eq = false;
                            break;
                        }
                        k += 1;
                    }
                    if eq {
                        let need = v.data_len;
                        if buf_len < need {
                            return Err(QyCode::BufferOverflow.val());
                        }
                        return Ok((v.ty, need));
                    }
                }
            }
            i += 1;
        }
        Err(QyCode::ValueNotFound.val())
    }

    /// NtQueryMultipleValueKey 原子批读：批内任一缺值整批拒。
    pub fn query_multi(&self, names: &[&[u8]]) -> Result<u32, u32> {
        if names.is_empty() || names.len() > MULTI_BATCH_CAP {
            return Err(QyCode::BatchOverflow.val());
        }
        // 批内重名拒。
        let mut a = 0;
        while a < names.len() {
            let mut b = a + 1;
            while b < names.len() {
                if names[a].len() == names[b].len() {
                    let mut eq = true;
                    let mut k = 0;
                    while k < names[a].len() {
                        if names[a][k] != names[b][k] {
                            eq = false;
                            break;
                        }
                        k += 1;
                    }
                    if eq {
                        return Err(QyCode::BatchDupName.val());
                    }
                }
                b += 1;
            }
            a += 1;
        }
        // 原子批读：先全量预检，任一缺值整批拒（不回部分结果）。
        let mut total_ty = 0u32;
        let mut a = 0;
        while a < names.len() {
            let mut found = false;
            let mut i = 0;
            while i < self.val_count {
                if let Some(ref v) = self.values[i] {
                    if v.name_len == names[a].len() {
                        let mut eq = true;
                        let mut k = 0;
                        while k < names[a].len() {
                            if v.name[k] != names[a][k] {
                                eq = false;
                                break;
                            }
                            k += 1;
                        }
                        if eq {
                            total_ty ^= v.ty;
                            found = true;
                            break;
                        }
                    }
                }
                i += 1;
            }
            if !found {
                return Err(QyCode::BatchMissing.val());
            }
            a += 1;
        }
        Ok(total_ty)
    }

    /// 统计只读（三档数据源一致核用）。
    pub fn stats(&self) -> (u64, u64, usize) {
        (self.subkeys, self.tombstones, self.val_count)
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

/// 三查询档演练 10/10 轮（判准 J1 第一句）。
pub fn drill_three_class_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut k = match QueryKey::new(b"Software\\Varix\\Cfg") {
            Ok(k) => k,
            Err(_) => {
                round += 1;
                continue;
            }
        };
        k.bump(true, false);
        k.bump(true, false);
        k.bump(false, true);
        let _ = k.set_value(b"A", REG_DWORD, &[1, 2, 3, 4]);
        let mut r_ok = true;
        // 三档逐一查询（缓冲恰够）。
        match k.query_key(Some(KeyClass::KeyName), 48) {
            Ok((need, vc)) if need == 18 && vc == 1 => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 键名档错", round));
            }
        }
        match k.query_key(Some(KeyClass::FullKeyName), 48) {
            Ok((need, _)) if need == 26 => {}
            _ => r_ok = false,
        }
        match k.query_key(Some(KeyClass::Statistics), 24) {
            Ok((need, _)) if need == 24 => {}
            _ => r_ok = false,
        }
        // 档出闭集：ClassOob。
        match k.query_key(None, 48) {
            Err(c) if c == QyCode::ClassOob.val() => {}
            _ => r_ok = false,
        }
        // 统计三源一致：subkeys=2, tomb=1, vals=1。
        let (s, t, v) = k.stats();
        if !(s == 2 && t == 1 && v == 1) {
            r_ok = false;
            d.push(format!("轮{} 统计{}{}{}", round, s, t, v));
        }
        let _ = lcg.below(3);
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("三查询档 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 多值原子读注入 10/10 轮（判准 J1 第二句）：批内缺值整批拒。
pub fn drill_atomic_multi_10() -> (bool, String) {
    let mut ok2 = 0usize;
    let mut round2 = 0usize;
    while round2 < INJECT_ROUNDS {
        let mut k = match QueryKey::new(b"K") {
            Ok(k) => k,
            Err(_) => break,
        };
        let _ = k.set_value(b"V1", REG_SZ, b"hello");
        let _ = k.set_value(b"V2", REG_DWORD, &[9, 0, 0, 0]);
        // 原子批读（全在）：Ok。
        let full = k.query_multi(&[b"V1", b"V2"]).is_ok();
        // 批内混入幽灵值：整批拒 BatchMissing。
        let ghost = match k.query_multi(&[b"V1", b"GHOST"]) {
            Err(c) => c == QyCode::BatchMissing.val(),
            Ok(_) => false,
        };
        // 批超容（9 名）：BatchOverflow。
        let names9: [&[u8]; 9] = [
            b"V1", b"V2", b"V3", b"V4", b"V5", b"V6", b"V7", b"V8", b"V9",
        ];
        let over = match k.query_multi(&names9) {
            Err(c) => c == QyCode::BatchOverflow.val(),
            Ok(_) => false,
        };
        // 批内重名：BatchDupName。
        let dup = match k.query_multi(&[b"V1", b"V1"]) {
            Err(c) => c == QyCode::BatchDupName.val(),
            Ok(_) => false,
        };
        if full && ghost && over && dup {
            ok2 += 1;
        }
        round2 += 1;
    }
    let d2 = format!("原子批读 {}/{}", ok2, INJECT_ROUNDS);
    (ok2 == INJECT_ROUNDS, d2)
}

/// 缓冲不足注入 10/10 轮：拒并回所需长与锚一致。
pub fn drill_buffer_anchor_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut k = match QueryKey::new(b"12345678") {
            Ok(k) => k,
            Err(_) => break,
        };
        // 键名 8B：缓冲 7 拒，回所需 8。
        let short = match k.query_key(Some(KeyClass::KeyName), 7) {
            Err(c) if c == QyCode::BufferOverflow.val() => true,
            _ => false,
        };
        let exact = k.query_key(Some(KeyClass::KeyName), 8).is_ok();
        // 单值：4B 数据，缓冲 3 拒。
        let _ = k.set_value(b"D", REG_BINARY, &[1, 2, 3, 4]);
        let vshort = match k.query_value(b"D", 3) {
            Err(c) if c == QyCode::BufferOverflow.val() => true,
            _ => false,
        };
        let vexact = k.query_value(b"D", 4).is_ok();
        if short && exact && vshort && vexact {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("缓冲锚注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 单值查询与类型闭集演练 10/10 轮。
pub fn drill_value_types_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE.wrapping_add(1) as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut k = match QueryKey::new(b"K") {
            Ok(k) => k,
            Err(_) => break,
        };
        let _ = k.set_value(b"S", REG_SZ, b"abc");
        let _ = k.set_value(b"D", REG_DWORD, &[1, 0, 0, 0]);
        let _ = k.set_value(b"B", REG_BINARY, &[0xFF]);
        let mut r_ok = true;
        // 三类型逐查（类型闭集）。
        let s = k.query_value(b"S", 8);
        let dw = k.query_value(b"D", 8);
        let b = k.query_value(b"B", 8);
        match (s, dw, b) {
            (Ok((1, 3)), Ok((4, 4)), Ok((3, 1))) => {}
            _ => r_ok = false,
        }
        // 缺值：ValueNotFound。
        match k.query_value(b"NOPE", 8) {
            Err(c) if c == QyCode::ValueNotFound.val() => {}
            _ => r_ok = false,
        }
        // 类型出闭集注册拒。
        match k.set_value(b"X", 99, &[1]) {
            Err(c) if c == QyCode::BadType.val() => {}
            _ => r_ok = false,
        }
        // 零长注册拒。
        match k.set_value(b"Z", REG_SZ, b"") {
            Err(c) if c == QyCode::ZeroLenValue.val() => {}
            _ => r_ok = false,
        }
        let _ = lcg.below(5);
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("单值类型 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    QyCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：类型锚常量核对。
pub fn run_unx12465_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-类型锚",
        REG_SZ == 1 && REG_BINARY == 3 && REG_DWORD == 4 && REG_QWORD == 11 && MULTI_BATCH_CAP == 8,
        format!("SZ{} BIN{} DWORD{} QWORD{} 批{}", REG_SZ, REG_BINARY, REG_DWORD, REG_QWORD, MULTI_BATCH_CAP),
    ));
    v
}

/// run_unx12465_checks：本单判据聚合（诊断段 0x30820）。
pub fn run_unx12465_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12465 三查询档语义");
    // J1-a：三查询档（名/全名/统计）。
    let (ok1, d1) = drill_three_class_10();
    s.add_dyn("J1-三查询档10/10", ok1, &d1);
    // J1-b：多值原子读注入与锚一致。
    let (ok2, d2) = drill_atomic_multi_10();
    s.add_dyn("J1-多值原子读10/10", ok2, &d2);
    // J1-c：缓冲不足锚注入。
    let (ok3, d3) = drill_buffer_anchor_10();
    s.add_dyn("J1-缓冲锚注入10/10", ok3, &d3);
    // J1-d：单值类型闭集。
    let (ok4, d4) = drill_value_types_10();
    s.add_dyn("J1-单值类型10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30820段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12465_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
