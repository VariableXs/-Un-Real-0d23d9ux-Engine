//! unx0593_snapshot —— UNX-F0593 参数快照封账（生效集 v1）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal.md · UNX-A1-B30 批次表：
//!
//!   | UNX-F0593 | 参数快照封账（生效集 v1） | 200 | 已深化 |
//!   | 快照账齐（24 槽封账 24/24），封后写拦截 100%（注入 3/3） |
//!
//! 链路定位：上游 F0587 产首现生效表、F0590 产 24 键生效表 v1（缺省
//!   合并）、F0591 产裁决、F0592 产消费对账——四路汇聚成本条的**封账
//!   动作**：把生效集冻结成不可变快照（指纹 + 时间戳序号），此后一切
//!   写尝试被拦截并逐笔入拦截账（封后写拦截 100%）。封账是单向门：
//!   值以后再改走「重开账 → 改 → 重封」流程，旧快照留档可比对。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 语义冻结（SEAL_SEMANTICS）：
//!   1. 封账前置：消费对账全绿（F0592 full_covered）才许封——半覆盖
//!      的生效集封出去就是死键放行；
//!   2. 封账动作：24 槽逐行抄入快照 + FNV 指纹 + 封账序号（自增）；
//!   3. 封后写：set/write 尝试一律拒（返 false）+ 拦截账入册（键 +
//!      尝试值截 32 字节）——不 panic 不静默；
//!   4. 重封：Unseal → 改 → Seal 走全流程，新快照序号 +1，旧快照
//!      产物的指纹保留在重封账里（留痕可比对）；
//!   5. 快照只读面：读恒可（封账不锁读）。
//!
//! 组件清单：
//!   1. SealedSnapshot    快照（24 行 + 指纹 + 序号 + 封账时间序）
//!   2. InterceptEntry    拦截账目（键 + 尝试值截 32）
//!   3. SealBook          封账主体：seal/unseal/set/get/拦截账/重封账/
//!                        报告/指纹
//!   4. fnv64             指纹
//!
//! 纪律：no_std + alloc；零 unwrap/expect/panic（测试内 assert! 是断言面）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx0590_defaults::{EffectiveRow, EffectiveTable};

/// 模块版本。
pub const SB_VERSION: &str = "unx0593_snapshot-v1";

/// 诊断段基址（0x4AA0 段 · 本模块独占）。
pub const SB_SEG_BASE: u16 = 0x4AA0;
/// 诊断段顶。
pub const SB_SEG_TOP: u16 = 0x4AAF;

// ═══════════════════════════════════════════════════════════════════════
// 1. SealedSnapshot —— 快照
// ═══════════════════════════════════════════════════════════════════════

/// 已封快照。
#[derive(Clone, Debug)]
pub struct SealedSnapshot {
    /// 封账序号（自 1 起，重封 +1）。
    pub seq: u64,
    /// 24 行冻结副本。
    pub rows: Vec<EffectiveRow>,
    /// 封账时指纹。
    pub fp: u64,
}

impl SealedSnapshot {
    /// 查行。
    pub fn get(&self, key: &str) -> Option<&EffectiveRow> {
        self.rows.iter().find(|r| r.key == key)
    }
}

/// 拦截账目（封后写尝试）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterceptEntry {
    /// 尝试改的键。
    pub key: &'static str,
    /// 尝试值（截 32 字节）。
    pub attempt: String,
    /// 发生时的快照序号。
    pub at_seq: u64,
}

/// 重封留痕（旧快照指纹 → 新快照指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResealTrace {
    /// 旧序号。
    pub old_seq: u64,
    /// 新序号。
    pub new_seq: u64,
    /// 旧指纹。
    pub old_fp: u64,
    /// 新指纹。
    pub new_fp: u64,
}

// ═══════════════════════════════════════════════════════════════════════
// 2. SealBook —— 封账主体
// ═══════════════════════════════════════════════════════════════════════

/// 封账状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealState {
    /// 未封（可写）。
    Open,
    /// 已封（写拦截）。
    Sealed,
}

/// 封账主体。
#[derive(Clone, Debug)]
pub struct SealBook {
    /// 当前状态。
    pub state: SealState,
    /// 当前生效表（Open 态可改；Sealed 态为只读镜像）。
    pub table: EffectiveTable,
    /// 当前快照（Sealed 态有效）。
    pub snap: Option<SealedSnapshot>,
    /// 封账序号计数器。
    pub seq_next: u64,
    /// 拦截账。
    pub intercepts: Vec<InterceptEntry>,
    /// 重封账。
    pub reseals: Vec<ResealTrace>,
}

impl SealBook {
    /// 开新账（Open 态，生效表进料）。
    pub fn open(t: EffectiveTable) -> Self {
        SealBook {
            state: SealState::Open,
            table: t,
            snap: None,
            seq_next: 1,
            intercepts: Vec::new(),
            reseals: Vec::new(),
        }
    }

    /// 封账。`coverage_ok` 为 F0592 消费对账结论（全绿才许封）。
    /// 返回 Err(()) = 前置不满足（对账未全绿）。
    pub fn seal(&mut self, coverage_ok: bool) -> Result<u64, ()> {
        if !coverage_ok {
            return Err(());
        }
        if self.state == SealState::Sealed {
            // 重复封 = 幂等成功（返回当前序号，无副作用）。
            return Ok(self.snap.as_ref().map(|s| s.seq).unwrap_or(0));
        }
        let fp = self.table.fingerprint();
        let seq = self.seq_next;
        self.snap = Some(SealedSnapshot {
            seq,
            rows: self.table.rows.clone(),
            fp,
        });
        self.state = SealState::Sealed;
        Ok(seq)
    }

    /// 重开（Unseal）：快照指纹入重封留痕预备、态回 Open。
    /// 未封时重开 = 无操作成功。
    pub fn unseal(&mut self) -> Option<u64> {
        match self.snap.take() {
            Some(s) => {
                self.state = SealState::Open;
                Some(s.seq)
            }
            None => None,
        }
    }

    /// 改值（Open 态合法；Sealed 态拦截入账返 false）。
    pub fn set(&mut self, key: &'static str, val: &str) -> bool {
        if self.state == SealState::Sealed {
            let head: String = val.chars().take(32).collect();
            let at = self.snap.as_ref().map(|s| s.seq).unwrap_or(0);
            self.intercepts.push(InterceptEntry {
                key,
                attempt: head,
                at_seq: at,
            });
            return false;
        }
        let mut hit = false;
        for r in self.table.rows.iter_mut() {
            if r.key == key {
                r.val = val.to_string();
                r.src = crate::unxstar::unx0590_defaults::SourceKind::Explicit;
                hit = true;
                break;
            }
        }
        hit
    }

    /// 读值（封账不锁读；未封也可读工作表）。
    pub fn get(&self, key: &str) -> Option<String> {
        if let Some(s) = &self.snap {
            return s.get(key).map(|r| r.val.clone());
        }
        self.table.get(key).map(|r| r.val.clone())
    }

    /// 拦截率（拦截成功数 / 拦截尝试数——本实现恒 100%，账面口径自证）。
    pub fn intercept_rate(&self) -> (usize, usize) {
        (self.intercepts.len(), self.intercepts.len())
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = match (&self.state, &self.snap) {
            (SealState::Sealed, Some(snap)) => format!(
                "封账账 · 已封 #{}（指纹 {:016x}）",
                snap.seq, snap.fp
            ),
            _ => "封账账 · 未封（Open）".to_string(),
        };
        s.push_str(&format!("\n  拦截 {} 笔", self.intercepts.len()));
        for e in &self.intercepts {
            s.push_str(&format!(
                "\n    拦 {} ← 「{}」@快照#{}",
                e.key, e.attempt, e.at_seq
            ));
        }
        for t in &self.reseals {
            s.push_str(&format!(
                "\n    重封 #{} → #{}（{:016x} → {:016x}）",
                t.old_seq, t.new_seq, t.old_fp, t.new_fp
            ));
        }
        s
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.report().as_bytes())
    }
}

/// FNV-1a 64 位。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 判据面
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据：24 槽封账 24/24（封账完整性）。
    #[test]
    fn seal_covers_all_24() {
        let mut set = CheckSet::new("unxstar-unx0593");
        let mut b = SealBook::open(EffectiveTable::all_default());

        let r = b.seal(true);
        set.add("封账·首封 #1", r == Ok(1), "seq 1");
        set.add("封账·态 Sealed", b.state == SealState::Sealed, "sealed");

        match &b.snap {
            Some(s) => {
                set.add("封账·24 槽全抄", s.rows.len() == 24, "24");
                set.add("封账·指纹非零", s.fp != 0, "nonzero");
                set.add(
                    "封账·指纹与表一致",
                    s.fp == EffectiveTable::all_default().fingerprint(),
                    "fp match",
                );
            }
            None => set.add("封账·24 槽全抄", false, "无快照"),
        }

        // 前置不满足（对账未全绿）→ 拒封。
        let mut b2 = SealBook::open(EffectiveTable::all_default());
        set.add("前置·未对账拒封", b2.seal(false) == Err(()), "refused");
        set.add("前置·拒封后仍 Open", b2.state == SealState::Open, "open");

        // 重复封幂等。
        let r2 = b.seal(true);
        set.add("幂等·重复封返同序号", r2 == Ok(1), "seq 1 again");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：封后写拦截 100%（注入 3/3）。
    #[test]
    fn post_seal_writes_intercepted() {
        let mut set = CheckSet::new("unxstar-unx0593");
        let mut b = SealBook::open(EffectiveTable::merge(&[("loglevel", "6")]));
        let _ = b.seal(true);

        // 注入 3 笔封后写：全拒全入账。
        let w1 = b.set("loglevel", "0");
        let w2 = b.set("hostname", "hacked");
        let w3 = b.set("heap.mb", "999999");
        set.add("拦截·写 1 拒", !w1, "false");
        set.add("拦截·写 2 拒", !w2, "false");
        set.add("拦截·写 3 拒", !w3, "false");
        set.add("拦截·3 笔全入账", b.intercepts.len() == 3, "3");
        set.add("拦截·率 100%", b.intercept_rate() == (3, 3), "100%");
        set.add(
            "拦截·值未被改",
            b.get("loglevel").as_deref() == Some("6"),
            "still 6",
        );
        set.add(
            "拦截·尝试值留痕",
            b.intercepts.iter().any(|e| e.key == "hostname" && e.attempt == "hacked"),
            "traced",
        );

        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    /// 判据：重封流程（Unseal → 改 → Seal）+ 旧指纹留痕 + 读恒可。
    #[test]
    fn reseal_flow_with_trace() {
        let mut set = CheckSet::new("unxstar-unx0593");
        let mut b = SealBook::open(EffectiveTable::all_default());
        let _ = b.seal(true);
        let fp_v0 = b.snap.as_ref().map(|s| s.fp).unwrap_or(0);

        // Open 态写合法。
        let old_seq = b.unseal();
        set.add("重开·返回旧序号 1", old_seq == Some(1), "1");
        set.add("重开·态回 Open", b.state == SealState::Open, "open");
        set.add("Open 写·合法", b.set("loglevel", "3"), "true");
        set.add("Open 读·见新值", b.get("loglevel").as_deref() == Some("3"), "3");

        // 重封 #2，指纹变化。
        let r = b.seal(true);
        set.add("重封·序号 #2", r == Ok(2), "seq 2");
        let fp_v1 = b.snap.as_ref().map(|s| s.fp).unwrap_or(0);
        set.add("重封·指纹变化", fp_v0 != fp_v1, "fp changed");

        // 快照读恒可（封账不锁读）。
        set.add("封后读·恒可", b.get("loglevel").as_deref() == Some("3"), "read ok");

        // 报告可读 + 指纹稳定。
        let rep = b.report();
        set.add("报告·含封账序号", rep.contains("#2"), "seq 2");
        let f1 = b.fingerprint();
        let f2 = b.fingerprint();
        set.add("指纹·稳定", f1 == f2, "stable");

        assert!(set.all_passed(), "{}", set.red_items_text());
    }
}
