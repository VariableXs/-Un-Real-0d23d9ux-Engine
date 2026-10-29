//! unx36018_posixbridge — UNX-F36018 SID 与 POSIX uid/gid 桥的最小对照面
//! （B31 桥锚预告）。
//!
//! 判据原文：
//!   J1 uid↔RID 双向 100 例与发射器零差；无对应 uid 的 SID 注入 10 例
//!      桥面返回 NOT_MAPPED 显式码。
//! 设计纪律：
//!   1. 桥锚：RID = RID_BASE + uid、域前缀 = unx36005 发射器同锚
//!      （S-1-5-21-<a>-<b>-<c>）——两模块单一真源复用，杜绝双口径。
//!   2. 桥表：映射账（uid, gid, rid）登记制——uid↔RID 双向查表 +
//!      登记时复算发射器公式对平（双端口互证）。
//!   3. NOT_MAPPED 显性面：RID 越出发射器全域（或未登记）一律
//!      NOT_MAPPED 显式码，绝不猜测映射。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7490;

// ── 诊断码闭集（0x7490~0x74AF，独占）─────────────────────────
const E_NOT_MAPPED: u16 = SEG_BASE + 1; // 无对应 uid（NOT_MAPPED 显式码）
const E_RANGE: u16 = SEG_BASE + 2; // uid/RID 越出发射器全域
const E_ANCHOR: u16 = SEG_BASE + 3; // 与发射器公式对平失配
const E_BAL: u16 = SEG_BASE + 4; // 桥账守恒破
const E_CAP: u16 = SEG_BASE + 5; // 桥表满（显性拒）

use crate::checks::CheckSet;
use super::unx36001_sidcore::{Sid, MAX_SUBS};
use super::unx36005_issuer;

/// 桥表容量。
pub const BRIDGE_CAP: usize = 128;

/// 桥表行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BridgeRow {
    pub uid: u32,
    pub gid: u32,
    pub rid: u32,
}

/// POSIX 桥。
pub struct PosixBridge {
    rows: [Option<BridgeRow>; BRIDGE_CAP],
    pub mapped: u32,
    pub not_mapped: u32,
    pub unbinds: u32,
    pub req_total: u32,
}

impl PosixBridge {
    pub fn new() -> PosixBridge {
        PosixBridge {
            rows: [None; BRIDGE_CAP],
            mapped: 0,
            not_mapped: 0,
            unbinds: 0,
            req_total: 0,
        }
    }

    /// 登记：uid/gid → rid（发射器公式）+ 入账（同 uid 幂等更新）。
    pub fn bind(&mut self, uid: u32, gid: u32) -> Result<u32, u16> {
        let rid = unx36005_issuer::rid_of(uid)?; // 越界 uid 显性拒
        let mut i = 0;
        while i < BRIDGE_CAP {
            if let Some(r) = self.rows[i] {
                if r.uid == uid {
                    self.rows[i] = Some(BridgeRow { uid, gid, rid });
                    return Ok(rid);
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < BRIDGE_CAP {
            if self.rows[j].is_none() {
                self.rows[j] = Some(BridgeRow { uid, gid, rid });
                return Ok(rid);
            }
            j += 1;
        }
        Err(E_CAP)
    }

    /// uid → RID（桥面：未登记 = NOT_MAPPED）。
    pub fn to_rid(&mut self, uid: u32) -> Result<u32, u16> {
        self.req_total += 1;
        let mut i = 0;
        while i < BRIDGE_CAP {
            if let Some(r) = self.rows[i] {
                if r.uid == uid {
                    self.mapped += 1;
                    return Ok(r.rid);
                }
            }
            i += 1;
        }
        self.not_mapped += 1;
        Err(E_NOT_MAPPED)
    }

    /// RID → uid（桥面反向：未登记/越界 = NOT_MAPPED）。
    pub fn to_uid(&mut self, rid: u32) -> Result<u32, u16> {
        self.req_total += 1;
        let mut i = 0;
        while i < BRIDGE_CAP {
            if let Some(r) = self.rows[i] {
                if r.rid == rid {
                    self.mapped += 1;
                    return Ok(r.uid);
                }
            }
            i += 1;
        }
        self.not_mapped += 1;
        Err(E_NOT_MAPPED)
    }

    /// RID → 域 SID（发射器同锚桥面：越出全域 NOT_MAPPED）。
    pub fn to_domain_sid(&mut self, rid: u32) -> Result<Sid, u16> {
        self.req_total += 1;
        // RID 必须落在发射器全域 [RID_BASE, RID_BASE + UID_MAX]。
        if rid < unx36005_issuer::RID_BASE
            || rid > unx36005_issuer::RID_BASE.wrapping_add(unx36005_issuer::UID_MAX)
        {
            self.not_mapped += 1;
            return Err(E_NOT_MAPPED);
        }
        let uid = match self.to_uid_inner(rid) {
            Some(u) => u,
            None => {
                self.not_mapped += 1;
                return Err(E_NOT_MAPPED);
            }
        };
        // 与发射器公式对平（双端口互证）。
        if unx36005_issuer::rid_of(uid)? != rid {
            self.req_total -= 1; // 对平失败不入账
            return Err(E_ANCHOR);
        }
        let mut subs = [0u32; MAX_SUBS];
        subs[0] = unx36005_issuer::DOMAIN_SUBS[0];
        subs[1] = unx36005_issuer::DOMAIN_SUBS[1];
        subs[2] = unx36005_issuer::DOMAIN_SUBS[2];
        subs[3] = rid;
        self.mapped += 1;
        Sid::new(1, Sid::ia_from(5), &subs[..4])
    }

    fn to_uid_inner(&self, rid: u32) -> Option<u32> {
        let mut i = 0;
        while i < BRIDGE_CAP {
            if let Some(r) = self.rows[i] {
                if r.rid == rid {
                    return Some(r.uid);
                }
            }
            i += 1;
        }
        None
    }

    /// 守恒：映射 + 未映射 == 总请求。
    pub fn conserve(&self) -> bool {
        self.mapped + self.not_mapped == self.req_total
    }

    /// 解绑：uid → 移除桥行（墓碑语义：行清空，账面留 unbinds 计）。
    /// 未登记 uid 显性 NOT_MAPPED。
    pub fn unbind(&mut self, uid: u32) -> Result<(), u16> {
        let mut i = 0;
        while i < BRIDGE_CAP {
            if let Some(r) = self.rows[i] {
                if r.uid == uid {
                    self.rows[i] = None;
                    self.unbinds += 1;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(E_NOT_MAPPED)
    }

    /// 桥表行数（活行）。
    pub fn live_rows(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < BRIDGE_CAP {
            if self.rows[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// gid 反查（组映射面）：按 gid 找 uid（未命中 NOT_MAPPED）。
    pub fn uid_of_gid(&mut self, gid: u32) -> Result<u32, u16> {
        self.req_total += 1;
        let mut i = 0;
        while i < BRIDGE_CAP {
            if let Some(r) = self.rows[i] {
                if r.gid == gid {
                    self.mapped += 1;
                    return Ok(r.uid);
                }
            }
            i += 1;
        }
        self.not_mapped += 1;
        Err(E_NOT_MAPPED)
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_MAPPED => "无对应 uid（NOT_MAPPED 显式码）",
        E_RANGE => "uid/RID 越出发射器全域",
        E_ANCHOR => "与发射器公式对平失配",
        E_BAL => "桥账守恒破",
        E_CAP => "桥表满（显性拒绝）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例双向零差 + 10 例 NOT_MAPPED（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut br = PosixBridge::new();

    // R1：登记 100 例（uid 0..99 / gid 交错），双向查表与发射器公式
    // 逐例零差；往返 uid→rid→uid 还原。
    let mut n = 0u32;
    while n < 100 {
        let gid = 5000 + n;
        let rid = br.bind(n, gid)?;
        if rid != unx36005_issuer::rid_of(n)? {
            return Err(E_ANCHOR);
        }
        if br.to_rid(n)? != rid || br.to_uid(rid)? != n {
            return Err(E_ANCHOR);
        }
        n += 1;
    }
    hits += 1;

    // R2：域 SID 桥面 100 例——to_domain_sid 与发射器构造逐位对平
    //（独立重建 SID 比指纹）。
    let mut n = 0u32;
    while n < 100 {
        let rid = unx36005_issuer::rid_of(n)?;
        let s = br.to_domain_sid(rid)?;
        if s.rid() != rid || s.ia_value() != 5 {
            return Err(E_ANCHOR);
        }
        if s.subs()[0] != unx36005_issuer::DOMAIN_SUBS[0]
            || s.subs()[1] != unx36005_issuer::DOMAIN_SUBS[1]
            || s.subs()[2] != unx36005_issuer::DOMAIN_SUBS[2]
        {
            return Err(E_ANCHOR);
        }
        let mut expect_subs = [0u32; MAX_SUBS];
        expect_subs[0] = unx36005_issuer::DOMAIN_SUBS[0];
        expect_subs[1] = unx36005_issuer::DOMAIN_SUBS[1];
        expect_subs[2] = unx36005_issuer::DOMAIN_SUBS[2];
        expect_subs[3] = rid;
        let expect = Sid::new(1, Sid::ia_from(5), &expect_subs[..4])?;
        if s.fingerprint() != expect.fingerprint() {
            return Err(E_ANCHOR);
        }
        n += 1;
    }
    hits += 1;

    // R3：NOT_MAPPED 注入 10 例——未登记 RID（全域内但桥表无）×5 +
    // 全域外 RID ×5，全部显性 NOT_MAPPED。
    let mut k = 0u32;
    while k < 5 {
        let rid = unx36005_issuer::RID_BASE + 100 + k; // 桥表只登记 0..99
        if br.to_uid(rid) != Err(E_NOT_MAPPED) {
            return Err(E_NOT_MAPPED);
        }
        if br.to_domain_sid(rid) != Err(E_NOT_MAPPED) {
            return Err(E_NOT_MAPPED);
        }
        k += 1;
    }
    let outside: [u32; 5] = [0, 999, unx36005_issuer::RID_BASE - 1,
        unx36005_issuer::RID_BASE.wrapping_add(unx36005_issuer::UID_MAX) + 1, 0xFFFF_FFFF];
    let mut k = 0;
    while k < 5 {
        if br.to_domain_sid(outside[k]) != Err(E_NOT_MAPPED) {
            return Err(E_NOT_MAPPED);
        }
        k += 1;
    }
    if br.not_mapped < 10 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：守恒——映射 + 未映射 == 总请求；gid 反查（登记面零差）。
    if !br.conserve() {
        return Err(E_BAL);
    }
    // gid 反查：gid=5000+50 → uid=50（登记面零差）。
    if br.uid_of_gid(5000 + 50)? != 50 {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // R5：解绑与重绑——解绑后双向查 NOT_MAPPED；活行数 99；
    // 解绑后重绑（幂等重建）活行数回 100。
    br.unbind(7)?;
    if br.to_rid(7) != Err(E_NOT_MAPPED) {
        return Err(E_NOT_MAPPED);
    }
    if br.to_uid(unx36005_issuer::rid_of(7)?) != Err(E_NOT_MAPPED) {
        return Err(E_NOT_MAPPED);
    }
    if br.live_rows() != 99 {
        return Err(E_BAL);
    }
    if br.unbinds != 1 {
        return Err(E_BAL);
    }
    // 解绑后重绑（幂等重建）。
    if br.bind(7, 5007)? != unx36005_issuer::rid_of(7)? {
        return Err(E_ANCHOR);
    }
    if br.live_rows() != 100 {
        return Err(E_BAL);
    }
    if !br.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36018_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36018_posixbridge");
    cs.add("SEG 段位独占", SEG_BASE == 0x7490, "0x7490~0x74AF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
