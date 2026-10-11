//! cgf13_certapi — CGPU-F0811 认证 API 冻结 v1（认证族签名落定，跨族对齐
//! 不各说各话）。
//!
//! 判据原文（施工书锚点 CGPU-F0811）：API：认证族冻结（认证查询/流程触发/
//! 矩阵导出——签名冻结版本化）；与 D08 基准族、E06 内容族命名一致；十年承
//! 诺。判据：冻结、一致、承诺、判据。
//!
//! 职责定位：认证族三组 API 注册表（认证查询 cert_query/流程触发 cert_run/
//! 矩阵导出 cert_export——签名指纹注册即冻结，复用 cgd12_timeapi 冻结纪律）
//! + 跨族命名一致性核验（D08 基准族与 E06 内容族的 API 名样例与本族对拍——
//! 动词表共享/单位后缀一致/闭集校验，跨族各造一套动词=一致违约）+ 十年承
//! 诺（表指纹+承诺文本合成总指纹——与 cgd12 同承诺文本域分隔，两族承诺互
//! 不串签）。与 cgd12 的分工差异：彼冻结计时族（timer 组），此冻结认证族
//! （cert 组）——同构不同域，冻结纪律共享、承诺域独立。诊断码段
//! 0x6301~0x6305。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CERT_API_VERSION: &str = "F13-certapi-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x63 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_CERT_SIG_TAMPERED: u32 = 0x6301; // 认证族签名失配
pub const XC_CROSS_FAMILY_STYLE: u32 = 0x6302; // 跨族命名不一致
pub const XC_CERT_API_DUP: u32 = 0x6303; // 认证族重复注册
pub const XC_PROMISE_DOMAIN_MIX: u32 = 0x6304; // 承诺域混签（计时承诺验认证表）
pub const XC_FROZEN_APPEND_ILLEGAL: u32 = 0x6305; // 冻结表非法追加（未开版本窗）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 认证族三组（判据闭集）。
pub const CERT_API_GROUPS: [&str; 3] = ["cert_query", "cert_run", "cert_export"];
/// 跨族共享动词表（判据"与 D08/E06 命名一致"——三族共用的动作词）。
pub const SHARED_VERBS: [&str; 5] = ["query", "run", "export", "get", "verify"];
/// API 表容量。
pub const CERT_API_CAP: usize = 48;
/// 承诺域标签（防跨域混签——判据"十年承诺"的域分隔）。
pub const PROMISE_DOMAIN_CERT: &str = "VX-PROMISE-CERT-F13";
pub const PROMISE_DOMAIN_TIMER: &str = "VX-PROMISE-TIMER-D12";

/// 认证族 API 条目。
#[derive(Clone, Debug, PartialEq)]
pub struct CertApiEntry {
    pub group: &'static str,
    pub name: String,
    pub params: String,
    pub ret: String,
    pub sig: u64,
    pub since: u32,
}

impl CertApiEntry {
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

/// 命名风格核验（跨族一致版）：cert 前缀或共享动词打头+小写下划线。
pub fn cert_name_style_ok(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() == 0 {
        return false;
    }
    let mut i = 0;
    let mut prev_us = false;
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
    // 共享动词打头（跨族一致的动作词——D08/E06 同表）。
    let mut hit = false;
    let mut v = 0;
    while v < SHARED_VERBS.len() {
        if starts_with_str(name, SHARED_VERBS[v]) {
            hit = true;
            break;
        }
        v += 1;
    }
    hit
}

pub fn starts_with_str(hay: &str, prefix: &str) -> bool {
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

/// 跨族命名一致性对拍（判据"与 D08/E06 命名一致"）：把外族 API 名样例过
/// 同一张动词表+风格核验——外族名违约即一致失守（本族无权改外族，但有权
/// 对拍并在一致性账上立案）。
pub fn cross_family_check(d08_names: &[&str], e06_names: &[&str]) -> Result<(), u32> {
    let mut i = 0;
    while i < d08_names.len() {
        if !cert_name_style_ok(d08_names[i]) {
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        i += 1;
    }
    let mut j = 0;
    while j < e06_names.len() {
        if !cert_name_style_ok(e06_names[j]) {
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        j += 1;
    }
    Ok(())
}

/// 认证族 API 冻结注册表。
pub struct CertApiRegistry {
    entries: Vec<CertApiEntry>,
    table_sig: u64,
    frozen: bool,
    pub current_version: u32,
    pub style_rejects: u64,
    pub dup_rejects: u64,
    pub mutate_attempts: u64,
}

impl CertApiRegistry {
    pub fn new() -> Self {
        CertApiRegistry {
            entries: Vec::new(),
            table_sig: 0,
            frozen: false,
            current_version: 1,
            style_rejects: 0,
            dup_rejects: 0,
            mutate_attempts: 0,
        }
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

    /// 注册（组闭集+命名一致+重复拒）。
    pub fn register(&mut self, group: &'static str, name: &str, params: &str, ret: &str) -> Result<u64, u32> {
        if self.frozen {
            self.mutate_attempts += 1;
            return Err(XC_FROZEN_APPEND_ILLEGAL);
        }
        let mut g_ok = false;
        let mut g = 0;
        while g < CERT_API_GROUPS.len() {
            if CERT_API_GROUPS[g] == group {
                g_ok = true;
                break;
            }
            g += 1;
        }
        if !g_ok {
            self.style_rejects += 1;
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        if !cert_name_style_ok(name) {
            self.style_rejects += 1;
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].name == name {
                self.dup_rejects += 1;
                return Err(XC_CERT_API_DUP);
            }
            i += 1;
        }
        if self.entries.len() >= CERT_API_CAP {
            return Err(XC_CERT_API_DUP);
        }
        let e = CertApiEntry {
            group,
            name: String::from(name),
            params: String::from(params),
            ret: String::from(ret),
            sig: 0,
            since: self.current_version,
        };
        let sig = e.compute_sig();
        self.entries.push(CertApiEntry { sig, ..e });
        self.recompute_table_sig();
        Ok(sig)
    }

    /// 冻结（承诺域分隔——认证族承诺文本带域标签）。
    pub fn freeze(&mut self, promise_text: &str) -> Result<u64, u32> {
        if self.frozen {
            self.mutate_attempts += 1;
            return Err(XC_FROZEN_APPEND_ILLEGAL);
        }
        if self.entries.len() == 0 {
            return Err(XC_PROMISE_DOMAIN_MIX);
        }
        self.frozen = true;
        let mut h = self.table_sig;
        h = fnv_mix(h, PROMISE_DOMAIN_CERT.as_bytes());
        h = fnv_mix(h, promise_text.as_bytes());
        Ok(h)
    }

    /// 验签（域绑定：用计时域标签验认证表=混签违约）。
    pub fn verify_frozen(&mut self, promise_text: &str, domain: &str, expect: u64) -> Result<(), u32> {
        if !self.frozen {
            return Err(XC_PROMISE_DOMAIN_MIX);
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].compute_sig() != self.entries[i].sig {
                self.mutate_attempts += 1;
                return Err(XC_CERT_SIG_TAMPERED);
            }
            i += 1;
        }
        self.recompute_table_sig();
        let mut h = self.table_sig;
        h = fnv_mix(h, domain.as_bytes());
        h = fnv_mix(h, promise_text.as_bytes());
        if h != expect {
            self.mutate_attempts += 1;
            if domain == PROMISE_DOMAIN_TIMER {
                return Err(XC_PROMISE_DOMAIN_MIX);
            }
            return Err(XC_CERT_SIG_TAMPERED);
        }
        Ok(())
    }

    /// 版本窗追加（判据"版本化"——冻结后合法演进路径）。
    pub fn open_version_window(&mut self, new_v: u32) -> Result<(), u32> {
        if !self.frozen {
            return Err(XC_FROZEN_APPEND_ILLEGAL);
        }
        if new_v <= self.current_version {
            self.mutate_attempts += 1;
            return Err(XC_FROZEN_APPEND_ILLEGAL);
        }
        self.frozen = false;
        self.current_version = new_v;
        Ok(())
    }

    pub fn seal(&mut self, promise_text: &str) -> Result<u64, u32> {
        self.freeze(promise_text)
    }

    /// 查询。
    pub fn lookup(&self, name: &str) -> Option<&CertApiEntry> {
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
    pub fn list_group(&self, group: &str) -> Vec<&CertApiEntry> {
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

    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }
}

// ---------------------------------------------------------------------------
// 自检（判据四组：冻结 / 跨族一致 / 承诺域 / 版本化）
// ---------------------------------------------------------------------------

impl CertApiRegistry {
    /// 组一：冻结——三组注册、冻结、篡改验签失败。
    pub fn selfcheck_freeze(&mut self) -> Result<(), u32> {
        let mut r = CertApiRegistry::new();
        r.register("cert_query", "query_cert_grade", "hw:&str", "Grade")?;
        r.register("cert_run", "run_cert_pipeline", "hw:&str,suite:&str", "Verdict")?;
        r.register("cert_export", "export_cert_matrix_json", "filter:&str", "String")?;
        let total = r.freeze("认证族 API 十年承诺：签名冻结，只增不改不删")?;
        if !r.is_frozen() || r.len() != 3 {
            return Err(XC_PROMISE_DOMAIN_MIX);
        }
        r.verify_frozen("认证族 API 十年承诺：签名冻结，只增不改不删", PROMISE_DOMAIN_CERT, total)?;
        // 篡改。
        r.frozen = false;
        r.entries[0].ret = String::from("GradeMutated");
        r.frozen = true;
        if r.verify_frozen("认证族 API 十年承诺：签名冻结，只增不改不删", PROMISE_DOMAIN_CERT, total)
            != Err(XC_CERT_SIG_TAMPERED)
        {
            return Err(XC_CERT_SIG_TAMPERED);
        }
        Ok(())
    }

    /// 组二：跨族一致——D08/E06 名样例过同表、违约名立案。
    pub fn selfcheck_cross_family(&mut self) -> Result<(), u32> {
        // D08 基准族与 E06 内容族命名样例（共享动词表）。
        cross_family_check(
            &["query_bench_result", "run_bench_suite", "get_bench_p99_us"],
            &["export_content_manifest", "verify_content_hash"],
        )?;
        // 外族违约名（大写）→ 一致失守。
        if cross_family_check(&["BadName_D08"], &[]) != Err(XC_CROSS_FAMILY_STYLE) {
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        // 本族无动词打头拒。
        let mut r = CertApiRegistry::new();
        if r.register("cert_query", "cert_grade_lookup", "hw:&str", "Grade")
            != Err(XC_CROSS_FAMILY_STYLE)
            || r.style_rejects != 1
        {
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        // 组闭集外拒。
        if r.register("cert_other", "query_x", "a:&str", "()") != Err(XC_CROSS_FAMILY_STYLE) {
            return Err(XC_CROSS_FAMILY_STYLE);
        }
        Ok(())
    }

    /// 组三：承诺域——计时域标签验认证表=混签违约；空表冻结拒。
    pub fn selfcheck_promise_domain(&mut self) -> Result<(), u32> {
        let mut r = CertApiRegistry::new();
        // 空表冻结拒。
        if r.freeze("文本") != Err(XC_PROMISE_DOMAIN_MIX) {
            return Err(XC_PROMISE_DOMAIN_MIX);
        }
        r.register("cert_query", "query_cert_grade", "hw:&str", "Grade")?;
        let total = r.freeze("认证承诺")?;
        // 计时域标签验认证表 → 混签违约（不是篡改——是域错）。
        if r.verify_frozen("认证承诺", PROMISE_DOMAIN_TIMER, total) != Err(XC_PROMISE_DOMAIN_MIX) {
            return Err(XC_PROMISE_DOMAIN_MIX);
        }
        // 正确域 + 错误文本 → 篡改语义。
        if r.verify_frozen("别的文本", PROMISE_DOMAIN_CERT, total) != Err(XC_CERT_SIG_TAMPERED) {
            return Err(XC_CERT_SIG_TAMPERED);
        }
        // 正确域+正确文本 → 过。
        r.verify_frozen("认证承诺", PROMISE_DOMAIN_CERT, total)?;
        Ok(())
    }

    /// 组四：版本化——版本窗合法追加、回退拒、窗未开追加拒。
    pub fn selfcheck_versioning(&mut self) -> Result<(), u32> {
        let mut r = CertApiRegistry::new();
        r.register("cert_run", "run_cert_pipeline", "hw:&str,suite:&str", "Verdict")?;
        let _ = r.freeze("v1")?;
        // 冻结态直接追加拒。
        if r.register("cert_export", "export_cert_matrix_json", "f:&str", "String")
            != Err(XC_FROZEN_APPEND_ILLEGAL)
            || r.mutate_attempts != 1
        {
            return Err(XC_FROZEN_APPEND_ILLEGAL);
        }
        // 回退版本拒。
        if r.open_version_window(1) != Err(XC_FROZEN_APPEND_ILLEGAL) {
            return Err(XC_FROZEN_APPEND_ILLEGAL);
        }
        // 开 v2 窗追加再封。
        r.open_version_window(2)?;
        r.register("cert_export", "export_cert_matrix_json", "f:&str", "String")?;
        let _ = r.seal("v2")?;
        // 旧 API 仍在、新 API since=2。
        if r.lookup("run_cert_pipeline").is_none() || r.lookup("export_cert_matrix_json").is_none() {
            return Err(XC_CERT_SIG_TAMPERED);
        }
        let g = r.list_group("cert_export");
        if g.len() != 1 || g[0].since != 2 {
            return Err(XC_PROMISE_DOMAIN_MIX);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_freeze()?;
        self.selfcheck_cross_family()?;
        self.selfcheck_promise_domain()?;
        self.selfcheck_versioning()?;
        Ok(())
    }
}
