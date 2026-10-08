//! VE-F0014 · 上下文快照与场景重放（VE-A 域 · 内核图形抽象层 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0014`
//!
//! **判据（锚点原文）**：渲染状态的快照与重放（诊断用：设备丢失前后状态
//! 对比），快照体积预算（只存关键状态），重放确定性（同快照同渲染序列）；
//! 快照含差量模式（两次快照只存差异省体积）；重放含与实机对拍校验（重放
//! 结果与实际渲染的偏差量化）；关键状态的定义公开（存什么不存什么写明）；
//! 快照含加密选项（诊断数据可含敏感场景）。错误路径：快照损坏→标注；重放
//! 失真→归因；超预算→裁剪声明。
//!
//! **设计要点**：
//! - **关键状态定义公开**：[`KEY_STATE_KEYS`] 是唯一权威清单（常量公开），
//!   每个键带"存/不存"理由——存什么不存什么写明，不是隐式约定；
//! - **体积预算**：每快照 [`SNAPSHOT_BUDGET_ENTRIES`] 上限，超预算按
//!   裁剪序（非关键→大值优先）裁剪并产出裁剪声明（`E_SNAPSHOT_TRIMMED`），
//!   绝不静默截断；
//! - **差量模式**：两次快照只存差异（键值对比），基线不变项不重复占预算；
//!   差量可独立标注（`delta_of: 基线帧号`）；
//! - **重放确定性**：同快照 + 同渲染序列 = 同结果（确定性摘要逐字节可复现
//!   ——FNV-1a 稳定哈希，不依赖浮点/时钟/迭代序）；重放器按快照重建状态
//!   后逐条执行序列，结果摘要可跨机对拍；
//! - **实机对拍校验**：重放摘要 vs 实测摘要的偏差量化（差值 + 超容限
//!   判定）；偏差超容限 → 归因输出（E_REPLAY_DIVERGENCE + 最可疑差异键）；
//! - **损坏→标注**：快照校验和（FNV-1a over 键值对）不符 → 标注
//!   `E_SNAPSHOT_CORRUPT`，拒绝重放不静默；
//! - **加密选项**：诊断数据可含敏感场景——`encrypt` 选项开启时载荷以
//!   XOR 流密码混淆并标注加密旗标（no_std 无 AES；诚实标注"混淆级"，
//!   声明与能力一致，不虚标为强加密）；
//! - **A09 诊断联动**：`A09_LINK` 契约 + 快照带 `lost_reason` 字段
//!   （设备丢失前后对比诊断的对接口）；
//! - **读屏可达**：快照状态 `screen_text()` 人话摘要。
//!
//! **跨批对接点**：A09 诊断联动；上游 F0012 探针、F0013 软渲回退。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 与 A09 诊断的衔接契约版本（跨批对接点显式化）。
pub const A09_LINK: u32 = 1;

/// 快照体积预算（键值对条数上限；超预算裁剪并声明）。
pub const SNAPSHOT_BUDGET_ENTRIES: usize = 64;

/// 实机对拍容限（摘要差绝对值上限；超出判失真）。
pub const REPLAY_TOLERANCE: u64 = 0;

/// 快照载荷最大字节数（加密/序列化后的硬上限）。
pub const SNAPSHOT_MAX_BYTES: usize = 4096;

/// 关键状态定义（公开判据：存什么不存什么写明）。
///
/// 元组 = (键名, 是否入快照, 公开理由)。
pub const KEY_STATE_KEYS: [(&str, bool, &str); 13] = [
    ("viewport", true, "视口尺寸决定合成布局——必存"),
    ("clear_color", true, "清屏色决定基帧——必存"),
    ("blend_mode", true, "混合模式改变像素语义——必存"),
    ("shader_profile", true, "着色器档位决定可执行特性集——必存"),
    ("vsync", true, "垂直同步影响帧节奏——必存"),
    ("texture_cache_bytes", true, "纹理缓存水位用于丢失前后对比——必存"),
    ("cmd_queue_depth", true, "命令队列深度反映在途工作——必存"),
    ("probe_aux_a", true, "诊断扩展键 A（探针辅助采样，回归门禁用）"),
    ("probe_aux_b", true, "诊断扩展键 B（探针辅助采样，回归门禁用）"),
    ("probe_aux_c", true, "诊断扩展键 C（探针辅助采样，回归门禁用）"),
    ("raw_password_or_token", false, "凭据类永不入快照（隐私红线）"),
    ("wall_clock", false, "墙钟破坏重放确定性——不存"),
    ("driver_private_blob", false, "驱动私有块无法跨版本解释——不存"),
];

// ---------------------------------------------------------------------------
// 二、稳定摘要（确定性判据的基元）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位稳定哈希（无溢出 panic：wrapping 语义）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// 三、快照器（状态快照 + 体积预算 + 差量模式 + 加密选项）
// ---------------------------------------------------------------------------

/// 单条快照键值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapEntry {
    /// 状态键（必须在 KEY_STATE_KEYS 声明过的键空间内）。
    pub key: String,
    /// 值（诊断用原始字符串形态）。
    pub val: String,
}

/// 一份渲染状态快照。
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// 帧号（快照时刻）。
    pub frame: u64,
    /// 差量基线帧号（None = 全量快照）。
    pub delta_of: Option<u64>,
    /// 键值对（已过预算与键空间校验）。
    pub entries: Vec<SnapEntry>,
    /// 被裁剪的键（超预算裁剪声明）。
    pub trimmed: Vec<String>,
    /// 载荷校验和（FNV-1a over 键值序列）。
    pub checksum: u64,
    /// 是否加密（混淆级）。
    pub encrypted: bool,
    /// 载荷字节（未加密=键值文本；加密=XOR 流）。
    pub payload: Vec<u8>,
    /// 设备丢失原因（A09 诊断联动字段；未丢失为空）。
    pub lost_reason: String,
    /// 快照时刻（逻辑 tick）。
    pub tick: u64,
}

impl Snapshot {
    /// 载荷校验：校验和与内容一致。
    pub fn intact(&self) -> bool {
        self.checksum == fnv1a(&self.payload)
    }
}

/// 快照器（状态采集 → 预算裁剪 → 差量 → 加密 → 落快照）。
pub struct Snapshotter {
    /// 上一份全量快照（差量基线）。
    baseline: Option<Snapshot>,
    /// 快照账本。
    pub snaps: Vec<Snapshot>,
    /// 裁剪声明账（超预算→裁剪声明，零静默）。
    pub trim_decls: Vec<(u64, String)>,
    /// 键空间违规账（声明外键拒绝入快照）。
    pub key_rejects: Vec<(String, String)>,
    /// 预算覆盖（测试注入；None = 用默认 SNAPSHOT_BUDGET_ENTRIES）。
    budget_override: Option<usize>,
    tick: u64,
}

impl Snapshotter {
    /// 构造。
    pub fn new() -> Self {
        Snapshotter {
            baseline: None,
            snaps: Vec::new(),
            trim_decls: Vec::new(),
            key_rejects: Vec::new(),
            budget_override: None,
            tick: 0,
        }
    }

    /// 预算覆盖注入（回归门禁用：小预算触发真裁剪路径；生产零调用）。
    pub fn set_budget_override(&mut self, budget: usize) {
        self.budget_override = Some(budget);
    }

    /// 逻辑时钟推进。
    pub fn advance_tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    fn key_declared(key: &str) -> Option<bool> {
        KEY_STATE_KEYS.iter().find(|(k, _, _)| *k == key).map(|(_, store, _)| *store)
    }

    /// 采集快照。
    ///
    /// * `state`：候选键值对（声明外的键被拒绝并入账——键空间纪律）；
    /// * `delta`：true 时相对上一份全量快照做差量（只存差异）；
    /// * `encrypt`：加密选项（诊断数据可含敏感场景）；
    /// * `lost_reason`：设备丢失原因（A09 对比诊断）。
    pub fn capture(
        &mut self,
        frame: u64,
        state: &[(&str, &str)],
        delta: bool,
        encrypt: bool,
        lost_reason: &str,
    ) -> Snapshot {
        self.tick = self.tick.saturating_add(1);
        // 1) 键空间纪律：声明外/声明不存的键拒绝并入账（隐私红线在源头）。
        let mut kept: Vec<SnapEntry> = Vec::new();
        for (k, v) in state {
            match Self::key_declared(k) {
                Some(true) => kept.push(SnapEntry { key: (*k).to_string(), val: (*v).to_string() }),
                Some(false) => {
                    self.key_rejects.push(((*k).to_string(), "声明为不存（理由见 KEY_STATE_KEYS）".to_string()));
                }
                None => {
                    self.key_rejects.push(((*k).to_string(), "声明外键（KEY_STATE_KEYS 未收录）".to_string()));
                }
            }
        }
        // 2) 差量：与基线同键同值的项剔除（两次快照只存差异省体积）。
        let mut delta_of = None;
        if delta {
            if let Some(base) = &self.baseline {
                let base_map = |b: &Snapshot| -> Vec<(String, String)> {
                    b.entries.iter().map(|e| (e.key.clone(), e.val.clone())).collect()
                };
                let bm = base_map(base);
                kept.retain(|e| !bm.iter().any(|(bk, bv)| *bk == e.key && *bv == e.val));
                delta_of = Some(base.frame);
            }
        }
        // 3) 体积预算：超预算裁剪并声明（裁剪序：值越长越先裁——大块头优先让位）。
        let budget = self.budget_override.unwrap_or(SNAPSHOT_BUDGET_ENTRIES);
        let mut trimmed: Vec<String> = Vec::new();
        if kept.len() > budget {
            kept.sort_by(|a, b| b.val.len().cmp(&a.val.len()));
            let excess = kept.split_off(budget);
            for e in excess.iter() {
                trimmed.push(e.key.clone());
            }
            kept.sort_by(|a, b| a.key.cmp(&b.key));
            self.trim_decls.push((
                frame,
                format!("超预算裁剪 {} 项：{}", trimmed.len(), trimmed.join(",")),
            ));
        }
        // 4) 载荷与校验和。
        let mut text = String::new();
        for e in kept.iter() {
            text.push_str(&format!("{}={};", e.key, e.val));
        }
        text.push_str(&format!("|frame={};lost={}", frame, lost_reason));
        let mut payload = text.into_bytes();
        if payload.len() > SNAPSHOT_MAX_BYTES {
            payload.truncate(SNAPSHOT_MAX_BYTES);
            self.trim_decls.push((frame, format!("载荷超 {} 字节硬上限，已截断", SNAPSHOT_MAX_BYTES)));
        }
        if encrypt {
            // 混淆级加密（no_std 诚实方案；XOR 流，键=帧号派生）——
            // 能力与声明一致：这是混淆，不是强加密（诚实铁律）。
            let key_stream = fnv1a(&frame.to_le_bytes());
            for (i, b) in payload.iter_mut().enumerate() {
                let ks = fnv1a(&(key_stream.wrapping_add(i as u64)).to_le_bytes());
                *b ^= (ks & 0xFF) as u8;
            }
        }
        // 校验和对最终载荷（密文或明文）计算——intact() 检的是传输完整性。
        let checksum = fnv1a(&payload);
        let snap = Snapshot {
            frame,
            delta_of,
            entries: kept,
            trimmed,
            checksum,
            encrypted: encrypt,
            payload,
            lost_reason: lost_reason.to_string(),
            tick: self.tick,
        };
        if delta_of.is_none() {
            self.baseline = Some(snap.clone());
        }
        self.snaps.push(snap.clone());
        snap
    }

    /// 快照状态读屏摘要（无障碍判据）。
    pub fn screen_text(snap: &Snapshot) -> String {
        format!(
            "快照状态：帧 {}（{}），{} 项状态，裁剪 {} 项，校验{}，载荷 {} 字节{}",
            snap.frame,
            if snap.delta_of.is_some() { "差量" } else { "全量" },
            snap.entries.len(),
            snap.trimmed.len(),
            if snap.intact() { "一致" } else { "损坏" },
            snap.payload.len(),
            if snap.encrypted { "，已加密（混淆级）" } else { "" },
        )
    }
}

impl Default for Snapshotter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、重放器（确定性重放 + 实机对拍 + 失真归因）
// ---------------------------------------------------------------------------

/// 渲染序列单步（确定性命令——无时钟无随机）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayStep {
    /// 清屏（颜色索引）。
    Clear(u64),
    /// 绘制矩形（x, y, w, h 量化值）。
    Rect(u64, u64, u64, u64),
    /// 设置混合模式（档位）。
    Blend(u64),
}

/// 重放结果。
#[derive(Clone, Debug)]
pub struct ReplayResult {
    /// 重放摘要（确定性）。
    pub digest: u64,
    /// 实机对拍偏差（|重放-实测|）。
    pub divergence: u64,
    /// 是否超容限判失真。
    pub divergent: bool,
    /// 归因（失真时给最可疑差异键；未失真为空）。
    pub attribution: String,
}

/// 重放器：同快照 + 同渲染序列 = 同结果（重放确定性判据）。
pub struct Replayer;

impl Replayer {
    /// 重放：按快照重建状态 → 逐条执行确定性序列 → 稳定摘要。
    pub fn replay(snap: &Snapshot, seq: &[ReplayStep]) -> Result<u64, &'static str> {
        // 损坏→标注：校验和不符拒绝重放（不静默给伪结果）。
        if !snap.intact() {
            return Err("E_SNAPSHOT_CORRUPT");
        }
        // 确定性摘要流：状态键序 + 序列步进全部进哈希（同输入同输出）。
        let mut h = fnv1a(&snap.frame.to_le_bytes());
        let mut keys: Vec<&str> = snap.entries.iter().map(|e| e.key.as_str()).collect();
        keys.sort_unstable();
        for k in keys {
            let v = snap.entries.iter().find(|e| e.key == k).map(|e| e.val.as_str()).unwrap_or("");
            h = fnv1a(&[h.to_le_bytes(), fnv1a(k.as_bytes()).to_le_bytes()].concat());
            h = fnv1a(&[h.to_le_bytes(), fnv1a(v.as_bytes()).to_le_bytes()].concat());
        }
        for step in seq {
            let bytes: Vec<u8> = match step {
                ReplayStep::Clear(c) => {
                    let mut b = b"C".to_vec();
                    b.extend_from_slice(&c.to_le_bytes());
                    b
                }
                ReplayStep::Rect(x, y, w, hh) => {
                    let mut b = b"R".to_vec();
                    for v in [x, y, w, hh] {
                        b.extend_from_slice(&v.to_le_bytes());
                    }
                    b
                }
                ReplayStep::Blend(m) => {
                    let mut b = b"B".to_vec();
                    b.extend_from_slice(&m.to_le_bytes());
                    b
                }
            };
            h = fnv1a(&[h.to_le_bytes().to_vec(), bytes].concat());
        }
        Ok(h)
    }

    /// 实机对拍校验：重放摘要 vs 实测摘要偏差量化；超容限归因（重放失真→归因）。
    pub fn verify(snap: &Snapshot, seq: &[ReplayStep], measured_digest: u64) -> ReplayResult {
        let digest = match Self::replay(snap, seq) {
            Ok(d) => d,
            Err(code) => {
                return ReplayResult {
                    digest: 0,
                    divergence: u64::MAX,
                    divergent: true,
                    attribution: code.to_string(),
                };
            }
        };
        let divergence = digest.abs_diff(measured_digest);
        let divergent = divergence > REPLAY_TOLERANCE;
        let attribution = if divergent {
            // 归因：找出对摘要影响最大的状态键（逐键扰动重算——最可疑差异键）。
            let mut suspect = String::from("无状态差异（疑在渲染序列本身）");
            let mut best: u64 = 0;
            for e in snap.entries.iter() {
                let mut probe = snap.clone();
                probe.entries = snap
                    .entries
                    .iter()
                    .map(|x| if x.key == e.key {
                        SnapEntry { key: x.key.clone(), val: format!("{}#probe", x.val) }
                    } else {
                        x.clone()
                    })
                    .collect();
                if let Ok(d2) = Self::replay(&probe, seq) {
                    let delta = d2.abs_diff(digest);
                    if delta > best {
                        best = delta;
                        suspect = e.key.clone();
                    }
                }
            }
            format!("重放失真：偏差 {}，最可疑差异键：{}", divergence, suspect)
        } else {
            String::new()
        };
        ReplayResult { digest, divergence, divergent, attribution }
    }
}

// ---------------------------------------------------------------------------
// 五、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0014 域自检（判据逐条映射见 `vea14_checks.rs`）。
pub fn run_vea14_checks() -> CheckSet {
    super::vea14_checks::run_vea14_checks()
}

// ---------------------------------------------------------------------------
// 六、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn state() -> Vec<(&'static str, &'static str)> {
        vec![
            ("viewport", "3840x2160"),
            ("clear_color", "#101418"),
            ("blend_mode", "over"),
            ("shader_profile", "sm2_soft"),
            ("vsync", "on"),
            ("texture_cache_bytes", "18446744073709551"),
            ("cmd_queue_depth", "3"),
        ]
    }

    fn seq() -> Vec<ReplayStep> {
        vec![
            ReplayStep::Clear(0),
            ReplayStep::Rect(10, 20, 300, 200),
            ReplayStep::Blend(1),
            ReplayStep::Rect(40, 50, 800, 600),
        ]
    }

    #[test]
    fn vea14_snapshot_deterministic_replay() {
        let mut s = Snapshotter::new();
        let snap = s.capture(7, &state(), false, false, "");
        assert!(snap.intact());
        // 同快照同序列 → 同摘要（确定性）。
        let d1 = Replayer::replay(&snap, &seq()).unwrap();
        let d2 = Replayer::replay(&snap, &seq()).unwrap();
        assert_eq!(d1, d2, "同快照同渲染序列必须同摘要");
        // 对拍校验：实测=重放 → 零偏差；实测异值 → 失真归因。
        let ok = Replayer::verify(&snap, &seq(), d1);
        assert!(!ok.divergent && ok.divergence == 0);
        let bad = Replayer::verify(&snap, &seq(), d1 ^ 0xFF);
        assert!(bad.divergent && bad.attribution.contains("重放失真"));
    }

    #[test]
    fn vea14_delta_and_privacy_and_encrypt() {
        let mut s = Snapshotter::new();
        let base = s.capture(1, &state(), false, false, "");
        assert!(base.delta_of.is_none() && base.entries.len() == 7);
        // 差量：3 项变化 → 只存 3 项。
        let mut st2 = state();
        st2[1] = ("clear_color", "#202830");
        st2[4] = ("vsync", "off");
        st2[6] = ("cmd_queue_depth", "9");
        let d = s.capture(2, &st2, true, false, "");
        assert_eq!(d.delta_of, Some(1));
        assert_eq!(d.entries.len(), 3, "差量只存变化项");
        // 隐私红线：凭据与墙钟在源头被拒。
        let mut st3 = state();
        st3.push(("raw_password_or_token", "hunter2"));
        st3.push(("wall_clock", "2026-10-06T02:00:00"));
        st3.push(("undeclared_key", "x"));
        let p = s.capture(3, &st3, false, false, "");
        assert!(!p.payload.windows(7).any(|w| w == b"hunter2"), "凭据不入载荷");
        assert_eq!(s.key_rejects.len(), 3, "两个不存键+一个声明外键");
        // 加密选项：载荷混淆、旗标在、校验一致、可读文本不可见。
        let e = s.capture(4, &state(), false, true, "驱动重置");
        assert!(e.encrypted && e.intact());
        assert!(!String::from_utf8_lossy(&e.payload).contains("viewport="));
        assert_eq!(e.lost_reason, "驱动重置", "A09 对比诊断字段");
    }

    #[test]
    fn vea14_corrupt_detected_not_replayed() {
        let mut s = Snapshotter::new();
        let mut snap = s.capture(5, &state(), false, false, "");
        snap.payload[0] ^= 0xFF; // 篡改载荷
        assert!(!snap.intact(), "篡改后校验和应失配");
        assert_eq!(Replayer::replay(&snap, &seq()), Err("E_SNAPSHOT_CORRUPT"));
        let v = Replayer::verify(&snap, &seq(), 0);
        assert!(v.divergent && v.attribution == "E_SNAPSHOT_CORRUPT", "损坏标注进归因");
    }

    #[test]
    fn vea14_checks_all_green() {
        let set = run_vea14_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0014 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
