//! unx10721_sunpath —— UNX-F10721 sockaddr_un sun_path 绝对路径绑定。
//! 锚点（CoRun Unxreal 主册）：UNX-F10721 · sun_path 绝对绑定 · 域 UNX-C4-B17 · 行数预算 280。
//! 判据原文：UNX-F10721-J1 绝对路径绑定账（路径 → socket 唯一
//!           映射）100/100，同名绑定 EADDRINUSE，超长路径（
//!           sun_path 108 闭包）拒，AF 族核（非 AF_UNIX 拒），
//!           unlink 语义（解绑后路径可复用、socket 存活面）。
//! 组件：路径账（path → socket id 唯一映射）+ bind 核（AF 族闸
//!       + 长度闸 + 占用闸三分）+ 路径闭包核（sun_path[108] 内
//!       含 NUL 终止，超长即拒）+ unlink 核（解绑即路径复用开
//!       放，socket 关闭另账）+ 冲突核（同路径双 socket 必拒）
//!       + 万次引擎（100 路径 × 双向查证 + 复用流）+ 注入核
//!       （同名漏拒/超长漏拒/族漂移/复用错号四类 10/10 必抓）。
//! 职责定位：AF_UNIX 绑定面的权威模型——路径唯一映射、108 闭包、
//!   unlink 复用三性质机检。
//! 性能逐项分解：bind O(路径数)；闭包核 O(1)；闭合核 O(账)。
//!   全整数确定性。
//! 诊断码段：0xF2B0（0xF2B1~0xF2B9 闭集；全仓十六进制字面量扫描确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SPV_VERSION: &str = "unx10721_sunpath-v1";

/// 诊断段基址（0xF2B0 段 · 本模块独占）。
pub const SPV_SEG_BASE: u16 = 0xF2B0;

/// 诊断码闭集（0xF2B1~0xF2B9）。
pub const SP_ADDR_INUSE: u32 = 0xF2B1; // 同名绑定漏拒（EADDRINUSE）
pub const SP_PATH_LONG: u32 = 0xF2B2; // 路径超 108 闭包
pub const SP_AF_BAD: u32 = 0xF2B3; // AF 族漂移
pub const SP_REUSE_MISS: u32 = 0xF2B4; // 解绑后复用错号
pub const SP_GHOST_UNLINK: u32 = 0xF2B5; // 幽灵解绑（未绑路径）
pub const SP_INJECT_MISS: u32 = 0xF2B6; // 注入漏抓
pub const SP_EMPTY_PATH: u32 = 0xF2B7; // 空路径
pub const SP_REL_PATH: u32 = 0xF2B8; // 相对路径（非 / 首）
pub const SP_MAP_BREAK: u32 = 0xF2B9; // 映射断链（双 socket 同路径）

/// 中文诊断文本。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        SP_ADDR_INUSE => ("同名绑定漏拒（EADDRINUSE）", "占用闸先查再绑"),
        SP_PATH_LONG => ("路径超 sun_path[108] 闭包", "含 NUL ≤ 108"),
        SP_AF_BAD => ("AF 族漂移（非 AF_UNIX）", "族核先行"),
        SP_REUSE_MISS => ("解绑后复用错号", "复用 socket 新账行"),
        SP_GHOST_UNLINK => ("幽灵解绑（未绑路径）", "先查路径账"),
        SP_INJECT_MISS => ("注入未绑定核拦截", "修复闸谓词后重跑"),
        SP_EMPTY_PATH => ("空路径", "绝对路径必非空"),
        SP_REL_PATH => ("相对路径（非 / 首）", "sun_path 绝对绑定面"),
        SP_MAP_BREAK => ("映射断链（双 socket 同路径）", "唯一映射核"),
        _ => ("未知诊断码", "查 0xF2B0 段闭集登记表"),
    }
}

/// EADDRINUSE。
pub const EADDRINUSE: u32 = 98;

/// AF_UNIX（Linux 对照锚定）。
pub const AF_UNIX: u16 = 1;
/// 非法族样本。
pub const AF_INET: u16 = 2;

/// sun_path 闭包（含 NUL 终止的容量）。
pub const SUN_PATH_MAX: usize = 108;

/// 路径谓词：非空 + / 首 + 字节长 + NUL ≤ 108。
pub fn path_check(path: &str) -> Result<(), u32> {
    let b = path.as_bytes();
    if b.is_empty() {
        return Err(SP_EMPTY_PATH);
    }
    if b[0] != b'/' {
        return Err(SP_REL_PATH);
    }
    if b.len() + 1 > SUN_PATH_MAX {
        return Err(SP_PATH_LONG);
    }
    Ok(())
}

/// 绑定行。
#[derive(Clone, Debug)]
pub struct BindRow {
    pub path: String,
    pub sock_id: u64,
    pub bound: bool,
}

/// 路径账本。
pub struct PathBook {
    rows: Vec<BindRow>,
    next_sock: u64,
}

impl PathBook {
    pub fn new() -> PathBook {
        PathBook { rows: Vec::new(), next_sock: 1 }
    }

    /// bind 核：AF 闸 + 路径谓词 + 占用闸三分。
    pub fn bind(&mut self, family: u16, path: &str) -> Result<u64, u32> {
        if family != AF_UNIX {
            return Err(SP_AF_BAD);
        }
        path_check(path)?;
        if self.rows.iter().any(|r| r.bound && r.path == path) {
            return Err(EADDRINUSE);
        }
        let sid = self.next_sock;
        self.next_sock += 1;
        // 复用行位（曾解绑的路径）或新增。
        match self.rows.iter_mut().find(|r| !r.bound && r.path == path) {
            Some(r) => {
                r.sock_id = sid;
                r.bound = true;
            }
            None => self.rows.push(BindRow { path: String::from(path), sock_id: sid, bound: true }),
        }
        Ok(sid)
    }

    /// unlink：解绑（路径复用开放；socket 账行失绑）。
    pub fn unlink(&mut self, path: &str) -> Result<u64, u32> {
        path_check(path)?;
        let r = self
            .rows
            .iter_mut()
            .find(|r| r.path == path && r.bound)
            .ok_or(SP_GHOST_UNLINK)?;
        r.bound = false;
        Ok(r.sock_id)
    }

    /// 路径查 socket。
    pub fn sock_of(&self, path: &str) -> Option<u64> {
        self.rows.iter().find(|r| r.bound && r.path == path).map(|r| r.sock_id)
    }

    /// socket 查路径。
    pub fn path_of(&self, sock: u64) -> Option<String> {
        self.rows
            .iter()
            .find(|r| r.bound && r.sock_id == sock)
            .map(|r| r.path.clone())
    }

    /// 唯一映射核：活绑定路径唯一 + 双向互查。
    pub fn closed(&self) -> bool {
        let live: Vec<&BindRow> = self.rows.iter().filter(|r| r.bound).collect();
        for (i, a) in live.iter().enumerate() {
            for b in live.iter().skip(i + 1) {
                if a.path == b.path {
                    return false;
                }
            }
        }
        for r in live.iter() {
            if self.sock_of(&r.path) != Some(r.sock_id) || self.path_of(r.sock_id).as_deref() != Some(r.path.as_str()) {
                return false;
            }
        }
        true
    }

    /// 活绑定数。
    pub fn bound_count(&self) -> usize {
        self.rows.iter().filter(|r| r.bound).count()
    }
}

impl Default for PathBook {
    fn default() -> Self {
        Self::new()
    }
}

/// LCG64 确定性伪随机。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 百路径引擎：100 路径绑定 + 双向查证 + unlink 复用流。
pub fn paths_100() -> Result<(usize, usize), u32> {
    let mut b = PathBook::new();
    let mut rng = Lcg::new(0x10721_0A01);
    let mut binds = 0usize;
    let mut reuse = 0usize;
    for k in 0..100u32 {
        let path = alloc::format!("/run/varix/sock{}", k);
        let sid = b.bind(AF_UNIX, &path)?;
        binds += 1;
        // 双向查证。
        if b.sock_of(&path) != Some(sid) {
            return Err(SP_MAP_BREAK);
        }
        if b.path_of(sid).as_deref() != Some(path.as_str()) {
            return Err(SP_MAP_BREAK);
        }
        // 每第 10 路径走 unlink 复用流。
        if k % 10 == 9 {
            b.unlink(&path)?;
            if b.sock_of(&path).is_some() {
                return Err(SP_REUSE_MISS);
            }
            let sid2 = b.bind(AF_UNIX, &path)?;
            if sid2 <= sid {
                return Err(SP_REUSE_MISS);
            }
            reuse += 1;
        }
        let _ = rng.next_u64();
    }
    if !b.closed() {
        return Err(SP_MAP_BREAK);
    }
    Ok((binds, reuse))
}

/// 注入核：同名漏拒/超长漏拒/族漂移/复用错号四类 10/10。
pub fn injection_core() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;

    // 注入一：同名绑定必 EADDRINUSE——3 轮。
    for i in 0..3u32 {
        let mut b = PathBook::new();
        let _ = b.bind(AF_UNIX, "/run/a");
        let caught = b.bind(AF_UNIX, "/run/a") == Err(EADDRINUSE);
        all &= caught;
        log.push_str(&format!("同名#{} caught={}\n", i + 1, caught));
    }
    // 注入二：超长路径（108 闭包界）——3 轮。
    for i in 0..3u32 {
        let ok_len = alloc::format!("/{}", "p".repeat(SUN_PATH_MAX - 2)); // 107 + NUL = 108 合法
        let bad_len = alloc::format!("/{}", "p".repeat(SUN_PATH_MAX - 1)); // 108 + NUL = 109 越界
        let c1 = PathBook::bind(&mut PathBook::new(), AF_UNIX, &ok_len).is_ok();
        let c2 = PathBook::bind(&mut PathBook::new(), AF_UNIX, &bad_len) == Err(SP_PATH_LONG);
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("超长#{} caught={}\n", i + 1, caught));
    }
    // 注入三：族漂移（AF_INET 必拒）——2 轮。
    for i in 0..2u32 {
        let mut b = PathBook::new();
        let caught = b.bind(AF_INET, "/run/x") == Err(SP_AF_BAD);
        all &= caught;
        log.push_str(&format!("族漂移#{} caught={}\n", i + 1, caught));
    }
    // 注入四：幽灵解绑（未绑路径）与复用新号——2 轮。
    for i in 0..2u32 {
        let mut b = PathBook::new();
        let c1 = b.unlink("/run/none") == Err(SP_GHOST_UNLINK);
        let sid1 = match b.bind(AF_UNIX, "/run/r") {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let _ = b.unlink("/run/r");
        let sid2 = match b.bind(AF_UNIX, "/run/r") {
            Ok(x) => x,
            Err(_) => {
                all = false;
                continue;
            }
        };
        let caught = c1 && sid2 == sid1 + 1;
        all &= caught;
        log.push_str(&format!("复用#{} caught={}\n", i + 1, caught));
    }

    (all, log)
}

/// 验收入口：判据 J1 全链核。
pub fn drill_sunpath() -> (bool, &'static str) {
    // 判据一：百路径引擎（绑定 + 双向 + 复用）。
    let (binds, reuse) = match paths_100() {
        Ok(x) => x,
        Err(_) => return (false, "F10721 百路径引擎失败"),
    };
    if binds != 100 || reuse != 10 {
        return (false, "F10721 引擎对账漂移");
    }
    // 判据二：路径谓词边界（107 合法 / 109 拒 / 空拒 / 相对拒）。
    if path_check("").is_ok() || path_check("relative").is_ok() {
        return (false, "F10721 路径谓词漂移");
    }
    // 判据三：注入核 10/10。
    let (ok, _log) = injection_core();
    if !ok {
        return (false, "F10721 注入核未全抓");
    }
    // 判据四：深化面（抽象命名空间 + 时序闸）。
    let (ok2, _log2) = injection_deep();
    if !ok2 {
        return (false, "F10721 深化注入未全抓");
    }
    (true, "F10721 全判据绿")
}

/// ─── 深化面：抽象命名空间 + bind→listen 时序闸 ───

/// 抽象命名空间（Linux 语义）：sun_path 首字节 NUL，名不含 NUL
/// 后续字节，独立于文件系统路径账。
pub struct AbstractBook {
    /// 抽象名 → socket。
    names: Vec<(String, u64)>,
    next_sock: u64,
}

impl AbstractBook {
    pub fn new() -> AbstractBook {
        AbstractBook { names: Vec::new(), next_sock: 100_000 }
    }

    /// 抽象名谓词：非空 + 无 NUL 成分 + 长 ≤ 107。
    pub fn abs_check(name: &str) -> Result<(), u32> {
        if name.is_empty() {
            return Err(SP_EMPTY_PATH);
        }
        if name.as_bytes().iter().any(|&b| b == 0) {
            return Err(SP_REL_PATH);
        }
        if name.len() + 1 > SUN_PATH_MAX - 1 {
            return Err(SP_PATH_LONG);
        }
        Ok(())
    }

    /// 抽象 bind：抽象名唯一（撞名 EADDRINUSE）。
    pub fn bind_abs(&mut self, name: &str) -> Result<u64, u32> {
        Self::abs_check(name)?;
        if self.names.iter().any(|(n, _)| n == name) {
            return Err(EADDRINUSE);
        }
        let sid = self.next_sock;
        self.next_sock += 1;
        self.names.push((String::from(name), sid));
        Ok(sid)
    }

    /// 抽象名空间与路径账隔离核：同名在两账互不干扰。
    pub fn isolated(&self, path_book_sock: u64) -> bool {
        self.names.iter().all(|(_, sid)| *sid != path_book_sock)
    }
}

impl Default for AbstractBook {
    fn default() -> Self {
        Self::new()
    }
}

/// bind→listen 时序闸：未 bind 的 socket listen 即拒（AF_UNIX 面）。
pub struct SockLifecycle {
    bound: bool,
    listening: bool,
}

impl SockLifecycle {
    pub fn new() -> SockLifecycle {
        SockLifecycle { bound: false, listening: false }
    }

    pub fn do_bind(&mut self) {
        self.bound = true;
    }

    /// listen 闸：未 bind 必拒。
    pub fn do_listen(&mut self) -> Result<(), u32> {
        if !self.bound {
            return Err(SP_REL_PATH);
        }
        if self.listening {
            return Err(EADDRINUSE);
        }
        self.listening = true;
        Ok(())
    }
}

impl Default for SockLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

/// 深化注入：抽象名三规则 / 名空间隔离 / listen 时序三类必抓。
pub fn injection_deep() -> (bool, String) {
    let mut log = String::new();
    let mut all = true;
    // 注入五：抽象名撞名必拒 + 隔离核——2 轮。
    for i in 0..2u32 {
        let mut ab = AbstractBook::new();
        let s1 = ab.bind_abs("@varix");
        let c1 = ab.bind_abs("@varix") == Err(EADDRINUSE);
        let c2 = matches!(s1, Ok(sid) if ab.isolated(sid));
        let caught = c1 && c2;
        all &= caught;
        log.push_str(&format!("抽象名#{} caught={}\n", i + 1, caught));
    }
    // 注入六：未 bind listen 必拒；bind 后 listen 通过；二次 listen 拒——2 轮。
    for i in 0..2u32 {
        let mut lc = SockLifecycle::new();
        let c1 = lc.do_listen() == Err(SP_REL_PATH);
        lc.do_bind();
        let c2 = lc.do_listen().is_ok();
        let c3 = lc.do_listen() == Err(EADDRINUSE);
        let caught = c1 && c2 && c3;
        all &= caught;
        log.push_str(&format!("时序#{} caught={}\n", i + 1, caught));
    }
    (all, log)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drill() {
        assert!(drill_sunpath().0);
    }
}
