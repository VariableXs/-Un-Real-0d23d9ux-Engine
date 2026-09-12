//! m6sync — VARIX-M600 AI-21 数据流动同步域 (F501~F525)
//!
//! 多端同步的总引擎：E2E 加密、冲突合并、设备接力、通用剪贴板、书签/密码桥、
//! 块级增量、流量预算、离线队列、变更日志、云适配器、迁移导出、备份恢复、
//! 一致性巡检、可视化、带宽礼貌、血缘账本、安全审计、版本分叉、回归走廊、
//! 测试舰队、年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille / 整数计数）。

use crate::checks::CheckSet;

// ===========================================================================
// F501 — 同步引擎中枢：多端数据的总调度
// ===========================================================================

pub const PEERS_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq)]
pub enum PeerState {
    Online,
    Offline,
    Syncing,
    Error,
}

#[derive(Clone, Copy)]
pub struct Peer {
    pub id: u16,
    pub clock_permille: u64, // 对端逻辑时钟
    pub state: PeerState,
}

/// 中枢：只对 Online/正在同步的对端派发任务。
pub fn dispatchable_peers(peers: &[Peer], n: usize) -> usize {
    let n = n.min(PEERS_CAP);
    peers[..n]
        .iter()
        .filter(|p| matches!(p.state, PeerState::Online | PeerState::Syncing))
        .count()
}

// ===========================================================================
// F502 — 端到端加密同步：密钥不出设备
// ===========================================================================

pub const E2E_KEY_LEN: usize = 32;

/// 简化 XTEA 风格轮函数做完整性演示：密钥参与运算，密文与明文长度一致。
pub fn e2e_mix(word: u32, key: u32, rounds: u8) -> u32 {
    let mut v = word;
    let mut k = key;
    for _ in 0..rounds {
        v = v.wrapping_add(k).rotate_left(7);
        k = k.rotate_left(3).wrapping_add(0x9E37_79B9);
    }
    v
}

pub fn e2e_blob_len_ok(plain_len: usize) -> bool {
    plain_len <= 4096 // 单条同步载荷上限
}

// ===========================================================================
// F503 — 冲突优雅合并：文档级冲突建议
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
#[derive(Debug)]
pub enum MergeAdvice {
    FastForward,   // 单边修改，直接前进
    AutoMerge,     // 不同段落，自动合并
    ThreeWay,      // 同段落但可三方合并
    ManualReview,  // 需要人工裁决
}

pub fn merge_advice(local_changed: bool, remote_changed: bool, same_hunk: bool, both_trivial: bool) -> MergeAdvice {
    match (local_changed, remote_changed) {
        (false, false) => MergeAdvice::FastForward,
        (true, false) | (false, true) => MergeAdvice::FastForward,
        (true, true) if !same_hunk => MergeAdvice::AutoMerge,
        (true, true) if both_trivial => MergeAdvice::ThreeWay,
        _ => MergeAdvice::ManualReview,
    }
}

// ===========================================================================
// F504 — 设备间接力：任务无缝接力
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Handoff {
    pub task_id: u16,
    pub from_dev: u16,
    pub to_dev: u16,
    pub progress_permille: u16,
}

pub fn handoff_usable(h: &Handoff) -> bool {
    h.from_dev != h.to_dev && h.progress_permille <= 1000
}

/// 接力后新设备从保存的进度继续。
pub fn handoff_resume_permille(h: &Handoff) -> u16 {
    if handoff_usable(h) { h.progress_permille } else { 0 }
}

// ===========================================================================
// F505 — 通用剪贴板协议
// ===========================================================================

pub const CLIP_MAGIC: u32 = 0x5658_434C; // "VXCL"
pub const CLIP_MIME_CAP: usize = 4;

#[derive(Clone, Copy)]
pub struct ClipPacket {
    pub magic: u32,
    pub version: u8,
    pub mime_count: u8,
    pub payload_len: u16,
}

pub fn clip_packet_ok(p: &ClipPacket) -> bool {
    p.magic == CLIP_MAGIC
        && p.version >= 1
        && p.version <= 2
        && (p.mime_count as usize) <= CLIP_MIME_CAP
        && p.payload_len <= 4096
}

// ===========================================================================
// F506 — 浏览器书签桥
// ===========================================================================

pub const BOOKMARK_KINDS: [&str; 4] = ["url", "folder", "separator", "alias"];

/// 书签去重：同 URL 视为同一书签。
pub fn bookmark_dedup(urls: &mut [&str], n: usize) -> usize {
    let n = n.min(urls.len());
    let mut kept = 0usize;
    for i in 0..n {
        let mut dup = false;
        for j in 0..i {
            if urls[j] == urls[i] {
                dup = true;
                break;
            }
        }
        if !dup {
            urls.swap(kept, i);
            kept += 1;
        }
    }
    kept
}

// ===========================================================================
// F507 — 密码跨平台：条目格式互通
// ===========================================================================

pub const VAULT_ENTRY_VER: u8 = 3;

pub fn vault_entry_convertible(ver: u8) -> bool {
    ver >= 1 && ver <= VAULT_ENTRY_VER
}

pub fn vault_entry_len_ok(title_len: usize, secret_len: usize) -> bool {
    title_len > 0 && title_len <= 64 && secret_len > 0 && secret_len <= 512
}

// ===========================================================================
// F508 — 文件增量同步：块级哈希增量
// ===========================================================================

pub const CHUNK_SIZE: u64 = 4096;

/// FNV-1a 块指纹。
pub fn chunk_fingerprint(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBD1_9E04;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 旧文件块指纹表还在新文件中出现的比例（permille）。
pub fn chunk_hit_permille(old: &[u64], new: &[u64]) -> u16 {
    if old.is_empty() {
        return 0;
    }
    let mut hits = 0u32;
    for &n in new {
        for &o in old {
            if o == n {
                hits += 1;
                break;
            }
        }
    }
    ((hits as u32 * 1000) / new.len().max(1) as u32) as u16
}

// ===========================================================================
// F509 — 同步预算器：流量月度预算
// ===========================================================================

pub struct SyncBudget {
    pub monthly_kb: u32,
    pub used_kb: u32,
}

impl SyncBudget {
    pub fn admit(&mut self, want_kb: u32) -> bool {
        let room = self.monthly_kb.saturating_sub(self.used_kb);
        if want_kb <= room {
            self.used_kb += want_kb;
            true
        } else {
            false
        }
    }
    pub fn remaining_kb(&self) -> u32 {
        self.monthly_kb.saturating_sub(self.used_kb)
    }
}

// ===========================================================================
// F510 — 离线队列：断网改动排队补同步
// ===========================================================================

pub const OFFQ_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct OfflineOp {
    pub seq: u64,
    pub kind: u8, // 0=put 1=delete 2=mkdir
    pub key: u32,
}

#[derive(Clone, Copy)]
pub struct OfflineQueue {
    pub head: usize,
    pub len: usize,
}

pub fn offq_push(q: &mut OfflineQueue) -> bool {
    if q.len >= OFFQ_CAP {
        return false;
    }
    q.len += 1;
    true
}

pub fn offq_drain_count(q: &mut OfflineQueue, batch: usize) -> usize {
    let n = batch.min(q.len);
    q.head = (q.head + n) % OFFQ_CAP;
    q.len -= n;
    n
}

// ===========================================================================
// F511 — 变更日志公开：可读的同步日志
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ChangeRec {
    pub seq: u64,
    pub key: u32,
    pub op: u8,
    pub peer: u16,
}

pub fn change_log_line(rec: &ChangeRec, buf: &mut [u8]) -> usize {
    // 极简十进制序列化：seq key op peer
    let mut w = 0usize;
    let push = |v: u64, buf: &mut [u8], w: &mut usize| {
        let mut tmp = [0u8; 20];
        let mut i = 0;
        if v == 0 {
            tmp[0] = b'0';
            i = 1;
        }
        let mut v = v;
        while v > 0 {
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
        while i > 0 {
            i -= 1;
            if *w < buf.len() {
                buf[*w] = tmp[i];
                *w += 1;
            }
        }
    };
    push(rec.seq, buf, &mut w);
    if w < buf.len() { buf[w] = b' '; w += 1; }
    push(rec.key as u64, buf, &mut w);
    if w < buf.len() { buf[w] = b' '; w += 1; }
    push(rec.op as u64, buf, &mut w);
    if w < buf.len() { buf[w] = b' '; w += 1; }
    push(rec.peer as u64, buf, &mut w);
    w
}

// ===========================================================================
// F512 — 第三方云适配器：统一适配器接口
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum CloudCap {
    Upload,
    Download,
    Delete,
    Versioning,
    Trash,
}

pub fn cloud_adapter_compatible(caps: &[CloudCap; 5]) -> bool {
    // 最低要求：上传+下载
    caps.contains(&CloudCap::Upload) && caps.contains(&CloudCap::Download)
}

// ===========================================================================
// F513 — 导入迁移官：整体迁移进度
// ===========================================================================

pub struct Migration {
    pub total_items: u32,
    pub done_items: u32,
    pub failed_items: u32,
}

impl Migration {
    pub fn progress_permille(&self) -> u16 {
        if self.total_items == 0 {
            return 1000;
        }
        ((self.done_items as u32 * 1000) / self.total_items) as u16
    }
    pub fn finished(&self) -> bool {
        self.done_items + self.failed_items >= self.total_items
    }
    pub fn healthy(&self) -> bool {
        self.finished() && self.failed_items * 100 <= self.total_items * 10
    }
}

// ===========================================================================
// F514 — 导出全家桶：导出清单完整性
// ===========================================================================

pub const EXPORT_KINDS: [&str; 6] = ["docs", "photos", "settings", "passwords", "bookmarks", "apps"];

pub fn export_manifest_complete(marks: u8) -> bool {
    marks == (1 << EXPORT_KINDS.len()) - 1
}

// ===========================================================================
// F515 — 备份时间机器：时间点恢复
// ===========================================================================

pub const SNAP_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub epoch_s: u64,
    pub size_kb: u32,
    pub kind: u8, // 0=full 1=incremental
}

/// 找到不晚于目标时间的最近快照。
pub fn snapshot_for_restore(snaps: &[Snapshot], n: usize, target_epoch: u64) -> Option<usize> {
    let n = n.min(SNAP_CAP);
    let mut best: Option<usize> = None;
    for i in 0..n {
        if snaps[i].epoch_s <= target_epoch {
            match best {
                None => best = Some(i),
                Some(b) if snaps[i].epoch_s > snaps[b].epoch_s => best = Some(i),
                _ => {}
            }
        }
    }
    best
}

// ===========================================================================
// F516 — 恢复演练：备份可恢复性定期验证
// ===========================================================================

pub const DRILL_INTERVAL_DAYS: u32 = 14;

pub fn drill_due(last_epoch_day: u32, now_epoch_day: u32) -> bool {
    now_epoch_day.saturating_sub(last_epoch_day) >= DRILL_INTERVAL_DAYS
}

pub fn drill_pass(restored_bytes: u32, original_bytes: u32) -> bool {
    original_bytes > 0 && restored_bytes == original_bytes
}

// ===========================================================================
// F517 — 多设备一致性检查
// ===========================================================================

/// 各设备版本号全部一致 = 一致；否则返回不一致设备数。
pub fn consistency_check(versions: &[u64], n: usize) -> u32 {
    let n = n.min(versions.len());
    if n == 0 {
        return 0;
    }
    let max = versions[..n].iter().copied().max().unwrap_or(0);
    versions[..n].iter().filter(|&&v| v != max).count() as u32
}

// ===========================================================================
// F518 — 同步状态可视化：进度条 permille
// ===========================================================================

pub struct SyncGauge {
    pub total_kb: u32,
    pub done_kb: u32,
    pub pending_ops: u32,
}

impl SyncGauge {
    pub fn permille(&self) -> u16 {
        if self.total_kb == 0 {
            return 1000;
        }
        ((self.done_kb as u32 * 1000) / self.total_kb) as u16
    }
    pub fn done(&self) -> bool {
        self.done_kb >= self.total_kb && self.pending_ops == 0
    }
}

// ===========================================================================
// F519 — 带宽礼貌协议：不挤占交互流量
// ===========================================================================

pub const POLITE_BG_CAP_PERMILLE: u16 = 300; // 后台同步最多占总带宽 30%

pub fn polite_admit(bg_kbps: u32, total_kbps: u32) -> bool {
    if total_kbps == 0 {
        return false;
    }
    (bg_kbps as u64 * 1000) <= (total_kbps as u64 * POLITE_BG_CAP_PERMILLE as u64)
}

// ===========================================================================
// F520 — 数据血缘账本：来源与去向
// ===========================================================================

pub const LINEAGE_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct LineageHop {
    pub data_id: u32,
    pub from: u16, // 0=本机
    pub to: u16,
    pub op: u8,    // 0=copy 1=derive 2=share
}

pub fn lineage_trace(hops: &[LineageHop], n: usize, data_id: u32) -> usize {
    let n = n.min(LINEAGE_CAP);
    hops[..n].iter().filter(|h| h.data_id == data_id).count()
}

pub fn lineage_legit(h: &LineageHop) -> bool {
    h.from != h.to || h.op == 1 // derive 可以在本机
}

// ===========================================================================
// F521 — 同步安全审计
// ===========================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum SyncAudit {
    Ok,
    UnencryptedChannel,
    UnknownPeer,
    OversizePayload,
}

pub fn sync_audit(encrypted: bool, peer_known: bool, payload_len: usize) -> SyncAudit {
    if !peer_known {
        SyncAudit::UnknownPeer
    } else if !encrypted {
        SyncAudit::UnencryptedChannel
    } else if payload_len > 4096 {
        SyncAudit::OversizePayload
    } else {
        SyncAudit::Ok
    }
}

// ===========================================================================
// F522 — 版本分叉管理
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Fork {
    pub base_ver: u64,
    pub left_ver: u64,
    pub right_ver: u64,
}

impl Fork {
    pub fn diverged(&self) -> bool {
        self.left_ver != self.right_ver
    }
    pub fn depth(&self) -> u64 {
        self.left_ver.saturating_sub(self.base_ver) + self.right_ver.saturating_sub(self.base_ver)
    }
}

// ===========================================================================
// F523 — 同步回归走廊
// ===========================================================================

pub const SYNC_CORRIDOR_CASES: [&str; 5] =
    ["offline-replay", "conflict-merge", "e2e-roundtrip", "budget-cap", "snapshot-restore"];

pub fn sync_corridor_pass(results: &[bool; 5]) -> bool {
    results.iter().all(|&r| r) && results.len() == SYNC_CORRIDOR_CASES.len()
}

// ===========================================================================
// F524 — 跨设备测试舰队
// ===========================================================================

#[derive(Clone, Copy)]
pub struct DeviceProfile {
    pub kind: u8, // 0=desktop 1=laptop 2=tablet 3=phone
    pub offline_ok: bool,
    pub e2e_ok: bool,
}

pub const FLEET_MIN_KINDS: usize = 3;

pub fn fleet_ready(devs: &[DeviceProfile], n: usize) -> bool {
    let n = n.min(devs.len());
    if n < FLEET_MIN_KINDS {
        return false;
    }
    let mut kinds = [0u8; 4];
    for d in &devs[..n] {
        if (d.kind as usize) < kinds.len() {
            kinds[d.kind as usize] += 1;
        }
    }
    kinds.iter().filter(|&&k| k > 0).count() >= FLEET_MIN_KINDS
        && devs[..n].iter().all(|d| d.offline_ok && d.e2e_ok)
}

// ===========================================================================
// F525 — 同步年报
// ===========================================================================

pub struct SyncYearbook {
    pub bytes_synced_kb: u32,
    pub conflicts: u32,
    pub auto_merged: u32,
    pub manual_reviews: u32,
    pub outages: u32,
}

impl SyncYearbook {
    pub fn auto_merge_rate_permille(&self) -> u16 {
        let total = self.conflicts;
        if total == 0 {
            return 1000;
        }
        ((self.auto_merged as u32 * 1000) / total) as u16
    }
    pub fn grade(&self) -> u8 {
        // 0=A 1=B 2=C
        if self.outages <= 2 && self.manual_reviews * 10 <= self.conflicts {
            0
        } else if self.outages <= 8 {
            1
        } else {
            2
        }
    }
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m6sync_checks() -> CheckSet {
    let mut set = CheckSet::new("m6sync");

    // F501 中枢
    let peers = [
        Peer { id: 1, clock_permille: 10, state: PeerState::Online },
        Peer { id: 2, clock_permille: 12, state: PeerState::Offline },
        Peer { id: 3, clock_permille: 11, state: PeerState::Syncing },
        Peer { id: 4, clock_permille: 9, state: PeerState::Error },
    ];
    set.add("F501 dispatch", dispatchable_peers(&peers, 4) == 2, "online+syncing only");

    // F502 E2E
    let a = e2e_mix(0x1234_5678, 0xDEAD_BEEF, 4);
    set.add(
        "F502 e2e mix",
        a != 0x1234_5678 && e2e_blob_len_ok(4096) && !e2e_blob_len_ok(4097),
        "keyed mix + len cap",
    );

    // F503 冲突合并
    set.add(
        "F503 merge advice",
        merge_advice(true, true, false, false) == MergeAdvice::AutoMerge
            && merge_advice(true, true, true, true) == MergeAdvice::ThreeWay
            && merge_advice(true, true, true, false) == MergeAdvice::ManualReview
            && merge_advice(false, true, false, false) == MergeAdvice::FastForward,
        "4-level advice",
    );

    // F504 接力
    let h = Handoff { task_id: 7, from_dev: 1, to_dev: 2, progress_permille: 420 };
    set.add(
        "F504 handoff",
        handoff_usable(&h) && handoff_resume_permille(&h) == 420,
        "resume from progress",
    );

    // F505 剪贴板
    let cp = ClipPacket { magic: CLIP_MAGIC, version: 2, mime_count: 3, payload_len: 100 };
    let bad = ClipPacket { magic: 1, version: 2, mime_count: 3, payload_len: 100 };
    set.add("F505 clip protocol", clip_packet_ok(&cp) && !clip_packet_ok(&bad), "magic+ver+cap");

    // F506 书签
    let mut urls = ["a", "b", "a", "c", "b", "d"];
    let kept = bookmark_dedup(&mut urls, 6);
    set.add("F506 bookmark dedup", kept == 4, "unique urls");

    // F507 密码
    set.add(
        "F507 vault",
        vault_entry_convertible(2) && !vault_entry_convertible(4) && vault_entry_len_ok(8, 32)
            && !vault_entry_len_ok(0, 32),
        "ver+len gates",
    );

    // F508 增量
    let d1 = [1u8, 2, 3, 4];
    let d2 = [1u8, 2, 3, 5];
    let fp1 = chunk_fingerprint(&d1);
    let fp2 = chunk_fingerprint(&d2);
    let hits = chunk_hit_permille(&[fp1, fp2], &[fp1, fp2, fp1]);
    set.add(
        "F508 chunk delta",
        fp1 != fp2 && hits == 1000,
        "fnv fingerprint + hit rate",
    );

    // F509 预算
    let mut b = SyncBudget { monthly_kb: 1000, used_kb: 900 };
    set.add(
        "F509 budget",
        !b.admit(200) && b.admit(100) && b.remaining_kb() == 0 && !b.admit(1),
        "cap enforced",
    );

    // F510 离线队列
    let mut q = OfflineQueue { head: 0, len: 0 };
    for _ in 0..OFFQ_CAP {
        offq_push(&mut q);
    }
    let full = !offq_push(&mut q);
    let drained = offq_drain_count(&mut q, 20);
    set.add("F510 offline queue", full && drained == OFFQ_CAP && q.len == 0, "bounded drain");

    // F511 变更日志
    let mut buf = [0u8; 64];
    let w = change_log_line(&ChangeRec { seq: 42, key: 7, op: 1, peer: 3 }, &mut buf);
    set.add(
        "F511 change log",
        w > 6 && &buf[..6] == b"42 7 1" ,
        "readable line",
    );

    // F512 云适配器
    let caps = [CloudCap::Upload, CloudCap::Download, CloudCap::Trash, CloudCap::Delete, CloudCap::Versioning];
    let no_dl = [CloudCap::Upload, CloudCap::Delete, CloudCap::Trash, CloudCap::Versioning, CloudCap::Delete];
    set.add(
        "F512 cloud adapter",
        cloud_adapter_compatible(&caps) && !cloud_adapter_compatible(&no_dl),
        "min caps",
    );

    // F513 迁移
    let m = Migration { total_items: 100, done_items: 60, failed_items: 5 };
    let mf = Migration { total_items: 100, done_items: 97, failed_items: 3 };
    set.add(
        "F513 migration",
        m.progress_permille() == 600 && !m.finished() && mf.finished() && mf.healthy() && !m.healthy(),
        "progress + health",
    );

    // F514 导出
    set.add(
        "F514 export",
        export_manifest_complete(0b11_1111) && !export_manifest_complete(0b11_1110),
        "all kinds",
    );

    // F515 备份
    let snaps = [
        Snapshot { epoch_s: 100, size_kb: 10, kind: 0 },
        Snapshot { epoch_s: 250, size_kb: 5, kind: 1 },
        Snapshot { epoch_s: 400, size_kb: 5, kind: 1 },
    ];
    let pick = snapshot_for_restore(&snaps, 3, 300);
    set.add("F515 time machine", pick == Some(1) && snapshot_for_restore(&snaps, 3, 50).is_none(), "nearest<=target");

    // F516 演练
    set.add(
        "F516 drill",
        drill_due(0, 14) && !drill_due(1, 14) && drill_pass(1024, 1024) && !drill_pass(1024, 2048),
        "biweekly + byte equal",
    );

    // F517 一致性
    set.add(
        "F517 consistency",
        consistency_check(&[5, 5, 5], 3) == 0 && consistency_check(&[5, 6, 5], 3) == 2,
        "lagging peers counted",
    );

    // F518 仪表
    let g = SyncGauge { total_kb: 800, done_kb: 200, pending_ops: 3 };
    set.add("F518 sync gauge", g.permille() == 250 && !g.done(), "progress");

    // F519 礼貌
    set.add(
        "F519 polite bw",
        polite_admit(300, 1000) && !polite_admit(301, 1000),
        "30% cap",
    );

    // F520 血缘
    let hops = [
        LineageHop { data_id: 1, from: 0, to: 2, op: 0 },
        LineageHop { data_id: 1, from: 2, to: 3, op: 2 },
        LineageHop { data_id: 2, from: 0, to: 4, op: 1 },
    ];
    set.add(
        "F520 lineage",
        lineage_trace(&hops, 3, 1) == 2 && lineage_legit(&hops[2]) && !lineage_legit(&LineageHop { data_id: 9, from: 5, to: 5, op: 0 }),
        "trace + legit",
    );

    // F521 审计
    set.add(
        "F521 sync audit",
        sync_audit(true, true, 100) == SyncAudit::Ok
            && sync_audit(false, true, 100) == SyncAudit::UnencryptedChannel
            && sync_audit(true, false, 100) == SyncAudit::UnknownPeer
            && sync_audit(true, true, 8192) == SyncAudit::OversizePayload,
        "4 verdicts",
    );

    // F522 分叉
    let f = Fork { base_ver: 10, left_ver: 13, right_ver: 11 };
    set.add(
        "F522 fork",
        f.diverged() && f.depth() == 4,
        "depth = left+right over base",
    );

    // F523 回归走廊
    set.add(
        "F523 corridor",
        sync_corridor_pass(&[true; 5]) && !sync_corridor_pass(&[true, true, true, true, false]),
        "5 cases",
    );

    // F524 舰队
    let fleet = [
        DeviceProfile { kind: 0, offline_ok: true, e2e_ok: true },
        DeviceProfile { kind: 1, offline_ok: true, e2e_ok: true },
        DeviceProfile { kind: 3, offline_ok: true, e2e_ok: true },
    ];
    let small = [
        DeviceProfile { kind: 0, offline_ok: true, e2e_ok: true },
        DeviceProfile { kind: 1, offline_ok: true, e2e_ok: true },
    ];
    set.add("F524 fleet", fleet_ready(&fleet, 3) && !fleet_ready(&small, 2), ">=3 kinds");

    // F525 年报
    let yb = SyncYearbook { bytes_synced_kb: 500_000, conflicts: 100, auto_merged: 90, manual_reviews: 10, outages: 1 };
    set.add(
        "F525 yearbook",
        yb.auto_merge_rate_permille() == 900 && yb.grade() == 0,
        "rate + grade",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f503_merge_matrix() {
        assert_eq!(merge_advice(true, false, true, false), MergeAdvice::FastForward);
        assert_eq!(merge_advice(true, true, true, false), MergeAdvice::ManualReview);
    }

    #[test]
    fn f508_fingerprint_stable() {
        let d = [9u8; 64];
        assert_eq!(chunk_fingerprint(&d), chunk_fingerprint(&d));
        assert_ne!(chunk_fingerprint(&d), chunk_fingerprint(&[9u8; 63]));
    }

    #[test]
    fn f510_queue_roundtrip() {
        let mut q = OfflineQueue { head: 0, len: 0 };
        assert!(offq_push(&mut q));
        assert_eq!(offq_drain_count(&mut q, 1), 1);
        assert_eq!(q.len, 0);
    }

    #[test]
    fn f515_restore_picks_latest_before_target() {
        let snaps = [
            Snapshot { epoch_s: 10, size_kb: 1, kind: 0 },
            Snapshot { epoch_s: 20, size_kb: 1, kind: 1 },
        ];
        assert_eq!(snapshot_for_restore(&snaps, 2, 25), Some(1));
        assert_eq!(snapshot_for_restore(&snaps, 2, 15), Some(0));
    }

    #[test]
    fn f525_domain_selfcheck_all_pass() {
        let set = run_m6sync_checks();
        assert!(set.len() >= 25, "got {}", set.len());
                    for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
                        for i in 0..set.len() {
                let c = set.get(i).unwrap();
                if !c.passed {
                    eprintln!("CHKFAIL {} | {}", c.name, c.detail);
                }
            }
            assert!(set.all_passed());
    }
}
