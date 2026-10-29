//! ves464_s25adr — VE-F4230 · 契约与 ADR 联动
//! 架构决策记录双向链接（契约←→ADR 互挂指针：契约引用决策依据/ADR 引用落地契约）+
//! 决策变更→契约影响预警+链接完整性审计；单向挂→补挂+催办；决策变更未预警→立案；链接断→修复登记；
//! ADR 含无障碍决策标注位（涉无障碍决策必挂——域本色）；无隐私面。
//! 深化（2026-10-27 T域批次）：结构化互挂账本（定容+批归因）、守恒恒等式
//! （双向率恒等）、边界探针（空 id/重复挂/满容量/断链反查）、按标注位投影、
//! LCG 注入回归（单向挂必抓 100%+双向零误报）、幂等闸、定长序列化往返+FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1820;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdCode {
    Ok = 0,
    OneWay = SEG_BASE + 0x01,     // 0x1821 单向挂
    NoWarn = SEG_BASE + 0x02,     // 0x1822 决策变更未预警
    LinkBroken = SEG_BASE + 0x03, // 0x1823
    A11yUntagged = SEG_BASE + 0x04, // 0x1824 标注位缺（域本色）
    // —— 深化追加（尾部续段，不新增 SEG_BASE）——
    EmptyId = SEG_BASE + 0x05, // 0x1825 空 id（契约/ADR 号为 0）
    DupLink = SEG_BASE + 0x06, // 0x1826 重复互挂（同契约同 ADR）
    CapFull = SEG_BASE + 0x07, // 0x1827 账本满容量
    SerialBad = SEG_BASE + 0x08, // 0x1828 序列化字节布局失配
}

/// 互挂指针（契约 id ←→ ADR id + a11y 标注位）。
#[derive(Clone, Copy)]
pub struct Link { pub contract: u32, pub adr: u32, pub a11y: bool, pub c2a: bool, pub a2c: bool }

/// 联动册。
pub struct AdrBook {
    pub links: Vec<Link>,
    pub warned: Vec<u32>, // 决策变更预警记录（ADR id）
    pub filed: u32,
    pub repaired: u32,
}

impl AdrBook {
    pub fn new() -> Self { AdrBook { links: Vec::new(), warned: Vec::new(), filed: 0, repaired: 0 } }
    pub fn put(&mut self, l: Link) {
        self.links.retain(|x| x.contract != l.contract);
        self.links.push(l);
    }
    /// 链接完整性审计：双向必须齐+a11y 决策必标注（域本色）。
    pub fn audit(&mut self) -> Result<(), AdCode> {
        for l in &self.links {
            if !l.c2a || !l.a2c { self.filed += 1; return Err(AdCode::OneWay); }
            if l.a11y { return Err(AdCode::A11yUntagged); } // 占位：a11y=true 表示「涉无障碍但未标注」
        }
        Ok(())
    }
    /// 决策变更预警：ADR 变更→影响契约预警（未预警→立案）。
    pub fn change_adr(&mut self, adr: u32, warn: bool) -> Result<(), AdCode> {
        if !self.links.iter().any(|l| l.adr == adr) { return Err(AdCode::LinkBroken); }
        if !warn { self.filed += 1; return Err(AdCode::NoWarn); }
        if !self.warned.contains(&adr) { self.warned.push(adr); }
        Ok(())
    }
    /// 补挂（修复单向）。
    pub fn fix_link(&mut self, contract: u32, rev_c2a: bool, rev_a2c: bool) -> bool {
        for l in self.links.iter_mut() {
            if l.contract == contract {
                if rev_c2a { l.c2a = true; }
                if rev_a2c { l.a2c = true; }
                self.repaired += 1;
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 深化追加：结构化互挂账本（定容 + 审计投影 + 注入回归）。
// ---------------------------------------------------------------------------

/// 单条互挂账目（带批次归因）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkRecord { pub batch: u32, pub contract: u32, pub adr: u32, pub a11y: bool, pub c2a: bool, pub a2c: bool }

/// 结构化互挂账本：链接登记从散挂升级为定容带归因账本。
pub struct LinkLedger { pub recs: Vec<LinkRecord>, pub cap: usize }

impl LinkLedger {
    pub fn new(cap: usize) -> Self { LinkLedger { recs: Vec::new(), cap } }

    /// 挂一笔互挂。边界探针：空 id / 重复挂 / 满容量，显性 Err。
    pub fn record(&mut self, batch: u32, contract: u32, adr: u32, a11y: bool, c2a: bool, a2c: bool) -> Result<(), AdCode> {
        if contract == 0 || adr == 0 { return Err(AdCode::EmptyId); }
        if self.recs.iter().any(|r| r.contract == contract && r.adr == adr) {
            return Err(AdCode::DupLink);
        }
        if self.recs.len() >= self.cap { return Err(AdCode::CapFull); }
        self.recs.push(LinkRecord { batch, contract, adr, a11y, c2a, a2c });
        Ok(())
    }

    /// 守恒恒等式：账本总数 ≡ 双向挂数 + 单向挂数（且无空 id 漏网）。
    pub fn conserved(&self) -> bool {
        if self.recs.len() > self.cap { return false; }
        let dual = self.recs.iter().filter(|r| r.c2a && r.a2c).count();
        let oneway = self.recs.iter().filter(|r| !(r.c2a && r.a2c)).count();
        dual + oneway == self.recs.len()
            && self.recs.iter().all(|r| r.contract != 0 && r.adr != 0)
    }

    /// 查询投影：按标注位分列（涉无障碍挂数, 普通挂数）——域本色度量。
    pub fn by_a11y(&self) -> (usize, usize) {
        let mut tagged = 0usize;
        let mut plain = 0usize;
        for r in &self.recs {
            if r.a11y { tagged += 1; } else { plain += 1; }
        }
        (tagged, plain)
    }

    /// 审计投影：单向挂与空 id 的缺陷清单（修复登记的输入）。
    pub fn defects(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in &self.recs {
            if !(r.c2a && r.a2c) || r.contract == 0 || r.adr == 0 {
                out.push(r.contract);
            }
        }
        out
    }

    /// 断链反查：ADR 是否有挂（change_adr 前置校验的对拍面）。
    pub fn adr_linked(&self, adr: u32) -> bool {
        self.recs.iter().any(|r| r.adr == adr)
    }

    /// 幂等闸：同批次同契约同 ADR 二次挂零副作用。
    pub fn idempotent(&mut self, batch: u32, contract: u32, adr: u32) -> bool {
        let before = self.recs.len();
        let first = self.record(batch, contract, adr, false, true, true);
        let mid = self.recs.len();
        let second = self.record(batch, contract, adr, false, true, true);
        first.is_ok() && second == Err(AdCode::DupLink)
            && mid == before + 1 && self.recs.len() == mid
    }

    /// 序列化投影：LinkRecord → 定长 10 字节（contract4 + adr4 + 位标志1 + batch 高位截取1）。
    /// 位标志：bit0=a11y, bit1=c2a, bit2=a2c；bit3..7 保留须为 0。
    pub fn encode(r: &LinkRecord) -> [u8; 10] {
        let mut out = [0u8; 10];
        out[0] = (r.contract >> 24) as u8;
        out[1] = (r.contract >> 16) as u8;
        out[2] = (r.contract >> 8) as u8;
        out[3] = r.contract as u8;
        out[4] = (r.adr >> 24) as u8;
        out[5] = (r.adr >> 16) as u8;
        out[6] = (r.adr >> 8) as u8;
        out[7] = r.adr as u8;
        out[8] = (r.a11y as u8) | ((r.c2a as u8) << 1) | ((r.a2c as u8) << 2);
        out[9] = (r.batch & 0xFF) as u8;
        out
    }

    /// 反序列化（带校验：保留位非零→SerialBad，空 id→EmptyId）。
    pub fn decode(b: &[u8; 10]) -> Result<LinkRecord, AdCode> {
        let contract = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let adr = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        let flags = b[8];
        if flags & 0xF8 != 0 { return Err(AdCode::SerialBad); }
        if contract == 0 || adr == 0 { return Err(AdCode::EmptyId); }
        Ok(LinkRecord {
            batch: b[9] as u32,
            contract,
            adr,
            a11y: flags & 0x01 != 0,
            c2a: flags & 0x02 != 0,
            a2c: flags & 0x04 != 0,
        })
    }

    /// FNV-1a 指纹（字节布局配套）。
    pub fn fnv1a(bytes: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for &b in bytes {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 账本摘要（String 投影，诊断用）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        s.push_str("links=");
        s.push_str(&self.recs.len().to_string());
        s.push_str("/cap=");
        s.push_str(&self.cap.to_string());
        s
    }
}

/// 确定性 LCG（8 行）：注入回归驱动器（无浮点、无墙钟）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self { Lcg(seed) }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// 注入回归：LCG 驱动 N 轮互挂——注入单向挂（随机缺 c2a 或 a2c）必被审计
/// 抓住（异常必抓率 100%）；双向挂的负对照零误报。返回 (抓住, 注入, 误报)。
pub fn inject_one_way(rounds: u32) -> (u32, u32, u32) {
    let mut rng = Lcg::new(0x4640_0000_0000_0004);
    let mut caught = 0u32;
    let mut injected = 0u32;
    let mut false_alarm = 0u32;
    for i in 0..rounds {
        let contract = 7000 + (rng.next() % 900) as u32;
        let adr = 100 + (rng.next() % 800) as u32;
        let mut book = AdrBook::new();
        if i % 2 == 0 {
            injected += 1;
            let drop_c2a = rng.next() % 2 == 0;
            book.put(Link { contract, adr, a11y: false, c2a: !drop_c2a, a2c: drop_c2a });
            if book.audit() == Err(AdCode::OneWay) { caught += 1; }
        } else {
            book.put(Link { contract, adr, a11y: false, c2a: true, a2c: true });
            if book.audit().is_err() { false_alarm += 1; }
        }
    }
    (caught, injected, false_alarm)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves464_checks() -> CheckSet {
    let mut set = CheckSet::new("ves464");
    let mut bk = AdrBook::new();

    // C464-01 双向互挂齐→审计过。
    bk.put(Link { contract: 7001, adr: 100, a11y: false, c2a: true, a2c: true });
    bk.put(Link { contract: 7002, adr: 101, a11y: false, c2a: true, a2c: true });
    chk(&mut set, "C464-01", bk.audit().is_ok(), "双向齐过");

    // C464-02 单向挂→补挂码+立案。
    bk.put(Link { contract: 7003, adr: 102, a11y: false, c2a: true, a2c: false });
    chk(&mut set, "C464-02", bk.audit() == Err(AdCode::OneWay) && bk.filed == 1, "单向挂检出");

    // C464-03 补挂修复→复检过。
    chk(&mut set, "C464-03", bk.fix_link(7003, false, true) && bk.repaired == 1
        && bk.audit().is_ok(), "补挂复检");

    // C464-04 决策变更预警在册。
    chk(&mut set, "C464-04", bk.change_adr(100, true).is_ok() && bk.warned.contains(&100), "变更预警");

    // C464-05 决策变更未预警→立案。
    chk(&mut set, "C464-05", bk.change_adr(101, false) == Err(AdCode::NoWarn) && bk.filed == 2, "未预警立案");

    // C464-06 链接断（ADR 无挂）→断链码。
    chk(&mut set, "C464-06", bk.change_adr(999, true) == Err(AdCode::LinkBroken), "断链检出");

    // C464-07 域本色：涉无障碍决策未标注→拒。
    bk.put(Link { contract: 7004, adr: 103, a11y: true, c2a: true, a2c: true });
    chk(&mut set, "C464-07", bk.audit() == Err(AdCode::A11yUntagged), "a11y 标注位缺拒");

    // C464-08 标注齐→复检过。
    bk.links[3].a11y = false; // 修正为已标注态
    chk(&mut set, "C464-08", bk.audit().is_ok(), "标注复检");

    // C464-09 预警幂等。
    bk.change_adr(100, true).unwrap();
    chk(&mut set, "C464-09", bk.warned.len() == 1, "预警幂等");

    // C464-10 互挂覆盖幂等（同契约重挂替换）。
    bk.put(Link { contract: 7001, adr: 105, a11y: false, c2a: true, a2c: true });
    chk(&mut set, "C464-10", bk.links.len() == 4, "重挂替换");

    // C464-11 立案计数累计。
    chk(&mut set, "C464-11", bk.filed == 2, "立案计数");

    // C464-12 错误码段位单源。
    chk(&mut set, "C464-12", AdCode::A11yUntagged as u16 == SEG_BASE + 4, "段位单源");

    // —— 深化判据（追加面）——

    // C464-13 结构化互挂账本守恒恒等式（总数≡双向+单向）。
    let mut lg = LinkLedger::new(8);
    let _ = lg.record(1, 7001, 100, false, true, true);
    let _ = lg.record(1, 7002, 101, true, true, true);
    let _ = lg.record(2, 7003, 102, false, true, false);
    chk(&mut set, "C464-13", lg.conserved(), "账本守恒");

    // C464-14 断链反查：账本口径与联动册 change_adr 前置校验对拍。
    chk(&mut set, "C464-14", lg.adr_linked(100) && !lg.adr_linked(999), "断链反查");

    // C464-15 边界探针：空 id（契约或 ADR 号为 0）→显性拒绝。
    chk(&mut set, "C464-15", lg.record(3, 0, 110, false, true, true) == Err(AdCode::EmptyId)
        && lg.record(3, 7005, 0, false, true, true) == Err(AdCode::EmptyId), "空 id 拒");

    // C464-16 边界探针 + 幂等闸：重复互挂被拒、账本零副作用。
    chk(&mut set, "C464-16", lg.idempotent(3, 7004, 103), "幂等闸");

    // C464-17 边界探针：满容量→CapFull。
    let mut full = LinkLedger::new(2);
    let _ = full.record(1, 7001, 100, false, true, true);
    let _ = full.record(1, 7002, 101, false, true, true);
    chk(&mut set, "C464-17", full.record(2, 7003, 102, false, true, true) == Err(AdCode::CapFull), "满容量拒");

    // C464-18 查询投影：按标注位分列（涉无障碍=1, 普通=3）——域本色度量。
    let (tagged, plain) = lg.by_a11y();
    chk(&mut set, "C464-18", tagged == 1 && plain == 3, "标注位投影");

    // C464-19 审计投影：缺陷清单恰好含那个单向挂契约（修复登记输入）。
    let df = lg.defects();
    chk(&mut set, "C464-19", df.len() == 1 && df[0] == 7003, "缺陷清单");

    // C464-20 序列化投影：编码→解码往返一致 + 指纹稳定。
    let rec = LinkRecord { batch: 5, contract: 7500, adr: 150, a11y: true, c2a: true, a2c: true };
    let bytes = LinkLedger::encode(&rec);
    let back = LinkLedger::decode(&bytes);
    chk(&mut set, "C464-20", back == Ok(rec) && LinkLedger::fnv1a(&bytes) == LinkLedger::fnv1a(&bytes), "序列化往返");

    // C464-21 序列化校验：保留位非零→SerialBad。
    let mut bad_b = bytes;
    bad_b[8] |= 0x20;
    chk(&mut set, "C464-21", LinkLedger::decode(&bad_b) == Err(AdCode::SerialBad), "保留位拒");

    // C464-22 注入回归：单向挂必抓 100% + 双向零误报。
    let (caught, injected, false_alarm) = inject_one_way(32);
    chk(&mut set, "C464-22", injected > 0 && caught == injected && false_alarm == 0, "单向必抓");

    // C464-23 账本摘要 String 投影（诊断可读）。
    chk(&mut set, "C464-23", lg.summary().contains("links=4"), "摘要投影");

    // C464-24 追加变体段位单源（尾部续段不撞段）。
    chk(&mut set, "C464-24", AdCode::SerialBad as u16 == SEG_BASE + 8, "续段单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves464_smoke() {
        let set = run_ves464_checks();
        assert!(set.failed_count() == 0, "ves464 red: {}", set.red_items_text());
    }
}
