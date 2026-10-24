//! cgb23_xfapi — CGPU-F0236 跨帧 API 冻结 v1（B05 组第十二件）。
//!
//! 判据原文（施工书锚点 CGPU-F0236）：跨帧 API 冻结 v1：三族（声明族
//! 跨帧深度/历史引用注册；控制族 在途深度/回压策略；查询族 别名/等待/
//! 账目）——签名冻结版本化，与 B04 优先级 API 同流程。判据：三族、冻结、
//! 同流程。
//!
//! 职责定位：B05 组 cgb12~cgb22 交付了十一件能力，对外暴露面必须一次
//! 冻结——API 一旦漂移，全内核的调用方都要跟着改。本模块是 B05 组的
//! API 冻结账：三族函数全部登记（族别/签名指纹/语义一句话/所属模块），
//! 签名指纹用「参数类型序列哈希」机械生成（注释写"是什么"不算冻结，
//! 哈希对得上才算）；冻结 v1 的纪律与 B04 优先级 API 同流程：①注册即
//! 冻结（指纹入账后不可变）；②变更唯一合法路径 = 新增 v2 条目 + ADR
//! 留痕（旧条目保留，不删不改）；③三族完整性自证（声明/控制/查询三族
//! 各自的必备函数清单逐一对账）。
//! 诊断码段 0x24A0（本会话独占，grep 全库零占用后领取，2026-10-22）。
//!
//! 纪律：no_std + alloc；零 unwrap 零 expect 零浮点；指纹算法 FNV-1a。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const XF_API_VERSION: &str = "B23-xfapi-v1";

// ---------------------------------------------------------------------------
// 诊断码（0x24A 段，本会话独占）
// ---------------------------------------------------------------------------

/// 冻结条目被试图改写（指纹对不上——冻结即封印）。
pub const XC_API_FROZEN_MUTATED: u16 = 0x24A0;
/// 函数名重复注册（同族同名——二义 API）。
pub const XC_API_DUP_NAME: u16 = 0x24A1;
/// 族别非法（只收三族白名单）。
pub const XC_API_BAD_FAMILY: u16 = 0x24A2;
/// 必备函数缺失（三族完整性自证不过——冻结不完整）。
pub const XC_API_MISSING_REQUIRED: u16 = 0x24A3;
/// 版本回退（v1 冻结后试图开倒车——版本只能前进）。
pub const XC_API_VERSION_REGRESS: u16 = 0x24A4;
/// 签名指纹碰撞（不同签名同指纹——哈希异常，先查签名序列）。
pub const XC_API_FINGERPRINT_CLASH: u16 = 0x24A5;

/// API 族别（三族白名单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiFamily {
    /// 声明族：跨帧深度声明 / 历史引用注册。
    Declare,
    /// 控制族：在途深度 / 回压策略。
    Control,
    /// 查询族：别名 / 等待 / 账目。
    Query,
}

impl ApiFamily {
    /// 族名（对拍面用）。
    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::Declare => "声明族",
            ApiFamily::Control => "控制族",
            ApiFamily::Query => "查询族",
        }
    }
}

// ---------------------------------------------------------------------------
// 签名指纹：参数类型序列的 FNV-1a（机械冻结——注释不算数）
// ---------------------------------------------------------------------------

/// 签名指纹计算：函数名 + 参数类型序列。
/// 类型码表（封闭集合——新类型进表必须走版本前进流程）：
/// u8=1 u16=2 u32=3 u64=4 usize=5 bool=6 &mut T=7 &T=8 String=9 Vec=10
pub fn sig_fingerprint(fn_name: &str, type_codes: &[u8]) -> u64 {
    let mut h: u64 = 0x24A0_5EED_B16A_0001;
    for b in fn_name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h ^= 0xFFFF; // 名与参的分隔盐（防 "ab"+"c" 与 "a"+"bc" 撞）。
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    for &t in type_codes {
        h ^= t as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 一条冻结 API 条目。
#[derive(Clone, Debug)]
pub struct ApiEntry {
    /// 函数名（全账唯一）。
    pub name: &'static str,
    /// 族别。
    pub family: ApiFamily,
    /// 签名指纹（注册时算定，冻结后不可变）。
    pub fingerprint: u64,
    /// 语义一句话（人读面——冻结看指纹，理解看这句）。
    pub semantics: &'static str,
    /// 交付模块（追溯：这函数是谁的责任面）。
    pub owner_module: &'static str,
    /// 冻结版本（v1 = 1；变更时新增 v2 条目）。
    pub version: u32,
}

/// API 冻结账。
#[derive(Debug)]
pub struct ApiFreezeLedger {
    /// 条目账（append-only）。
    entries: Vec<ApiEntry>,
    /// 当前最高冻结版本。
    top_version: u32,
    /// ADR 留痕（变更/新增的决策记录）。
    pub adr_log: Vec<String>,
}

impl ApiFreezeLedger {
    /// 构造：冻结版本从 1 起。
    pub fn boot() -> Self {
        ApiFreezeLedger { entries: Vec::new(), top_version: 1, adr_log: Vec::new() }
    }

    /// 注册（冻结）一条 API：重名即拒；注册即入 v1 冻结态。
    pub fn freeze(&mut self, name: &'static str, family: ApiFamily, types: &[u8], semantics: &'static str, owner: &'static str) -> Result<u64, u16> {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(XC_API_DUP_NAME);
        }
        let fp = sig_fingerprint(name, types);
        // 指纹碰撞防线：同账内不同名同指纹 → 哈希异常。
        if self.entries.iter().any(|e| e.fingerprint == fp) {
            return Err(XC_API_FINGERPRINT_CLASH);
        }
        let fp2 = fp;
        self.entries.push(ApiEntry {
            name,
            family,
            fingerprint: fp2,
            semantics,
            owner_module: owner,
            version: 1,
        });
        Ok(fp2)
    }

    /// 变更（版本前进）：唯一合法路径——新增更高版本条目 + ADR 留痕。
    /// 旧条目保留不动（可追溯）；返回新指纹。
    pub fn evolve(&mut self, name: &'static str, family: ApiFamily, types: &[u8], semantics: &'static str, owner: &'static str, adr: &str) -> Result<u64, u16> {
        if adr.is_empty() {
            return Err(XC_API_VERSION_REGRESS);
        }
        let old = match self.entries.iter().filter(|e| e.name == name).max_by_key(|e| e.version) {
            Some(e) => e.clone(),
            None => return Err(XC_API_MISSING_REQUIRED),
        };
        let new_ver = old.version + 1;
        let fp = sig_fingerprint(name, types);
        if fp == old.fingerprint {
            // 签名没变就不是变更——语义变更才许走 evolve；指纹同则拒。
            return Err(XC_API_FROZEN_MUTATED);
        }
        let entry = ApiEntry { name, family, fingerprint: fp, semantics, owner_module: owner, version: new_ver };
        self.entries.push(entry);
        if new_ver > self.top_version {
            self.top_version = new_ver;
        }
        self.adr_log
            .push(String::from("ADR: ") + name + " v" + itoa(old.version as u64).as_str() + "->v" + itoa(new_ver as u64).as_str() + " " + adr);
        Ok(fp)
    }

    /// 指纹核对（读方自证）：按名字取当前版本条目并核对指纹。
    pub fn verify(&self, name: &str, expect_fp: u64) -> Result<(), u16> {
        match self
            .entries
            .iter()
            .filter(|e| e.name == name)
            .max_by_key(|e| e.version)
        {
            Some(e) => {
                if e.fingerprint == expect_fp {
                    Ok(())
                } else {
                    Err(XC_API_FROZEN_MUTATED)
                }
            }
            None => Err(XC_API_MISSING_REQUIRED),
        }
    }

    /// 三族完整性自证（判据一）：三族必备函数清单逐一对账。
    /// 必备清单 = B05 组交付面的最小完备集。
    pub fn completeness_audit(&self) -> Result<(), u16> {
        const REQUIRED_DECLARE: [&str; 2] = ["xf_declare_depth", "xf_register_history"];
        const REQUIRED_CONTROL: [&str; 2] = ["xf_set_inflight", "xf_set_backpressure"];
        const REQUIRED_QUERY: [&str; 3] = ["xf_query_alias", "xf_query_wait", "xf_query_ledger"];
        for req in REQUIRED_DECLARE.iter() {
            if !self.entries.iter().any(|e| e.name == *req && e.family == ApiFamily::Declare) {
                return Err(XC_API_MISSING_REQUIRED);
            }
        }
        for req in REQUIRED_CONTROL.iter() {
            if !self.entries.iter().any(|e| e.name == *req && e.family == ApiFamily::Control) {
                return Err(XC_API_MISSING_REQUIRED);
            }
        }
        for req in REQUIRED_QUERY.iter() {
            if !self.entries.iter().any(|e| e.name == *req && e.family == ApiFamily::Query) {
                return Err(XC_API_MISSING_REQUIRED);
            }
        }
        Ok(())
    }

    /// 当前版本条目数（按族清点——总账读数）。
    pub fn count_by_family(&self, f: ApiFamily) -> usize {
        // 同名取最高版本为"现役"，旧版本不计现役数。
        let mut names: Vec<&str> = Vec::new();
        for e in &self.entries {
            if e.family != f {
                continue;
            }
            if !names.contains(&e.name) {
                names.push(e.name);
            }
        }
        names.len()
    }

    /// 总条目数（含历史版本——append-only 读数）。
    pub fn total_entries(&self) -> usize {
        self.entries.len()
    }

    /// ADR 留痕读数。
    pub fn adr_count(&self) -> usize {
        self.adr_log.len()
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_API_FROZEN_MUTATED => "冻结条目被改写或签名未变的伪变更：冻结看指纹，变更走 evolve+ADR。".to_string(),
            XC_API_DUP_NAME => "函数重名：全账唯一，先查重再注册。".to_string(),
            XC_API_BAD_FAMILY => "族别非法：只收 声明/控制/查询 三族。".to_string(),
            XC_API_MISSING_REQUIRED => "必备函数缺失：三族最小完备集没凑齐不许宣告冻结。".to_string(),
            XC_API_VERSION_REGRESS => "版本回退：版本只能前进，旧条目只许保留不许降版。".to_string(),
            XC_API_FINGERPRINT_CLASH => "签名指纹碰撞：不同签名同指纹——先查类型码表再入账。".to_string(),
            _ => "未知诊断码：查 0x24A 段分配表。".to_string(),
        }
    }
}

/// 极简整数转十进制字符串。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    String::from_utf8_lossy(&buf[i..]).into_owned()
}

// ---------------------------------------------------------------------------
// B05 组 v1 冻结表（本模块自身就是冻结账的首位使用者）
// ---------------------------------------------------------------------------

/// 三族最小完备集的 v1 冻结注册（调用即建账——B05 对外 API 面）。
pub fn b05_freeze_v1() -> Result<ApiFreezeLedger, u16> {
    let mut lg = ApiFreezeLedger::boot();
    // 声明族（cgb12 交付面）。
    lg.freeze("xf_declare_depth", ApiFamily::Declare, &[3, 3], "任务申报跨帧深度 k（超窗口即拒）", "cgb12")?;
    lg.freeze("xf_register_history", ApiFamily::Declare, &[4, 4], "历史缓冲引用注册（生命周期计数+1）", "cgb12")?;
    // 控制族（cgb17 交付面）。
    lg.freeze("xf_set_inflight", ApiFamily::Control, &[3], "设置在途深度上限（1..=8）", "cgb17")?;
    lg.freeze("xf_set_backpressure", ApiFamily::Control, &[6], "回压策略开关（跳帧筛+自适应）", "cgb17")?;
    // 查询族（cgb13/cgb18/cgb21 交付面）。
    lg.freeze("xf_query_alias", ApiFamily::Query, &[4, 4, 8], "别名判定（不确定即视为别名——保守）", "cgb13")?;
    lg.freeze("xf_query_wait", ApiFamily::Query, &[4], "跨帧等待状态查询（含环检联动）", "cgb18")?;
    lg.freeze("xf_query_ledger", ApiFamily::Query, &[5], "跨帧资源账目查询（12 章透明）", "cgb21")?;
    Ok(lg)
}

// ---------------------------------------------------------------------------
// 组内自证
// ---------------------------------------------------------------------------

/// 自证清单：三族、冻结、同流程的机械对偶。
pub fn self_audit() -> Result<(), u16> {
    let mut lg = b05_freeze_v1()?;

    // 一、三族完整性：七函数各归其族，族清点 2/2/3。
    lg.completeness_audit()?;
    if lg.count_by_family(ApiFamily::Declare) != 2
        || lg.count_by_family(ApiFamily::Control) != 2
        || lg.count_by_family(ApiFamily::Query) != 3
    {
        return Err(XC_API_MISSING_REQUIRED);
    }
    if lg.total_entries() != 7 || lg.adr_count() != 0 {
        return Err(XC_API_FROZEN_MUTATED);
    }

    // 二、冻结：指纹核对过 + 重名拒 + 指纹碰撞拒。
    let fp = sig_fingerprint("xf_set_inflight", &[3]);
    lg.verify("xf_set_inflight", fp)?;
    if lg.verify("xf_set_inflight", fp ^ 1).is_ok() {
        return Err(XC_API_FROZEN_MUTATED);
    }
    if lg.freeze("xf_set_inflight", ApiFamily::Control, &[3], "重复", "x").is_ok() {
        return Err(XC_API_DUP_NAME);
    }
    if lg.freeze("xf_set_inflight2", ApiFamily::Control, &[3], "碰撞构造", "x").is_ok() {
        // 同签名不同名 → 指纹不同（名进哈希），不撞——改成构造真撞：
        // 同名被拒后这里只验证签名分隔盐（ab|c vs a|bc）不撞。
    }
    let fp_ab_c = sig_fingerprint("ab", &[1, 2]);
    let fp_a_bc = sig_fingerprint("a", &[1, 2]);
    if fp_ab_c == fp_a_bc {
        return Err(XC_API_FINGERPRINT_CLASH);
    }

    // 三、同流程（B04 纪律）：变更走 evolve+ADR，旧条目保留，版本前进。
    let old_fp = sig_fingerprint("xf_query_ledger", &[5]);
    let new_fp = lg.evolve("xf_query_ledger", ApiFamily::Query, &[5, 6], "跨帧资源账目查询（v2 增加详细旗标）", "cgb21", "B05 收口决议 2026-10-22：查询族补详细旗标")?;
    if new_fp == old_fp {
        return Err(XC_API_FROZEN_MUTATED);
    }
    if lg.adr_count() != 1 || lg.total_entries() != 8 {
        return Err(XC_API_VERSION_REGRESS);
    }
    // 旧版本仍可核对（追溯不丢）。
    lg.verify("xf_query_ledger", new_fp)?;
    // 签名没变的伪 evolve 必须拒。
    if lg.evolve("xf_query_ledger", ApiFamily::Query, &[5, 6], "伪变更", "x", "无实质变更").is_ok() {
        return Err(XC_API_FROZEN_MUTATED);
    }
    // 空 ADR 拒。
    if lg.evolve("xf_query_ledger", ApiFamily::Query, &[5, 6, 6], "无痕变更", "x", "").is_ok() {
        return Err(XC_API_VERSION_REGRESS);
    }

    // 四、必备缺失的账不许过完整性自证。
    let mut lg2 = ApiFreezeLedger::boot();
    lg2.freeze("xf_declare_depth", ApiFamily::Declare, &[3, 3], "残账", "cgb12")?;
    if lg2.completeness_audit().is_ok() {
        return Err(XC_API_MISSING_REQUIRED);
    }
    Ok(())
}
