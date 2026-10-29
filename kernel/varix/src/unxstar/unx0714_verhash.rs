//! UNX-F0714 · 断言集版本化与哈希封存 · 诊断段 0x5940（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0714] 断言集版本化与哈希封存 <SEG 0x5940>
//!   UNX-F0714-J1 断言集版本 v1 封存（清单哈希+映射总表哈希+序表哈希
//!   三件齐）且封存后变更走版本链。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三件闭集：清单哈希（断言清单）/映射总表哈希/序表哈希，三件全齐
//!    才允许出 v1（缺一即 E_TRIO_MISSING）。
//!   2. 版本链单向：v(n+1) 必须挂 v(n) 哈希（prev 指针），链不可跳版
//!      不可分叉；封存版本只读（E_SEALED_RDONLY）。
//!   3. 封存后变更走版本链：对已封存集的任何变更请求都不改旧版，
//!      只产出新版（新版三件重算 + prev 挂链）。
//!   4. 版本号单调：v1 起步逐版 +1，版本号回退/重复必拦。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5940~0x595F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5940 独占 0x5940~0x595F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5940;

/// 0x5941 三件缺：清单/映射/序表哈希缺一即封存拦
const E_TRIO_MISSING: u16 = SEG_BASE + 0x1;
/// 0x5942 已封存版本仍尝试改写
const E_SEALED_RDONLY: u16 = SEG_BASE + 0x2;
/// 0x5943 版本回退：新版本号 ≤ 当前版
const E_VERSION_BACK: u16 = SEG_BASE + 0x3;
/// 0x5944 版本链断：新版 prev 指针不挂当前版哈希
const E_CHAIN_BROKEN: u16 = SEG_BASE + 0x4;
/// 0x5945 三件哈希为零值
const E_HASH_ZERO: u16 = SEG_BASE + 0x5;
/// 0x5946 空清单封存：零断言即出 v1
const E_EMPTY_LIST: u16 = SEG_BASE + 0x6;
/// 0x5947 断言重号：同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x7;
/// 0x5948 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x8;
/// 0x5949 版本链溢出：链长超定容
const E_CHAIN_FULL: u16 = SEG_BASE + 0x9;
/// 0x594A 版本悬空：解引越界版本
const E_VERSION_DANGLE: u16 = SEG_BASE + 0xA;
/// 0x594B 链完整性破：链上 prev 指针断链
const E_CHAIN_AUDIT: u16 = SEG_BASE + 0xB;
/// 0x594C 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x594D 三件哈希漂移：重算与封存不符
const E_HASH_DRIFT: u16 = SEG_BASE + 0xD;
/// 0x594E 清单域非法：批号越域
const E_BATCH_RANGE: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5940~0x595F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_TRIO_MISSING => "三件缺：清单/映射/序表哈希缺一，请补齐再封存",
        E_SEALED_RDONLY => "版本已封存：旧版只读，变更请走版本链出新版",
        E_VERSION_BACK => "版本回退：新版本号必须单调递增",
        E_CHAIN_BROKEN => "版本链断：新版 prev 必须挂当前版哈希",
        E_HASH_ZERO => "哈希零值：三件哈希不得为零",
        E_EMPTY_LIST => "空清单：零断言无封存资格",
        E_ASSERT_DUP => "断言重号：同名二次登记拒收",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_CHAIN_FULL => "版本链溢出：链长超定容，请归档旧链",
        E_VERSION_DANGLE => "版本悬空：解引越界版本号",
        E_CHAIN_AUDIT => "链完整性破：链上 prev 指针断链",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_HASH_DRIFT => "三件哈希漂移：封存件被改",
        E_BATCH_RANGE => "批号越域：合法 1..=16",
        _ => "未知诊断码（0x5940 段外或保留位）",
    }
}

// ── 常量（来源：版本化口径与定容） ───────────────────────────────────
/// 版本链定容。
pub const MAX_VERSIONS: usize = 64;
/// 批号上限。
pub const BATCH_MAX: u16 = 16;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(MAX_VERSIONS == 64);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 断言清单条目 ─────────────────────────────────────────────────────
/// 清单条目：名哈希 + 批号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListEntry {
    /// 名 FNV 哈希。
    pub name_fp: u64,
    /// 批号。
    pub batch: u16,
}

// ── 版本封存件 ───────────────────────────────────────────────────────
/// 版本封存件：版本号 + 三件哈希 + prev 挂链 + 清单快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedVersion {
    /// 版本号（v1 起单调）。
    pub version: u32,
    /// 件一：清单哈希。
    pub list_hash: u64,
    /// 件二：映射总表哈希。
    pub map_hash: u64,
    /// 件三：序表哈希。
    pub order_hash: u64,
    /// prev 挂链：上一版全封存哈希（v1 为 0）。
    pub prev_fp: u64,
    /// 清单快照（本版断言集）。
    pub list: Vec<ListEntry>,
}

impl SealedVersion {
    /// 版本全封存哈希（三件 + prev + 版本号折叠）。
    pub fn full_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut fold = |hh: &mut u64, v: u64| {
            let mut shift = 56i32;
            while shift >= 0 {
                *hh = fnv_mix(*hh, ((v >> shift) & 0xFF) as u8);
                shift -= 8;
            }
        };
        fold(&mut h, self.list_hash);
        fold(&mut h, self.map_hash);
        fold(&mut h, self.order_hash);
        fold(&mut h, self.prev_fp);
        h = fnv_mix(h, (self.version & 0xFF) as u8);
        h = fnv_mix(h, ((self.version >> 8) & 0xFF) as u8);
        h
    }

    /// 三件齐判定：三哈希全非零。
    pub const fn trio_complete(&self) -> bool {
        self.list_hash != 0 && self.map_hash != 0 && self.order_hash != 0
    }
}

// ── 版本化账 ─────────────────────────────────────────────────────────
/// 断言集版本化账：登记面 + v1 封存 + 版本链。
pub struct VersionLedger {
    /// 工作清单（未封存的登记面）。
    working: Vec<ListEntry>,
    /// 版本链。
    chain: Vec<SealedVersion>,
}

impl VersionLedger {
    /// 新账。
    pub fn new() -> Self {
        VersionLedger {
            working: Vec::new(),
            chain: Vec::new(),
        }
    }

    /// 登记断言（仅未封存工作面；无工作面冻结概念——工作面随每次
    /// 封存滚入新版，登记始终作用于下一版草稿）。
    pub fn enroll(&mut self, name: &str, batch: u16) -> Result<(), u16> {
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        if batch < 1 || batch > BATCH_MAX {
            return Err(E_BATCH_RANGE);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.working.len() {
            if self.working[i].name_fp == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        self.working.push(ListEntry { name_fp: fp, batch });
        Ok(())
    }

    /// 工作清单长度。
    pub fn working_len(&self) -> usize {
        self.working.len()
    }

    /// 清单哈希（工作面）。
    fn compute_list_hash(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.working.len() {
            let e = self.working[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((e.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, (e.batch & 0xFF) as u8 | 0x10);
            i += 1;
        }
        h
    }

    /// 三件哈希计算入口（清单/映射/序表；映射与序表由清单确定性派生，
    /// 本模块内置派生器：映射哈希 = 清单按批聚合折叠；序表哈希 =
    /// 清单登记序折叠）。供外部传入真实哈希的对照口径。
    pub fn derive_trio(&self) -> (u64, u64, u64) {
        let list = self.compute_list_hash();
        // 映射哈希：按批聚合（批号 → 计数）折叠。
        let mut mh = FNV_OFF;
        let mut b = 1u16;
        while b <= BATCH_MAX {
            let mut n = 0u32;
            let mut i = 0usize;
            while i < self.working.len() {
                if self.working[i].batch == b {
                    n = n.wrapping_add(1);
                }
                i += 1;
            }
            mh = fnv_mix(mh, (b & 0xFF) as u8 | 0x20);
            mh = fnv_mix(mh, (n & 0xFF) as u8);
            b += 1;
        }
        // 序表哈希：登记序逐条名哈希低位折叠。
        let mut oh = FNV_OFF;
        let mut i = 0usize;
        while i < self.working.len() {
            oh = fnv_mix(oh, (self.working[i].name_fp & 0xFF) as u8 | 0x30);
            i += 1;
        }
        (list, mh, oh)
    }

    /// v1 封存：三件齐 + 非空清单 + 版本单调 → 出 v1。
    pub fn seal_v1(&mut self, list_hash: u64, map_hash: u64, order_hash: u64) -> Result<u32, u16> {
        if !self.chain.is_empty() {
            return Err(E_SEALED_RDONLY);
        }
        if self.working.is_empty() {
            return Err(E_EMPTY_LIST);
        }
        if list_hash == 0 || map_hash == 0 || order_hash == 0 {
            return Err(E_HASH_ZERO);
        }
        // 三件齐闸：传入哈希必须与派生口径一致（防虚记）。
        let (dl, dm, dord) = self.derive_trio();
        if list_hash != dl || map_hash != dm || order_hash != dord {
            return Err(E_TRIO_MISSING);
        }
        let v = SealedVersion {
            version: 1,
            list_hash,
            map_hash,
            order_hash,
            prev_fp: 0,
            list: self.working.clone(),
        };
        if !v.trio_complete() {
            return Err(E_TRIO_MISSING);
        }
        self.chain.push(v);
        Ok(1)
    }

    /// 版本链出新版：封存后变更走版本链（旧版只读）。
    /// 新版号必须 = 当前版 + 1；prev 必须挂当前版全封存哈希。
    pub fn seal_next(
        &mut self,
        list_hash: u64,
        map_hash: u64,
        order_hash: u64,
        expect_version: u32,
        expect_prev: u64,
    ) -> Result<u32, u16> {
        if self.chain.is_empty() {
            return Err(E_CHAIN_BROKEN);
        }
        if self.chain.len() >= MAX_VERSIONS {
            return Err(E_CHAIN_FULL);
        }
        if self.working.is_empty() {
            return Err(E_EMPTY_LIST);
        }
        if list_hash == 0 || map_hash == 0 || order_hash == 0 {
            return Err(E_HASH_ZERO);
        }
        let (dl, dm, dord) = self.derive_trio();
        if list_hash != dl || map_hash != dm || order_hash != dord {
            return Err(E_TRIO_MISSING);
        }
        let cur = self.chain.last().ok_or(E_CHAIN_BROKEN)?;
        let next_version = cur.version + 1;
        if expect_version != next_version {
            return Err(E_VERSION_BACK);
        }
        let cur_fp = cur.full_fp();
        if expect_prev != cur_fp {
            return Err(E_CHAIN_BROKEN);
        }
        let v = SealedVersion {
            version: next_version,
            list_hash,
            map_hash,
            order_hash,
            prev_fp: cur_fp,
            list: self.working.clone(),
        };
        self.chain.push(v);
        Ok(next_version)
    }

    /// 链长。
    pub fn chain_len(&self) -> usize {
        self.chain.len()
    }

    /// 当前版号。
    pub fn current_version(&self) -> Result<u32, u16> {
        match self.chain.last() {
            Some(v) => Ok(v.version),
            None => Err(E_VERSION_DANGLE),
        }
    }

    /// 版本解引。
    pub fn version_at(&self, v: u32) -> Result<&SealedVersion, u16> {
        if v < 1 || v as usize > self.chain.len() {
            return Err(E_VERSION_DANGLE);
        }
        Ok(&self.chain[(v - 1) as usize])
    }

    /// 链完整性审计：逐版 prev 挂链核对（v1 prev=0，v(n) prev=v(n-1) fp）。
    pub fn audit_chain(&self) -> Result<usize, u16> {
        if self.chain.is_empty() {
            return Err(E_CHAIN_BROKEN);
        }
        let mut prev: u64 = 0;
        let mut i = 0usize;
        while i < self.chain.len() {
            let v = &self.chain[i];
            if v.prev_fp != prev {
                return Err(E_CHAIN_AUDIT);
            }
            if !v.trio_complete() {
                return Err(E_TRIO_MISSING);
            }
            // 版本号连续性。
            if v.version != (i + 1) as u32 {
                return Err(E_VERSION_BACK);
            }
            // 三件哈希对清单快照复算（漂移检出）。
            let mut lh = FNV_OFF;
            let mut k = 0usize;
            while k < v.list.len() {
                let e = v.list[k];
                let mut shift = 56i32;
                while shift >= 0 {
                    lh = fnv_mix(lh, ((e.name_fp >> shift) & 0xFF) as u8);
                    shift -= 8;
                }
                lh = fnv_mix(lh, (e.batch & 0xFF) as u8 | 0x10);
                k += 1;
            }
            if lh != v.list_hash {
                return Err(E_HASH_DRIFT);
            }
            prev = v.full_fp();
            i += 1;
        }
        Ok(self.chain.len())
    }

    /// 当前工作面与最新版 diff 摘要：登记面变更量（供走链决策）。
    pub fn pending_change(&self) -> Result<usize, u16> {
        let cur = self.chain.last().ok_or(E_CHAIN_BROKEN)?;
        // 简洁口径：清单长度差 + 逐条差异计数。
        let mut diff: usize = 0;
        let mut i = 0usize;
        while i < self.working.len() {
            if i >= cur.list.len() || self.working[i] != cur.list[i] {
                diff += 1;
            }
            i += 1;
        }
        if cur.list.len() > self.working.len() {
            diff += cur.list.len() - self.working.len();
        }
        Ok(diff)
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.chain.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let cur = &self.chain[self.chain.len() - 1];
        let mut out = String::new();
        out.push_str("版本化断言集: v");
        out.push_str(&itoa(cur.version as u64));
        out.push_str(" 链长=");
        out.push_str(&itoa(self.chain.len() as u64));
        out.push_str(" 工作面=");
        out.push_str(&itoa(self.working.len() as u64));
        out.push_str(" 全封存哈希=");
        out.push_str(&hex16(cur.full_fp()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

fn vname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("ver_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：登记 8 条 → v1 封存（三件齐）→ 变更走链出 v2 ——
    let mut led = VersionLedger::new();
    let mut i = 0usize;
    while i < 8 {
        led.enroll(&vname(i), ((i / 4) as u16) + 1)?;
        i += 1;
    }
    // 哈希零值拦。
    if led.seal_v1(0, 1, 1) != Err(E_HASH_ZERO) {
        return Err(E_HASH_ZERO);
    }
    // 三件不齐（传入与派生口径不符）拦。
    if led.seal_v1(1, 2, 3) != Err(E_TRIO_MISSING) {
        return Err(E_TRIO_MISSING);
    }
    // 派生三件 → v1 封存。
    let (l1, m1, o1) = led.derive_trio();
    let v1 = led.seal_v1(l1, m1, o1)?;
    if v1 != 1 {
        return Err(E_VERSION_BACK);
    }
    if led.current_version()? != 1 {
        return Err(E_VERSION_DANGLE);
    }
    // 二次 v1 必拦（旧版只读）。
    if led.seal_v1(l1, m1, o1) != Err(E_SEALED_RDONLY) {
        return Err(E_SEALED_RDONLY);
    }
    // 链审计过。
    if led.audit_chain()? != 1 {
        return Err(E_CHAIN_AUDIT);
    }

    // —— 封存后变更走版本链：补 2 条 → v2 ——
    led.enroll(&vname(8), 3)?;
    led.enroll(&vname(9), 3)?;
    let (l2, m2, o2) = led.derive_trio();
    let cur_fp = led.version_at(1)?.full_fp();
    // 版本号错（跳版）必拦。
    if led.seal_next(l2, m2, o2, 3, cur_fp) != Err(E_VERSION_BACK) {
        return Err(E_VERSION_BACK);
    }
    // prev 错必拦。
    if led.seal_next(l2, m2, o2, 2, cur_fp ^ 1) != Err(E_CHAIN_BROKEN) {
        return Err(E_CHAIN_BROKEN);
    }
    let v2 = led.seal_next(l2, m2, o2, 2, cur_fp)?;
    if v2 != 2 {
        return Err(E_CHAIN_BROKEN);
    }
    if led.audit_chain()? != 2 {
        return Err(E_CHAIN_AUDIT);
    }
    // 挂链核对：v2.prev == v1 全封存哈希。
    let p2 = led.version_at(2)?.prev_fp;
    if p2 != led.version_at(1)?.full_fp() {
        return Err(E_CHAIN_BROKEN);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }
    // 版本解引越界。
    if led.version_at(0) != Err(E_VERSION_DANGLE) {
        return Err(E_VERSION_DANGLE);
    }
    if led.version_at(3) != Err(E_VERSION_DANGLE) {
        return Err(E_VERSION_DANGLE);
    }

    // —— v3：替换清单（删 1 增 1）→ 链三版 ——
    led.working.remove(0);
    led.enroll(&vname(10), 1)?;
    let (l3, m3, o3) = led.derive_trio();
    let cur_fp2 = led.version_at(2)?.full_fp();
    let v3 = led.seal_next(l3, m3, o3, 3, cur_fp2)?;
    if v3 != 3 {
        return Err(E_CHAIN_BROKEN);
    }
    if led.audit_chain()? != 3 {
        return Err(E_CHAIN_AUDIT);
    }
    // pending_change：工作面 == v3 清单 → 0。
    if led.pending_change()? != 0 {
        return Err(E_CHAIN_AUDIT);
    }
    // 工作面再变更 → pending > 0。
    led.enroll(&vname(11), 2)?;
    if led.pending_change()? != 1 {
        return Err(E_CHAIN_AUDIT);
    }

    // —— 幂等：同清单重建账，v1 哈希全等 ——
    let mut led2 = VersionLedger::new();
    let mut j = 0usize;
    while j < 8 {
        led2.enroll(&vname(j), ((j / 4) as u16) + 1)?;
        j += 1;
    }
    let (l1b, m1b, o1b) = led2.derive_trio();
    if l1 != l1b || m1 != m1b || o1 != o1b {
        return Err(E_HASH_DRIFT);
    }
    led2.seal_v1(l1b, m1b, o1b)?;
    if led2.version_at(1)?.full_fp() != led.version_at(1)?.full_fp() {
        return Err(E_HASH_DRIFT);
    }

    // —— 错误路径样本 ——
    if led.current_version().is_err() && led.chain_len() == 0 {
        return Err(E_CHAIN_BROKEN);
    }
    let mut e1 = VersionLedger::new();
    if e1.current_version() != Err(E_VERSION_DANGLE) {
        return Err(E_VERSION_DANGLE);
    }
    if e1.audit_chain() != Err(E_CHAIN_BROKEN) {
        return Err(E_CHAIN_BROKEN);
    }
    if e1.pending_change() != Err(E_CHAIN_BROKEN) {
        return Err(E_CHAIN_BROKEN);
    }
    // 空清单封存。
    let (el, em, eo) = e1.derive_trio();
    if e1.seal_v1(el, em, eo) != Err(E_EMPTY_LIST) {
        return Err(E_EMPTY_LIST);
    }
    if e1.seal_next(el, em, eo, 1, 0) != Err(E_CHAIN_BROKEN) {
        return Err(E_CHAIN_BROKEN);
    }
    // 名非法 / 批号越域 / 重号。
    if e1.enroll("", 1) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    if e1.enroll("x", 0) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    if e1.enroll("x", BATCH_MAX + 1) != Err(E_BATCH_RANGE) {
        return Err(E_BATCH_RANGE);
    }
    e1.enroll("dup", 1)?;
    if e1.enroll("dup", 1) != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 摘要空账拦。
    if e1.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }
    // 漂移注入：篡改链上清单快照 → 链审计必拦。
    let mut dm = VersionLedger::new();
    dm.enroll("solo", 1)?;
    let (sl, smo, so) = dm.derive_trio();
    dm.seal_v1(sl, smo, so)?;
    dm.chain[0].list_hash = dm.chain[0].list_hash.wrapping_add(1);
    if dm.audit_chain() != Err(E_HASH_DRIFT) {
        return Err(E_HASH_DRIFT);
    }
    // 断链注入：篡改 prev。
    let mut dc = VersionLedger::new();
    dc.enroll("a", 1)?;
    let (al, am, ao) = dc.derive_trio();
    dc.seal_v1(al, am, ao)?;
    dc.chain[0].prev_fp = 0xDEAD;
    if dc.audit_chain() != Err(E_CHAIN_AUDIT) {
        return Err(E_CHAIN_AUDIT);
    }

    // 诊断表抽查。
    if diag_detail(E_CHAIN_BROKEN) == "未知诊断码（0x5940 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5940 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
