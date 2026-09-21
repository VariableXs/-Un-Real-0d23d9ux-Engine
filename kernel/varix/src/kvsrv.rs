//! 任务24 · 内核 KV 存储服务（双域总案 2.3 服务层：前端 localStorage 语义，
//! 差异清单如实公示）。
//!
//! **持久化复用 fs23_journal 后端，不另造持久化**（总案验收红线）：
//! `DiskJournal` 即 KV 账本——每条 set/remove = 一条 WAL 槽
//! （tier=3 writeahead，append 返回即 flush 持久化点）；账本槽自由区
//! 携带命名空间/键名/值，撕裂/腐坏语义逐条继承（torn 丢该点及之后、
//! open 可重复、seq 严格递增）。值超过账本槽容量时走溢出槽
//! （数据区追加式，fnv 校验），账本条目 `blk` 字段存溢出槽基块。
//!
//! 账本槽布局（offset 24..512 为 KV 载荷区，0..24 是 fs23 槽头）：
//!
//! ```text
//! [24..28] ns_len u32      （≤ NS_MAX=16）
//! [28..32] key_len u32     （≤ KEY_MAX=48）
//! [32..36] val_len u32     （inline 时 ≤ INLINE_MAX=352；DEL 恒 0）
//! [36..44] body_fnv u64    （fnv1a64(ns || key || val)）
//! [44..]   ns || key || val（val 仅 inline；溢出条目 val 区空）
//! ```
//!
//! 溢出槽布局（`KV_SLOT_BLOCKS`=64 块 = 32KiB/槽，`KV_OVERFLOW_SLOTS`=64）：
//!
//! ```text
//! [0..8]   magic u64 = "VARXKVD1"
//! [8..16]  val_len u64
//! [16..24] val_fnv u64
//! [24..]   val bytes（≤ OVERFLOW_MAX=32744）
//! ```
//!
//! 重放语义（每次操作现场扫描账本，内存零驻留——真源恒为盘面）：
//! - `get`：倒序扫账本，首条匹配（ns,key）且载荷完好的 SET 即值；
//!   溢出槽腐坏 = 该条目忽略、继续回退更早条目（零丢失）；DEL 先见 = 无值。
//! - `keys`：正向扫账本，末态活键名列表。
//! - **满容量明确报错**（总案验收点）：账本 64 条满 / 溢出区 64 槽满
//!   → `Err(KvError::Full)`，绝不静默覆盖。
//! - **命名空间隔离**：API 全带 `ns`，不同应用互不可见（垫片层把
//!   前端 origin→ns 映射，任务26 落地）。
//!
//! 与 localStorage 的语义差异清单见
//! `docs/双域-任务24-KV语义差异清单-2026-09-17.md`（如实公示）。

use alloc::format;
use alloc::vec::Vec;
use crate::drivers::blk::{fnv1a64, BlockDevice, BlockError};
use crate::fs::fs23_disk::{DiskJournal, OpenReport};

/// 账本槽 KV 载荷区起点（0..24 是 fs23 槽头 seq/blk/tag/crc）。
const KV_BODY_OFF: usize = 24;
/// 命名空间长度上限（字节）。
pub const NS_MAX: usize = 16;
/// 键名长度上限（字节）。
pub const KEY_MAX: usize = 48;
/// inline 值上限：载荷 468B − 头 20B − ns 16B − key 48B 取整。
pub const INLINE_MAX: usize = 352;
/// 溢出槽块数（32KiB）。
/// 边界推导（任务65 根因攻坚）：kheap 仅 256KiB（mem/heap.rs ARENA_BYTES），
/// 槽 64KiB 时 write/read_overflow 的整槽 Vec 物化超 slab 单次分配上限、
/// 页分配器 fallback 缺位 → alloc.rs:573 panic → panic-in-panic 静默 halt
/// （实机 NVMe 复现：单次 512B put 确定性停摆；宿主 MemDisk 因 std 分配器
/// 不复现）。槽减半 + 分片直写后单次堆分配 ≤32KiB（1/8 预算内）。
pub const KV_SLOT_BLOCKS: u64 = 64;
/// 溢出槽位数。
pub const KV_OVERFLOW_SLOTS: u64 = 64;
/// 溢出值上限。
pub const OVERFLOW_MAX: usize = (KV_SLOT_BLOCKS * 512) as usize - 24;

/// 溢出槽 magic "VARXKVD1"（LE）。
const OVER_MAGIC: u64 = 0x5641_5258_4B56_4431;

const TAG_SET: u32 = 1; // 与 fs23 TAG_WRITE 同值
const TAG_DEL: u32 = 2; // 与 fs23 TAG_DELETE 同值

/// KV 服务错误——满容量/超限均明确报错（总案验收点），绝不静默。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvError {
    /// 账本 64 条满或溢出区 64 槽满。
    Full,
    /// 值超过 [`OVERFLOW_MAX`]。
    TooLarge,
    /// 命名空间为空/超 [`NS_MAX`]/含 NUL。
    BadNamespace,
    /// 键名为空/超 [`KEY_MAX`]/含 NUL。
    BadKey,
    /// 块设备错误。
    Io(BlockError),
}

impl From<BlockError> for KvError {
    fn from(e: BlockError) -> Self {
        KvError::Io(e)
    }
}

/// KV 存储（持有 DiskJournal 账本 + 溢出区基址；内存零驻留，
/// 每次 API 现场重放账本——真源恒为盘面）。
pub struct KvStore<B: BlockDevice> {
    journal: DiskJournal<B>,
    data_base: u64,
}

/// 校验 ns/key：非空、≤上限、不含 NUL。
fn check_name(b: &[u8], max: usize) -> Result<(), KvError> {
    if b.is_empty() || b.len() > max || b.contains(&0) {
        return Err(if max == NS_MAX { KvError::BadNamespace } else { KvError::BadKey });
    }
    Ok(())
}


/// 账本条目解析结果（KV 载荷区视角）。
struct Entry<'a> {
    tag: u32,
    ns: &'a [u8],
    key: &'a [u8],
    val: &'a [u8],
    blk: u64,
}

impl<'a> Entry<'a> {
    /// 自由区字节解析 + body_fnv 校验；坏条目返回 None（条目级忽略，
    /// 不毒化账本序——fs23 槽级撕裂由 tag/crc 与 open 的 torn 报告承担）。
    fn parse(slot: &'a [u8; 512], blk: u64, tag: u32) -> Option<Entry<'a>> {
        if tag != TAG_SET && tag != TAG_DEL {
            return None;
        }
        let b = &slot[KV_BODY_OFF..];
        if b.len() < 20 {
            return None;
        }
        let rd32 = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
        let ns_len = rd32(0) as usize;
        let key_len = rd32(4) as usize;
        let val_len = rd32(8) as usize;
        let want_fnv = u64::from_le_bytes([b[12], b[13], b[14], b[15], b[16], b[17], b[18], b[19]]);
        let body = &b[20..];
        if ns_len > NS_MAX || key_len > KEY_MAX || body.len() < ns_len + key_len + val_len {
            return None;
        }
        let ns = &body[..ns_len];
        let key = &body[ns_len..ns_len + key_len];
        let val = &body[ns_len + key_len..ns_len + key_len + val_len];
        let mut all = Vec::with_capacity(ns_len + key_len + val_len);
        all.extend_from_slice(ns);
        all.extend_from_slice(key);
        all.extend_from_slice(val);
        if fnv1a64(&all) != want_fnv {
            return None; // 载荷腐坏/半写——条目级忽略
        }
        Some(Entry { tag, ns, key, val, blk })
    }
}

/// 账本操作选择（映射 fs23 LogOp：Write→tag1/SET，Delete→tag2/DEL）。
enum LogOpSel {
    Set,
    Del,
}

impl<B: BlockDevice> KvStore<B> {
    /// 打开（账本必须已 format——探针层负责首轮 format；超块损坏透传 Err）。
    pub fn open(dev: B, journal_base: u64, data_base: u64) -> Result<(Self, OpenReport), BlockError> {
        let (journal, rep) = DiskJournal::open(dev, crate::fs::fs23_journal::JOURNAL_DEFAULT, journal_base)?;
        Ok((KvStore { journal, data_base }, rep))
    }

    /// 首轮格式化（幂等；只清账本，溢出区按槽头 magic 逐槽判定无需清）。
    pub fn format(dev: &mut B, journal_base: u64) -> Result<(), BlockError> {
        DiskJournal::format(dev, crate::fs::fs23_journal::JOURNAL_DEFAULT, journal_base)
    }

    /// 追加一条账本（载荷拼装 + append = tier3 flush 持久化点）。
    fn append_entry(&mut self, op: LogOpSel, ns: &[u8], key: &[u8], val: &[u8], blk: u64) -> Result<(), KvError> {
        // 488B 载荷（未用尾部保持零——fs23 Empty 判定要求余量全零）。
        let mut payload = [0u8; 512 - KV_BODY_OFF];
        payload[0..4].copy_from_slice(&(ns.len() as u32).to_le_bytes());
        payload[4..8].copy_from_slice(&(key.len() as u32).to_le_bytes());
        payload[8..12].copy_from_slice(&(val.len() as u32).to_le_bytes());
        let mut all = Vec::with_capacity(ns.len() + key.len() + val.len());
        all.extend_from_slice(ns);
        all.extend_from_slice(key);
        all.extend_from_slice(val);
        payload[12..20].copy_from_slice(&fnv1a64(&all).to_le_bytes());
        let mut off = 20;
        payload[off..off + ns.len()].copy_from_slice(ns);
        off += ns.len();
        payload[off..off + key.len()].copy_from_slice(key);
        off += key.len();
        if !val.is_empty() {
            payload[off..off + val.len()].copy_from_slice(val);
        }
        let op = match op {
            LogOpSel::Set => crate::fs::fs23_journal::LogOp::Write { blk },
            LogOpSel::Del => crate::fs::fs23_journal::LogOp::Delete { blk },
        };
        // append_slot：载荷与 fs23 头同块落盘 → tier≥2 flush（持久化点=Some）。
        match self.journal.append_slot(op, &payload) {
            Some(_) => Ok(()),
            None => Err(KvError::Full), // 满——本服务恒 tier=3，None 即满
        }
    }

    /// 分配一个空闲溢出槽并写入值（写+flush 落盘后账本才承诺）。
    fn write_overflow(&mut self, val: &[u8]) -> Result<u64, KvError> {
        let mut probe = [0u8; 512];
        let mut base = None;
        for s in 0..KV_OVERFLOW_SLOTS {
            let lba = self.data_base + s * KV_SLOT_BLOCKS;
            self.journal.dev_read(lba, &mut probe).map_err(KvError::Io)?;
            if u64::from_le_bytes(probe[0..8].try_into().unwrap()) == 0 {
                base = Some(lba);
                break;
            }
        }
        let base = base.ok_or(KvError::Full)?;
        // 分片直写（任务56 戒律）：整槽 64KiB Vec 物化超 kheap slab 上限，
        // 实机 alloc panic 静默 halt——改为 512B 栈片逐块写，零大堆分配。
        // 全 128 块（现 64 块）都写：零填充尾块保证整槽覆盖，残留防泄露语义不变。
        let mut head = [0u8; 512];
        head[0..8].copy_from_slice(&OVER_MAGIC.to_le_bytes());
        head[8..16].copy_from_slice(&(val.len() as u64).to_le_bytes());
        head[16..24].copy_from_slice(&fnv1a64(val).to_le_bytes());
        let head_data = val.len().min(512 - 24);
        head[24..24 + head_data].copy_from_slice(&val[..head_data]);
        self.journal.dev_write(base, &head).map_err(KvError::Io)?;
        let mut off = head_data;
        let mut blk = 1usize;
        while off < val.len() {
            let n = (val.len() - off).min(512);
            let mut body = [0u8; 512];
            body[..n].copy_from_slice(&val[off..off + n]);
            self.journal.dev_write(base + blk as u64, &body).map_err(KvError::Io)?;
            off += n;
            blk += 1;
        }
        let zero = [0u8; 512];
        while blk < KV_SLOT_BLOCKS as usize {
            self.journal.dev_write(base + blk as u64, &zero).map_err(KvError::Io)?;
            blk += 1;
        }
        self.journal.dev_flush().map_err(KvError::Io)?;
        Ok(base)
    }

    /// 读溢出槽（magic/len/fnv 校验；坏 = None）。
    fn read_overflow(&mut self, base: u64) -> Option<Vec<u8>> {
        let mut head = [0u8; 512];
        self.journal.dev_read(base, &mut head).ok()?;
        let magic = u64::from_le_bytes(head[0..8].try_into().ok()?);
        if magic != OVER_MAGIC {
            return None;
        }
        let len = u64::from_le_bytes(head[8..16].try_into().ok()?) as usize;
        if len > OVERFLOW_MAX {
            return None;
        }
        // 分片读：只物化 val 本身（≤ OVERFLOW_MAX ≤ 32KiB，kheap 预算内），
        // 头片 488B 直接入 Vec，其余 512B 栈片逐块续读。
        let mut val = Vec::with_capacity(len);
        let head_data = len.min(512 - 24);
        val.extend_from_slice(&head[24..24 + head_data]);
        let mut blk = 1usize;
        let mut off = head_data;
        let mut body = [0u8; 512];
        while off < len {
            self.journal.dev_read(base + blk as u64, &mut body).ok()?;
            let n = (len - off).min(512);
            val.extend_from_slice(&body[..n]);
            off += n;
            blk += 1;
        }
        if fnv1a64(&val) != u64::from_le_bytes(head[16..24].try_into().ok()?) {
            return None;
        }
        Some(val)
    }

    /// 倒序扫账本：返回 (ns,key) 的末态值。
    ///
    /// None = 不存在/已删；溢出槽腐坏 = 该条目忽略、继续回退更早条目
    /// （零丢失语义）。
    pub fn get(&mut self, ns: &[u8], key: &[u8]) -> Result<Option<Vec<u8>>, KvError> {
        check_name(ns, NS_MAX)?;
        check_name(key, KEY_MAX)?;
        let n = self.journal.len();
        for i in (0..n).rev() {
            let Some((_seq, blk, tag)) = self.journal.read_entry(i) else {
                continue; // 槽 CRC 坏/读失败——open 已按撕裂计 torn，跳过残条
            };
            let mut slot = [0u8; 512];
            self.journal.dev_read(self.journal.slot_lba(i), &mut slot).map_err(KvError::Io)?;
            let Some(e) = Entry::parse(&slot, blk, tag) else {
                continue;
            };
            if e.ns != ns || e.key != key {
                continue;
            }
            if e.tag == TAG_DEL {
                return Ok(None);
            }
            if e.blk == 0 {
                return Ok(Some(e.val.to_vec()));
            }
            if let Some(v) = self.read_overflow(e.blk) {
                return Ok(Some(v));
            }
            // 溢出槽腐坏——该条目忽略，回退更早条目。
        }
        Ok(None)
    }

    /// 写入（inline ≤[`INLINE_MAX`] 直进账本槽；否则溢出槽 + 账本记基块）。
    pub fn set(&mut self, ns: &[u8], key: &[u8], val: &[u8]) -> Result<(), KvError> {
        check_name(ns, NS_MAX)?;
        check_name(key, KEY_MAX)?;
        if val.len() > OVERFLOW_MAX {
            return Err(KvError::TooLarge);
        }
        if val.len() <= INLINE_MAX {
            return self.append_entry(LogOpSel::Set, ns, key, val, 0);
        }
        let base = self.write_overflow(val)?;
        self.append_entry(LogOpSel::Set, ns, key, &[], base)
    }

    /// 删除（对齐 localStorage：键不存在 = no-op Ok，不消耗账本条目）。
    pub fn remove(&mut self, ns: &[u8], key: &[u8]) -> Result<(), KvError> {
        check_name(ns, NS_MAX)?;
        check_name(key, KEY_MAX)?;
        if self.get(ns, key)?.is_none() {
            return Ok(());
        }
        self.append_entry(LogOpSel::Del, ns, key, &[], 0)
    }

    /// 列命名空间末态活键名（正向扫账本聚合，末写覆盖先写、DEL 剔除）。
    pub fn keys(&mut self, ns: &[u8]) -> Result<Vec<Vec<u8>>, KvError> {
        check_name(ns, NS_MAX)?;
        // 活键表：64 项 × 48B 固定缓冲——栈上 4KB 内，零堆重分配风险。
        let mut live = [[0u8; KEY_MAX]; KV_OVERFLOW_SLOTS as usize];
        let mut lens = [0usize; KV_OVERFLOW_SLOTS as usize];
        let mut dead = [false; KV_OVERFLOW_SLOTS as usize];
        let mut used = 0usize;
        let n = self.journal.len();
        for i in 0..n {
            let Some((_seq, _blk, tag)) = self.journal.read_entry(i) else {
                continue;
            };
            let mut slot = [0u8; 512];
            self.journal.dev_read(self.journal.slot_lba(i), &mut slot).map_err(KvError::Io)?;
            let Some(e) = Entry::parse(&slot, 0, tag) else {
                continue;
            };
            if e.ns != ns {
                continue;
            }
            let mut found = None;
            for j in 0..used {
                if live[j][..lens[j]] == *e.key {
                    found = Some(j);
                    break;
                }
            }
            match (e.tag, found) {
                (TAG_SET, Some(j)) => dead[j] = false,
                (TAG_SET, None) => {
                    if used < KV_OVERFLOW_SLOTS as usize {
                        live[used][..e.key.len()].copy_from_slice(e.key);
                        lens[used] = e.key.len();
                        dead[used] = false;
                        used += 1;
                    }
                    // 活键表满：条目仍有效可 get，keys 截断——v1 差异清单公示。
                }
                (TAG_DEL, Some(j)) => dead[j] = true,
                (TAG_DEL, None) => {}
                _ => {}
            }
        }
        let mut out = Vec::new();
        for j in 0..used {
            if !dead[j] {
                out.push(live[j][..lens[j]].to_vec());
            }
        }
        Ok(out)
    }

    /// 账本剩余可写条数（满容量预检）。
    pub fn remaining(&self) -> usize {
        crate::fs::fs23_disk::SLOTS - self.journal.len()
    }

    /// 收回块设备（重开/断电恢复测试用——盘面即真源）。
    pub fn into_device(self) -> B {
        self.journal.into_device()
    }
}

// ---------------------------------------------------------------------------
// 目标态探针：QEMU NVMe 实机跨断电恢复（nvme 探针链挂接；驱动脚本按标志序核对）。
// ---------------------------------------------------------------------------

pub mod target {
    use super::*;

    /// 盘1（init 512MiB）高区布局：溢出区 60000..68192（64 槽×128 块）、
    /// 账本 70000..70193——与 loopback(≈16007)/fs23(20000..20193)/
    /// milestone(40000..40193) 均无交集。
    pub const KV_DATA_BASE: u64 = 60_000;
    pub const KV_JOURNAL_BASE: u64 = 70_000;

    /// 双会话协议（跨 kill QEMU 断电）：
    ///   每会话：boot-counter 累加（sys ns）+ 写本会话 10 键（probe ns，
    ///   64B 指纹值）+ 回读精确核对 → `kvsrv: session=N keys=K verdict=ok`；
    ///   会话 ≥2 额外核对上一会话 10 键跨断电零丢失
    ///   → `kvsrv: powercut-recovered verdict=ok recovered=10`。
    pub fn kv_probe(mut dev: &mut dyn BlockDevice) {
        // 首轮：账本未格式化 → format（幂等；open Err 仅超块级损坏）。
        if DiskJournal::open(&mut *dev, crate::fs::fs23_journal::JOURNAL_DEFAULT, KV_JOURNAL_BASE).is_err() {
            if KvStore::format(&mut dev, KV_JOURNAL_BASE).is_err() {
                crate::kwarn!("kvsrv: format failed base={}", KV_JOURNAL_BASE);
                return;
            }
        }
        let mut store = match KvStore::open(&mut *dev, KV_JOURNAL_BASE, KV_DATA_BASE) {
            Ok((s, rep)) => {
                if rep.torn != 0 {
                    crate::kwarn!("kvsrv: journal torn={} — 尾部丢弃语义生效", rep.torn);
                }
                s
            }
            Err(e) => {
                crate::kwarn!("kvsrv: open failed {:?}", e);
                return;
            }
        };
        // boot-counter 累加（跨断电零丢失活证据）。
        let counter = match store.get(b"sys", b"boot-counter") {
            Ok(Some(v)) if !v.is_empty() => v[0] as u32,
            Ok(_) => 0,
            Err(e) => {
                crate::kwarn!("kvsrv: get counter failed {:?}", e);
                return;
            }
        };
        if store.set(b"sys", b"boot-counter", &[(counter + 1) as u8]).is_err() {
            crate::kwarn!("kvsrv: set counter failed");
            return;
        }
        let session = counter + 1;
        // 本会话 10 键（64B 指纹值：全 byte=session 花色）。
        for i in 0..10u32 {
            let mut key = format!("s{}-k{}", session, i);
            key.truncate(KEY_MAX);
            let val = [(session * 16 + i) as u8; 64];
            if let Err(e) = store.set(b"probe", key.as_bytes(), &val) {
                crate::kwarn!("kvsrv: set s{}-k{} failed {:?}", session, i, e);
                return;
            }
        }
        // 回读精确核对。
        let mut ok_all = true;
        for i in 0..10u32 {
            let key = format!("s{}-k{}", session, i);
            let expect = [(session * 16 + i) as u8; 64];
            match store.get(b"probe", key.as_bytes()) {
                Ok(Some(v)) if v == expect => {}
                other => {
                    crate::kwarn!("kvsrv: verify {} failed {:?}", key, other.map(|o| o.as_ref().map(|v| v.len())));
                    ok_all = false;
                }
            }
        }
        // 会话 ≥2：核对上一会话 10 键跨断电零丢失。
        let mut recovered = 0usize;
        if counter >= 1 {
            for i in 0..10u32 {
                let key = format!("s{}-k{}", counter, i);
                let expect = [(counter * 16 + i) as u8; 64];
                if matches!(store.get(b"probe", key.as_bytes()), Ok(Some(v)) if v == expect) {
                    recovered += 1;
                }
            }
            if recovered == 10 {
                crate::kinfo!("kvsrv: powercut-recovered verdict=ok recovered=10");
            } else {
                crate::kwarn!("kvsrv: powercut-recovered verdict=tainted recovered={}", recovered);
            }
        }
        let nkeys = store.keys(b"probe").map(|k| k.len()).unwrap_or(0);
        if ok_all {
            crate::kinfo!("kvsrv: session={} keys={} remaining={} verdict=ok", session, nkeys, store.remaining());
        } else {
            crate::kwarn!("kvsrv: session={} verdict=tainted", session);
        }
    }
}

// ---------------------------------------------------------------------------
// S2.03 · 内核 KV 命令面（垫片 kv_get/kv_set/kv_remove/kv_keys 的内核半边，AI-3）。
//
// 线缆契约（协议规范 §7，与 tools/shim-protocol.source.json v2 同源）：
// 嵌入层（S2.05+）负责前端 JSON args ⇄ 本帧格式翻译；本模块只认帧、只出帧，
// 对任意输入字节流零 panic（fuzz 门禁），错误统一映射协议错误码：
//   BadNamespace/BadKey/TooLarge → SHIM_INVALID_ARGS
//   Full                         → SHIM_KV_FULL（一等码，绝不静默覆盖）
//   Io(_)                        → SHIM_INTERNAL
// ---------------------------------------------------------------------------

pub mod cmd {
    use super::{KvError, KvStore};
    use crate::drivers::blk::BlockDevice;
    use crate::vport::shim_protocol as proto;
    // 显式导入：宿主测试构建 std prelude 自带 Vec，no_std 镜像构建（kcheck）必须自带。
    use alloc::vec::Vec;

    /// 请求帧 tag（kv_* 四命令）。
    pub const TAG_GET: u8 = 1;
    pub const TAG_SET: u8 = 2;
    pub const TAG_REMOVE: u8 = 3;
    pub const TAG_KEYS: u8 = 4;

    /// 请求帧定长头（tag 1 + ns_len 2 + key_len 2 + val_len 4）。
    pub const REQ_HEAD: usize = 9;
    /// 应答帧定长头（status 1 + code 1 + msg_len 2 + payload_len 4）。
    pub const REP_HEAD: usize = 8;

    /// 解析后的请求（全部借自输入帧，零拷贝）。
    struct Req<'a> {
        tag: u8,
        ns: &'a [u8],
        key: &'a [u8],
        val: &'a [u8],
    }

    /// 严格解析：长度精确匹配（尾部垃圾 = 坏帧）、tag 白名单、非 SET 恒 val_len=0。
    /// 任何不合规 = Err(()) → SHIM_INVALID_ARGS；绝不 panic。
    fn parse(frame: &[u8]) -> Result<Req<'_>, ()> {
        if frame.len() < REQ_HEAD {
            return Err(());
        }
        let tag = frame[0];
        if !matches!(tag, TAG_GET | TAG_SET | TAG_REMOVE | TAG_KEYS) {
            return Err(());
        }
        let rd16 = |o: usize| u16::from_le_bytes([frame[o], frame[o + 1]]) as usize;
        let val_len =
            u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]) as usize;
        let ns_len = rd16(1);
        let key_len = rd16(3);
        if tag != TAG_SET && val_len != 0 {
            return Err(());
        }
        let body = &frame[REQ_HEAD..];
        if body.len() != ns_len + key_len + val_len {
            return Err(());
        }
        Ok(Req {
            tag,
            ns: &body[..ns_len],
            key: &body[ns_len..ns_len + key_len],
            val: &body[ns_len + key_len..],
        })
    }

    /// OK 应答（msg 恒空；payload 见协议规范 §7 载荷约定）。
    fn reply_ok(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(REP_HEAD + payload.len());
        out.push(proto::REPLY_OK);
        out.push(0);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// MAPPED_ERR 应答（payload 恒空；msg 为诊断文案，非用户直出——前端走降级词条）。
    fn reply_err(code: u8, msg: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(REP_HEAD + msg.len());
        out.push(proto::REPLY_MAPPED_ERR);
        out.push(code);
        out.extend_from_slice(&(msg.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(msg.as_bytes());
        out
    }

    /// KvError → 协议错误码映射（单一出口，防映射漂移）。
    fn kv_err_reply(e: KvError) -> Vec<u8> {
        match e {
            KvError::Full => reply_err(proto::err::KV_FULL, "kv: journal/overflow full"),
            KvError::TooLarge => reply_err(proto::err::INVALID_ARGS, "kv: value too large"),
            KvError::BadNamespace => reply_err(proto::err::INVALID_ARGS, "kv: bad namespace"),
            KvError::BadKey => reply_err(proto::err::INVALID_ARGS, "kv: bad key"),
            KvError::Io(_) => reply_err(proto::err::INTERNAL, "kv: block device io"),
        }
    }

    /// 执行一帧：恒返回应答帧，对任意输入零 panic（fuzz 契约）。
    pub fn exec<B: BlockDevice>(store: &mut KvStore<B>, frame: &[u8]) -> Vec<u8> {
        let req = match parse(frame) {
            Ok(r) => r,
            Err(()) => return reply_err(proto::err::INVALID_ARGS, "kv: malformed frame"),
        };
        match req.tag {
            TAG_GET => match store.get(req.ns, req.key) {
                Ok(Some(v)) => {
                    let mut p = Vec::with_capacity(1 + v.len());
                    p.push(1);
                    p.extend_from_slice(&v);
                    reply_ok(&p)
                }
                Ok(None) => reply_ok(&[0]),
                Err(e) => kv_err_reply(e),
            },
            TAG_SET => match store.set(req.ns, req.key, req.val) {
                Ok(()) => reply_ok(&[]),
                Err(e) => kv_err_reply(e),
            },
            TAG_REMOVE => match store.remove(req.ns, req.key) {
                Ok(()) => reply_ok(&[]),
                Err(e) => kv_err_reply(e),
            },
            TAG_KEYS => match store.keys(req.ns) {
                Ok(ks) => {
                    // 键 ≤ KEY_MAX=48 → klen u8 安全；活键 ≤64 → count u16 安全。
                    let mut p = Vec::with_capacity(2 + ks.len() * 9);
                    p.extend_from_slice(&(ks.len() as u16).to_le_bytes());
                    for k in &ks {
                        p.push(k.len() as u8);
                        p.extend_from_slice(k);
                    }
                    reply_ok(&p)
                }
                Err(e) => kv_err_reply(e),
            },
            _ => reply_err(proto::err::INVALID_ARGS, "kv: unknown tag"),
        }
    }

    /// 应答帧结构校验（fuzz/测试用）：status 合法、长度自洽、错误码界内。
    #[cfg(test)]
    pub fn validate_reply(rep: &[u8]) -> Result<(), &'static str> {
        if rep.len() < REP_HEAD {
            return Err("帧头不完整");
        }
        let msg_len = u16::from_le_bytes([rep[2], rep[3]]) as usize;
        let payload_len = u32::from_le_bytes([rep[4], rep[5], rep[6], rep[7]]) as usize;
        if rep.len() != REP_HEAD + msg_len + payload_len {
            return Err("长度不自洽");
        }
        match rep[0] {
            proto::REPLY_OK => {}
            proto::REPLY_MAPPED_ERR => {
                if rep[1] >= proto::err::COUNT {
                    return Err("错误码越界");
                }
                if msg_len == 0 {
                    return Err("映射错误必须携带诊断文案");
                }
            }
            _ => return Err("未知状态字节"),
        }
        Ok(())
    }

    /// 请求帧组装（嵌入层翻译器的内核侧镜像；测试与文档示例共用）。
    #[cfg(test)]
    pub fn build_req(tag: u8, ns: &[u8], key: &[u8], val: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(REQ_HEAD + ns.len() + key.len() + val.len());
        out.push(tag);
        out.extend_from_slice(&(ns.len() as u16).to_le_bytes());
        out.extend_from_slice(&(key.len() as u16).to_le_bytes());
        out.extend_from_slice(&(val.len() as u32).to_le_bytes());
        out.extend_from_slice(ns);
        out.extend_from_slice(key);
        out.extend_from_slice(val);
        out
    }
}


// ---------------------------------------------------------------------------
// 宿主测试：断电恢复 ×10 零丢失 / 撕裂注入 / 满容量明确报错 / 命名空间隔离 /
// localStorage 语义边界 / 大值往返 / S2.03 命令面（帧往返/fuzz/经面断电）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::target::{KV_DATA_BASE, KV_JOURNAL_BASE};
    use super::*;
    use std::collections::BTreeMap;

    /// 内存块设备（写即落盘模型——掉电零丢失上界）。
    struct MemDisk {
        blocks: BTreeMap<u64, [u8; 512]>,
        total: u64,
    }

    impl MemDisk {
        fn new(total: u64) -> Self {
            MemDisk { blocks: BTreeMap::new(), total }
        }
    }

    impl BlockDevice for MemDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.total
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            if dst.is_empty() || dst.len() % 512 != 0 {
                return Err(BlockError::InvalidRange);
            }
            let n = (dst.len() / 512) as u64;
            if lba.checked_add(n).ok_or(BlockError::InvalidRange)? > self.total {
                return Err(BlockError::InvalidRange);
            }
            let zero = [0u8; 512];
            for (i, chunk) in dst.chunks_mut(512).enumerate() {
                let b = self.blocks.get(&(lba + i as u64)).unwrap_or(&zero);
                chunk.copy_from_slice(b);
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            if src.is_empty() || src.len() % 512 != 0 {
                return Err(BlockError::InvalidRange);
            }
            let n = (src.len() / 512) as u64;
            if lba.checked_add(n).ok_or(BlockError::InvalidRange)? > self.total {
                return Err(BlockError::InvalidRange);
            }
            for (i, chunk) in src.chunks(512).enumerate() {
                let mut b = [0u8; 512];
                b.copy_from_slice(chunk);
                self.blocks.insert(lba + i as u64, b);
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    /// 撕裂注入器：第 `torn_at` 次（1-based）写只让前 `keep_prefix` 字节
    /// 到达介质，其余保持原盘面内容（= 半扇区到达模型，fs23_disk tests
    /// CrashSim 同方法论）。fs23 槽撕裂用 keep_prefix=16（seq/blk 存活、
    /// tag/crc 缺失 → Bad → torn）；溢出槽撕裂用小前缀（值截断 → fnv 败）。
    struct TornDisk {
        inner: MemDisk,
        torn_at: usize,
        keep_prefix: usize,
        writes: usize,
    }

    impl TornDisk {
        fn wrap(inner: MemDisk, torn_at: usize, keep_prefix: usize) -> Self {
            TornDisk { inner, torn_at, keep_prefix, writes: 0 }
        }
    }

    impl BlockDevice for TornDisk {
        fn block_size(&self) -> u32 {
            512
        }
        fn capacity_blocks(&self) -> u64 {
            self.inner.capacity_blocks()
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            self.inner.read_blocks(lba, dst)
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            self.writes += 1;
            if self.writes == self.torn_at {
                let mut b = src.to_vec();
                for byte in b.iter_mut().skip(self.keep_prefix) {
                    *byte = 0;
                }
                return self.inner.write_blocks(lba, &b);
            }
            self.inner.write_blocks(lba, src)
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    fn fresh(total: u64) -> KvStore<MemDisk> {
        let mut dev = MemDisk::new(total);
        KvStore::format(&mut dev, KV_JOURNAL_BASE).unwrap();
        KvStore::open(dev, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0
    }

    #[test]
    fn kv_powercut_x10_zero_loss() {
        // 断电恢复 ×10：每轮写 3 键 + 删上轮 1 键 → 收回盘面重开 → 全量断言。
        let mut store = fresh(131_072);
        for round in 1u8..=10 {
            // 「断电重上电」：收回块设备重开（MemDisk 写即落盘 = 掉电零丢失上界）。
            let dev = store.into_device();
            store = KvStore::open(dev, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
            for k in 0..3u8 {
                let key = format!("r{}-k{}", round, k);
                let val = vec![round; 100];
                store.set(b"t", key.as_bytes(), &val).unwrap();
            }
            if round > 1 {
                let prev = format!("r{}-k0", round - 1);
                store.remove(b"t", prev.as_bytes()).unwrap();
                assert!(store.get(b"t", prev.as_bytes()).unwrap().is_none());
            }
            // 全量断言：1..=round 轮的键（除被删的 r{m<round}-k0）值逐字节精确。
            for r in 1..=round {
                for k in 0..3u8 {
                    let key = format!("r{}-k{}", r, k);
                    if k == 0 && r < round {
                        continue; // 已被后续轮删除
                    }
                    let got = store.get(b"t", key.as_bytes()).unwrap();
                    assert_eq!(got, Some(vec![r; 100]), "round {} key {}", r, key);
                }
            }
        }
    }

    #[test]
    fn kv_torn_journal_head_drops_tail_entries() {
        // 账本槽头部撕裂（fs23 槽头 seq/blk/tag/crc 清零）→ open torn≥1
        // → 该条及之后全部丢弃 → 旧值完好（fs23 撕裂语义逐条继承）。
        let mut base = MemDisk::new(131_072);
        KvStore::format(&mut base, KV_JOURNAL_BASE).unwrap();
        let mut store = KvStore::open(base, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
        store.set(b"t", b"k", b"old-value").unwrap();
        let plain = store.into_device(); // 盘面：1 条有效 SET
        // 注入器：下一次写 = 第 2 条账本槽，仅前 16B（seq/blk）到达——
        // tag/crc 区缺失 → unpack 判 Bad → torn 语义（该点及之后丢弃）。
        let torn = TornDisk::wrap(plain, 1, 16);
        let mut store = KvStore::open(torn, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
        store.set(b"t", b"k", b"new-torn-value").unwrap(); // 写返回成功，撕裂在介质侧
        let dev = store.into_device();
        let (mut store2, rep) = KvStore::open(dev, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap();
        assert!(rep.torn >= 1, "torn 必须被检出 rep={:?}", rep);
        assert_eq!(store2.get(b"t", b"k").unwrap(), Some(b"old-value".to_vec()));
    }

    #[test]
    fn kv_torn_overflow_tail_falls_back() {
        // 溢出槽尾部撕裂（值载荷 fnv 败）→ 该 SET 条目忽略 → 回退更早条目。
        let mut base = MemDisk::new(131_072);
        KvStore::format(&mut base, KV_JOURNAL_BASE).unwrap();
        let mut store = KvStore::open(base, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
        store.set(b"t", b"cfg", b"old-inline").unwrap(); // 写1：账本槽
        let plain = store.into_device();
        // 注入器：下一次写 = 溢出槽（8KiB 单次 write_blocks），仅前 200B
        // 到达介质——值载荷截断 → val_fnv 校验失败。
        let torn = TornDisk::wrap(plain, 1, 200);
        let mut store = KvStore::open(torn, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
        let big = vec![5u8; INLINE_MAX + 1]; // 353B → 溢出路径
        store.set(b"t", b"cfg", &big).unwrap(); // 溢出槽被撕裂，账本条目完整
        let dev = store.into_device();
        let (mut store2, rep) = KvStore::open(dev, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap();
        assert_eq!(rep.torn, 0, "账本槽完整（撕裂在溢出槽载荷）");
        assert_eq!(store2.get(b"t", b"cfg").unwrap(), Some(b"old-inline".to_vec()), "溢出值腐坏 → 回退旧值");
    }

    #[test]
    fn kv_full_capacity_explicit_error() {
        // 账本 64 条满：第 65 次 set → Err(Full)（明确报错，绝不静默覆盖）。
        let mut store = fresh(131_072);
        for i in 0..64u32 {
            let key = format!("k{:02}", i);
            store.set(b"t", key.as_bytes(), b"v").unwrap();
        }
        assert_eq!(store.remaining(), 0);
        assert_eq!(store.set(b"t", b"overflow", b"v"), Err(KvError::Full));
        // remove 也需账本条目 → 满 = Err(Full)。
        assert_eq!(store.remove(b"t", b"k00"), Err(KvError::Full));
        // 已满账本上既有键仍可读（真源在盘面）。
        assert_eq!(store.get(b"t", b"k00").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn kv_overflow_and_journal_full_explicit_error() {
        // 大值路径：账本与溢出区同步消耗（1 set = 1 条目 + 1 槽），
        // 第 65 次 set 两个资源同时耗尽 → Err(Full) 明确报错。
        let mut store = fresh(131_072);
        let big = vec![7u8; INLINE_MAX + 1]; // 353B → 溢出路径
        let mut err = None;
        for i in 0..66u32 {
            let key = format!("b{:02}", i);
            if let Err(e) = store.set(b"t", key.as_bytes(), &big) {
                err = Some((i, e));
                break;
            }
        }
        assert_eq!(err, Some((64, KvError::Full)), "第 65 条（索引 64）必须明确报满");
    }

    #[test]
    fn kv_namespace_isolation() {
        // 命名空间隔离：不同应用互不可见（总案开放性验收点）。
        let mut store = fresh(131_072);
        store.set(b"app-a", b"same-key", b"value-A").unwrap();
        store.set(b"app-b", b"same-key", b"value-B").unwrap();
        assert_eq!(store.get(b"app-a", b"same-key").unwrap(), Some(b"value-A".to_vec()));
        assert_eq!(store.get(b"app-b", b"same-key").unwrap(), Some(b"value-B".to_vec()));
        store.set(b"app-a", b"a-only", b"1").unwrap();
        store.set(b"app-b", b"b-only", b"2").unwrap();
        let ka = store.keys(b"app-a").unwrap();
        let kb = store.keys(b"app-b").unwrap();
        assert!(ka.contains(&b"same-key".to_vec()) && ka.contains(&b"a-only".to_vec()) && !ka.contains(&b"b-only".to_vec()));
        assert!(kb.contains(&b"same-key".to_vec()) && kb.contains(&b"b-only".to_vec()) && !kb.contains(&b"a-only".to_vec()));
        // 删 A 不影响 B。
        store.remove(b"app-a", b"same-key").unwrap();
        assert_eq!(store.get(b"app-a", b"same-key").unwrap(), None);
        assert_eq!(store.get(b"app-b", b"same-key").unwrap(), Some(b"value-B".to_vec()));
    }

    #[test]
    fn kv_semantics_edge_cases() {
        // localStorage 语义边界：remove 不存在 = no-op；get 不存在 = None；
        // 超限/坏名明确报错；同键覆盖取末值；0 长值合法；DEL 后重写生效。
        let mut store = fresh(131_072);
        assert_eq!(store.remove(b"t", b"no-such"), Ok(()), "remove 不存在键 = no-op（不消耗账本）");
        assert_eq!(store.get(b"t", b"no-such").unwrap(), None);
        assert_eq!(store.set(b"t", b"k", &vec![0u8; OVERFLOW_MAX + 1]), Err(KvError::TooLarge));
        assert_eq!(store.set(b"", b"k", b"v"), Err(KvError::BadNamespace));
        assert_eq!(store.set(b"t", b"", b"v"), Err(KvError::BadKey));
        assert_eq!(store.set(b"t", &[b'x'; KEY_MAX + 1], b"v"), Err(KvError::BadKey));
        store.set(b"t", b"k", b"v1").unwrap();
        store.set(b"t", b"k", b"v2").unwrap();
        assert_eq!(store.get(b"t", b"k").unwrap(), Some(b"v2".to_vec()));
        store.set(b"t", b"empty", b"").unwrap();
        assert_eq!(store.get(b"t", b"empty").unwrap(), Some(Vec::new()));
        store.remove(b"t", b"k").unwrap();
        store.set(b"t", b"k", b"v3").unwrap();
        assert_eq!(store.get(b"t", b"k").unwrap(), Some(b"v3".to_vec()));
        let keys = store.keys(b"t").unwrap();
        assert!(keys.contains(&b"k".to_vec()) && !keys.contains(&b"no-such".to_vec()));
    }

    #[test]
    fn kv_large_value_roundtrip() {
        // 大值溢出槽往返：30KiB（分片直写路径逐字节校验；槽 32KiB 见
        // KV_SLOT_BLOCKS 边界推导——任务65 根因攻坚）。
        let mut store = fresh(131_072);
        let big: Vec<u8> = (0..30_000u32).map(|i| (i % 251) as u8).collect();
        store.set(b"t", b"big", &big).unwrap();
        assert_eq!(store.get(b"t", b"big").unwrap(), Some(big));
        // 大值覆盖：新槽追加、旧槽成垃圾（v1 不回收，差异清单公示）。
        let big2 = vec![9u8; 30_000];
        store.set(b"t", b"big", &big2).unwrap();
        assert_eq!(store.get(b"t", b"big").unwrap(), Some(big2));
        // 边界值：恰好 OVERFLOW_MAX。
        let edge = vec![1u8; OVERFLOW_MAX];
        store.set(b"t", b"edge", &edge).unwrap();
        assert_eq!(store.get(b"t", b"edge").unwrap().unwrap().len(), OVERFLOW_MAX);
        // 跨重开持久。
        let dev = store.into_device();
        let mut store = KvStore::open(dev, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
        assert_eq!(store.get(b"t", b"edge").unwrap().unwrap().len(), OVERFLOW_MAX);
    }

    // -----------------------------------------------------------------------
    // S2.03 命令面（帧级契约，协议规范 §7）
    // -----------------------------------------------------------------------

    use crate::vport::shim_protocol as proto;
    use cmd::{TAG_GET, TAG_KEYS, TAG_REMOVE, TAG_SET};

    fn expect_ok(rep: Vec<u8>) -> Vec<u8> {
        cmd::validate_reply(&rep).unwrap();
        assert_eq!(rep[0], proto::REPLY_OK, "期望 OK，实际见上方应答");
        rep[cmd::REP_HEAD..].to_vec()
    }

    #[test]
    fn cmd_roundtrip_all_four() {
        let mut store = fresh(131_072);
        // SET → OK。
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"app", b"layout", b"deck-1")));
        // GET 命中 → found=1 + 值逐字节。
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app", b"layout", &[])));
        assert_eq!(payload[0], 1);
        assert_eq!(&payload[1..], b"deck-1");
        // GET miss → found=0；空串值必须与 miss 可区分。
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app", b"no-such", &[])));
        assert_eq!(payload, &[0]);
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"app", b"empty", b"")));
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app", b"empty", &[])));
        assert_eq!(payload, &[1], "空串 ≠ miss");
        // KEYS → count + (klen,key)*，只含本 ns。
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"app", b"second", b"v")));
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_KEYS, b"app", &[], &[])));
        let count = u16::from_le_bytes([payload[0], payload[1]]);
        assert_eq!(count, 3, "layout/empty/second");
        // REMOVE → OK；GET 回 found=0。
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_REMOVE, b"app", b"second", &[])));
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app", b"second", &[])));
        assert_eq!(payload, &[0]);
    }

    #[test]
    fn cmd_bad_frames_fuzz_1000() {
        // fuzz 契约：任意输入字节流 → 良序应答（OK | MAPPED_ERR），零 panic；
        // fuzz 不保证保留垃圾写入，终态在全新盘面上验证存储仍可用。
        let mut store = fresh(131_072);
        let mut seed: u64 = 0x243F_6A88_85A3_08D3;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            seed
        };
        let valid = cmd::build_req(TAG_SET, b"app", b"k", b"v");
        let mut mapped = 0usize;
        for i in 0..1000u32 {
            let mut frame: Vec<u8> = if i % 3 == 0 {
                let mut f = valid.clone();
                let cut = (next() as usize) % (f.len() + 8);
                f.truncate(cut.min(f.len()));
                if next() & 1 == 1 {
                    f.push((next() & 0xFF) as u8);
                }
                f
            } else {
                let n = (next() as usize) % 96;
                (0..n).map(|_| (next() & 0xFF) as u8).collect()
            };
            if i % 3 == 0 && !frame.is_empty() {
                let pos = (next() as usize) % frame.len();
                frame[pos] ^= 0xFF;
            }
            let rep = cmd::exec(&mut store, &frame);
            cmd::validate_reply(&rep).unwrap_or_else(|e| panic!("round {i}: {e}"));
            if rep[0] == proto::REPLY_MAPPED_ERR {
                mapped += 1;
            }
        }
        assert!(mapped > 0, "坏帧变异必须产出过映射错误");
        // 全新盘面：fuzz 后命令面照常工作。
        let mut clean = fresh(131_072);
        expect_ok(cmd::exec(&mut clean, &cmd::build_req(TAG_SET, b"app", b"alive", b"yes")));
    }

    #[test]
    fn cmd_full_maps_to_kv_full() {
        let mut store = fresh(131_072);
        for i in 0..64u32 {
            let key = format!("k{:02}", i);
            expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"t", key.as_bytes(), b"v")));
        }
        // 第 65 条 → MAPPED_ERR(SHIM_KV_FULL)：明确拒绝，绝不静默覆盖。
        let rep = cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"t", b"overflow", b"v"));
        cmd::validate_reply(&rep).unwrap();
        assert_eq!(rep[0], proto::REPLY_MAPPED_ERR);
        assert_eq!(rep[1], proto::err::KV_FULL);
        assert!(!proto::err::is_retryable(rep[1]));
        // 已满账本上既有键经命令面仍可读（真源在盘面）。
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"t", b"k00", &[])));
        assert_eq!(payload, &[1, b'v']);
    }

    #[test]
    fn cmd_arg_violations_map_invalid_args() {
        let mut store = fresh(131_072);
        // ns 空 / ns 17B / key 空 / key 49B：结构合法帧 + store 校验 → INVALID_ARGS。
        let cases: [(&[u8], &[u8]); 4] = [
            (b"", b"k"),
            (&[b'a'; 17], b"k"),
            (b"ns", b""),
            (b"ns", &[b'k'; 49]),
        ];
        for (ns, key) in cases {
            let rep = cmd::exec(&mut store, &cmd::build_req(TAG_SET, ns, key, b"v"));
            cmd::validate_reply(&rep).unwrap();
            assert_eq!(rep[1], proto::err::INVALID_ARGS, "ns={:?} key len={}", ns, key.len());
        }
        // 值超溢出上限 → TooLarge → INVALID_ARGS（值约束）。
        let big = vec![7u8; OVERFLOW_MAX + 1];
        let rep = cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"t", b"big", &big));
        assert_eq!(rep[1], proto::err::INVALID_ARGS);
        // 非 SET 带 val_len → 坏帧。
        let mut f = cmd::build_req(TAG_GET, b"t", b"k", &[]);
        f[5] = 1;
        let rep = cmd::exec(&mut store, &f);
        assert_eq!(rep[1], proto::err::INVALID_ARGS);
        // 未知 tag → 坏帧（本层不产生 MISSING——tag 白名单在 parse）。
        let mut f = cmd::build_req(TAG_GET, b"t", b"k", &[]);
        f[0] = 200;
        let rep = cmd::exec(&mut store, &f);
        assert_eq!(rep[1], proto::err::INVALID_ARGS);
        // 尾部垃圾 → 坏帧（长度必须精确匹配）。
        let mut f = cmd::build_req(TAG_GET, b"t", b"k", &[]);
        f.push(0);
        let rep = cmd::exec(&mut store, &f);
        assert_eq!(rep[1], proto::err::INVALID_ARGS);
    }

    #[test]
    fn cmd_powercut_x10_zero_loss() {
        // 经命令面的断电 ×10：每轮「收回盘面重开」→ 写 3 键 → 命令面回读精确
        // → 上一轮首键跨断电零丢失。
        let mut store = fresh(131_072);
        for round in 1u8..=10 {
            let dev = store.into_device();
            store = KvStore::open(dev, KV_JOURNAL_BASE, KV_DATA_BASE).unwrap().0;
            for k in 0..3u8 {
                let key = format!("r{}-k{}", round, k);
                let val = vec![round; 80];
                expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"t", key.as_bytes(), &val)));
            }
            for k in 0..3u8 {
                let key = format!("r{}-k{}", round, k);
                let expect = vec![round; 80];
                let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"t", key.as_bytes(), &[])));
                assert_eq!(&payload[1..], &expect[..], "round {} key {}", round, key);
            }
            if round > 1 {
                let key = format!("r{}-k0", round - 1);
                let expect = vec![round - 1; 80];
                let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"t", key.as_bytes(), &[])));
                assert_eq!(&payload[1..], &expect[..], "跨断电零丢失 round {}", round);
            }
        }
    }

    #[test]
    fn cmd_namespace_isolation() {
        let mut store = fresh(131_072);
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"app-a", b"same-key", b"from-a")));
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_SET, b"app-b", b"same-key", b"from-b")));
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app-a", b"same-key", &[])));
        assert_eq!(&payload[1..], b"from-a");
        // KEYS 只列本 ns。
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_KEYS, b"app-b", &[], &[])));
        assert_eq!(u16::from_le_bytes([payload[0], payload[1]]), 1);
        assert_eq!(&payload[3..], b"same-key");
        // 删 A 不影响 B。
        expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_REMOVE, b"app-a", b"same-key", &[])));
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app-a", b"same-key", &[])));
        assert_eq!(payload, &[0]);
        let payload = expect_ok(cmd::exec(&mut store, &cmd::build_req(TAG_GET, b"app-b", b"same-key", &[])));
        assert_eq!(&payload[1..], b"from-b");
    }
}

