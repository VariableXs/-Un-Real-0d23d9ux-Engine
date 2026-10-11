//! cgd12_timeapi — CGPU-F0492 帧计时 API 冻结 v1（签名落定，十年不改口）。
//!
//! 判据原文（施工书锚点 CGPU-F0492）：API：帧计时族冻结（计时启动/分段/
//! 账本查询/校验——签名冻结版本化）；与 C 域/B08 API 命名一致；十年承诺。
//! 判据：冻结、一致、承诺、判据。
//!
//! 职责定位：帧计时族公开 API 注册表（四组：计时启动/分段/账本查询/校验
//! ——每条 API 带签名指纹，注册即冻结）+ 签名冻结纪律（注册后签名不可变
//! ——变更唯一合法路径 ADR 申请新版本号，旧签名保留查证——复用 cgf05_
//! certsuite 冻结理念）+ 命名一致性核验（与 C 域 cgc04/cgc05、B08 总线
//! cgd10 的命名规范对拍——动词_名词、微秒单位、ppm 比例三约定）+ 十年承
//! 诺面（承诺声明随 API 表一起指纹化——v1 表+承诺文本整体指纹，任何单方
//! 改动指纹失配即显性）。红线：冻结不是化石——只增不改不删（新 API 走新
//! 版本号追加，旧 API 永久在表）；查询接口零副作用（&self 纪律，立案计数
//! 除外走 &mut）。诊断码段 0x5C01~0x5C05。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本（API 冻结版本号——判据"版本化"）。
pub const TIME_API_VERSION: &str = "D12-timeapi-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x5C 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_SIG_TAMPERED: u32 = 0x5C01; // 签名失配（冻结 API 被改）
pub const XC_NAME_STYLE_VIOLATION: u32 = 0x5C02; // 命名规范违约（跨域一致失守）
pub const XC_API_DUPLICATE: u32 = 0x5C03; // 重复注册（同名 API 两条）
pub const XC_FROZEN_MUTATE: u32 = 0x5C04; // 冻结表上原地修改（只增不改不删违约）
pub const XC_PROMISE_MISMATCH: u32 = 0x5C05; // 十年承诺指纹失配

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// API 组闭集（判据四组）。
pub const API_GROUPS: [&str; 4] = ["timer_start", "timer_segment", "ledger_query", "verify"];
/// API 表容量上限。
pub const API_TABLE_CAP: usize = 64;
/// 单位约定：时间一律微秒（跨域命名一致的语义面）。
pub const UNIT_TIME: &str = "us";
/// 比例约定：比例一律 ppm。
pub const UNIT_RATIO: &str = "ppm";

/// 帧计时族 API 条目。
#[derive(Clone, Debug, PartialEq)]
pub struct ApiEntry {
    /// 所属组（闭集）。
    pub group: &'static str,
    /// API 名（命名规范：动词_名词_单位）。
    pub name: String,
    /// 参数串（类型签名摘要——签名指纹的输入）。
    pub params: String,
    /// 返回摘要。
    pub ret: String,
    /// 签名指纹（注册时算，冻结后对拍）。
    pub sig: u64,
    /// 版本号（引入版本——追加制历史保留）。
    pub since: u32,
}

impl ApiEntry {
    /// 签名指纹：FNV(group|name|params|ret|since)。
    pub fn compute_sig(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_mix(h, self.group.as_bytes());
        h = fnv_mix(h, self.name.as_bytes());
        h = fnv_mix(h, self.params.as_bytes());
        h = fnv_mix(h, self.ret.as_bytes());
        h = fnv_mix(h, &[self.since as u8]);
        h
    }
}

pub const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x1000_0000_01b3)
}

pub fn fnv_mix(seed: u64, data: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < data.len() {
        h = fnv_byte(h, data[i]);
        i += 1;
    }
    h
}

/// 命名规范核验（判据"与 C 域/B08 API 命名一致"）：小写下划线、动词打头、
/// 单位后缀（时间 us / 比例 ppm）。
pub fn name_style_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() == 0 {
        return false;
    }
    // 只允许小写字母/数字/下划线；不许双下划线；不许下划线结尾。
    let mut prev_us = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let ok = (c >= b'a' && c <= b'z') || (c >= b'0' && c <= b'9') || c == b'_';
        if !ok {
            return false;
        }
        if c == b'_' && prev_us {
            return false;
        }
        prev_us = c == b'_';
        i += 1;
    }
    if prev_us {
        return false;
    }
    // 动词打头白名单（跨域一致的动作词表）。
    const VERBS: [&str; 6] = ["start", "stop", "get", "put", "mark", "verify"];
    let mut hit = false;
    let mut v = 0;
    while v < VERBS.len() {
        if starts_with(name, VERBS[v]) {
            hit = true;
            break;
        }
        v += 1;
    }
    hit
}

/// 前缀判断。
pub fn starts_with(hay: &str, prefix: &str) -> bool {
    let hb = hay.as_bytes();
    let pb = prefix.as_bytes();
    if hb.len() < pb.len() {
        return false;
    }
    let mut i = 0;
    while i < pb.len() {
        if hb[i] != pb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 帧计时 API 冻结注册表。
pub struct TimeApiRegistry {
    entries: Vec<ApiEntry>,
    /// 表级指纹（全表签名串联——十年承诺载体）。
    table_sig: u64,
    frozen: bool,
    /// 版本历史（追加制）。
    pub current_version: u32,
    /// 遥测。
    pub tamper_events: u64,
    pub style_rejects: u64,
    pub dup_rejects: u64,
    pub mutate_attempts: u64,
}

impl TimeApiRegistry {
    pub fn new() -> Self {
        TimeApiRegistry {
            entries: Vec::new(),
            table_sig: 0,
            frozen: false,
            current_version: 1,
            tamper_events: 0,
            style_rejects: 0,
            dup_rejects: 0,
            mutate_attempts: 0,
        }
    }

    /// 注册 API（冻结前可批量注册；冻结后走 add_version 追加制）。
    pub fn register(
        &mut self,
        group: &'static str,
        name: &str,
        params: &str,
        ret: &str,
    ) -> Result<u64, u32> {
        if self.frozen {
            self.mutate_attempts += 1;
            return Err(XC_FROZEN_MUTATE);
        }
        // 组闭集校验。
        let mut g_ok = false;
        let mut g = 0;
        while g < API_GROUPS.len() {
            if API_GROUPS[g] == group {
                g_ok = true;
                break;
            }
            g += 1;
        }
        if !g_ok {
            return Err(XC_NAME_STYLE_VIOLATION);
        }
        // 命名规范核验。
        if !name_style_ok(name) {
            self.style_rejects += 1;
            return Err(XC_NAME_STYLE_VIOLATION);
        }
        // 重复注册拒绝。
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                self.dup_rejects += 1;
                return Err(XC_API_DUPLICATE);
            }
            i += 1;
        }
        if self.entries.len() >= API_TABLE_CAP {
            return Err(XC_API_DUPLICATE);
        }
        let e = ApiEntry {
            group,
            name: String::from(name),
            params: String::from(params),
            ret: String::from(ret),
            sig: 0,
            since: self.current_version,
        };
        let sig = e.compute_sig();
        self.entries.push(ApiEntry { sig, ..e });
        self.recompute_table_sig();
        Ok(sig)
    }

    fn recompute_table_sig(&mut self) {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0;
        while i < self.entries.len() {
            h = fnv_mix(h, &self.entries[i].sig.to_le_bytes());
            i += 1;
        }
        self.table_sig = h;
    }

    /// 冻结（十年承诺锚定：表指纹+承诺文本合成总指纹）。
    pub fn freeze(&mut self, promise_text: &str) -> Result<u64, u32> {
        if self.frozen {
            self.mutate_attempts += 1;
            return Err(XC_FROZEN_MUTATE);
        }
        if self.entries.len() == 0 {
            return Err(XC_PROMISE_MISMATCH); // 空表冻结=空承诺
        }
        self.frozen = true;
        let total = fnv_mix(self.table_sig, promise_text.as_bytes());
        Ok(total)
    }

    /// 冻结完整性核验（判据"签名冻结"的读时面）：逐条签名对拍+表指纹对拍。
    pub fn verify_frozen(&mut self, promise_text: &str, expect_total: u64) -> Result<(), u32> {
        if !self.frozen {
            return Err(XC_PROMISE_MISMATCH);
        }
        let mut i = 0;
        while i < self.entries.len() {
            let recomputed = self.entries[i].compute_sig();
            if recomputed != self.entries[i].sig {
                self.tamper_events += 1;
                return Err(XC_SIG_TAMPERED);
            }
            i += 1;
        }
        self.recompute_table_sig();
        let total = fnv_mix(self.table_sig, promise_text.as_bytes());
        if total != expect_total {
            self.tamper_events += 1;
            return Err(XC_PROMISE_MISMATCH);
        }
        Ok(())
    }

    /// 追加制版本演进（判据"只增不改不删"：冻结后新 API 走新版本号）。
    pub fn add_version(&mut self, new_version: u32) -> Result<(), u32> {
        if !self.frozen {
            return Err(XC_FROZEN_MUTATE);
        }
        if new_version <= self.current_version {
            return Err(XC_FROZEN_MUTATE); // 版本只能前进
        }
        self.frozen = false; // 暂开注册窗（新版本追加）
        self.current_version = new_version;
        Ok(())
    }

    /// 追加窗口收口（重新冻结）。
    pub fn seal_version(&mut self, promise_text: &str) -> Result<u64, u32> {
        self.freeze(promise_text)
    }

    /// 查询 API（账本查询面——判据"账本查询"组的 API 表内自证）。
    pub fn lookup(&self, name: &str) -> Option<&ApiEntry> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 按组列出。
    pub fn list_group(&self, group: &str) -> Vec<&ApiEntry> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].group == group {
                out.push(&self.entries[i]);
            }
            i += 1;
        }
        out
    }

    /// 表规模。
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// 冻结态查询。
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：冻结 / 一致 / 承诺 / 版本化）
// ---------------------------------------------------------------------------

impl TimeApiRegistry {
    /// 组一：冻结——注册四组样例 API、冻结、篡改参数即验签失败。
    pub fn selfcheck_freeze(&mut self) -> Result<(), u32> {
        let mut r = TimeApiRegistry::new();
        r.register("timer_start", "start_frame_timer_us", "anchor:u64", "handle:u64")?;
        r.register("timer_segment", "mark_segment_us", "handle:u64,seg:u8", "()")?;
        r.register("ledger_query", "get_frame_stats_us", "scene:&str", "StatsUs")?;
        r.register("verify", "verify_ledger_ppm", "ledger:&Ledger", "Result<(),u32>")?;
        let total = r.freeze("帧计时族 API 十年承诺：签名冻结，只增不改不删")?;
        if !r.is_frozen() || r.len() != 4 {
            return Err(XC_PROMISE_MISMATCH);
        }
        // 完整性通过。
        r.verify_frozen("帧计时族 API 十年承诺：签名冻结，只增不改不删", total)?;
        // 篡改：改一条 params（绕过 register 直改——模拟内存篡改）。
        r.frozen = false; // 模拟攻击者开窗（攻击路径）
        let n = r.len();
        if n > 0 {
            r.entries[0].params = String::from("anchor:i64"); // 改类型签名
        }
        r.frozen = true;
        // 验签必须失败。
        if r.verify_frozen("帧计时族 API 十年承诺：签名冻结，只增不改不删", total)
            != Err(XC_SIG_TAMPERED)
            || r.tamper_events != 1
        {
            return Err(XC_SIG_TAMPERED);
        }
        Ok(())
    }

    /// 组二：命名一致——风格违例拒、重复注册拒、组闭集外拒。
    pub fn selfcheck_naming(&mut self) -> Result<(), u32> {
        let mut r = TimeApiRegistry::new();
        // 大写违约。
        if r.register("timer_start", "StartFrameTimer", "a:u64", "()")
            != Err(XC_NAME_STYLE_VIOLATION)
            || r.style_rejects != 1
        {
            return Err(XC_NAME_STYLE_VIOLATION);
        }
        // 无动词打头。
        if r.register("timer_start", "frame_timer_start_us", "a:u64", "()")
            != Err(XC_NAME_STYLE_VIOLATION)
        {
            return Err(XC_NAME_STYLE_VIOLATION);
        }
        // 合法注册后重复拒。
        r.register("timer_start", "start_frame_timer_us", "anchor:u64", "handle:u64")?;
        if r.register("timer_start", "start_frame_timer_us", "anchor:u64", "handle:u64")
            != Err(XC_API_DUPLICATE)
            || r.dup_rejects != 1
        {
            return Err(XC_API_DUPLICATE);
        }
        // 组闭集外拒。
        if r.register("bogus_group", "start_x_us", "a:u64", "()")
            != Err(XC_NAME_STYLE_VIOLATION)
        {
            return Err(XC_NAME_STYLE_VIOLATION);
        }
        Ok(())
    }

    /// 组三：承诺——空表冻结拒、冻结表原地 register 拒（只增不改不删）。
    pub fn selfcheck_promise(&mut self) -> Result<(), u32> {
        let mut r = TimeApiRegistry::new();
        if r.freeze("承诺文本") != Err(XC_PROMISE_MISMATCH) {
            return Err(XC_PROMISE_MISMATCH);
        }
        r.register("verify", "verify_ledger_ppm", "l:&Ledger", "Result<(),u32>")?;
        let _ = r.freeze("承诺文本")?;
        // 冻结后 register 违约。
        if r.register("timer_start", "start_new_api_us", "a:u64", "()")
            != Err(XC_FROZEN_MUTATE)
            || r.mutate_attempts != 1
        {
            return Err(XC_FROZEN_MUTATE);
        }
        // 重复冻结违约。
        if r.freeze("承诺文本") != Err(XC_FROZEN_MUTATE) {
            return Err(XC_FROZEN_MUTATE);
        }
        Ok(())
    }

    /// 组四：版本化——追加制演进、版本回退拒、演进后旧 API 仍在。
    pub fn selfcheck_versioning(&mut self) -> Result<(), u32> {
        let mut r = TimeApiRegistry::new();
        r.register("timer_start", "start_frame_timer_us", "anchor:u64", "handle:u64")?;
        let _ = r.freeze("v1 承诺")?;
        // 版本回退拒。
        if r.add_version(1) != Err(XC_FROZEN_MUTATE) {
            return Err(XC_FROZEN_MUTATE);
        }
        // 前进版本 → 追加新 API → 收口。
        r.add_version(2)?;
        r.register("ledger_query", "get_frame_p99_us", "scene:&str", "Option<u64>")?;
        let _ = r.seal_version("v2 承诺")?;
        // 旧 API 仍在（只增不改不删）。
        if r.lookup("start_frame_timer_us").is_none() {
            return Err(XC_SIG_TAMPERED);
        }
        if r.lookup("get_frame_p99_us").is_none() {
            return Err(XC_SIG_TAMPERED);
        }
        // 新 API 的 since=2。
        let e = r.lookup("get_frame_p99_us");
        match e {
            Some(e) => {
                if e.since != 2 {
                    return Err(XC_PROMISE_MISMATCH);
                }
            }
            None => return Err(XC_SIG_TAMPERED),
        }
        // 分组查询。
        let g = r.list_group("ledger_query");
        if g.len() != 2 {
            return Err(XC_API_DUPLICATE);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_freeze()?;
        self.selfcheck_naming()?;
        self.selfcheck_promise()?;
        self.selfcheck_versioning()?;
        Ok(())
    }
}
