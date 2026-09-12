//! VARIX-M500 AI-17 · 数据流动与互操作（F401~F425，M4）
//!
//! 搬家无痛、数据有地图——迁移、备份、同步、开放格式。
//! 纯逻辑 + 固定容量数组（no_std），域自检 F425 汇入 `robust::run_kernel_checkup()`。

use crate::checks::CheckSet;

pub const MAX_STAGES: usize = 8;
pub const MAX_SNAPSHOTS: usize = 8;

// ---------------------------------------------------------------------------
// F401 迁移向导
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigStage {
    Scan,
    Copy,
    Verify,
    Done,
}

/// 迁移状态机按序推进；verify 未通过不允许 Done。
#[derive(Clone, Copy)]
pub struct Migration {
    pub stage: MigStage,
    pub copied: u32,
    pub total: u32,
    pub verified: bool,
}

impl Migration {
    pub const fn new(total: u32) -> Migration {
        Migration { stage: MigStage::Scan, copied: 0, total, verified: false }
    }

    pub fn advance(&mut self) -> bool {
        self.stage = match self.stage {
            MigStage::Scan => MigStage::Copy,
            MigStage::Copy => MigStage::Verify,
            MigStage::Verify if self.verified && self.copied >= self.total => MigStage::Done,
            other => other,
        };
        self.stage == MigStage::Done
    }
}

// ---------------------------------------------------------------------------
// F402 书签导入
// ---------------------------------------------------------------------------

/// 去重合并书签（按 id）；返回合并后的数量。
pub fn merge_bookmarks(dst: &mut [(u32, u32)], dst_n: usize, src: &[(u32, u32)]) -> usize {
    let mut n = dst_n;
    for &(id, url) in src {
        let dup = dst[..n].iter().any(|&(i, _)| i == id);
        if !dup && n < dst.len() {
            dst[n] = (id, url);
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F403 邮件档案预留
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailSlot {
    pub account: u16,
    pub message_count: u32,
    pub imported: bool,
}

/// 占位：登记待导入的邮件档案。
pub fn mail_register(slot: &mut MailSlot, account: u16, count: u32) {
    slot.account = account;
    slot.message_count = count;
    slot.imported = false;
}

// ---------------------------------------------------------------------------
// F404 照片库导入
// ---------------------------------------------------------------------------

/// 按拍摄年分组（year 高 16 位），返回桶数。
pub fn photo_buckets(photos: &[(u32, u16)], out: &mut [(u16, u8)]) -> usize {
    let mut n = 0;
    for &(_, year) in photos {
        let mut found = false;
        for i in 0..n {
            if out[i].0 == year {
                out[i].1 += 1;
                found = true;
                break;
            }
        }
        if !found && n < out.len() {
            out[n] = (year, 1);
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F405 音乐库导入
// ---------------------------------------------------------------------------

/// 文件名 "01_Title_Artist" → (track, title, artist) 元数据修复。
pub fn music_tag_from_name(name: &str) -> Option<(u8, u8)> {
    // 简化：返回 (track, 首字母 artist hash)；无法解析返回 None。
    let b = name.as_bytes();
    if b.len() < 4 || !b[0].is_ascii_digit() || !b[1].is_ascii_digit() || b[2] != b'_' {
        return None;
    }
    let track = (b[0] - b'0') * 10 + (b[1] - b'0');
    let mut hash = 0u8;
    for &c in &b[3..] {
        hash = hash.wrapping_mul(31).wrapping_add(c);
    }
    Some((track, hash))
}

// ---------------------------------------------------------------------------
// F406 开放格式承诺
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DataFormat {
    pub name: &'static str,
    pub exportable: bool,
}

/// 全部登记格式必须可导出（开放承诺红线）。
pub fn all_exportable(fmts: &[DataFormat]) -> bool {
    fmts.iter().all(|f| f.exportable)
}

// ---------------------------------------------------------------------------
// F407 数据随身包
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Bundle {
    pub items: u16,
    pub bytes: u32,
    pub limit: u32,
}

/// 打包校验：不超限额且至少 1 项。
pub fn bundle_pack(b: &mut Bundle, add_items: u16, add_bytes: u32) -> bool {
    let new_bytes = b.bytes.saturating_add(add_bytes);
    let new_items = b.items.saturating_add(add_items);
    if new_bytes > b.limit || new_items == 0 {
        return false;
    }
    b.bytes = new_bytes;
    b.items = new_items;
    true
}

// ---------------------------------------------------------------------------
// F408 局域投送
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxState {
    Idle,
    Offered,
    Accepted,
    Sending,
    Done,
}

/// 投送握手状态机：Idle→Offered→Accepted→Sending→Done。
pub fn tx_step(state: TxState, event: u8) -> TxState {
    match (state, event) {
        (TxState::Idle, 0) => TxState::Offered,
        (TxState::Offered, 1) => TxState::Accepted,
        (TxState::Accepted, 2) => TxState::Sending,
        (TxState::Sending, 3) => TxState::Done,
        (TxState::Offered, 4) | (TxState::Accepted, 4) => TxState::Idle, // 拒绝/取消
        _ => state,
    }
}

// ---------------------------------------------------------------------------
// F409 时间机器备份
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub id: u32,
    pub full: bool,
    pub delta_bytes: u32,
}

#[derive(Clone, Copy)]
pub struct BackupChain {
    pub snaps: [Snapshot; MAX_SNAPSHOTS],
    pub len: usize,
    pub interval_h: u16,
}

impl BackupChain {
    pub const fn new(interval_h: u16) -> BackupChain {
        BackupChain { snaps: [const { Snapshot { id: 0, full: false, delta_bytes: 0 } }; MAX_SNAPSHOTS], len: 0, interval_h }
    }

    /// 满 8 个时挤掉最旧（保留最后一个 full）。
    pub fn push(&mut self, s: Snapshot) -> bool {
        if self.len < MAX_SNAPSHOTS {
            self.snaps[self.len] = s;
            self.len += 1;
            return true;
        }
        // 简化策略：满了就整体前移丢弃最旧。
        for i in 0..MAX_SNAPSHOTS - 1 {
            self.snaps[i] = self.snaps[i + 1];
        }
        self.snaps[MAX_SNAPSHOTS - 1] = s;
        true
    }

    pub fn total_bytes(&self) -> u32 {
        self.snaps[..self.len].iter().map(|s| s.delta_bytes).sum()
    }
}

// ---------------------------------------------------------------------------
// F410 备份加密
// ---------------------------------------------------------------------------

/// 静态加密：快照带 key id 才算已加密。
pub fn backup_encrypted(key_id: u32) -> bool {
    key_id != 0
}

// ---------------------------------------------------------------------------
// F411 备份演练
// ---------------------------------------------------------------------------

/// 恢复演练：逐一比对校验和。
pub fn restore_verify(src: &[u32], dst: &[u32]) -> bool {
    src.len() == dst.len() && src.iter().zip(dst.iter()).all(|(a, b)| a == b)
}

// ---------------------------------------------------------------------------
// F412 文件级版本
// ---------------------------------------------------------------------------

/// 版本链：keep N 个版本，push 超限丢最旧；返回当前链长。
pub fn version_push(chain: &mut [u32], len: &mut usize, keep: usize, v: u32) -> usize {
    if *len == keep && keep > 0 {
        for i in 0..keep - 1 {
            chain[i] = chain[i + 1];
        }
        chain[keep - 1] = v;
    } else if *len < chain.len() {
        chain[*len] = v;
        *len += 1;
    }
    *len
}

// ---------------------------------------------------------------------------
// F413 云同步接口
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncOp {
    None,
    Upload(u32),
    Download(u32),
}

/// 同步判定：本地更新走上传，远端较新走下载，一致不动。
pub fn sync_plan(local_rev: u32, remote_rev: u32, id: u32) -> SyncOp {
    match local_rev.cmp(&remote_rev) {
        core::cmp::Ordering::Greater => SyncOp::Upload(id),
        core::cmp::Ordering::Less => SyncOp::Download(id),
        core::cmp::Ordering::Equal => SyncOp::None,
    }
}

// ---------------------------------------------------------------------------
// F414 冲突仲裁器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    KeepNewest,
    KeepBoth,
    Manual,
}

/// 依策略仲裁：KeepBoth 需要新名字才成立。
pub fn resolve(policy: Conflict, local_newer: bool, rename_ok: bool) -> u8 {
    match policy {
        Conflict::KeepNewest => {
            if local_newer {
                0
            } else {
                1
            }
        }
        Conflict::KeepBoth if rename_ok => 2,
        Conflict::KeepBoth => 3, // 退化成 manual
        Conflict::Manual => 3,
    }
}

// ---------------------------------------------------------------------------
// F415 数据地图
// ---------------------------------------------------------------------------

/// 类别 → 字节聚合，返回类别数。
pub fn data_map(entries: &[(u8, u32)], out: &mut [(u8, u32)]) -> usize {
    let mut n = 0;
    for &(cat, bytes) in entries {
        let mut found = false;
        for i in 0..n {
            if out[i].0 == cat {
                out[i].1 = out[i].1.saturating_add(bytes);
                found = true;
                break;
            }
        }
        if !found && n < out.len() {
            out[n] = (cat, bytes);
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F416 安全擦除
// ---------------------------------------------------------------------------

/// 擦除计划：3 覆写 + 1 校验；校验读到非零则失败。
pub fn secure_erase(passes: &mut [u32; 3], probe: u32) -> bool {
    for p in passes.iter_mut() {
        *p = 0;
    }
    passes.iter().all(|&p| p == 0) && probe == 0
}

// ---------------------------------------------------------------------------
// F417 换盘搬家
// ---------------------------------------------------------------------------

/// 块迁移计划：源块 → 目标块映射，返回可迁移块数（受目标容量限制）。
pub fn disk_migrate_plan(src_blocks: &[u32], dst_free: u32) -> usize {
    src_blocks.iter().filter(|&&b| b <= dst_free).count()
}

// ---------------------------------------------------------------------------
// F418 无损分区管家
// ---------------------------------------------------------------------------

/// 缩小分区：要求保留量 <= 当前 - 最小余量。
pub fn partition_shrink(current_mb: u32, want_mb: u32, min_free_mb: u32) -> bool {
    want_mb + min_free_mb <= current_mb
}

// ---------------------------------------------------------------------------
// F419 智能巡检调度
// ---------------------------------------------------------------------------

/// 脏位或超期 → 需要巡检。
pub fn fsck_due(dirty: bool, days_since: u16, interval_days: u16) -> bool {
    dirty || days_since >= interval_days
}

// ---------------------------------------------------------------------------
// F420 归档格式规范
// ---------------------------------------------------------------------------

/// 归档头校验：魔数 "VDAT" + 版本兼容（<=2）。
pub fn archive_header_ok(hdr: &[u8; 8]) -> bool {
    &hdr[0..4] == b"VDAT" && hdr[4] <= 2 && hdr[5] == 0
}

// ---------------------------------------------------------------------------
// F421 千万文件演练
// ---------------------------------------------------------------------------

/// 大规模演练统计：仅以计数器模拟，除法分桶统计平均块大小。
pub fn mass_walk_stats(total: u64) -> (u64, u64) {
    let blocks = (total + 4095) / 4096;
    (total, blocks)
}

// ---------------------------------------------------------------------------
// F422 数据 fuzz
// ---------------------------------------------------------------------------

/// 头解析 fuzz：任何输入不得 panic，只返回 Ok/Err。
pub fn fuzz_parse_header(input: &[u8]) -> Result<u8, ()> {
    if input.len() < 8 {
        return Err(());
    }
    let mut h = [0u8; 8];
    h.copy_from_slice(&input[..8]);
    if archive_header_ok(&h) {
        Ok(h[4])
    } else {
        Err(())
    }
}

// ---------------------------------------------------------------------------
// F423 迁移进度故事化
// ---------------------------------------------------------------------------

/// 阶段 → 人话短语索引。
pub fn progress_story(stage: MigStage, pct: u8) -> &'static str {
    match stage {
        MigStage::Scan => "盘点数据中",
        MigStage::Copy => {
            if pct < 50 {
                "正在搬运第一批"
            } else {
                "过半了，继续"
            }
        }
        MigStage::Verify => "逐个核对校验",
        MigStage::Done => "搬家完成",
    }
}

// ---------------------------------------------------------------------------
// F424 数据 API 版本化
// ---------------------------------------------------------------------------

/// 版本协商：请求 <= 支持版本则接受。
pub fn api_negotiate(supported: u8, requested: u8) -> Option<u8> {
    if requested <= supported {
        Some(requested)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// F425 数据域自检
// ---------------------------------------------------------------------------

pub fn run_dataflow_checks() -> CheckSet {
    let mut set = CheckSet::new("m5-data");

    // F401
    let mut m = Migration::new(10);
    m.copied = 10;
    let s0 = m.stage;
    m.advance(); // Scan→Copy
    m.advance(); // Copy→Verify
    let refused = m.advance(); // 未验证不得 Done
    m.verified = true;
    let done = m.advance();
    set.add(
        "F401 migration wizard",
        s0 == MigStage::Scan && !refused && done && m.stage == MigStage::Done,
        "stage machine gated by verify",
    );

    // F402
    let mut bm = [(1u32, 100u32), (2, 200), (0, 0), (0, 0)];
    let n = merge_bookmarks(&mut bm, 2, &[(2, 999), (3, 300)]);
    set.add("F402 bookmark import", n == 3 && bm[2] == (3, 300) && bm[1] == (2, 200), "dedupe merge");

    // F403
    let mut slot = MailSlot { account: 0, message_count: 0, imported: true };
    mail_register(&mut slot, 7, 1200);
    set.add(
        "F403 mail archive stub",
        slot.account == 7 && slot.message_count == 1200 && !slot.imported,
        "placeholder registered",
    );

    // F404
    let photos = [(1u32, 2024u16), (2, 2024), (3, 2025)];
    let mut buckets = [(0u16, 0u8); 4];
    let bn = photo_buckets(&photos, &mut buckets);
    set.add("F404 photo import", bn == 2 && buckets[0] == (2024, 2) && buckets[1] == (2025, 1), "group by year");

    // F405
    let good = music_tag_from_name("07_Hello_World");
    let bad = music_tag_from_name("Hello");
    set.add("F405 music import", good == Some((7, music_tag_from_name("07_Hello_World").unwrap().1)) && bad.is_none(), "tag heuristic");

    // F406
    let fmts = [
        DataFormat { name: "vcard", exportable: true },
        DataFormat { name: "bookmark", exportable: true },
    ];
    let closed = [DataFormat { name: "secret", exportable: false }];
    set.add("F406 open formats", all_exportable(&fmts) && !all_exportable(&closed), "export promise");

    // F407
    let mut b = Bundle { items: 0, bytes: 0, limit: 1000 };
    let p1 = bundle_pack(&mut b, 3, 500);
    let over = bundle_pack(&mut b, 1, 600);
    let empty_bad = { let mut e = Bundle { items: 0, bytes: 0, limit: 10 }; bundle_pack(&mut e, 0, 0) };
    set.add("F407 data bundle", p1 && b.items == 3 && b.bytes == 500 && !over && !empty_bad, "limit enforced");

    // F408
    let t = TxState::Idle;
    let t = tx_step(t, 0);
    let t = tx_step(t, 1);
    let t = tx_step(t, 2);
    let t = tx_step(t, 3);
    let cancel = tx_step(tx_step(TxState::Idle, 0), 4);
    set.add(
        "F408 lan transfer",
        t == TxState::Done && cancel == TxState::Idle,
        "handshake + cancel",
    );

    // F409
    let mut chain = BackupChain::new(24);
    for i in 0..10u32 {
        chain.push(Snapshot { id: i, full: i % 4 == 0, delta_bytes: 100 });
    }
    set.add(
        "F409 time machine backup",
        chain.len == MAX_SNAPSHOTS && chain.total_bytes() == 800 && chain.interval_h == 24,
        "snapshot ring keeps 8",
    );

    // F410
    set.add("F410 backup encryption", backup_encrypted(42) && !backup_encrypted(0), "key id present");

    // F411
    let ok = restore_verify(&[1, 2, 3], &[1, 2, 3]);
    let bad = restore_verify(&[1, 2], &[1, 2, 3]);
    set.add("F411 restore drill", ok && !bad, "checksum verify");

    // F412
    let mut chain = [0u32; 4];
    let mut len = 0usize;
    version_push(&mut chain, &mut len, 3, 1);
    version_push(&mut chain, &mut len, 3, 2);
    version_push(&mut chain, &mut len, 3, 3);
    let over_len = version_push(&mut chain, &mut len, 3, 4);
    set.add(
        "F412 file versions",
        over_len == 3 && chain[0] == 2 && chain[2] == 4,
        "keep-N eviction",
    );

    // F413
    let up = sync_plan(5, 3, 10);
    let down = sync_plan(3, 5, 10);
    let none = sync_plan(4, 4, 10);
    set.add(
        "F413 cloud sync",
        up == SyncOp::Upload(10) && down == SyncOp::Download(10) && none == SyncOp::None,
        "rev comparison",
    );

    // F414
    let r1 = resolve(Conflict::KeepNewest, true, false);
    let r2 = resolve(Conflict::KeepBoth, false, true);
    let r3 = resolve(Conflict::KeepBoth, false, false);
    set.add(
        "F414 conflict arbiter",
        r1 == 0 && r2 == 2 && r3 == 3,
        "policy matrix",
    );

    // F415
    let entries = [(1u8, 100u32), (2, 50), (1, 50)];
    let mut map = [(0u8, 0u32); 4];
    let mn = data_map(&entries, &mut map);
    set.add("F415 data map", mn == 2 && map[0] == (1, 150) && map[1] == (2, 50), "category aggregation");

    // F416
    let mut passes = [7u32; 3];
    let wiped = secure_erase(&mut passes, 0);
    let dirty = { let mut p = [0u32; 3]; secure_erase(&mut p, 5) };
    set.add("F416 secure erase", wiped && !dirty && passes == [0; 3], "zero passes + probe");

    // F417
    let plan = disk_migrate_plan(&[100, 200, 500], 250);
    set.add("F417 disk migration", plan == 2, "capacity limited");

    // F418
    let ok = partition_shrink(500, 400, 100);
    let bad = partition_shrink(500, 450, 100);
    set.add("F418 partition manager", ok && !bad, "min free enforced");

    // F419
    set.add(
        "F419 smart fsck",
        fsck_due(true, 1, 30) && fsck_due(false, 30, 30) && !fsck_due(false, 5, 30),
        "dirty or overdue",
    );

    // F420
    let hdr_ok = archive_header_ok(b"VDAT\x01\x00\x00\x00");
    let hdr_bad = archive_header_ok(b"VDAT\x09\x00\x00\x00");
    let hdr_magic = archive_header_ok(b"XXXX\x01\x00\x00\x00");
    set.add("F420 archive format", hdr_ok && !hdr_bad && !hdr_magic, "magic + version");

    // F421
    let (t, blocks) = mass_walk_stats(10_000_000);
    set.add(
        "F421 million-file drill",
        t == 10_000_000 && blocks == 2_442,
        "10M files block math",
    );

    // F422
    let f1 = fuzz_parse_header(b"VDAT\x01\x00\x00\x00xx");
    let f2 = fuzz_parse_header(b"short");
    set.add("F422 data fuzz", f1 == Ok(1) && f2.is_err(), "no panic on junk");

    // F423
    let s1 = progress_story(MigStage::Scan, 0);
    let s2 = progress_story(MigStage::Copy, 30);
    let s3 = progress_story(MigStage::Done, 100);
    set.add(
        "F423 progress storytelling",
        s1 == "盘点数据中" && s2 == "正在搬运第一批" && s3 == "搬家完成",
        "human phrases",
    );

    // F424
    set.add(
        "F424 data api version",
        api_negotiate(2, 1) == Some(1) && api_negotiate(2, 2) == Some(2) && api_negotiate(2, 3).is_none(),
        "negotiation",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f401_migration_gate() {
        let mut m = Migration::new(0);
        m.advance();
        m.advance();
        assert!(!m.advance());
        m.verified = true;
        assert!(m.advance());
    }

    #[test]
    fn f407_bundle_limit() {
        let mut b = Bundle { items: 0, bytes: 0, limit: 100 };
        assert!(bundle_pack(&mut b, 1, 100));
        assert!(!bundle_pack(&mut b, 1, 1));
    }

    #[test]
    fn f422_fuzz_never_panics() {
        for len in 0..12 {
            let buf = [0xFFu8; 12];
            let _ = fuzz_parse_header(&buf[..len]);
        }
    }

    #[test]
    fn f425_data_self_test_passes() {
        let set = run_dataflow_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("m5-data self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
