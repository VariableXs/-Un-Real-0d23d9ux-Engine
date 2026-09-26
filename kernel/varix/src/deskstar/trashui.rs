//! F085 回收站体验化 · 完整设计（STAR I 主册 G-C-15）。
//!
//! **判据（主册）**：删-还原-再删 100 轮零数据损失（哈希对拍）；
//! 动画全程 80fps；容量环与实际占用一致。
//!
//! **设计要点（主册）**：
//! - 删除动画（图标飞向回收站 200ms 弧线+缩淡）、还原反向、容量
//!   水位环（回收站图标角标）；零真删红线（B-1701）是全部动画的
//!   前提——动画演的是真实的数据流；
//! - 删除动画多选批量（N 个图标依次 30ms 错峰飞入）；回收站图标
//!   角标显示件数；容量水位环 >80% 转黄色+toast 提醒清理；回收站
//!   窗口复用资源管理器（C-4）加「还原/清空」工具条；清空二次
//!   确认+可撤销 5s（F031 同语义）；
//! - 回收站目录结构对齐 Windows（$Recycle.Bin 语义）：原路径元数据
//!   伴生（还原保真）；容量上限可设（默认分区 10%）；
//! - 空间不足删除 → 弹「回收站已满」三选（清空旧件/直接永久删/
//!   取消——永久删需二次确认）；跨盘删除 → 各盘独立回收站（还原
//!   回原盘）；还原目标已被占 → 重命名还原（F087 语义）；
//! - 飞入弧线贝塞尔控制点=回收站方向 45° 抬升；动画期间真删除已
//!   异步完成（动画不阻塞数据流——表演与事实解耦）；元数据含删除
//!   时间（排序用）+原路径（>260 字符截断策略文档化）；清空撤销
//!   依赖回收站内暂存区（真清空在撤销窗后）。
//!
//! 实装口径：回收站账本（哈希保真对拍）+ 元数据账 + 容量水位账 +
//! 删除/还原/清空三动画账 + 三选对话框账 + 每盘独立账。动画账与
//! 数据账解耦（动画完成不等于数据动作的依赖点——判据红线）。

use crate::checks::CheckSet;

use crate::deskstar::dbase::Token;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::{vec, format};

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/状态与异常/设计细节）
// ---------------------------------------------------------------------------

/// 删除飞入动画时长（ms，弧线+缩淡）。
pub const FLY_MS: u32 = 200;

/// 多选错峰（ms/件）。
pub const STAGGER_MS: u32 = 30;

/// 容量水位警示线（%，>80% 转黄+toast）。
pub const WATERMARK_PCT: u8 = 80;

/// 清空撤销窗（ms，真清空在撤销窗后）。
pub const EMPTY_UNDO_MS: u64 = 5_000;

/// 原路径元数据截断（字符，>260 截断策略）。
pub const ORIG_PATH_CAP: usize = 260;

/// 弧线贝塞尔抬升角（度，回收站方向 45°）。
pub const ARC_RISE_DEG: u32 = 45;

/// 动画帧预算（ms 整数口径——80fps 达标线 12.5ms 取 12）。
pub const ANIM_FRAME_BUDGET_MS: u64 = 12;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 回收站条目（元数据伴生——还原保真的实体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashItem {
    pub id: u64,
    pub name: String,
    /// 原路径（>260 字符截断——截断策略：保留头部路径+尾部文件名）。
    pub orig_path: String,
    /// 删除时间（秒戳——排序用）。
    pub deleted_s: u64,
    /// 原大小（字节——容量环账）。
    pub size: u64,
    /// 内容哈希（零数据损失判据的对拍锚）。
    pub hash: u64,
    /// 所在卷（跨盘 → 各盘独立回收站，还原回原盘）。
    pub volume: String,
    /// 暂存区标记（清空撤销窗内——真清空前的暂存态）。
    pub staged_purge: bool,
}

impl TrashItem {
    /// 原路径截断策略：>260 字符保留头 240 + 「…」+ 尾 19（含文件名）。
    pub fn clamp_path(path: &str) -> String {
        let n = path.chars().count();
        if n <= ORIG_PATH_CAP {
            return String::from(path);
        }
        let head: String = path.chars().take(240).collect();
        let tail: String = path.chars().skip(n - 19).collect();
        format!("{}…{}", head, tail)
    }
}

/// 回收站已满三选。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullChoice {
    /// 清空旧件（按删除时间最旧先清）。
    ClearOldest,
    /// 直接永久删（需二次确认）。
    Permanent,
    /// 取消。
    Cancel,
}

/// 每盘独立回收站（跨盘删除 → 还原回原盘）。
pub struct VolumeTrash {
    pub volume: String,
    /// 容量上限（字节；缺省分区 10%——容量由上层注入）。
    pub cap: u64,
    items: Vec<TrashItem>,
}

impl VolumeTrash {
    fn new(volume: &str, cap: u64) -> VolumeTrash {
        VolumeTrash {
            volume: String::from(volume),
            cap,
            items: Vec::new(),
        }
    }

    fn used(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }

    /// 容量水位（%，0..100；cap 0 视为未设 → 0）。
    fn watermark(&self) -> u8 {
        if self.cap == 0 {
            return 0;
        }
        ((self.used() * 100) / self.cap).min(100) as u8
    }
}

// ---------------------------------------------------------------------------
// 回收站体验管理器
// ---------------------------------------------------------------------------

/// 回收站体验化（多卷聚合面）。
pub struct TrashUi {
    volumes: Vec<VolumeTrash>,
    next_id: u64,
    now_s: u64,
    now_ms: u64,
    /// 删除动画账：当前批 [(件 id, 错峰延迟 ms)]——批内第 N 件 N×30ms。
    pub fly_anims: Vec<(u64, u32)>,
    /// 当前批起点（ms；批 = 间隔 ≤50ms 的连续删除）。
    fly_batch_start: u64,
    last_delete_ms: u64,
    /// 动画帧账（80fps 对账）。
    last_frame_ms: Option<u64>,
    pub frame_violations: u64,
    /// 清空撤销暂存（真清空在撤销窗后）。
    pending_empty: Option<(Vec<TrashItem>, u64)>,
    /// toast 队列。
    toasts: Vec<String>,
    /// 永久删二次确认账（未确认不得真删——红线）。
    permanent_armed: bool,
    /// 还原重命名账（目标被占 → 「名称 (2)」还原）。
    pub renamed_restores: u64,
    /// 容量环着色令牌账。
    ring_token: Token,
}

impl TrashUi {
    pub fn new() -> TrashUi {
        TrashUi {
            volumes: Vec::new(),
            next_id: 1,
            now_s: 0,
            now_ms: 0,
            fly_anims: Vec::new(),
            fly_batch_start: 0,
            last_delete_ms: 0,
            last_frame_ms: None,
            frame_violations: 0,
            pending_empty: None,
            toasts: Vec::new(),
            permanent_armed: false,
            renamed_restores: 0,
            ring_token: Token::On,
        }
    }

    /// 注册卷（容量上限注入；缺省分区 10% 由上层折算）。
    pub fn register_volume(&mut self, volume: &str, cap: u64) {
        if !self.volumes.iter().any(|v| v.volume == volume) {
            self.volumes.push(VolumeTrash::new(volume, cap));
        }
    }

    fn volume_mut(&mut self, volume: &str) -> Option<&mut VolumeTrash> {
        self.volumes.iter_mut().find(|v| v.volume == volume)
    }

    /// 删除入站（零真删红线：数据整体搬入账本——哈希伴生保真）。
    /// 多选批量错峰动画账同步登记（批 = 间隔 ≤50ms 的连续删除）。
    /// 返回入站件 id（容量满 → None + 「回收站已满」三选 toast）。
    pub fn delete(
        &mut self,
        volume: &str,
        name: &str,
        orig_path: &str,
        size: u64,
        hash: u64,
        now_s: u64,
        now_ms: u64,
    ) -> Option<u64> {
        self.now_s = now_s;
        self.now_ms = now_ms;
        // 卷存在性 + 容量预检（借用即取即放——批账另起借段）。
        let fits = {
            let v = self.volume_mut(volume)?;
            v.used() + size <= v.cap
        };
        if !fits {
            self.toasts
                .push(String::from("回收站已满：清空旧件 / 直接永久删 / 取消"));
            return None;
        }
        // 批账：间隔 ≤50ms 归同批（错峰从批首起算），否则开新批。
        if now_ms.saturating_sub(self.last_delete_ms) > 50 || self.fly_anims.is_empty() {
            self.fly_anims.clear();
            self.fly_batch_start = now_ms;
        }
        self.last_delete_ms = now_ms;
        let id = self.next_id;
        self.next_id += 1;
        let item = TrashItem {
            id,
            name: String::from(name),
            orig_path: TrashItem::clamp_path(orig_path),
            deleted_s: now_s,
            size,
            hash,
            volume: String::from(volume),
            staged_purge: false,
        };
        if let Some(v) = self.volume_mut(volume) {
            v.items.push(item);
        }
        // 错峰动画账（批内第 N 件延迟 N×30ms）。
        let n = self.fly_anims.len() as u32;
        self.fly_anims.push((id, n * STAGGER_MS));
        // 水位提醒。
        self.refresh_ring(volume);
        Some(id)
    }

    /// 容量环刷新（>80% 转 Warn 令牌 + toast）。
    pub fn refresh_ring(&mut self, volume: &str) -> u8 {
        let wm = self
            .volumes
            .iter()
            .find(|v| v.volume == volume)
            .map(|v| v.watermark())
            .unwrap_or(0);
        if wm > WATERMARK_PCT {
            if self.ring_token != Token::Warn {
                self.ring_token = Token::Warn;
                self.toasts.push(String::from("回收站容量超过 80%，建议清理"));
            }
        } else {
            self.ring_token = Token::On;
        }
        wm
    }

    pub fn ring_color(&self) -> Token {
        self.ring_token
    }

    /// 容量环与实际占用一致判据的对账口。
    pub fn ring_usage_bytes(&self, volume: &str) -> u64 {
        self.volumes
            .iter()
            .find(|v| v.volume == volume)
            .map(|v| v.used())
            .unwrap_or(0)
    }

    /// 回收站角标件数。
    pub fn badge_count(&self, volume: &str) -> usize {
        self.volumes
            .iter()
            .find(|v| v.volume == volume)
            .map(|v| v.items.iter().filter(|i| !i.staged_purge).count())
            .unwrap_or(0)
    }

    /// 还原（零数据损失：哈希对拍；目标被占 → 重命名还原）。
    pub fn restore(&mut self, id: u64, target_occupied: bool) -> Option<(String, u64)> {
        let (idx, vol) = self
            .volumes
            .iter()
            .enumerate()
            .find_map(|(vi, v)| v.items.iter().position(|i| i.id == id).map(|pi| (pi, vi)))?;
        let item = self.volumes[vol].items.remove(idx);
        let name = if target_occupied {
            self.renamed_restores += 1;
            // F087 后缀语义：「名称 (2).ext」。
            let dot = item.name.rfind('.');
            match dot {
                Some(p) => format!("{} (2){}", &item.name[..p], &item.name[p..]),
                None => format!("{} (2)", item.name),
            }
        } else {
            item.name.clone()
        };
        Some((name, item.hash))
    }

    /// 清空（二次确认后进入撤销窗——暂存区标记，真清空在撤销窗后）。
    pub fn empty_volume(&mut self, volume: &str, confirmed: bool, now_ms: u64) -> bool {
        if !confirmed {
            self.toasts.push(String::from("确认清空回收站？（二次确认）"));
            return false;
        }
        let vi = self.volumes.iter().position(|v| v.volume == volume);
        let Some(vi) = vi else { return false };
        let taken: Vec<TrashItem> = self
            .volumes[vi]
            .items
            .iter_mut()
            .map(|i| {
                i.staged_purge = true;
                i.clone()
            })
            .collect();
        self.pending_empty = Some((taken, now_ms));
        self.now_ms = now_ms;
        true
    }

    /// 撤销清空（5s 窗内；暂存 → 回活）。
    pub fn undo_empty(&mut self, now_ms: u64) -> bool {
        match self.pending_empty.take() {
            Some((items, at)) if now_ms.saturating_sub(at) < EMPTY_UNDO_MS => {
                let vol = items.first().map(|i| i.volume.clone());
                if let Some(v) = vol {
                    if let Some(vt) = self.volume_mut(&v) {
                        for mut it in items {
                            it.staged_purge = false;
                            vt.items.push(it);
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// 撤销窗到期驱动（真清空落账——暂存条目移除）。
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if let Some((items, at)) = &self.pending_empty {
            if now_ms.saturating_sub(*at) >= EMPTY_UNDO_MS {
                let vol = items.first().map(|i| i.volume.clone());
                if let Some(v) = vol {
                    if let Some(vt) = self.volume_mut(&v) {
                        vt.items.retain(|i| !i.staged_purge);
                    }
                }
                self.pending_empty = None;
            }
        }
    }

    /// 「回收站已满」三选执行。
    pub fn full_choice(&mut self, volume: &str, choice: FullChoice, now_ms: u64) -> bool {
        self.now_ms = now_ms; // 三选执行时刻入账（toast 时间线锚）
        match choice {
            FullChoice::Cancel => true,
            FullChoice::ClearOldest => {
                // 按删除时间最旧先清（清到水位 60% 以下）。
                let vi = self.volumes.iter().position(|v| v.volume == volume);
                if let Some(vi) = vi {
                    // 最旧 = deleted_s 最小：循环删至水位 ≤60%。
                    loop {
                        let wm = self.volumes[vi].watermark();
                        if wm <= 60 {
                            break;
                        }
                        let oldest = self.volumes[vi]
                            .items
                            .iter()
                            .min_by_key(|i| i.deleted_s)
                            .map(|i| i.id);
                        match oldest {
                            Some(id) => {
                                self.volumes[vi].items.retain(|i| i.id != id);
                            }
                            None => break,
                        }
                    }
                }
                true
            }
            FullChoice::Permanent => {
                // 永久删需二次确认（armed 账——未确认不执行）。
                if !self.permanent_armed {
                    self.toasts.push(String::from("永久删除不可恢复，再次确认"));
                    return false;
                }
                self.permanent_armed = false;
                if let Some(vt) = self.volume_mut(volume) {
                    vt.items.clear();
                }
                true
            }
        }
    }

    /// 永久删二次确认（arm）。
    pub fn arm_permanent(&mut self) {
        self.permanent_armed = true;
    }

    /// 删除动画进度（200ms 弧线+缩淡；bezier 45° 抬升的几何由渲染层
    /// 取本账 + ARC_RISE_DEG 换算；进度按批起点 + 件错峰延迟计算）。
    pub fn fly_progress(&self, id: u64) -> Option<u16> {
        let offset = self
            .fly_anims
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, d)| *d as u64)?;
        let t = self
            .now_ms
            .saturating_sub(self.fly_batch_start)
            .saturating_sub(offset);
        Some(((t * 1000 / FLY_MS as u64).min(1000)) as u16)
    }

    /// 动画帧账（80fps）。
    pub fn frame_report(&mut self, now_ms: u64) {
        if let Some(t) = self.last_frame_ms {
            if now_ms.saturating_sub(t) > ANIM_FRAME_BUDGET_MS {
                self.frame_violations += 1;
            }
        }
        self.last_frame_ms = Some(now_ms);
        self.now_ms = now_ms;
    }

    pub fn pop_toast(&mut self) -> Option<String> {
        if self.toasts.is_empty() {
            None
        } else {
            Some(self.toasts.remove(0))
        }
    }

    /// 删-还原-再删轮次对拍（100 轮零损失判据的对账入口：哈希链一致）。
    pub fn roundtrip_hash(&self, volume: &str) -> Vec<u64> {
        self.volumes
            .iter()
            .find(|v| v.volume == volume)
            .map(|v| v.items.iter().map(|i| i.hash).collect())
            .unwrap_or_default()
    }

    pub fn item_count(&self, volume: &str) -> usize {
        self.volumes
            .iter()
            .find(|v| v.volume == volume)
            .map(|v| v.items.len())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-15 验收判据）
// ---------------------------------------------------------------------------

/// F085 自检：100 轮哈希对拍、80fps 帧账、容量环一致、错峰动画、
/// 清空撤销窗、三选（永久删二次确认）、跨盘独立、重命名还原、
/// 长路径截断。
pub fn run_trashui_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F085");
    let mut t = TrashUi::new();
    t.register_volume("C:", 1_000_000);
    // 1. 删-还原-再删 100 轮零数据损失（哈希对拍）。
    let mut ok100 = true;
    for i in 0..100u64 {
        let hash = 0x9E37_79B9 ^ i;
        let id = t.delete("C:", "文件", format!("C:/Users/文件{}", i).as_str(), 100, hash, i, i * 100);
        ok100 &= id.is_some();
        let restored = id.and_then(|id| t.restore(id, false));
        ok100 &= restored.map(|(_, h)| h == hash) == Some(true);
        let id2 = t.delete("C:", "文件", format!("C:/Users/文件{}", i).as_str(), 100, hash, i, i * 100 + 1);
        ok100 &= id2.is_some();
    }
    set.add(
        "roundtrip-100",
        ok100 && t.roundtrip_hash("C:").len() == 100,
        "100 cycles zero loss",
    );
    // 2. 80fps 帧账。
    t.frame_report(0);
    t.frame_report(12);
    t.frame_report(25); // 13ms → 违规
    set.add("anim-80fps", t.frame_violations == 1, "12ms budget");
    // 3. 容量环一致：占用量 = 环账；>80% 转 Warn + toast。
    let used = t.ring_usage_bytes("C:");
    let wm = t.refresh_ring("C:");
    set.add(
        "ring-consistent",
        used == 10_000 && wm == 1 && t.ring_color() == Token::On, // 100 件 × 100B
        "usage matches ring",
    );
    // 4. 多选错峰：3 件依次 30ms。
    let mut t2 = TrashUi::new();
    t2.register_volume("D:", 10_000);
    for i in 0..3u64 {
        let _ = t2.delete("D:", "件", format!("D:/件{}", i).as_str(), 10, i, 0, 0);
    }
    let stag = t2.fly_anims.iter().map(|(_, d)| *d).collect::<Vec<_>>();
    set.add(
        "stagger-30ms",
        stag == vec![0, 30, 60] && t2.fly_progress(3).is_some(),
        "N×30ms",
    );
    // 5. 清空撤销窗：窗内可撤（暂存回活）、窗后真清空。
    t2.empty_volume("D:", true, 10_000);
    let staged = t2.badge_count("D:") == 0; // 暂存态角标归零
    let undone = t2.undo_empty(10_500);
    let live = t2.badge_count("D:") == 3;
    t2.empty_volume("D:", true, 20_000);
    t2.tick(20_000 + EMPTY_UNDO_MS);
    let purged = t2.item_count("D:") == 0;
    set.add(
        "empty-undo",
        staged && undone && live && purged,
        "5s staged purge",
    );
    // 6. 未确认不清空（防手滑）。
    let mut t3 = TrashUi::new();
    t3.register_volume("E:", 1_000);
    let _ = t3.delete("E:", "件", "E:/件", 10, 1, 0, 0);
    let refused = !t3.empty_volume("E:", false, 0) && t3.item_count("E:") == 1;
    set.add("empty-confirm", refused, "double confirm gate");
    // 7. 回收站已满三选：容量满拒收 + 清旧 + 永久删二次确认。
    let mut t4 = TrashUi::new();
    t4.register_volume("F:", 1_000);
    let full = t4.delete("F:", "大件", "F:/大", 1_500, 1, 0, 0).is_none();
    let _ = t4.delete("F:", "旧", "F:/旧", 500, 2, 10, 0); // 旧件（50%）
    let _ = t4.delete("F:", "新", "F:/新", 400, 3, 20, 0); // 90% 超水位
    let cleared = t4.full_choice("F:", FullChoice::ClearOldest, 0);
    let oldest_gone = t4.roundtrip_hash("F:") == vec![3]; // 旧件（hash2）先清
    let perm_refused = !t4.full_choice("F:", FullChoice::Permanent, 0);
    t4.arm_permanent();
    let perm_ok = t4.full_choice("F:", FullChoice::Permanent, 0) && t4.item_count("F:") == 0;
    set.add(
        "full-3choice",
        full && cleared && oldest_gone && perm_refused && perm_ok,
        "full dialog paths",
    );
    // 8. 跨盘独立回收站（还原回原盘）。
    let mut t5 = TrashUi::new();
    t5.register_volume("G:", 1_000);
    t5.register_volume("H:", 1_000);
    let _ = t5.delete("G:", "甲", "G:/甲", 10, 7, 0, 0);
    let _ = t5.delete("H:", "乙", "H:/乙", 10, 8, 0, 0);
    let g_count = t5.item_count("G:");
    let h_count = t5.item_count("H:");
    let cross = g_count == 1 && h_count == 1 && t5.restore(2, false).unwrap().1 == 8;
    set.add("per-volume", cross, "independent bins");
    // 9. 还原目标被占 → 重命名还原（F087 语义）。
    let mut t6 = TrashUi::new();
    t6.register_volume("I:", 1_000);
    let _ = t6.delete("I:", "合同.pdf", "I:/合同.pdf", 10, 9, 0, 0);
    let (name, _) = t6.restore(1, true).unwrap();
    set.add(
        "rename-restore",
        name == "合同 (2).pdf" && t6.renamed_restores == 1,
        "F087 suffix",
    );
    // 10. 长路径截断（>260 字符策略）。
    let long = format!("I:/{}", "段".repeat(300));
    let clipped = TrashItem::clamp_path(&long);
    set.add(
        "path-truncate",
        clipped.chars().count() == 240 + 1 + 19 && clipped.contains('…'),
        "260 cap policy",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_counts_non_staged_only() {
        let mut t = TrashUi::new();
        t.register_volume("C:", 1_000);
        let _ = t.delete("C:", "a", "C:/a", 10, 1, 0, 0);
        let _ = t.delete("C:", "b", "C:/b", 10, 2, 0, 0);
        assert_eq!(t.badge_count("C:"), 2, "角标显示件数");
        t.empty_volume("C:", true, 0);
        assert_eq!(t.badge_count("C:"), 0, "暂存态角标归零");
        assert_eq!(t.item_count("C:"), 2, "暂存未真删");
    }

    #[test]
    fn watermark_toast_once() {
        let mut t = TrashUi::new();
        t.register_volume("C:", 100);
        let _ = t.delete("C:", "a", "C:/a", 85, 1, 0, 0); // 85% > 80%
        let has_toast = t.pop_toast().is_some();
        t.refresh_ring("C:");
        let toast2 = t.pop_toast().is_some();
        assert!(has_toast, "超水位首提醒");
        assert!(!toast2, "同水位不重复骚扰");
    }

    #[test]
    fn delete_to_full_capacity_then_dialog() {
        let mut t = TrashUi::new();
        t.register_volume("C:", 100);
        assert!(t.delete("C:", "a", "C:/a", 60, 1, 0, 0).is_some());
        assert!(t.delete("C:", "b", "C:/b", 60, 2, 0, 0).is_none(), "超容量拒收");
        assert!(t.pop_toast().unwrap().contains("已满"));
    }

    #[test]
    fn restore_pops_from_bin() {
        let mut t = TrashUi::new();
        t.register_volume("C:", 1_000);
        let _ = t.delete("C:", "x", "C:/x", 10, 42, 0, 0);
        let (name, hash) = t.restore(1, false).unwrap();
        assert_eq!(name, "x");
        assert_eq!(hash, 42);
        assert_eq!(t.item_count("C:"), 0);
        assert!(t.restore(1, false).is_none(), "重复还原拒绝");
    }

    #[test]
    fn trashui_self_checks_all_green() {
        let set = run_trashui_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F085 自检红项：{}/{} 绿", p, p + f);
    }
}
