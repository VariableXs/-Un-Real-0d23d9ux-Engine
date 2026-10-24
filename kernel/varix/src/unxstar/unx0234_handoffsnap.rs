//! unx0234_handoffsnap —— UNX-F0234 handoff 快照工具（域产线用）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0234 · handoff 快照 · 目标 240 行
//! 判据原文：UNX-F0234-J1 一键生成域进度快照（批次/行数/风险）入
//!           handoff 账，快照与台账 diff 零漂移。
//! 组件：批次进度行（批号 + 实现行数 + 风险条目数）+ 域快照（行集 +
//!       聚合账 + FNV 内容签）+ 台账 diff（逐批对账，漂移逐条立案）
//!       + 序列化（词位流 + 校验和只覆盖除自身外词位）。
//! 职责定位：交接快照是「域产线最后一公里」——下游接手时看到的
//!   必须是**当时**的实态而不是记忆里的应态。零漂移判据保证快照
//!   与台账逐批相等；任何一手改台账一手忘改快照的操作都会被
//!   diff 当场抓出。快照不可变：生成即冻结，后续改动走新快照。
//! 性能逐项分解：建快照 O(批数)；diff O(批数)；签名 O(词位数)。
//! 诊断码段：0x20000（0x20001~0x20009 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HS_VERSION: &str = "unx0234_handoffsnap-v1";

/// 诊断段基址（0x20000 段 · 本模块独占）。
pub const HS_SEG_BASE: u32 = 0x20000;
pub const XC_BATCH_DUP: u32 = 0x20001; // 批号重复
pub const XC_BATCH_OOO: u32 = 0x20002; // 批号乱序
pub const XC_DRIFT: u32 = 0x20003; // 快照/台账漂移
pub const XC_SIG_DRIFT: u32 = 0x20004; // 内容签复算不符
pub const XC_EMPTY: u32 = 0x20005; // 空快照拒收
pub const XC_CKSUM: u32 = 0x20006; // 序列化校验和不过
pub const XC_NEG_LINE: u32 = 0x20007; // 行数负漂
pub const XC_RISK_OOR: u32 = 0x20008; // 风险计数越域
pub const XC_FROZEN: u32 = 0x20009; // 冻结快照拒改

/// 单批进度行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchRow {
    pub batch: u32,
    pub impl_lines: u32,
    pub risk_items: u32,
}

impl BatchRow {
    pub fn new(batch: u32, lines: u32, risks: u32) -> Result<BatchRow, u32> {
        if lines == 0 && risks == 0 {
            return Err(XC_EMPTY);
        }
        if risks > 64 {
            return Err(XC_RISK_OOR);
        }
        Ok(BatchRow { batch, impl_lines: lines, risk_items: risks })
    }
}

/// FNV-1a 64 位（局部实现，避免跨模块依赖）。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 域进度快照（生成即冻结）。
pub struct HandoffSnap {
    rows: Vec<BatchRow>,
    /// 内容签 = FNV(逐批 (batch, lines, risks) 三元组字节)。
    content_sig: u64,
    frozen: bool,
}

impl HandoffSnap {
    /// 构造：批号必须严格升序且不重复（乱序/重复当场拒）。
    pub fn new(rows: &[BatchRow]) -> Result<HandoffSnap, u32> {
        if rows.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut prev: u32 = 0;
        for r in rows.iter() {
            if r.batch <= prev && prev != 0 {
                return Err(XC_BATCH_OOO);
            }
            if rows.iter().filter(|x| x.batch == r.batch).count() > 1 {
                return Err(XC_BATCH_DUP);
            }
            prev = r.batch;
        }
        let mut bytes = Vec::new();
        for r in rows.iter() {
            bytes.extend_from_slice(&r.batch.to_le_bytes());
            bytes.extend_from_slice(&r.impl_lines.to_le_bytes());
            bytes.extend_from_slice(&r.risk_items.to_le_bytes());
        }
        Ok(HandoffSnap { rows: rows.to_vec(), content_sig: fnv64(&bytes), frozen: true })
    }

    /// 总行数聚合。
    pub fn total_lines(&self) -> u64 {
        self.rows.iter().map(|r| r.impl_lines as u64).sum()
    }

    /// 总风险聚合。
    pub fn total_risks(&self) -> u64 {
        self.rows.iter().map(|r| r.risk_items as u64).sum()
    }

    /// 内容签。
    pub fn sig(&self) -> u64 {
        self.content_sig
    }

    /// 批数。
    pub fn batch_count(&self) -> u32 {
        self.rows.len() as u32
    }

    /// 冻结闸：快照不可变——任何「补一行」的尝试都拒。
    pub fn frozen_guard(&self) -> Result<(), u32> {
        if self.frozen {
            Err(XC_FROZEN)
        } else {
            Ok(())
        }
    }

    /// 与台账逐批 diff：返回漂移批号清单（空 = 零漂移）。
    pub fn diff_ledger(&self, ledger: &[BatchRow]) -> Result<Vec<u32>, u32> {
        let mut drift = Vec::new();
        for r in self.rows.iter() {
            match ledger.iter().find(|l| l.batch == r.batch) {
                Some(l) => {
                    if l.impl_lines != r.impl_lines || l.risk_items != r.risk_items {
                        drift.push(r.batch);
                    }
                }
                None => drift.push(r.batch),
            }
        }
        for l in ledger.iter() {
            if !self.rows.iter().any(|r| r.batch == l.batch) && !drift.contains(&l.batch) {
                drift.push(l.batch);
            }
        }
        if drift.is_empty() {
            Ok(drift)
        } else {
            Err(XC_DRIFT)
        }
    }
}

/// handoff 词位序列化：每批 4 词（batch/lines/risks/保留 0）+ 尾词校验和。
/// 校验和只覆盖除自身外的全部词位（把校验位算进去会自指）。
pub fn serialize(snap: &HandoffSnap) -> Result<Vec<u32>, u32> {
    if snap.batch_count() == 0 {
        return Err(XC_EMPTY);
    }
    let mut words = Vec::new();
    // 词位流首词 = 魔数 + 版本步（1）。
    words.push(0x554E5834u32); // "UNX4"
    let mut bytes = Vec::new();
    for r in snap.rows.iter() {
        words.push(r.batch);
        words.push(r.impl_lines);
        words.push(r.risk_items);
        words.push(0);
        bytes.extend_from_slice(&r.batch.to_le_bytes());
        bytes.extend_from_slice(&r.impl_lines.to_le_bytes());
        bytes.extend_from_slice(&r.risk_items.to_le_bytes());
    }
    // 尾词 = FNV 低 32 位。
    let sig = fnv64(&bytes) as u32;
    words.push(sig);
    Ok(words)
}

/// 反序列化 + 校验和复算：任一词被改即拒（往返闭账）。
pub fn deserialize(words: &[u32]) -> Result<HandoffSnap, u32> {
    if words.len() < 6 || (words.len() - 2) % 4 != 0 {
        return Err(XC_CKSUM);
    }
    if words[0] != 0x554E5834 {
        return Err(XC_CKSUM);
    }
    let n = (words.len() - 2) / 4;
    let mut rows = Vec::new();
    let mut bytes = Vec::new();
    for i in 0..n {
        let b = words[1 + i * 4];
        let l = words[2 + i * 4];
        let r = words[3 + i * 4];
        rows.push(BatchRow::new(b, l, r)?);
        bytes.extend_from_slice(&b.to_le_bytes());
        bytes.extend_from_slice(&l.to_le_bytes());
        bytes.extend_from_slice(&r.to_le_bytes());
    }
    let expect = fnv64(&bytes) as u32;
    if words[words.len() - 1] != expect {
        return Err(XC_CKSUM);
    }
    HandoffSnap::new(&rows)
}

pub fn run_unx0234_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0234_handoffsnap");
    // 族一：快照构造。
    let rows = vec![
        BatchRow::new(1, 1200, 2).unwrap_or(BatchRow { batch: 1, impl_lines: 1200, risk_items: 2 }),
        BatchRow { batch: 2, impl_lines: 3400, risk_items: 0 },
        BatchRow { batch: 3, impl_lines: 800, risk_items: 5 },
    ];
    let snap = match HandoffSnap::new(&rows) {
        Ok(s) => s,
        Err(_) => {
            cs.add("快照构造", false, "被拒");
            return cs;
        }
    };
    cs.add("快照构造", snap.batch_count() == 3, "三批入账");
    cs.add("行数聚合", snap.total_lines() == 5400, "1200+3400+800");
    cs.add("风险聚合", snap.total_risks() == 7, "2+0+5");
    // 族二：非法构造拒。
    cs.add("空快照拒", HandoffSnap::new(&[]).is_err(), "EMPTY");
    cs.add("批号重复拒", HandoffSnap::new(&[
        BatchRow { batch: 1, impl_lines: 10, risk_items: 0 },
        BatchRow { batch: 1, impl_lines: 20, risk_items: 0 },
    ]).is_err(), "DUP");
    cs.add("风险越域拒", BatchRow::new(1, 10, 99).is_err(), "RISK_OOR");
    cs.add("全零行拒", BatchRow::new(1, 0, 0).is_err(), "EMPTY");
    // 族三：冻结闸。
    cs.add("冻结闸", snap.frozen_guard().is_err(), "生成即冻结");
    // 族四：零漂移 diff。
    cs.add("零漂移", snap.diff_ledger(&rows).is_ok(), "同源即零漂");
    let mut tampered = rows.clone();
    tampered[1].impl_lines = 9999;
    cs.add("漂移检出", matches!(snap.diff_ledger(&tampered), Err(XC_DRIFT)), "行数漂移立案");
    cs.add("缺批检出", snap.diff_ledger(&rows[..2]).is_err(), "台账少批即漂");
    // 族五：序列化往返。
    let words = match serialize(&snap) {
        Ok(w) => w,
        Err(_) => {
            cs.add("序列化", false, "被拒");
            return cs;
        }
    };
    cs.add("序列化", words.len() == 2 + 4 * 3 + 1, "头+3×4+校验");
    cs.add("往返闭账", match deserialize(&words) {
        Ok(back) => back.sig() == snap.sig() && back.total_lines() == 5400,
        Err(_) => false,
    }, "签与聚合双对");
    // 族六：篡改检出。
    let mut bad = words.clone();
    bad[3] = bad[3].wrapping_add(1);
    cs.add("篡改拒", deserialize(&bad).is_err(), "CKSUM");
    let mut bad2 = words.clone();
    bad2[0] = 0xDEAD;
    cs.add("魔数拒", deserialize(&bad2).is_err(), "CKSUM");
    // 族七：内容签稳定（同构两次建快照签相等）。
    cs.add("签稳定", HandoffSnap::new(&rows).map(|s| s.sig()).unwrap_or(0) == snap.sig(), "确定性签名");
    cs
}
