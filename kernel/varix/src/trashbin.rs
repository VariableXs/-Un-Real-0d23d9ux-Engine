//! trashbin — WP-205 · B-1701 回收站全语义（MD2 篇 17.2，数据红线）。
//!
//! 判据 B-1701：回收站全语义，SDK 零"真删"接口（审计）。
//! MD2 原文（17.2）："删除永远进回收站（宪章第十二章'可反悔'）：回收站是
//! DATA 分区的约定目录结构（条目含原路径、删除时间、原文件引用），vx-files
//! 与 vx-SDK 的删除接口统一走它——SDK 层没有'真删'接口，绕过回收站的唯一
//! 途径是用户显式使用'永久删除'（二次确认）。回收站配额 10% 自动清理最旧
//! （Q45），清理动作有通知。跨盘移动语义：跨文件系统的'移动'实为复制加删除
//! （删除进回收站）。"
//!
//! 结构性防线（本模块的核心论证）：**SDK 删除面类型上不存在"直接删除"变体**
//! ——`DeleteApi` 枚举只有 Trash（进回收站）与 PurgeConfirmed（仅显式二次
//! 确认）两个变体，没有 Direct/Unlink 变体可选；新增删除入口必须进
//! SDK_DELETE_APIS 穷举表，否则对练覆盖缺失。与 ntfsro（B-705"API 面上
//! 不存在写模式"）同族：危险通路的不存在性由类型面保证，不由纪律保证。

use crate::checks::CheckSet;

pub const TRASH_CAP: usize = 16;
/// 回收站配额（Q45）：DATA 分区的 10%。
pub const QUOTA_PCT: u64 = 10;
pub const NOTICE_CAP: usize = 8;
pub const PATH_CAP: usize = 32;

/// 永久删除的二次确认令牌（无令牌 = 拒绝）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Confirm {
    pub ack: bool,
}

/// SDK 删除面公开 API 枚举——类型面不存在"直接删除"变体（B-1701 审计锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeleteApi {
    /// 删除：进回收站（默认唯一路径）。
    Trash,
    /// 永久删除：仅用户显式二次确认后执行。
    PurgeConfirmed,
}

/// 穷举表：新增删除入口必须进表。
pub const SDK_DELETE_APIS: [DeleteApi; 2] = [DeleteApi::Trash, DeleteApi::PurgeConfirmed];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrashEntry {
    /// 原路径（定长 0 填充）。
    pub orig: [u8; PATH_CAP],
    pub orig_len: u8,
    /// 删除时间（单调钟）。
    pub deleted_at: u64,
    /// 原文件引用。
    pub file_ref: u32,
    pub size: u64,
}

impl TrashEntry {
    pub fn path(&self) -> &[u8] {
        &self.orig[..self.orig_len as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PurgeNotice {
    /// 本轮清理条数。
    pub count: u64,
    /// 本轮清理字节。
    pub bytes: u64,
}

pub struct TrashBin {
    entries: [Option<TrashEntry>; TRASH_CAP],
    /// DATA 分区总字节（模型面）。
    pub disk_bytes: u64,
    pub next_ref: u32,
    pub clock: u64,
    notices: [Option<PurgeNotice>; NOTICE_CAP],
    pub notice_count: usize,
    pub put_total: u64,
    pub restore_total: u64,
    pub purge_total: u64,
    pub confirm_refusals: u64,
}

impl TrashBin {
    pub fn new(disk_bytes: u64) -> TrashBin {
        TrashBin {
            entries: [None; TRASH_CAP],
            disk_bytes,
            next_ref: 1,
            clock: 0,
            notices: [None; NOTICE_CAP],
            notice_count: 0,
            put_total: 0,
            restore_total: 0,
            purge_total: 0,
            confirm_refusals: 0,
        }
    }

    fn find_free(&self) -> Option<usize> {
        let mut i = 0;
        while i < TRASH_CAP {
            if self.entries[i].is_none() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < TRASH_CAP {
            if self.entries[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    fn used_bytes(&self) -> u64 {
        let mut b = 0;
        let mut i = 0;
        while i < TRASH_CAP {
            if let Some(e) = self.entries[i] {
                b += e.size;
            }
            i += 1;
        }
        b
    }

    pub fn slot_of(&self, file_ref: u32) -> Option<usize> {
        let mut i = 0;
        while i < TRASH_CAP {
            if let Some(e) = self.entries[i] {
                if e.file_ref == file_ref {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 删除：永远进回收站（唯一默认入口）。
    /// 满时自愈：先按配额清理；配额未超则清最旧一条腾位——删除不因满而失败。
    /// 每个清理动作（批量/单条）都汇总产生一条通知。
    pub fn put(&mut self, path: &[u8], size: u64) -> Option<u32> {
        if path.is_empty() || path.len() > PATH_CAP {
            return None;
        }
        if self.find_free().is_none() {
            let cleared = self.enforce_quota();
            if cleared == 0 && self.find_free().is_none() {
                if let Some(sz) = self.purge_oldest() {
                    self.record_notice(PurgeNotice { count: 1, bytes: sz });
                }
            }
        }
        let slot = self.find_free()?;
        let mut orig = [0u8; PATH_CAP];
        orig[..path.len()].copy_from_slice(path);
        let e = TrashEntry {
            orig,
            orig_len: path.len() as u8,
            deleted_at: self.clock,
            file_ref: self.next_ref,
            size,
        };
        self.entries[slot] = Some(e);
        self.clock += 1;
        self.next_ref += 1;
        self.put_total += 1;
        Some(e.file_ref)
    }

    /// 还原：条目出环、原路径返回（可反悔的兑现面）。
    pub fn restore(&mut self, file_ref: u32) -> Option<[u8; PATH_CAP]> {
        let slot = self.slot_of(file_ref)?;
        let e = self.entries[slot].take()?;
        self.restore_total += 1;
        Some(e.orig)
    }

    fn record_notice(&mut self, n: PurgeNotice) {
        if self.notice_count < NOTICE_CAP {
            self.notices[self.notice_count] = Some(n);
            self.notice_count += 1;
        }
    }

    pub fn notice_at(&self, i: usize) -> Option<PurgeNotice> {
        if i < self.notice_count {
            self.notices[i]
        } else {
            None
        }
    }

    /// 清理最旧一条（deleted_at 最小）——纯删除动作，通知由调用方汇总。
    pub fn purge_oldest(&mut self) -> Option<u64> {
        let mut oldest = None;
        let mut i = 0;
        while i < TRASH_CAP {
            match (self.entries[i], oldest) {
                (Some(e), None) => oldest = Some(i),
                (Some(e), Some(o)) => {
                    if let Some(oe) = self.entries[o] {
                        if e.deleted_at < oe.deleted_at {
                            oldest = Some(i);
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let slot = oldest?;
        let e = self.entries[slot].take()?;
        self.purge_total += 1;
        Some(e.size)
    }

    /// 配额 10%：回收站总量超 DATA 分区 10% → 自动清最旧至额度内（Q45）。
    /// 每轮清理动作汇总产生**一条**通知（count/bytes 如实——清理动作有通知，
    /// 且通知面不因批量清理挤爆容量：数据面与记录面容量语义分离）。
    pub fn enforce_quota(&mut self) -> u64 {
        let quota = self.disk_bytes * QUOTA_PCT / 100;
        let mut count = 0u64;
        let mut bytes = 0u64;
        while self.used_bytes() > quota && self.count() > 0 {
            match self.purge_oldest() {
                Some(sz) => {
                    count += 1;
                    bytes += sz;
                }
                None => break,
            }
        }
        if count > 0 {
            self.record_notice(PurgeNotice { count, bytes });
        }
        count
    }

    /// 永久删除：仅用户显式二次确认（Confirm 令牌）后执行。
    pub fn permanent_purge(&mut self, file_ref: u32, confirm: Option<Confirm>) -> bool {
        let slot = match self.slot_of(file_ref) {
            Some(s) => s,
            None => return false,
        };
        match confirm {
            Some(c) if c.ack => {
                let e = self.entries[slot].take();
                if e.is_some() {
                    self.purge_total += 1;
                    let n = PurgeNotice { count: 1, bytes: e.unwrap().size };
                    self.record_notice(n);
                    true
                } else {
                    false
                }
            }
            _ => {
                // 无确认令牌：拒绝（二次确认是硬门）。
                self.confirm_refusals += 1;
                false
            }
        }
    }

    /// 对账：put 总数 == 现存 + 还原 + 清除（条目守恒）。
    pub fn reconcile(&self) -> bool {
        self.put_total == (self.count() as u64) + self.restore_total + self.purge_total
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoveReport {
    pub copied: bool,
    pub trashed: bool,
}

/// 跨盘移动语义：跨文件系统"移动"实为复制加删除（删除进回收站）。
pub fn move_cross_fs(bin: &mut TrashBin, path: &[u8], size: u64) -> MoveReport {
    // 复制步（模型面：目标盘落盘成功）。
    let copied = true;
    let trashed = bin.put(path, size).is_some();
    MoveReport { copied, trashed }
}

// ============ CheckSet（B-1701 ×9）============

pub fn run_trashbin_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-1701 回收站全语义");
    {
        // B-1701 删除必经回收站：穷举 SDK 删除面 API。
        // 类型面：DeleteApi 无 Direct 变体；行为面：Trash→put / PurgeConfirmed→二次确认。
        let mut bin = TrashBin::new(1_000_000);
        let mut route_ok = true;
        let mut i = 0;
        while i < SDK_DELETE_APIS.len() {
            match SDK_DELETE_APIS[i] {
                DeleteApi::Trash => {
                    if bin.put(b"/docs/a.txt", 100).is_none() {
                        route_ok = false;
                    }
                }
                DeleteApi::PurgeConfirmed => {
                    let r = match bin.put(b"/docs/b.txt", 100) {
                        Some(r) => r,
                        None => 0,
                    };
                    if bin.permanent_purge(r, Some(Confirm { ack: true })) != true {
                        route_ok = false;
                    }
                }
            }
            i += 1;
        }
        set.add(
            "B-1701 删除必经回收站",
            route_ok && SDK_DELETE_APIS.len() == 2,
            "类型面仅 Trash/PurgeConfirmed 两变体（无 Direct——新增入口必须进表）",
        );
    }
    {
        // B-1701 条目三字段齐。
        let mut bin = TrashBin::new(1_000_000);
        let r = bin.put(b"/home/user/report.txt", 4096).unwrap_or(0);
        let e = bin.slot_of(r).and_then(|s| bin_slot(&bin, s));
        let ok = match e {
            Some(e) => !e.path().is_empty() && e.deleted_at == 0 && e.file_ref == r,
            None => false,
        };
        set.add(
            "B-1701 条目三字段齐",
            ok,
            "原路径 + 删除时间 + 原文件引用全非空",
        );
    }
    {
        // B-1701 还原语义。
        let mut bin = TrashBin::new(1_000_000);
        let r = bin.put(b"/tmp/x.bin", 10).unwrap_or(0);
        let restored = bin.restore(r);
        let gone = bin.slot_of(r).is_none();
        set.add(
            "B-1701 还原语义",
            restored.is_some() && gone && bin.reconcile(),
            "restore 后条目出环、原路径返回、对账守恒",
        );
    }
    {
        // B-1701 永久删除二次确认（无令牌拒绝 + 有令牌执行且留痕）。
        let mut bin = TrashBin::new(1_000_000);
        let r = bin.put(b"/tmp/y.bin", 20).unwrap_or(0);
        let refused = !bin.permanent_purge(r, None) && bin.confirm_refusals == 1;
        let still_there = bin.slot_of(r).is_some();
        let done = bin.permanent_purge(r, Some(Confirm { ack: true }));
        set.add(
            "B-1701 永久删除二次确认",
            refused && still_there && done && bin.notice_count >= 1,
            "无 Confirm 拒绝并计数；有 Confirm 执行且产生通知",
        );
    }
    {
        // B-1701 配额 10% 清最旧（disk=1000 → 配额 100）。
        let mut bin = TrashBin::new(1_000);
        let mut i = 0;
        while i < 12 {
            let _ = bin.put(b"/q/f.bin", 20);
            i += 1;
        }
        // 12×20=240 > 100 → 清 7 条至 ≤100。
        let cleared = bin.enforce_quota();
        let after = bin_used(&bin);
        set.add(
            "B-1701 配额清最旧",
            cleared == 7 && after <= 100 && bin.count() > 0,
            "240>100 → 清最旧 7 条（240-140=100）",
        );
    }
    {
        // B-1701 清理有通知：一动作一汇总通知（记录面不因批量清理挤爆）。
        let mut bin = TrashBin::new(1_000);
        let mut i = 0;
        while i < 12 {
            let _ = bin.put(b"/q/g.bin", 30);
            i += 1;
        }
        let cleared = bin.enforce_quota();
        let noticed = match bin.notice_at(0) {
            Some(n) => n.count == cleared && n.bytes == cleared * 30,
            None => false,
        };
        set.add(
            "B-1701 清理有通知",
            noticed && bin.notice_count == 1 && cleared == 9,
            "360>100 → 清 9 条（360-270=90）；汇总一条 PurgeNotice",
        );
    }
    {
        // B-1701 跨盘移动语义：复制 + 删除进回收站（无直接删）。
        let mut bin = TrashBin::new(1_000_000);
        let rep = move_cross_fs(&mut bin, b"/ext4/big.dat", 500);
        set.add(
            "B-1701 跨盘移动语义",
            rep.copied && rep.trashed && bin.reconcile(),
            "跨文件系统移动 = 复制 + 回收站删除",
        );
    }
    {
        // B-1701 回收站满自愈：put 满时自动清最旧腾位，删除不失败。
        let mut bin = TrashBin::new(100_000_000); // 配额高：不触发配额清理
        let mut i = 0;
        while i < TRASH_CAP {
            let _ = bin.put(b"/full/x.bin", 1);
            i += 1;
        }
        let full = bin.slot_of(bin.next_ref - 1).is_some();
        let r = bin.put(b"/full/new.bin", 1);
        set.add(
            "B-1701 回收站满自愈",
            full && r.is_some() && bin.count() == TRASH_CAP,
            "满 16 条后仍可 put（自动清最旧腾位）",
        );
    }
    {
        // B-1701 条目守恒对账：put == 现存 + restore + purge。
        let mut bin = TrashBin::new(1_000_000);
        let mut refs = [0u32; 6];
        let mut i = 0;
        while i < 6 {
            refs[i] = bin.put(b"/acc/f.bin", 5).unwrap_or(0);
            i += 1;
        }
        let _ = bin.restore(refs[0]);
        let _ = bin.permanent_purge(refs[1], Some(Confirm { ack: true }));
        set.add(
            "B-1701 条目守恒对账",
            bin.reconcile() && bin.put_total == 6,
            "put_total == count + restore_total + purge_total",
        );
    }
    set
}

// 槽位读取辅助（避免借用冲突的最小封装）。
fn bin_slot(bin: &TrashBin, slot: usize) -> Option<TrashEntry> {
    if slot < TRASH_CAP {
        bin.entries[slot]
    } else {
        None
    }
}

fn bin_used(bin: &TrashBin) -> u64 {
    let mut b = 0;
    let mut i = 0;
    while i < TRASH_CAP {
        if let Some(e) = bin.entries[i] {
            b += e.size;
        }
        i += 1;
    }
    b
}

// ============ 单测（f903 ×4）============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f903_trash_roundtrip() {
        let mut bin = TrashBin::new(1_000_000);
        let r = bin.put(b"/home/me/note.md", 77).expect("put ok");
        assert_eq!(bin.count(), 1);
        let orig = bin.restore(r).expect("restore ok");
        // 路径还原（0 填充尾部）。
        assert_eq!(&orig[..16], b"/home/me/note.md");
        assert_eq!(orig[16], 0);
        assert!(bin.reconcile());
    }

    #[test]
    fn f903_quota_enforce() {
        let mut bin = TrashBin::new(1_000);
        let mut i = 0;
        while i < 20 {
            let _ = bin.put(b"/q/a.bin", 25);
            i += 1;
        }
        // put 17 时环满自愈：自动清 12 条（400→100）——汇总一条通知。
        // put 17~20 后 8 条 200B；显式 enforce 再清 4 条（200→100）。
        let cleared = bin.enforce_quota();
        assert!(bin_used(&bin) <= 100);
        assert_eq!(bin.notice_count, 2, "put 自愈一条 + 显式 enforce 一条");
        let n0 = bin.notice_at(0).expect("自愈通知");
        assert_eq!((n0.count, n0.bytes), (12, 300));
        let n1 = bin.notice_at(1).expect("显式通知");
        assert_eq!(n1.count, cleared);
        assert_eq!(n1.bytes, cleared * 25);
    }

    #[test]
    fn f903_purge_confirm() {
        let mut bin = TrashBin::new(1_000_000);
        let r = bin.put(b"/x/y", 1).expect("put ok");
        // 无令牌：拒绝 + 计数，条目仍在。
        assert!(!bin.permanent_purge(r, None));
        assert_eq!(bin.confirm_refusals, 1);
        assert!(bin.slot_of(r).is_some());
        // 不认账的令牌：同样拒绝。
        assert!(!bin.permanent_purge(r, Some(Confirm { ack: false })));
        assert_eq!(bin.confirm_refusals, 2);
        // 正式确认：执行。
        assert!(bin.permanent_purge(r, Some(Confirm { ack: true })));
        assert!(bin.slot_of(r).is_none());
        assert!(bin.reconcile());
    }

    #[test]
    fn f903_move_cross() {
        let mut bin = TrashBin::new(1_000_000);
        let rep = move_cross_fs(&mut bin, b"/ext4/a.dat", 9);
        assert!(rep.copied && rep.trashed);
        // 坏路径拒绝（空/超长）。
        assert!(bin.put(b"", 1).is_none());
        let long = [b'a'; PATH_CAP + 1];
        assert!(bin.put(&long, 1).is_none());
        assert!(bin.reconcile());
    }
}
