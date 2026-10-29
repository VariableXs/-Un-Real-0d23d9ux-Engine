//! UNX-F10209 执行产物落盘（日志+结果双产物可回放，缺产物拦截）（0x10d40 段）
//!
//! 锚点：UNX-F10209-J1 产物落盘达成（日志+结果双产物落盘可回放
//! 10/10 次），缺产物 10/10 次拦截。
//! 域：CoRun·UNX-C3（跑批执行面谱系）。
//!
//! 结构：产物仓（Log/Result 双类载荷 + 校验和折叠）+ 回放门
//! （双产物齐备方开启）+ 缺件检出核 + 折叠指纹 +
//! 注入核（缺日志/空载荷/空批/印篡改/缺件批量检出）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10d40;
/// 诊断码：日志产物缺失
pub const ERR_NO_LOG: u32 = SEG_BASE + 0x01;
/// 诊断码：结果产物缺失
pub const ERR_NO_RESULT: u32 = SEG_BASE + 0x02;
/// 诊断码：批次标识为空
pub const ERR_EMPTY_BATCH: u32 = SEG_BASE + 0x03;
/// 诊断码：产物载荷为空
pub const ERR_EMPTY_PAYLOAD: u32 = SEG_BASE + 0x04;
/// 诊断码：回放门未闭合
pub const ERR_REPLAY_GATE: u32 = SEG_BASE + 0x05;

/// 产物类别
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// 日志产物
    Log,
    /// 结果产物
    Result,
}

/// 产物条目
#[derive(Clone, PartialEq, Eq)]
pub struct Artifact {
    /// 批次标识
    pub batch: String,
    /// 产物类别
    pub kind: ArtifactKind,
    /// 载荷摘要（非空）
    pub payload: String,
    /// 校验和（payload 逐字节折叠）
    pub checksum: u32,
}

impl Artifact {
    /// 条目自洽：载荷非空且校验和一致
    pub fn ok(&self) -> bool {
        !self.batch.is_empty() && !self.payload.is_empty()
            && self.checksum == fold_payload(&self.payload)
    }
}

/// 载荷折叠校验和
pub fn fold_payload(p: &str) -> u32 {
    let mut acc = 0x811C9DC5u32;
    for b in p.as_bytes() {
        acc = acc ^ (*b as u32);
        acc = acc.wrapping_mul(0x01000193);
    }
    acc
}

/// 双产物落盘仓
pub struct ArtifactStore {
    /// 已落盘产物
    pub vault: Vec<Artifact>,
    /// 回放门是否闭合
    pub replay_open: bool,
}

impl ArtifactStore {
    /// 开仓（回放门默认关闭）
    pub fn new() -> Self {
        ArtifactStore { vault: Vec::new(), replay_open: false }
    }

    /// 落盘一件产物
    pub fn deposit(&mut self, batch: &str, kind: ArtifactKind, payload: &str) -> Result<(), u32> {
        if batch.is_empty() {
            return Err(ERR_EMPTY_BATCH);
        }
        if payload.is_empty() {
            return Err(ERR_EMPTY_PAYLOAD);
        }
        let art = Artifact {
            batch: batch.to_string(),
            kind,
            payload: payload.to_string(),
            checksum: fold_payload(payload),
        };
        if !art.ok() {
            return Err(ERR_EMPTY_PAYLOAD);
        }
        self.vault.push(art);
        Ok(())
    }

    /// 某批次双产物齐备检查
    pub fn pair_complete(&self, batch: &str) -> bool {
        self.vault.iter().any(|a| a.batch == batch && a.kind == ArtifactKind::Log)
            && self.vault.iter().any(|a| a.batch == batch && a.kind == ArtifactKind::Result)
    }

    /// 回放门：双产物齐备方可开启
    pub fn open_replay(&mut self, batch: &str) -> Result<(), u32> {
        if !self.pair_complete(batch) {
            return Err(ERR_REPLAY_GATE);
        }
        self.replay_open = true;
        Ok(())
    }

    /// 缺日志检出：登记了结果但无日志
    pub fn missing_log_batches(&self) -> Vec<String> {
        let mut out = Vec::new();
        for a in &self.vault {
            if a.kind == ArtifactKind::Result && !self.has_log(&a.batch) {
                if !out.contains(&a.batch) {
                    out.push(a.batch.clone());
                }
            }
        }
        out
    }

    /// 是否有该批次日志
    fn has_log(&self, batch: &str) -> bool {
        self.vault.iter().any(|a| a.batch == batch && a.kind == ArtifactKind::Log)
    }

    /// 封仓
    pub fn seal(&mut self) -> u32 {
        let mut acc = 0x10d40u32;
        for (i, a) in self.vault.iter().enumerate() {
            acc = acc.wrapping_mul(31337).wrapping_add(a.checksum >> (i % 4));
            acc = acc.wrapping_add(match a.kind {
                ArtifactKind::Log => 11,
                ArtifactKind::Result => 13,
            });
        }
        acc
    }

    /// 诊断面：产物总字节数
    pub fn total_payload_bytes(&self) -> usize {
        self.vault.iter().map(|a| a.payload.len()).sum()
    }

    /// 诊断面：某批次产物件数
    pub fn artifact_count_of(&self, batch: &str) -> usize {
        self.vault.iter().filter(|a| a.batch == batch).count()
    }

    /// 诊断面：已落盘批次清单（去重）
    pub fn distinct_batches(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in &self.vault {
            if !out.contains(&a.batch) {
                out.push(a.batch.clone());
            }
        }
        out
    }

    /// 诊断面：结果产物通过判定（载荷含 exit:0）
    pub fn result_passing(&self, batch: &str) -> Option<bool> {
        self.vault.iter()
            .find(|a| a.batch == batch && a.kind == ArtifactKind::Result)
            .map(|a| a.payload.contains("exit:0"))
    }

    /// 诊断面：回放门前置校验（不落状态）
    pub fn replay_gate_precheck(&self, batch: &str) -> Result<usize, u32> {
        if self.pair_complete(batch) {
            Ok(self.artifact_count_of(batch))
        } else {
            Err(ERR_REPLAY_GATE)
        }
    }

    /// 诊断面：日志产物清单
    pub fn log_payloads_of(&self, batch: &str) -> Vec<String> {
        self.vault.iter()
            .filter(|a| a.batch == batch && a.kind == ArtifactKind::Log)
            .map(|a| a.payload.clone())
            .collect()
    }

    /// 诊断面： vault 是否为空
    pub fn is_empty(&self) -> bool {
        self.vault.is_empty()
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 只有结果无日志 → 回放门拒绝
    let mut s1 = ArtifactStore::new();
    let _ = s1.deposit("b-one", ArtifactKind::Result, "res-data");
    out[0] = s1.open_replay("b-one") == Err(ERR_REPLAY_GATE);
    // 2: 空载荷 → 拒收
    let mut s2 = ArtifactStore::new();
    out[1] = s2.deposit("b-two", ArtifactKind::Log, "") == Err(ERR_EMPTY_PAYLOAD);
    // 3: 空批次 → 拒收
    let mut s3 = ArtifactStore::new();
    out[2] = s3.deposit("", ArtifactKind::Log, "x") == Err(ERR_EMPTY_BATCH);
    // 4: 校验和被篡改 → 自洽失败检出
    let mut s4 = ArtifactStore::new();
    let _ = s4.deposit("b-four", ArtifactKind::Log, "payload");
    let mut forged = s4.vault[0].clone();
    forged.checksum = forged.checksum.wrapping_add(1);
    out[3] = !forged.ok();
    // 5: 空仓缺产物批量检出命中
    let mut s5 = ArtifactStore::new();
    let _ = s5.deposit("b5", ArtifactKind::Result, "r");
    out[4] = s5.missing_log_batches() == vec!["b5".to_string()];
    out
}

/// 判据函数
pub fn run_unx10209_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10209");
    let mut st = ArtifactStore::new();
    let d1 = st.deposit("bt-101", ArtifactKind::Log, "run-log-line-1");
    let d2 = st.deposit("bt-101", ArtifactKind::Result, "exit:0 cases:10");
    s.add("双产物落盘", d1.is_ok() && d2.is_ok(), "日志与结果两件均受理");
    s.add("双产物齐备", st.pair_complete("bt-101"), "同批次两件齐备");
    let d3 = st.open_replay("bt-101");
    s.add("回放门开启", d3.is_ok() && st.replay_open, "齐备批次回放门放行");
    let mut sx = ArtifactStore::new();
    let _ = sx.deposit("bt-x", ArtifactKind::Log, "only-log");
    s.add("缺结果拒回放", sx.open_replay("bt-x") == Err(ERR_REPLAY_GATE), "只有日志不得回放");
    s.add("缺日志批量检出", st.missing_log_batches().is_empty() && sx.missing_log_batches().is_empty(), "齐备与仅日志仓均无缺日志批");
    s.add("校验和自洽", st.vault.iter().all(|a| a.ok()), "全部产物通过自洽");
    s.add("载荷折叠稳定", fold_payload("abc") == fold_payload("abc") && fold_payload("abc") != fold_payload("abd"), "同载荷同和、异载荷异和");
    s.add("载荷字节数", st.total_payload_bytes() > 0, "产物载荷统计非零");
    s.add("批次产物件数", st.artifact_count_of("bt-101") == 2 && sx.artifact_count_of("bt-x") == 1, "各批产物件数正确");
    s.add("批次去重清单", st.distinct_batches() == vec!["bt-101".to_string()], "去重后批次清单唯一");
    s.add("结果通过判定", st.result_passing("bt-101") == Some(true) && sx.result_passing("bt-x").is_none(), "exit:0 判通过且缺结果为 None");
    s.add("回放门前置校验", st.replay_gate_precheck("bt-101") == Ok(2) && sx.replay_gate_precheck("bt-x") == Err(ERR_REPLAY_GATE), "齐备批返回件数、缺批给诊断码");
    s.add("日志载荷清单", st.log_payloads_of("bt-101") == vec!["run-log-line-1".to_string()], "日志载荷抽取一致");
    s.add("空仓判定", ArtifactStore::new().is_empty() && !st.is_empty(), "空仓为空、实仓非空");
    s.add("空仓回放拒绝", ArtifactStore::new().open_replay("none") == Err(ERR_REPLAY_GATE), "空仓任何批次不得回放");
    s.add("缺件检出含改批", {
        let mut sy = ArtifactStore::new();
        let _ = sy.deposit("bt-y", ArtifactKind::Result, "r-only");
        let _ = sy.deposit("bt-z", ArtifactKind::Log, "l-only");
        sy.missing_log_batches().contains(&"bt-y".to_string())
    }, "仅有结果的批次进入缺件清单");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&b| b), "五类违规全部检出");
    let seal_v = st.seal();
    s.add("封仓指纹非零", seal_v != 0, "封仓折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_complete_and_replay_gate() {
        let mut st = ArtifactStore::new();
        assert_eq!(st.open_replay("b"), Err(ERR_REPLAY_GATE));
        let _ = st.deposit("b", ArtifactKind::Log, "l");
        assert_eq!(st.open_replay("b"), Err(ERR_REPLAY_GATE));
        let _ = st.deposit("b", ArtifactKind::Result, "r");
        assert!(st.open_replay("b").is_ok());
        assert!(st.replay_open);
    }

    #[test]
    fn checksum_folds_deterministically() {
        let c1 = fold_payload("kernel-artifact");
        let c2 = fold_payload("kernel-artifact");
        assert_eq!(c1, c2);
        assert_ne!(c1, fold_payload("kernel-artifacT"));
    }

    #[test]
    fn distinct_batches_and_counts() {
        let mut st = ArtifactStore::new();
        let _ = st.deposit("b1", ArtifactKind::Log, "l1");
        let _ = st.deposit("b1", ArtifactKind::Result, "r1");
        let _ = st.deposit("b2", ArtifactKind::Log, "l2");
        assert_eq!(st.distinct_batches().len(), 2);
        assert_eq!(st.artifact_count_of("b1"), 2);
        assert_eq!(st.artifact_count_of("b2"), 1);
        assert_eq!(st.total_payload_bytes(), 6);
    }

    #[test]
    fn empty_payload_rejected() {
        let mut st = ArtifactStore::new();
        assert_eq!(st.deposit("b", ArtifactKind::Log, ""), Err(ERR_EMPTY_PAYLOAD));
        assert_eq!(st.deposit("", ArtifactKind::Log, "x"), Err(ERR_EMPTY_BATCH));
        assert!(st.vault.is_empty());
    }

    #[test]
    fn log_payloads_and_passing() {
        let mut st = ArtifactStore::new();
        let _ = st.deposit("b", ArtifactKind::Log, "line-1");
        let _ = st.deposit("b", ArtifactKind::Result, "exit:0 ok");
        assert_eq!(st.log_payloads_of("b"), vec!["line-1".to_string()]);
        assert_eq!(st.result_passing("b"), Some(true));
        assert!(st.replay_gate_precheck("b") == Ok(2));
    }

    #[test]
    fn tampered_checksum_fails_selfcheck() {
        let mut st = ArtifactStore::new();
        let _ = st.deposit("b", ArtifactKind::Log, "good");
        st.vault[0].payload.push('x');
        assert!(!st.vault[0].ok());
    }

    #[test]
    fn replay_gate_blocks_incomplete_pair() {
        let mut st = ArtifactStore::new();
        let _ = st.deposit("b", ArtifactKind::Result, "r-only");
        assert_eq!(st.open_replay("b"), Err(ERR_REPLAY_GATE));
    }
}