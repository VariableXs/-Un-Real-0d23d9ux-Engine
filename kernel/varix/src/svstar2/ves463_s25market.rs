//! ves463_s25market — VE-F4229 · 契约市场与复用
//! 契约复用库（通用契约模板：分页/排序/过滤/上传四类起步）+模板实例化流程（实例化→定制→六检验证→注册）+
//! 复用率度量；模板缺陷→模板即缺陷立案；实例化失真→六检拦截；度量失真→口径对拍；
//! 模板内建无障碍判据骨架（实例化不可删除——域本色）；无隐私面。
//! 深化（2026-10-27 T域批次）：结构化复用账本（批次+类归因）、守恒恒等式、
//! 边界探针（重复实例/越界实例号/满容量/空库投影）、按类查询投影与分位、
//! LCG 注入回归（失真必抓 100%+好源零误报）、幂等闸、定长序列化往返+FNV 指纹。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1800;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MkCode {
    Ok = 0,
    TemplateDefect = SEG_BASE + 0x01, // 0x1801
    InstanceDistort = SEG_BASE + 0x02, // 0x1802
    MetricSkew = SEG_BASE + 0x03,     // 0x1803
    // —— 深化追加（尾部续段，不新增 SEG_BASE）——
    DupInstance = SEG_BASE + 0x04, // 0x1804 重复实例号
    CapFull = SEG_BASE + 0x05,     // 0x1805 账本满容量
    SerialBad = SEG_BASE + 0x06,   // 0x1806 序列化字节布局失配
}

/// 四类模板。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemplateKind { Pagination = 0, Sort = 1, Filter = 2, Upload = 3 }

impl TemplateKind {
    pub const ALL: [TemplateKind; 4] = [TemplateKind::Pagination, TemplateKind::Sort, TemplateKind::Filter, TemplateKind::Upload];
}

/// 模板（内建 a11y 判据骨架——域本色）。
#[derive(Clone, Copy)]
pub struct Template { pub kind: TemplateKind, pub a11y_skeleton: bool, pub body: u32 }

/// 实例化器。
pub struct MarketBook {
    pub templates: Vec<Template>,
    pub instances: Vec<(TemplateKind, u32)>, // (类, 实例 id)
    pub reused: u32,
    pub filed: u32,
}

impl MarketBook {
    pub fn new() -> Self { MarketBook { templates: Vec::new(), instances: Vec::new(), reused: 0, filed: 0 } }
    pub fn put_template(&mut self, k: TemplateKind, a11y: bool, body: u32) {
        self.templates.retain(|t| t.kind != k);
        self.templates.push(Template { kind: k, a11y_skeleton: a11y, body });
    }
    /// 实例化：模板齐+a11y 骨架在→实例；骨架缺→模板即缺陷立案。
    pub fn instantiate(&mut self, k: TemplateKind, custom: u32) -> Result<u32, MkCode> {
        let t = match self.templates.iter().find(|t| t.kind == k) {
            Some(t) => t.clone(), None => { self.filed += 1; return Err(MkCode::TemplateDefect); }
        };
        if !t.a11y_skeleton { self.filed += 1; return Err(MkCode::TemplateDefect); }
        // 实例化失真：定制体丢失内建骨架（custom 尾号 9 代）→六检拦截。
        if custom % 10 == 9 { return Err(MkCode::InstanceDistort); }
        let id = 8000 + self.instances.len() as u32;
        self.instances.push((k, id));
        self.reused += 1;
        Ok(id)
    }
    /// 复用率度量：实例数/模板数（口径对拍）。
    pub fn reuse_rate_bp(&self) -> Option<u32> {
        if self.templates.is_empty() { return None; }
        Some(self.instances.len() as u32 * 10_000 / self.templates.len() as u32)
    }
}

// ---------------------------------------------------------------------------
// 深化追加：结构化复用账本（批次 + 类归因 + 定制分位）。
// ---------------------------------------------------------------------------

/// 实例号合法区间（注册中心登记段——深化面单源）。
pub const IID_MIN: u32 = 8000;
pub const IID_MAX: u32 = 8999;

/// 单条复用账目：批次 + 模板类 + 实例号 + 定制体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReuseRecord { pub batch: u32, pub kind: TemplateKind, pub iid: u32, pub custom: u32 }

/// 结构化复用账本：把 reused 计数升级为带批次/类归因的定容记录。
pub struct ReuseLedger { pub recs: Vec<ReuseRecord>, pub cap: usize }

impl ReuseLedger {
    pub fn new(cap: usize) -> Self { ReuseLedger { recs: Vec::new(), cap } }

    /// 记一笔复用。边界探针：越界实例号 / 重复实例号 / 满容量，显性 Err。
    pub fn record(&mut self, batch: u32, kind: TemplateKind, iid: u32, custom: u32) -> Result<(), MkCode> {
        if iid < IID_MIN || iid > IID_MAX { return Err(MkCode::InstanceDistort); }
        if self.recs.iter().any(|r| r.iid == iid) { return Err(MkCode::DupInstance); }
        if self.recs.len() >= self.cap { return Err(MkCode::CapFull); }
        self.recs.push(ReuseRecord { batch, kind, iid, custom });
        Ok(())
    }

    /// 守恒恒等式：账本总数 ≡ 四类实例计数之和（且实例号两两不重）。
    pub fn conserved(&self) -> bool {
        if self.recs.len() > self.cap { return false; }
        let by_kind = self.by_kind();
        by_kind[0] + by_kind[1] + by_kind[2] + by_kind[3] == self.recs.len()
            && self.ids_unique()
    }

    /// 实例号唯一性（账本内自洽——重复即度量失真）。
    pub fn ids_unique(&self) -> bool {
        for i in 0..self.recs.len() {
            for j in (i + 1)..self.recs.len() {
                if self.recs[i].iid == self.recs[j].iid { return false; }
            }
        }
        true
    }

    /// 查询投影：按模板类分列（[Pagination, Sort, Filter, Upload]）。
    pub fn by_kind(&self) -> [usize; 4] {
        let mut out = [0usize; 4];
        for r in &self.recs {
            let k = match r.kind {
                TemplateKind::Pagination => 0,
                TemplateKind::Sort => 1,
                TemplateKind::Filter => 2,
                TemplateKind::Upload => 3,
            };
            out[k] += 1;
        }
        out
    }

    /// 分位投影：定制体 P50（排序取点，禁浮点；空账本→显性 Err）。
    pub fn p50_custom(&self) -> Result<u32, MkCode> {
        if self.recs.is_empty() { return Err(MkCode::MetricSkew); }
        let mut cs: Vec<u32> = self.recs.iter().map(|r| r.custom).collect();
        cs.sort_unstable();
        Ok(cs[(cs.len() - 1) / 2])
    }

    /// 口径对拍：账本分类合计 ≡ 复用率分母可对上（度量不歪）。
    pub fn metric_consistent_with(&self, book_instances: usize) -> bool {
        let total: usize = self.by_kind().iter().sum();
        total == book_instances
    }

    /// 幂等闸：同实例号二次登记零副作用（重记被拒、账本不变）。
    pub fn idempotent(&mut self, batch: u32, kind: TemplateKind, iid: u32, custom: u32) -> bool {
        let before = self.recs.len();
        let first = self.record(batch, kind, iid, custom);
        let mid = self.recs.len();
        let second = self.record(batch, kind, iid, custom);
        first.is_ok() && second == Err(MkCode::DupInstance)
            && mid == before + 1 && self.recs.len() == mid
    }

    /// 序列化投影：ReuseRecord → 定长 7 字节（大端 batch4 + kind1 + iid2 低位）。
    /// iid 段 8000..8999 收在 u16 内（iid - IID_MIN）。
    pub fn encode(r: &ReuseRecord) -> [u8; 7] {
        let mut out = [0u8; 7];
        out[0] = (r.batch >> 24) as u8;
        out[1] = (r.batch >> 16) as u8;
        out[2] = (r.batch >> 8) as u8;
        out[3] = r.batch as u8;
        out[4] = r.kind as u8;
        let off = (r.iid - IID_MIN) as u16;
        out[5] = (off >> 8) as u8;
        out[6] = off as u8;
        out
    }

    /// 反序列化（带校验：类非法→SerialBad，实例号越界→InstanceDistort）。
    pub fn decode(b: &[u8; 7]) -> Result<ReuseRecord, MkCode> {
        let batch = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let kind = match b[4] {
            0 => TemplateKind::Pagination,
            1 => TemplateKind::Sort,
            2 => TemplateKind::Filter,
            3 => TemplateKind::Upload,
            _ => return Err(MkCode::SerialBad),
        };
        let off = u16::from_be_bytes([b[5], b[6]]) as u32;
        let iid = IID_MIN + off;
        if iid > IID_MAX { return Err(MkCode::InstanceDistort); }
        Ok(ReuseRecord { batch, kind, iid, custom: 0 })
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
        s.push_str("reuse=");
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

/// 注入回归：LCG 驱动 N 轮实例化——注入失真定制体（尾号 9）必被六检拦截
/// （异常必抓率 100%）；完好定制的负对照零误报。返回 (抓住, 注入, 误报)。
pub fn inject_distort(rounds: u32) -> (u32, u32, u32) {
    let mut rng = Lcg::new(0x4630_0000_0000_0003);
    let mut caught = 0u32;
    let mut injected = 0u32;
    let mut false_alarm = 0u32;
    for i in 0..rounds {
        let mut bk = MarketBook::new();
        for k in TemplateKind::ALL { bk.put_template(k, true, 100); }
        let kind = TemplateKind::ALL[(rng.next() % 4) as usize];
        if i % 2 == 0 {
            injected += 1;
            let distort_custom = 100 + (rng.next() % 90) as u32 * 10 + 9; // 尾号 9
            if bk.instantiate(kind, distort_custom) == Err(MkCode::InstanceDistort) { caught += 1; }
        } else if bk.instantiate(kind, 100).is_err() {
            false_alarm += 1;
        }
    }
    (caught, injected, false_alarm)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves463_checks() -> CheckSet {
    let mut set = CheckSet::new("ves463");
    let mut bk = MarketBook::new();

    // C463-01 四类模板入库（内建 a11y 骨架——域本色）。
    for k in TemplateKind::ALL { bk.put_template(k, true, 100); }
    chk(&mut set, "C463-01", bk.templates.len() == 4
        && bk.templates.iter().all(|t| t.a11y_skeleton), "四类模板+骨架");

    // C463-02 实例化→定制→注册（复用计数）。
    let id1 = bk.instantiate(TemplateKind::Pagination, 100).unwrap();
    let id2 = bk.instantiate(TemplateKind::Sort, 200).unwrap();
    chk(&mut set, "C463-02", id1 == 8000 && id2 == 8001 && bk.reused == 2, "实例化注册");

    // C463-03 实例化失真→六检拦截。
    chk(&mut set, "C463-03", bk.instantiate(TemplateKind::Filter, 119) == Err(MkCode::InstanceDistort), "失真拦截");

    // C463-04 模板缺骨架→模板即缺陷立案。
    bk.put_template(TemplateKind::Upload, false, 100);
    chk(&mut set, "C463-04", bk.instantiate(TemplateKind::Upload, 100) == Err(MkCode::TemplateDefect)
        && bk.filed == 1, "缺陷立案");

    // C463-05 修复骨架→可实例化。
    bk.put_template(TemplateKind::Upload, true, 100);
    chk(&mut set, "C463-05", bk.instantiate(TemplateKind::Upload, 100).is_ok(), "修复可用");

    // C463-06 复用率度量。
    let rate = bk.reuse_rate_bp().unwrap();
    chk(&mut set, "C463-06", rate == 10_000, "复用率 bp");

    // C463-07 度量失真（零模板）→空态。
    let bk2 = MarketBook::new();
    chk(&mut set, "C463-07", bk2.reuse_rate_bp().is_none(), "零模板空态");

    // C463-08 四类单源断言。
    chk(&mut set, "C463-08", TemplateKind::ALL.len() == 4
        && TemplateKind::ALL[3] == TemplateKind::Upload, "四类单源");

    // C463-09 实例 id 递增唯一。
    chk(&mut set, "C463-09", bk.instances.iter().map(|(_, i)| *i).collect::<Vec<u32>>()
        == vec![8000, 8001, 8002], "实例唯一");

    // C463-10 立案计数累计。
    chk(&mut set, "C463-10", bk.filed == 1, "立案计数");

    // C463-11 模板覆盖幂等。
    bk.put_template(TemplateKind::Sort, true, 100);
    chk(&mut set, "C463-11", bk.templates.len() == 4, "模板幂等");

    // C463-12 错误码段位单源。
    chk(&mut set, "C463-12", MkCode::InstanceDistort as u16 == SEG_BASE + 2, "段位单源");

    // —— 深化判据（追加面）——

    // C463-13 结构化复用账本守恒恒等式（总数≡四类之和+实例号唯一）。
    let mut lg = ReuseLedger::new(8);
    let _ = lg.record(1, TemplateKind::Pagination, 8000, 120);
    let _ = lg.record(1, TemplateKind::Sort, 8001, 200);
    let _ = lg.record(2, TemplateKind::Filter, 8002, 90);
    chk(&mut set, "C463-13", lg.conserved(), "账本守恒");

    // C463-14 口径对拍：账本合计 ≡ 市场册实例数（度量不歪）。
    chk(&mut set, "C463-14", lg.metric_consistent_with(bk.instances.len()), "口径对拍");

    // C463-15 边界探针：越界实例号（低于登记段）→失真码。
    chk(&mut set, "C463-15", lg.record(3, TemplateKind::Sort, 7999, 1) == Err(MkCode::InstanceDistort), "越界实例拒");

    // C463-16 边界探针 + 幂等闸：重复实例号被拒、账本零副作用。
    chk(&mut set, "C463-16", lg.idempotent(3, TemplateKind::Upload, 8003, 60), "幂等闸");

    // C463-17 边界探针：满容量→CapFull。
    let mut full = ReuseLedger::new(2);
    let _ = full.record(1, TemplateKind::Pagination, 8000, 1);
    let _ = full.record(1, TemplateKind::Sort, 8001, 1);
    chk(&mut set, "C463-17", full.record(2, TemplateKind::Filter, 8002, 1) == Err(MkCode::CapFull), "满容量拒");

    // C463-18 查询投影：按类分列（Pagination=1, Sort=1, Filter=1, Upload=1）。
    let k = lg.by_kind();
    chk(&mut set, "C463-18", k[0] == 1 && k[1] == 1 && k[2] == 1 && k[3] == 1, "按类投影");

    // C463-19 分位投影：定制体 P50（90,120,200,60 → 90）。
    chk(&mut set, "C463-19", lg.p50_custom() == Ok(90), "P50 分位");

    // C463-20 边界探针：空账本分位→显性 Err（度量失真同源）。
    let empty = ReuseLedger::new(4);
    chk(&mut set, "C463-20", empty.p50_custom() == Err(MkCode::MetricSkew), "空账本拒");

    // C463-21 序列化投影：编码→解码往返一致 + 指纹稳定。
    let rec = ReuseRecord { batch: 7, kind: TemplateKind::Upload, iid: 8500, custom: 42 };
    let bytes = ReuseLedger::encode(&rec);
    let back = ReuseLedger::decode(&bytes);
    chk(&mut set, "C463-21", back == Ok(rec) && ReuseLedger::fnv1a(&bytes) == ReuseLedger::fnv1a(&bytes), "序列化往返");

    // C463-22 序列化校验：模板类非法→SerialBad。
    let mut bad_b = bytes;
    bad_b[4] = 9;
    chk(&mut set, "C463-22", ReuseLedger::decode(&bad_b) == Err(MkCode::SerialBad), "坏类拒");

    // C463-23 注入回归：失真必抓 100% + 好源零误报。
    let (caught, injected, false_alarm) = inject_distort(32);
    chk(&mut set, "C463-23", injected > 0 && caught == injected && false_alarm == 0, "失真必抓");

    // C463-24 账本摘要 String 投影（诊断可读）。
    chk(&mut set, "C463-24", lg.summary().contains("reuse=4"), "摘要投影");

    // C463-25 追加变体段位单源（尾部续段不撞段）。
    chk(&mut set, "C463-25", MkCode::SerialBad as u16 == SEG_BASE + 6, "续段单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves463_smoke() {
        let set = run_ves463_checks();
        assert!(set.failed_count() == 0, "ves463 red: {}", set.red_items_text());
    }
}
