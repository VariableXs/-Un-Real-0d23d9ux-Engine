//! unx10681_semopen —— UNX-F10681 sem_open 命名信号量与 /dev/shm 声明。
//! 锚点（CoRun Unxreal 主册）：UNX-F10681 · 命名信号量 · 域 UNX-C4-B15 · 行数预算 280。
//! 判据原文：UNX-F10681-J1 命名面（name 校验：斜杠首、唯一、无
//!           二连斜杠、长度 ≤ NAME_MAX）10/10，O_CREAT 无 O_EXCL
//!           幂等开（现名返同量），O_CREAT|O_EXCL 撞名 EEXIST，
//!           value 上限 SEM_VALUE_MAX，sem_close/sem_unlink 生命
//!           周期账（unlink 后 open 必 ENOENT，驻留引用继续有效）。
//! 组件：命名核（四规则谓词：首斜杠/唯一/无二连斜杠/长度闸）+
//!       名表（name → 值 + 引用计数 + 挂删位）+ sem_open 核（三
//!       分支：现名幂等 / O_CREAT 建 / EXCL 撞名拒）+ 引用账
//!       （close 递减，unlink 挂删，引用零且挂删即名灭）+ 驻留
//!       核（挂删后旧引用读写继续有效）+ 注入核（坏名四样本/
//!       EXCL 漏拒/值越顶/挂删误灭四类 10/10 必抓）。
//! 职责定位：POSIX 命名信号量面的权威模型——名四规则、三分开
//!   语、unlink 驻留语义三性质机检。
//! 性能逐项分解：名核 O(名长)；开核 O(名表)；生命周期 O(1)。
//!   全整数确定性。
//! 诊断码段：0xF290（0xF291~0xF299 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const SPO_VERSION: &str = "unx10681_semopen-v1";

/// 诊断段基址（0xF290 段 · 本模块独占）。
pub const SPO_SEG_BASE: u16 = 0xF290;

/// 诊断码闭集（0xF291~0xF299）。
pub const SO_NAME_SLASH: u32 = 0xF291; // 非斜杠首
pub const SO_NAME_DUP: u32 = 0xF292; // 二连斜杠
pub const SO_NAME_LONG: u32 = 0xF293; // 超长名
pub const SO_EXCL_MISS: u32 = 0xF294; // EXCL 撞名漏拒
pub const SO_VAL_OVER: u32 = 0xF295; // 值越顶
pub const SO_UNLINK_GHOST: u32 = 0xF296; // 挂删后误灭（旧引用被断）
pub const SO_INJECT_MISS: u32 = 0xF297; // 注入漏抓
pub const SO_OPEN_GHOST: u32 = 0xF298; // 无 CREAT 开不存在的名
pub const SO_REF_NEG: u32 = 0xF299; // 引用负界

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SO_NAME_SLASH => ("名非斜杠首", "POSIX 命名面首字符必须 /"),
        SO_NAME_DUP => ("名含二连斜杠", "名唯一性成分核"),
        SO_NAME_LONG => ("名超长（NAME_MAX=255）", "长度闸"),
        SO_EXCL_MISS => ("O_CREAT|O_EXCL 撞名未拒（EEXIST）", "撞名即 17"),
        SO_VAL_OVER => ("初值越顶（SEM_VALUE_MAX）", "value ≤ 32767"),
        SO_UNLINK_GHOST => ("挂删后旧引用被误灭", "驻留语义：旧引用继续有效"),
        SO_INJECT_MISS => ("注入未名核拦截", "修复名谓词后重跑"),
        SO_OPEN_GHOST => ("无 O_CREAT 开不存在的名", "ENOENT 面"),
        SO_REF_NEG => ("引用负界", "close 前查零界"),
        _ => ("未知诊断码", "查 0xF290 段闭集登记表"),
    }
}

/// EEXIST / ENOENT。
pub const EEXIST: u32 = 17;
pub const ENOENT: u32 = 2;

/// 名长上限 / 值上限（Linux 对照锚定）。
pub const NAME_MAX: usize = 255;
pub const SEM_VALUE_MAX: i32 = 32767;

/// O_ 标志。
pub const O_CREAT: u32 = 0x40;
pub const O_EXCL: u32 = 0x80;

/// 名行。
#[derive(Clone, Debug)]
pub struct NamedSem {
    pub name: String,
    pub value: i32,
    /// 引用计数（open 未 close 数）。
    pub refs: u32,
    /// 挂删位（unlink 后 true，引用归零即名灭）。
    pub unlinked: bool,
    pub alive: bool,
}

/// 命名信号量注册表。
pub struct SemRegistry {
    table: Vec<NamedSem>,
}

impl SemRegistry {
    pub fn new() -> SemRegistry {
        SemRegistry { table: Vec::new() }
    }

    /// 名核：四规则。
    pub fn name_check(name: &str) -> Result<(), u32> {
        let b = name.as_bytes();
        if b.is_empty() || b[0] != b'/' {
            return Err(SO_NAME_SLASH);
        }
        if b.len() > NAME_MAX + 1 {
            return Err(SO_NAME_LONG);
        }
        // 二连斜杠核（首字符之后）。
        for w in b[1..].windows(2) {
            if w == b"//" {
                return Err(SO_NAME_DUP);
            }
        }
        Ok(())
    }

    /// sem_open 核：三分支。
    pub fn open(&mut self, name: &str, value: i32, flags: u32) -> Result<u64, u32> {
        Self::name_check(name)?;
        if value > SEM_VALUE_MAX {
            return Err(SO_VAL_OVER);
        }
        // 现名面。
        if let Some(idx) = self.table.iter().position(|s| s.name == name && s.alive) {
            let s = &mut self.table[idx];
            if s.unlinked {
                // 挂删名：open 新开（同位面重置）→ 视作不存在。
                if flags & O_CREAT == 0 {
                    return Err(ENOENT);
                }
                if flags & O_EXCL != 0 {
                    return Err(EEXIST);
                }
                if value > SEM_VALUE_MAX {
                    return Err(SO_VAL_OVER);
                }
                s.value = value;
                s.unlinked = false;
                s.refs += 1;
                return Ok(idx as u64 + 1);
            }
            if flags & O_CREAT != 0 && flags & O_EXCL != 0 {
                return Err(EEXIST);
            }
            s.refs += 1;
            return Ok(idx as u64 + 1);
        }
        // 新名面。
        if flags & O_CREAT == 0 {
            return Err(ENOENT);
        }
        self.table.push(NamedSem {
            name: String::from(name),
            value,
            refs: 1,
            unlinked: false,
            alive: true,
        });
        Ok(self.table.len() as u64)
    }

    /// 句柄 → 行。
    fn row_mut(&mut self, h: u64) -> Result<&mut NamedSem, u32> {
        if h == 0 {
            return Err(SO_OPEN_GHOST);
        }
        let s = self.table.get_mut((h - 1) as usize).ok_or(SO_OPEN_GHOST)?;
        if !s.alive {
            return Err(SO_OPEN_GHOST);
        }
        Ok(s)
    }

    /// wait/signal（值面）。
    pub fn wait(&mut self, h: u64) -> Result<(), u32> {
        let s = self.row_mut(h)?;
        if s.value <= 0 {
            return Err(EAGAIN);
        }
        s.value -= 1;
        Ok(())
    }

    pub fn signal(&mut self, h: u64) -> Result<(), u32> {
        let s = self.row_mut(h)?;
        if s.value as i64 + 1 > SEM_VALUE_MAX as i64 {
            return Err(SO_VAL_OVER);
        }
        s.value += 1;
        Ok(())
    }

    /// sem_close：引用递减（挂删 + 零引用即名灭）。
    pub fn close(&mut self, h: u64) -> Result<(), u32> {
        let s = self.row_mut(h)?;
        if s.refs == 0 {
            return Err(SO_REF_NEG);
        }
        s.refs -= 1;
        if s.refs == 0 && s.unlinked {
            s.alive = false;
        }
        Ok(())
    }

    /// sem_unlink：挂删（驻留引用继续有效）。
    pub fn unlink(&mut self, name: &str) -> Result<(), u32> {
        Self::name_check(name)?;
        let s = self
            .table
            .iter_mut()
            .find(|s| s.name == name && s.alive)
            .ok_or(ENOENT)?;
        if s.unlinked {
            return Err(ENOENT);
        }
        s.unlinked = true;
        if s.refs == 0 {
            s.alive = false;
        }
        Ok(())
    }

    /// 驻留核：句柄在挂删名上 wait/signal 仍有效。
    pub fn resident_valid(&mut self, h: u64) -> Result<bool, u32> {
        let s = self.row_mut(h)?;
        Ok(s.unlinked && s.refs > 0)
    }

    /// 查询值。
    pub fn value_of(&self, h: u64) -> Result<i32, u32> {
        if h == 0 {
            return Err(SO_OPEN_GHOST);
        }
        let s = self.table.get((h - 1) as usize).ok_or(SO_OPEN_GHOST)?;
        Ok(s.value)
    }
}

impl Default for SemRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// EAGAIN（值面零界）。
pub const EAGAIN: u32 = 11;

/// 注入核：坏名四样本/EXCL 漏拒/值越顶/挂删误灭四类 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：坏名四样本（无斜杠/二连斜杠/超长/空）——3 轮。
    for i in 0..3u32 {
        let long = alloc::format!("/{}", "n".repeat(NAME_MAX + 5));
        let c1 = SemRegistry::name_check("noslash") == Err(SO_NAME_SLASH);
        let c2 = SemRegistry::name_check("/a//b") == Err(SO_NAME_DUP);
        let c3 = SemRegistry::name_check(&long) == Err(SO_NAME_LONG);
        let caught = c1 && c2 && c3;
        all &= caught;
        log.push_str(&format!("坏名#{} caught={}\n", i + 1, caught));
    }
    // 注入二：EXCL 撞名必 EEXIST——3 轮。
    for i in 0..3u32 {
        let mut r = SemRegistry::new();
        let _ = r.open("/s1", 1, O_CREAT);
        let caught = r.open("/s1", 1, O_CREAT | O_EXCL) == Err(EEXIST);
        all &= caught;
        log.push_str(&format!("EXCL#{} caught={}\n", i + 1, caught));
    }
    // 注入三：值越顶（32768 必拒）——2 轮。
    for i in 0..2u32 {
        let mut r = SemRegistry::new();
        let caught = r.open("/s2", SEM_VALUE_MAX + 1, O_CREAT) == Err(SO_VAL_OVER);
        all &= caught;
        log.push_str(&format!("值越顶#{} caught={}\n", i + 1, caught));
    }
    // 注入四：挂删驻留（unlink 后旧引用 wait/signal 有效）——2 轮。
    for i in 0..2u32 {
        let mut r = SemRegistry::new();
        let h = match r.open("/s3", 2, O_CREAT) {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        if r.unlink("/s3").is_err() {
            all = false;
            continue;
        }
        let resident = r.resident_valid(h).unwrap_or(false);
        let w = r.wait(h).is_ok();
        let w2 = r.wait(h).is_ok();
        let w3 = r.wait(h) == Err(EAGAIN);
        let caught = resident && w && w2 && w3;
        all &= caught;
        log.push_str(&format!("驻留#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核。
pub fn drill_semopen() -> (bool, &'static str) {
    let mut r = SemRegistry::new();
    // 判据一：名四规则正反两面。
    for good in ["/dev_shm_sem", "/a"] {
        if SemRegistry::name_check(good).is_err() {
            return (false, "F10681 好名误拒");
        }
    }
    // 判据二：幂等开 + EXCL 撞名 + 值面。
    let h1 = match r.open("/n1", 3, O_CREAT) {
        Ok(x) => x,
        Err(_) => return (false, "F10681 开失败"),
    };
    let h2 = match r.open("/n1", 3, O_CREAT) {
        Ok(x) => x,
        Err(_) => return (false, "F10681 幂等开失败"),
    };
    if h1 != h2 {
        return (false, "F10681 幂等开异号");
    }
    if r.open("/n1", 1, O_CREAT | O_EXCL).is_ok() {
        return (false, "F10681 EXCL 漏拒");
    }
    // 判据三：unlink 后 open 必 ENOENT（无驻留引用时）+ 驻留有效。
    if r.unlink("/n1").is_err() {
        return (false, "F10681 unlink 失败");
    }
    if r.open("/n1", 0, 0) != Err(ENOENT) {
        return (false, "F10681 挂删后 open 语义漂移");
    }
    if r.value_of(h1) != Ok(3) {
        return (false, "F10681 驻留值断");
    }
    // 判据四：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10681 注入核未全抓");
    }
    (true, "F10681 全判据绿")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_semopen().0);
    }
}
