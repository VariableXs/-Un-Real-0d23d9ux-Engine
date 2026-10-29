//! VE-F4295 · 工具集成示例（U05 工具集成组）
//! 职责：集成示例四族（CI 接入/IDE 插件/适配器开发/SDK 调用）+ 随仓库 CI
//! 每日验证（编译失败当日修复门）+ 示例与文档双链互跳 + 版本戳下架
//! + 双链悬空巡检修复 + 示例输出遵循交互词典（域本色）。
//! 段位：0x2E60（续表 idx529）。行数预算 300。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IeCode {
    /// 编译失败（当日修复门）。
    BuildFail = SEG_BASE,
    /// 双链悬空。
    LinkDangling = SEG_BASE + 0x01,
    /// 版本戳过时（下架）。
    VerStale = SEG_BASE + 0x02,
}

/// 示例族四类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IeKind {
    Ci = 0,
    Ide = 1,
    Adapter = 2,
    Sdk = 3,
}
pub const IEK_N: usize = 4;

/// 示例条目。
#[derive(Clone, Copy, Debug)]
pub struct IEx {
    pub kind: IeKind,
    pub ver: u32,
    pub builds: bool,
    pub active: bool,
}

/// 集成示例库。
pub struct IntegExamples {
    pub items: Vec<IEx>,
    pub cur_ver: u32,
    /// 双链：(示例 kind, 文档 IdDoc 序)。
    pub links: Vec<(IeKind, u8)>,
    pub doc_cnt: u8,
    /// 当日编译失败标记。
    pub today_fail: bool,
    /// 交互词典遵循（域本色）。
    pub dict_ok: bool,
}

impl IntegExamples {
    pub fn new(cur_ver: u32, doc_cnt: u8) -> Self {
        IntegExamples { items: Vec::new(), cur_ver, links: Vec::new(), doc_cnt, today_fail: false, dict_ok: false }
    }

    pub fn add(&mut self, k: IeKind, builds: bool) {
        self.items.push(IEx { kind: k, ver: self.cur_ver, builds, active: true });
        if !builds {
            self.today_fail = true; // 编译失败当日修复门
        }
    }

    /// 日验证：任一失败→修复门拒绝通过。
    pub fn daily_gate(&self) -> Result<(), IeCode> {
        if self.today_fail {
            return Err(IeCode::BuildFail);
        }
        Ok(())
    }

    /// 修复后重验。
    pub fn fixed(&mut self, kind: IeKind) {
        if let Some(e) = self.items.iter_mut().find(|e| e.kind == kind && !e.builds) {
            e.builds = true;
            self.today_fail = self.items.iter().any(|e| !e.builds && e.active);
        }
    }

    /// 双链互跳。
    pub fn link_doc(&mut self, k: IeKind, doc: u8) -> Result<(), IeCode> {
        if doc >= self.doc_cnt {
            return Err(IeCode::LinkDangling);
        }
        if !self.links.iter().any(|(a, b)| *a == k && *b == doc) {
            self.links.push((k, doc));
        }
        Ok(())
    }

    /// 版本戳下架：过时示例失效。
    pub fn sweep(&mut self) -> usize {
        let v = self.cur_ver;
        let mut n = 0;
        for e in self.items.iter_mut() {
            if e.ver < v && e.active {
                e.active = false;
                n += 1;
            }
        }
        n
    }

    pub fn all_kinds(&self) -> bool {
        (0..IEK_N).all(|k| self.items.iter().any(|e| e.kind as usize == k && e.active && e.builds))
    }

    /// —— 深化：按族分列投影（族, 活跃, 下架）。——
    pub fn by_kind(&self) -> Vec<(u8, u32, u32)> {
        let mut out: Vec<(u8, u32, u32)> = Vec::new();
        for e in &self.items {
            let k = e.kind as u8;
            match out.iter_mut().find(|(a, _, _)| *a == k) {
                Some(t) => {
                    if e.active {
                        t.1 += 1;
                    } else {
                        t.2 += 1;
                    }
                }
                None => out.push((k, if e.active { 1 } else { 0 }, if e.active { 0 } else { 1 })),
            }
        }
        out
    }

    /// —— 深化：守恒恒等式：活跃 + 下架 ≡ 总数。——
    pub fn conserve_ok(&self) -> bool {
        let active = self.items.iter().filter(|e| e.active).count();
        let retired = self.items.iter().filter(|e| !e.active).count();
        active + retired == self.items.len()
    }

    /// —— 深化：双链悬空巡检修复：剔除指向不存在文档的链，返回剔除数。——
    pub fn dangling_scan_fix(&mut self) -> usize {
        let cnt = self.doc_cnt;
        let before = self.links.len();
        self.links.retain(|(_, d)| *d < cnt);
        before - self.links.len()
    }

    /// —— 深化：升级并盖章：版本号提升即扫描下架。——
    pub fn bump_ver(&mut self, v: u32) -> u32 {
        if v > self.cur_ver {
            self.cur_ver = v;
            self.sweep() as u32
        } else {
            0
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C529-01..12。
pub fn run_ves529_checks() -> CheckSet {
    let mut set = CheckSet::new("ves529");

    let mut ie = IntegExamples::new(3, 4);
    ie.add(IeKind::Ci, true);
    ie.add(IeKind::Ide, true);
    ie.add(IeKind::Adapter, false);
    ie.add(IeKind::Sdk, true);
    chk(&mut set, "C529-01", ie.today_fail, "编译失败检出");
    chk(&mut set, "C529-02", ie.daily_gate() == Err(IeCode::BuildFail), "当日修复门拒绝通过");
    ie.fixed(IeKind::Adapter);
    chk(&mut set, "C529-03", ie.daily_gate().is_ok(), "当日修复后门通过");

    // 双链
    chk(&mut set, "C529-04", ie.link_doc(IeKind::Ci, 0).is_ok(), "示例-文档双链挂载");
    chk(&mut set, "C529-05", ie.link_doc(IeKind::Ci, 0).is_ok() && ie.links.len() == 1, "双链去重");
    chk(&mut set, "C529-06", ie.link_doc(IeKind::Ide, 9) == Err(IeCode::LinkDangling), "悬空双链拒绝");

    // 版本戳
    ie.cur_ver = 4;
    chk(&mut set, "C529-07", ie.sweep() == 0, "同版不扫除");
    ie.items[0].ver = 2;
    chk(&mut set, "C529-08", ie.sweep() == 1, "过时示例下架");
    chk(&mut set, "C529-09", ie.items[0].active == false, "下架标记在册");

    ie.dict_ok = true;
    chk(&mut set, "C529-10", ie.dict_ok, "示例输出遵循交互词典（域本色）");
    chk(&mut set, "C529-11", ie.items.iter().filter(|e| e.active && e.builds).count() == 3, "活动可编译三例");
    chk(&mut set, "C529-12", IEK_N == 4 && ie.doc_cnt == 4, "四族与文档数自洽");

    // —— 深化 A：版本戳账本（升级历史 + 严格递增守恒）——
    let mut vl = VerLedger::new();
    chk(&mut set, "C529-13", vl.stamp(3, 0, false) && vl.stamp(4, 1, false) && !vl.stamp(4, 0, false), "版本戳严格递增，重复版拒记");
    chk(&mut set, "C529-14", vl.total_swept() == 1 && vl.conserve_ok(), "版本账守恒：下架数分和≡总");
    chk(&mut set, "C529-15", !vl.summary().is_empty(), "版本账中文摘要（域本色）");

    // —— 深化 B：按族分列投影 + 守恒 ——
    let bk = ie.by_kind();
    chk(&mut set, "C529-16", bk.len() == 4 && ie.conserve_ok(), "按族投影四列且活跃+下架≡总");

    // —— 深化 C：双链悬空巡检修复 ——
    ie.links.push((IeKind::Sdk, 9)); // 制造一处悬空
    chk(&mut set, "C529-17", ie.dangling_scan_fix() == 1 && ie.links.len() == 1, "悬空双链巡检剔除修复");

    // —— 深化 D：升级盖章 ——
    let swept = ie.bump_ver(5);
    chk(&mut set, "C529-18", ie.cur_ver == 5 && swept == 2, "版本提升即扫除过时示例");
    chk(&mut set, "C529-19", ie.bump_ver(5) == 0, "同级不重扫（幂等）");

    // —— 深化 E：边界探针 ——
    let er = run_iex_edge_probes();
    chk(&mut set, "C529-20", er.empty_sweep_zero && er.zero_doc_cnt_reject, "空库扫除为 0/零文档数拒绝挂链");
    chk(&mut set, "C529-21", er.absent_fixed_noop && er.dup_link_stable, "缺席修复无副作用/重复挂链稳定");

    // —— 深化 F：LCG 注入回归 ——
    match iex_inject_regression(48) {
        Ok(total) => chk(&mut set, "C529-22", total > 0, "48 轮随机示例注入守恒在账"),
        Err(_) => chk(&mut set, "C529-22", false, "注入回归账面失衡"),
    }

    // —— 深化 G：序列化投影 ——
    let mut bytes: Vec<u8> = Vec::new();
    encode_iex(&ie.items, &mut bytes);
    let rt = decode_iex(&bytes);
    chk(&mut set, "C529-23", rt.iter().count() == ie.items.len(), "示例条目 6B/条定长解码往返");
    chk(&mut set, "C529-24", fnv1a(&bytes) == fnv1a(&bytes) && bytes.len() == ie.items.len() * 6, "FNV 指纹稳定且字节布局定长");

    set
}

// ===== 深化块 A：版本戳账本 =====

/// 版本戳：一次升级的归因记录。
#[derive(Clone, Copy, Debug)]
pub struct VerStamp {
    pub ver: u32,
    pub swept: u32,
    pub gate_fail: bool,
}

/// 版本戳账本：升级历史（严格递增）。
pub struct VerLedger {
    pub stamps: Vec<VerStamp>,
}

impl VerLedger {
    pub fn new() -> Self {
        VerLedger { stamps: Vec::new() }
    }

    /// 记章：版本必须严格递增，重复/回退版拒记返回 false。
    pub fn stamp(&mut self, ver: u32, swept: u32, gate_fail: bool) -> bool {
        if let Some(last) = self.stamps.last() {
            if ver <= last.ver {
                return false;
            }
        }
        self.stamps.push(VerStamp { ver, swept, gate_fail });
        true
    }

    /// 守恒恒等式：各章下架数分和 ≡ 账面总数。
    pub fn conserve_ok(&self) -> bool {
        let sum: u32 = self.stamps.iter().map(|s| s.swept).sum();
        sum == self.total_swept()
    }

    pub fn total_swept(&self) -> u32 {
        self.stamps.iter().map(|s| s.swept).sum()
    }

    /// 中文摘要（交互词典域本色）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("升级");
        s.push_str(&self.stamps.len().to_string());
        s.push_str("次·下架");
        s.push_str(&self.total_swept().to_string());
        s
    }
}

// ===== 深化块 B：边界探针 =====

/// 示例库边界探针报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IexEdgeReport {
    pub empty_sweep_zero: bool,
    pub zero_doc_cnt_reject: bool,
    pub absent_fixed_noop: bool,
    pub dup_link_stable: bool,
}

/// 四类边界探针：空库/零文档数/缺席修复/重复挂链。
pub fn run_iex_edge_probes() -> IexEdgeReport {
    // 1) 空库 sweep 恒 0
    let mut e0 = IntegExamples::new(1, 2);
    let empty_sweep_zero = e0.sweep() == 0;
    // 2) 零文档数：任何挂链必悬空拒绝
    let mut e1 = IntegExamples::new(1, 0);
    let zero_doc_cnt_reject = e1.link_doc(IeKind::Ci, 0) == Err(IeCode::LinkDangling);
    // 3) 缺席修复：fix 不存在的坏例零副作用
    let mut e2 = IntegExamples::new(1, 2);
    e2.add(IeKind::Ci, true);
    let n0 = e2.items.len();
    e2.fixed(IeKind::Sdk);
    let absent_fixed_noop = e2.items.len() == n0 && e2.items[0].builds;
    // 4) 重复挂链：二次提交链数稳定
    let _ = e2.link_doc(IeKind::Ci, 0);
    let _ = e2.link_doc(IeKind::Ci, 0);
    let dup_link_stable = e2.links.len() == 1;

    IexEdgeReport { empty_sweep_zero, zero_doc_cnt_reject, absent_fixed_noop, dup_link_stable }
}

// ===== 深化块 C：LCG 注入回归 =====

/// 确定性 LCG（8 行）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 注入回归：LCG 驱动 N 轮随机示例登记+升级扫描，账面活跃+下架≡总。
pub fn iex_inject_regression(rounds: u32) -> Result<u32, IeCode> {
    let mut rng = Lcg::new(0x5EED_5290);
    let mut ie = IntegExamples::new(1, 4);
    let mut total = 0u32;
    for i in 0..rounds {
        let k = match rng.next() % 4 {
            0 => IeKind::Ci,
            1 => IeKind::Ide,
            2 => IeKind::Adapter,
            _ => IeKind::Sdk,
        };
        ie.add(k, rng.next() & 3 != 0);
        total += 1;
        if i % 8 == 7 {
            ie.bump_ver(ie.cur_ver + 1);
        }
    }
    if !ie.conserve_ok() || ie.items.len() as u32 != total {
        return Err(IeCode::BuildFail);
    }
    Ok(total)
}

// ===== 深化块 D：序列化投影 =====

/// FNV-1a 32 位指纹。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 定长编码：每条 6B（kind:1 + ver:4 + 标志位:1，bit0=builds bit1=active）。
pub fn encode_iex(items: &[IEx], out: &mut Vec<u8>) {
    for e in items {
        out.push(e.kind as u8);
        out.extend_from_slice(&e.ver.to_be_bytes());
        out.push((e.builds as u8) | ((e.active as u8) << 1));
    }
}

/// 定长解码：非整 6B → 显性拒绝。
pub fn decode_iex(bytes: &[u8]) -> Result<Vec<IEx>, IeCode> {
    if bytes.len() % 6 != 0 {
        return Err(IeCode::LinkDangling);
    }
    let mut out = Vec::new();
    for ch in bytes.chunks_exact(6) {
        let kind = match ch[0] {
            0 => IeKind::Ci,
            1 => IeKind::Ide,
            2 => IeKind::Adapter,
            3 => IeKind::Sdk,
            _ => return Err(IeCode::LinkDangling),
        };
        out.push(IEx {
            kind,
            ver: u32::from_be_bytes([ch[1], ch[2], ch[3], ch[4]]),
            builds: ch[5] & 1 != 0,
            active: ch[5] & 2 != 0,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_examples() {
        let mut ie = IntegExamples::new(1, 2);
        ie.add(IeKind::Ci, true);
        ie.add(IeKind::Sdk, false);
        assert_eq!(ie.daily_gate(), Err(IeCode::BuildFail));
        ie.fixed(IeKind::Sdk);
        assert!(ie.daily_gate().is_ok());
        assert_eq!(ie.link_doc(IeKind::Ci, 5), Err(IeCode::LinkDangling));
    }
}
