//! cgb746_profapi —— CGPU-F0746 画像 API 冻结 v1。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0746 画像 API 冻结 v1  <SEG 0x4b80>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0746；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. API 冻结 = 把画像子系统的对外面（入口点/参数字节数/返回码/
//!      语义档位）落成不可变签名表；冻结后签名哈希恒定；
//!   2. 签名表 12 入口（写死），每条签名含：入口号/名称哈希/参数
//!      字节数/返回码基址/幂等旗/版本档位——六要素齐一不可缺；
//!   3. 兼容矩阵：调用方版本 v0/v1 对入口号的可用性 = 冻结表内
//!      位图判定，v0 只见基础 8 入口，v1 见全 12；
//!   4. 冻结核：对全表按序吃 FNV-1a 得表哈希，两次冻结同表必同哈希；
//!      表被增删改序后哈希必变（封账级不可变的第一防线）；
//!   5. 签名验证器：外部声明面（入口号+参数字节+期望返回码基址）
//!      与冻结表逐字段比对，任何漂移红码；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4b80 段为本模块独占（SEG_BASE=0x4b80），判别式
//!   SEG_BASE+1..+N；与 0x4b60/0x4ba0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const FA_VERSION: &str = "cgb746_profapi-v1";

/// 诊断段基址（0x4b80 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4B80;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4b81 入口号越界（0..=11 之外）。
pub const XC_BAD_ENTRY: u16 = SEG_BASE + 1;
/// 0x4b82 冻结后仍试图改表（不可变被破坏）。
pub const XC_FROZEN_WRITE: u16 = SEG_BASE + 2;
/// 0x4b83 表未冻结就对外（先冻结后暴露的次序红线）。
pub const XC_NOT_FROZEN: u16 = SEG_BASE + 3;
/// 0x4b84 签名漂移（字段与冻结表失配）。
pub const XC_SIG_DRIFT: u16 = SEG_BASE + 4;
/// 0x4b85 参数字节数越界（> 32）。
pub const XC_PARAM_OVER: u16 = SEG_BASE + 5;
/// 0x4b86 版本档位非法（v0/v1 之外）。
pub const XC_BAD_APIVER: u16 = SEG_BASE + 6;
/// 0x4b87 v0 调用扩展入口（v0 不可见 8..11 号入口）。
pub const XC_V0_HIDDEN: u16 = SEG_BASE + 7;
/// 0x4b88 表哈希复算失配（冻结表被篡改）。
pub const XC_HASH_MISMATCH: u16 = SEG_BASE + 8;
/// 0x4b89 入口重复注册（同号两签名）。
pub const XC_ENTRY_DUP: u16 = SEG_BASE + 9;

/// 冻结表容量（写死：12 入口）。
pub const ENTRY_CAP: usize = 12;
/// 参数字节数上限（写死：32）。
pub const PARAM_MAX: usize = 32;
/// v0 可见入口数（写死：基础 8）。
pub const V0_VISIBLE: usize = 8;

/// API 版本档位（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiVer {
    /// v0：基础面。
    V0,
    /// v1：全量面。
    V1,
}

/// 入口签名六要素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntrySig {
    /// 入口号（0 起，= 槽位）。
    pub eid: u8,
    /// 名称哈希（FNV-1a of 名称，冻结面）。
    pub name_hash: u32,
    /// 参数字节数。
    pub param_bytes: u8,
    /// 返回码基址（该入口的 XcCode 段位）。
    pub err_base: u16,
    /// 幂等旗：true = 重复调用结果不变。
    pub idempotent: bool,
    /// 最低版本档位（0=v0 可见，1=v1 才可见）。
    pub min_ver: u8,
}

/// 名称哈希（FNV-1a 逐字节）。
pub fn name_hash(name: &str) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for b in name.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 表哈希：按槽序逐条吃六要素（顺序敏感）。
pub fn table_hash(entries: &[Option<EntrySig>; ENTRY_CAP]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for slot in entries.iter() {
        match slot {
            Some(e) => {
                h = fnv_u32(h, e.eid as u32);
                h = fnv_u32(h, e.name_hash);
                h = fnv_u32(h, e.param_bytes as u32);
                h = fnv_u32(h, e.err_base as u32);
                h = fnv_u32(h, e.idempotent as u32);
                h = fnv_u32(h, e.min_ver as u32);
            }
            None => h = fnv_u32(h, 0xFFFF_0001),
        }
    }
    h
}

fn fnv_u32(h: u32, v: u32) -> u32 {
    let mut x = h;
    for i in 0..4 {
        x ^= (v >> (i * 8)) as u8 as u32;
        x = x.wrapping_mul(0x0100_0193);
    }
    x
}

/// 冻结表：登记 → 冻结 → 验签 三段生命周期。
pub struct ApiFreeze {
    entries: [Option<EntrySig>; ENTRY_CAP],
    frozen: bool,
    frozen_hash: Option<u32>,
}

impl ApiFreeze {
    /// 空表（未冻结）。
    pub fn new() -> Self {
        ApiFreeze {
            entries: [None; ENTRY_CAP],
            frozen: false,
            frozen_hash: None,
        }
    }

    /// 登记签名（冻结后红码；号越界/参数超限/重复注册红码）。
    pub fn register(&mut self, sig: EntrySig) -> Result<(), u16> {
        if self.frozen {
            return Err(XC_FROZEN_WRITE);
        }
        if (sig.eid as usize) >= ENTRY_CAP {
            return Err(XC_BAD_ENTRY);
        }
        if sig.param_bytes as usize > PARAM_MAX {
            return Err(XC_PARAM_OVER);
        }
        if sig.min_ver > 1 {
            return Err(XC_BAD_APIVER);
        }
        if self.entries[sig.eid as usize].is_some() {
            return Err(XC_ENTRY_DUP);
        }
        self.entries[sig.eid as usize] = Some(sig);
        Ok(())
    }

    /// 冻结：落表哈希。二次冻结红码。空槽允许（稀疏表合法——
    /// 冻结的是"表形状"，含空槽形状本身）。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.frozen {
            return Err(XC_FROZEN_WRITE);
        }
        let h = table_hash(&self.entries);
        self.frozen = true;
        self.frozen_hash = Some(h);
        Ok(h)
    }

    /// 表哈希（未冻结红码）。
    pub fn hash(&self) -> Result<u32, u16> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        Ok(self.frozen_hash.ok_or(XC_NOT_FROZEN)?)
    }

    /// 完整性复算：现表重算哈希必须等于冻结哈希。
    pub fn verify_integrity(&self) -> Result<(), u16> {
        let fh = self.hash()?;
        if table_hash(&self.entries) != fh {
            return Err(XC_HASH_MISMATCH);
        }
        Ok(())
    }

    /// 版本可见面：v0 见 min_ver=0 且号 < V0_VISIBLE 的入口；
    /// v1 见全部已登记入口。
    pub fn visible(&self, ver: ApiVer) -> Result<Vec<u8>, u16> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        let mut out = Vec::new();
        for (i, slot) in self.entries.iter().enumerate() {
            if let Some(e) = slot {
                let vis = match ver {
                    ApiVer::V0 => e.min_ver == 0 && i < V0_VISIBLE,
                    ApiVer::V1 => true,
                };
                if vis {
                    out.push(e.eid);
                }
            }
        }
        Ok(out)
    }

    /// 调用闸：给定版本与入口号，判定该调用是否允许（返回签名副本）。
    pub fn gate(&self, ver: ApiVer, eid: u8) -> Result<EntrySig, u16> {
        let e = self
            .entries
            .get(eid as usize)
            .and_then(|s| s.as_ref())
            .ok_or(XC_BAD_ENTRY)?;
        let ok = match ver {
            ApiVer::V1 => true,
            ApiVer::V0 => e.min_ver == 0 && (eid as usize) < V0_VISIBLE,
        };
        if !ok {
            return Err(XC_V0_HIDDEN);
        }
        Ok(*e)
    }

    /// 签名验证器：外部声明面逐字段比对冻结表。
    pub fn verify_sig(
        &self,
        eid: u8,
        name_hash_claim: u32,
        param_bytes: u8,
        err_base: u16,
    ) -> Result<(), u16> {
        let e = self.gate(ApiVer::V1, eid)?;
        if e.name_hash != name_hash_claim
            || e.param_bytes != param_bytes
            || e.err_base != err_base
        {
            return Err(XC_SIG_DRIFT);
        }
        Ok(())
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 已登记入口数。
    pub fn registered(&self) -> usize {
        self.entries.iter().filter(|s| s.is_some()).count()
    }
}

/// 冻结时刻的 v1 标准表构造（写死 12 入口的名称与参数面）。
pub fn standard_table() -> Result<ApiFreeze, u16> {
    let mut t = ApiFreeze::new();
    let names = [
        "profile_register",
        "profile_query",
        "scene_link",
        "scene_unlink",
        "telemetry_sample",
        "telemetry_export",
        "consistency_check",
        "ledger_replay",
        "bench_run",
        "bench_report",
        "doc_emit",
        "handover_seal",
    ];
    for (i, n) in names.iter().enumerate() {
        let min_ver = if i < V0_VISIBLE { 0u8 } else { 1u8 };
        let pb = match i {
            0 => 12u8,  // register: 权重 3×u16+掩码+版本
            1 => 2u8,   // query: pid
            2 => 4u8,   // link: sid+pid
            3 => 2u8,   // unlink: sid
            4 => 4u8,   // sample: cid+wid+敏感位
            5 => 2u8,   // export: redact 旗
            6 => 0u8,   // consistency: 无参
            7 => 2u8,   // replay: 账号
            8 => 8u8,   // bench: 轮数+种子
            9 => 2u8,   // report: 基准号
            10 => 4u8,  // doc: 段位+格式
            _ => 6u8,   // handover: 批号+双方 tag
        };
        t.register(EntrySig {
            eid: i as u8,
            name_hash: name_hash(n),
            param_bytes: pb,
            err_base: 0x4B80 + (i as u16 + 1) * 0x10,
            idempotent: !matches!(i, 0 | 2 | 4 | 8),
            min_ver,
        })?;
    }
    Ok(t)
}

/// 自例程：标准表冻结 → 完整性 → 版本可见面 → 调用闸 → 验签 →
/// 错误路径（未冻结暴露/冻结后写/重复注册/签名漂移/哈希失配）。
pub fn drill() -> Result<u32, u16> {
    // 错误路径 0：未冻结就取哈希。
    let empty = ApiFreeze::new();
    if empty.hash() != Err(XC_NOT_FROZEN) {
        return Err(XC_NOT_FROZEN);
    }

    // 主路径。
    let mut t = standard_table()?;
    if t.registered() != ENTRY_CAP {
        return Err(XC_ENTRY_DUP);
    }
    let h = t.freeze()?;
    if t.hash()? != h {
        return Err(XC_HASH_MISMATCH);
    }
    t.verify_integrity()?;
    if !t.is_frozen() {
        return Err(XC_NOT_FROZEN);
    }
    // 二次冻结红码。
    if t.freeze() != Err(XC_FROZEN_WRITE) {
        return Err(XC_FROZEN_WRITE);
    }

    // 版本可见面：v0 恰 8，v1 恰 12。
    let v0 = t.visible(ApiVer::V0)?;
    let v1 = t.visible(ApiVer::V1)?;
    if v0.len() != V0_VISIBLE || v1.len() != ENTRY_CAP {
        return Err(XC_BAD_APIVER);
    }
    // 调用闸：v0 调 8 号（bench_run，v1 才可见）红码。
    if t.gate(ApiVer::V0, 8) != Err(XC_V0_HIDDEN) {
        return Err(XC_V0_HIDDEN);
    }
    // v1 调 8 号放行且签名一致。
    let g = t.gate(ApiVer::V1, 8)?;
    if g.param_bytes != 8 || !g.idempotent {
        return Err(XC_SIG_DRIFT);
    }
    // 验签主路径：0 号入口按标准表声明。
    t.verify_sig(0, name_hash("profile_register"), 12, 0x4B90)?;
    // 验签漂移：参数字节错。
    if t.verify_sig(0, name_hash("profile_register"), 11, 0x4B90) != Err(XC_SIG_DRIFT) {
        return Err(XC_SIG_DRIFT);
    }

    // 错误路径：重复注册。
    let mut t2 = ApiFreeze::new();
    t2.register(EntrySig {
        eid: 0,
        name_hash: 1,
        param_bytes: 0,
        err_base: 1,
        idempotent: true,
        min_ver: 0,
    })?;
    if t2.register(EntrySig {
        eid: 0,
        name_hash: 2,
        param_bytes: 0,
        err_base: 1,
        idempotent: true,
        min_ver: 0,
    }) != Err(XC_ENTRY_DUP)
    {
        return Err(XC_ENTRY_DUP);
    }
    // 错误路径：参数超限。
    if t2.register(EntrySig {
        eid: 1,
        name_hash: 1,
        param_bytes: PARAM_MAX as u8 + 1,
        err_base: 1,
        idempotent: true,
        min_ver: 0,
    }) != Err(XC_PARAM_OVER)
    {
        return Err(XC_PARAM_OVER);
    }
    // 错误路径：入口号越界。
    if t2.register(EntrySig {
        eid: ENTRY_CAP as u8,
        name_hash: 1,
        param_bytes: 0,
        err_base: 1,
        idempotent: true,
        min_ver: 0,
    }) != Err(XC_BAD_ENTRY)
    {
        return Err(XC_BAD_ENTRY);
    }
    // 确定性：同表两次冻结同哈希。
    let a = standard_table()?.freeze()?;
    let b = standard_table()?.freeze()?;
    if a != b {
        return Err(XC_HASH_MISMATCH);
    }
    let _ = h;
    Ok(0)
}
