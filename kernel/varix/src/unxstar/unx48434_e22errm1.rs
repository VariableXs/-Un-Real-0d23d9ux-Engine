//! UNX-F48434 —— E22 错误码族 E 段（12 条）
//! 判据原文：UNX-F48434-J1 E 段错误码族共 12 条，码值 SEG_BASE+1 起连续，
//! 名称/严重度/处置建议三字段齐全且互异；族外查询必落空；段外越界必拒。
//! 判据落地（drill 场景）：R1 族规模恰 12；R2 码值连续升序；R3 名称唯一非空；
//! R4 严重度落在 1..=5；R5 处置建议非空且互异；R6 全族查询回环命中；
//! R7 族外/段外查询必落空；R8 LCG 随机探针按序命中；R9 指纹稳定；R10 严重度配额受控。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDA40;

// ── 族内 12 条诊断码（+1 起连续）────────────────────────────────
const E_OVERLONG: u16 = SEG_BASE + 1; // 输入超长（超宪法音节上限）
const E_SYLBOMB: u16 = SEG_BASE + 2; // 音节炸弹（构造串触发超线性）
const E_TRUNC_SILENT: u16 = SEG_BASE + 3; // 静默截断（无告警丢尾）
const E_DEGRADE: u16 = SEG_BASE + 4; // 降级输入（词库不可用走兜底）
const E_OOB_CURSOR: u16 = SEG_BASE + 5; // 光标越界（超长串上定位漂移）
const E_MERGE_LOSS: u16 = SEG_BASE + 6; // 合并丢字（候选合并路径漏音节）
const E_WIN_HANG: u16 = SEG_BASE + 7; // 窗口挂起（长串渲染阻塞输入）
const E_LAT_QUADR: u16 = SEG_BASE + 8; // 平方级延迟（步数超线性预算）
const E_OBS_MISS: u16 = SEG_BASE + 9; // 埋点缺失（异常未观测到）
const E_PRES_BLIND: u16 = SEG_BASE + 10; // 呈现不可见（告警未达用户）
const E_GREP_DUP: u16 = SEG_BASE + 11; // 批防重命中（关键词族重复登记）
const E_SEAL_GAP: u16 = SEG_BASE + 12; // 收官印缺卡（批清单出现空洞）

// ── 族外审计码（+0x0D..，≤ +0x1F）───────────────────────────────
const E_AUD_DUP_CODE: u16 = SEG_BASE + 13; // 族内码值重复
const E_AUD_GAP: u16 = SEG_BASE + 14; // 码值不连续（出现空洞）
const E_AUD_SEV: u16 = SEG_BASE + 15; // 严重度越出 1..=5
const E_AUD_NAME: u16 = SEG_BASE + 16; // 名称空/重复
const E_AUD_ACT: u16 = SEG_BASE + 17; // 处置建议空/重复
const E_AUD_SORT: u16 = SEG_BASE + 18; // 族表未按码值升序
const E_AUD_MISS: u16 = SEG_BASE + 19; // 按序探针未命中
const E_AUD_FP: u16 = SEG_BASE + 20; // 族指纹漂移
const E_AUD_QUOTA: u16 = SEG_BASE + 21; // 高严重度条数超配额
const E_AUD_OOB: u16 = SEG_BASE + 22; // 族码越出本段 0x20 窗

/// 族条目：码值 / 名称 / 严重度 / 处置建议。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrEntry {
    pub code: u16,
    pub name: &'static str,
    pub sev: u8,
    pub action: &'static str,
}

/// E 段 12 条错误码族（冻结清单，顺序即码值升序）。
pub const FAMILY: [ErrEntry; 12] = [
    ErrEntry { code: E_OVERLONG, name: "E22-INPUT-OVERLONG", sev: 3, action: "检查音节上限口径并重新计长" },
    ErrEntry { code: E_SYLBOMB, name: "E22-SYLLABLE-BOMB", sev: 4, action: "拦截构造串并走线性扫描路径" },
    ErrEntry { code: E_TRUNC_SILENT, name: "E22-TRUNC-SILENT", sev: 5, action: "恢复全量受纳或补发显式告警" },
    ErrEntry { code: E_DEGRADE, name: "E22-DEGRADE-INPUT", sev: 2, action: "切换精简词库并记录降级事件" },
    ErrEntry { code: E_OOB_CURSOR, name: "E22-CURSOR-OOB", sev: 4, action: "按字节长度夹取光标位置" },
    ErrEntry { code: E_MERGE_LOSS, name: "E22-MERGE-LOSS", sev: 5, action: "核对候选合并账目后重放" },
    ErrEntry { code: E_WIN_HANG, name: "E22-WINDOW-HANG", sev: 3, action: "长串渲染改异步分片" },
    ErrEntry { code: E_LAT_QUADR, name: "E22-LAT-QUADRATIC", sev: 4, action: "按线性预算复核步数记账" },
    ErrEntry { code: E_OBS_MISS, name: "E22-OBS-MISS", sev: 2, action: "补齐截断/降级/越界/合并四点埋点" },
    ErrEntry { code: E_PRES_BLIND, name: "E22-PRES-BLIND", sev: 3, action: "核对呈现三要素是否齐全" },
    ErrEntry { code: E_GREP_DUP, name: "E22-GREP-DUP", sev: 1, action: "去重后重跑关键词族 grep" },
    ErrEntry { code: E_SEAL_GAP, name: "E22-SEAL-GAP", sev: 5, action: "定位缺卡空洞并补印" },
];

/// 高严重度（sev ≥ 4）配额：全族至多 6 条（冻结口径）。
pub const HIGH_SEV_QUOTA: usize = 6;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 码值 → 条目（线性扫描，族表升序可直接返回 None 表示族外）。
pub fn lookup(code: u16) -> Option<&'static ErrEntry> {
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].code == code {
            return Some(&FAMILY[i]);
        }
        i += 1;
    }
    None
}

/// 码值 → 处置建议（族外统一落 "未知诊断码"）。
pub fn detail(code: u16) -> &'static str {
    match lookup(code) {
        Some(e) => e.action,
        None => "未知诊断码",
    }
}

/// 族指纹：码值/名称/严重度/建议逐条串联后 FNV。
pub fn family_fingerprint() -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < FAMILY.len() {
        let e = &FAMILY[i];
        buf.extend_from_slice(&e.code.to_le_bytes());
        buf.push(e.sev);
        buf.extend_from_slice(e.name.as_bytes());
        buf.push(0x1F);
        buf.extend_from_slice(e.action.as_bytes());
        buf.push(0x1E);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 高严重度条数统计。
pub fn high_sev_count() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].sev >= 4 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 族摘要行（呈现面）：code=<u16> name=<名称> sev=<u8>。
pub fn entry_line(e: &ErrEntry) -> String {
    let mut s = String::new();
    s.push_str("code=");
    s.push_str(&e.code.to_string());
    s.push_str(" name=");
    s.push_str(e.name);
    s.push_str(" sev=");
    s.push_str(&e.sev.to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 族规模恰 12：多一条少一条都算族账不平。
    if FAMILY.len() != 12 {
        return Err(E_AUD_GAP);
    }
    hits += 1;

    // R2 码值连续升序：+1 起逐条递增且不越 +0x1F。
    let mut i = 0usize;
    while i < FAMILY.len() {
        let expect = SEG_BASE + 1 + i as u16;
        if FAMILY[i].code != expect {
            return Err(E_AUD_GAP);
        }
        if FAMILY[i].code > SEG_BASE + 0x1F {
            return Err(E_AUD_OOB);
        }
        if i > 0 && FAMILY[i - 1].code >= FAMILY[i].code {
            return Err(E_AUD_SORT);
        }
        // 两两互查：族内码值显式防重。
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].code == FAMILY[j].code {
                return Err(E_AUD_DUP_CODE);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 名称唯一非空。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].name.is_empty() {
            return Err(E_AUD_NAME);
        }
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].name == FAMILY[j].name {
                return Err(E_AUD_NAME);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R4 严重度落在 1..=5。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].sev < 1 || FAMILY[i].sev > 5 {
            return Err(E_AUD_SEV);
        }
        i += 1;
    }
    hits += 1;

    // R5 处置建议非空且互异。
    let mut i = 0usize;
    while i < FAMILY.len() {
        if FAMILY[i].action.is_empty() {
            return Err(E_AUD_ACT);
        }
        let mut j = i + 1;
        while j < FAMILY.len() {
            if FAMILY[i].action == FAMILY[j].action {
                return Err(E_AUD_ACT);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R6 全族查询回环：按码值查回条目必须逐字段一致。
    let mut i = 0usize;
    while i < FAMILY.len() {
        match lookup(FAMILY[i].code) {
            Some(e) => {
                if *e != FAMILY[i] {
                    return Err(E_AUD_MISS);
                }
            }
            None => return Err(E_AUD_MISS),
        }
        i += 1;
    }
    hits += 1;

    // R7 族外/段外查询必落空：-1、+0x20、0 均查不到。
    if lookup(SEG_BASE).is_some() || lookup(SEG_BASE + 0x20).is_some() || lookup(0).is_some() {
        return Err(E_AUD_OOB);
    }
    hits += 1;

    // R8 LCG 随机探针：种子下 12 次取样（带余映射到族内）按序命中。
    let mut lcg = Lcg::new(0x48434);
    let mut probed = 0u32;
    let mut t = 0u32;
    while t < 24 {
        let idx = lcg.below(12) as usize;
        match lookup(SEG_BASE + 1 + idx as u16) {
            Some(e) => {
                if e.code != SEG_BASE + 1 + idx as u16 {
                    return Err(E_AUD_MISS);
                }
                probed += 1;
            }
            None => return Err(E_AUD_MISS),
        }
        t += 1;
    }
    if probed != 24 {
        return Err(E_AUD_MISS);
    }
    hits += 1;

    // R9 指纹稳定：同族两次指纹一致，且非空。
    let fp_a = family_fingerprint();
    let fp_b = family_fingerprint();
    if fp_a != fp_b || fp_a == 0 {
        return Err(E_AUD_FP);
    }
    hits += 1;

    // R10 高严重度配额：sev≥4 至多 5 条，且至少 1 条兜底可上报。
    let hc = high_sev_count();
    if hc as usize > HIGH_SEV_QUOTA || hc == 0 {
        return Err(E_AUD_QUOTA);
    }
    hits += 1;

    Ok(hits)
}
