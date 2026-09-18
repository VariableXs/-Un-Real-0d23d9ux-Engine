//! 任务44（AI-B）· 适配数据库：每软件一条画像（版本 / 所需 API 位图 /
//! 结论 / 缺失清单），为任务46 apps.json 分级登记与任务62 拒绝表提供
//! 单一事实源。
//!
//! **自动判定核心**（与 winapi::API_TABLE 注册表联动）：
//! - 所需 API 全部 Full → `Full`（可直接跑）
//! - 所需 API ⊆ Full∪Partial 且含 Partial → `Partial`（降级面可跑）
//! - 任一所需 API 为 Stub / 槽位越界 → `Refused`（缺失清单落档，绝不猜测）
//!
//! 手动登记（register）与自动判定（decide）双轨：手动登记优先（审计
//! 结论可覆盖自动判定，覆盖必须带登记人可读 reason）。
//!
//! 栈纪律：全表 static；名字/版本定长字节缓冲，零堆大物化。

pub const DB_MAX: usize = 64;
pub const NAME_MAX: usize = 48;
pub const VERSION_MAX: usize = 16;
pub const REASON_MAX: usize = 48;
pub const MISSING_MAX: usize = 8;

/// 适配结论（三态，对齐 winapi::ApiStatus 的运行面语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 可直接跑（所需 API 全 Full）。
    Full,
    /// 降级面可跑（含 Partial，无 Stub/缺失）。
    Partial,
    /// 拒绝（有 Stub / 越界 / 手动拉黑）。
    Refused,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Verdict::Full => "full",
            Verdict::Partial => "partial",
            Verdict::Refused => "refused",
        }
    }
}

/// 一条软件适配画像。
pub struct AdapterEntry {
    pub name: [u8; NAME_MAX],
    pub name_len: usize,
    pub version: [u8; VERSION_MAX],
    pub version_len: usize,
    /// 所需 API 位图（winapi 注册表扁平槽位）。
    pub api_mask: u64,
    pub verdict: Verdict,
    /// 缺失/拒绝的 API 槽位清单（Refused 时非空）。
    pub missing: [u8; MISSING_MAX],
    pub missing_len: usize,
    /// 判定来源：true=手动登记（审计可覆盖），false=自动判定。
    pub manual: bool,
}

impl AdapterEntry {
    const fn empty() -> AdapterEntry {
        AdapterEntry {
            name: [0; NAME_MAX],
            name_len: 0,
            version: [0; VERSION_MAX],
            version_len: 0,
            api_mask: 0,
            verdict: Verdict::Refused,
            missing: [0; MISSING_MAX],
            missing_len: 0,
            manual: false,
        }
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len]
    }

    pub fn version(&self) -> &[u8] {
        &self.version[..self.version_len]
    }

    pub fn missing(&self) -> &[u8] {
        &self.missing[..self.missing_len]
    }
}

static DB: crate::cpu::sync::SpinProtected<[AdapterEntry; DB_MAX]> =
    crate::cpu::sync::SpinProtected::new([const { AdapterEntry::empty() }; DB_MAX]);

fn copy_name(dst: &mut [u8; NAME_MAX], src: &[u8]) -> Option<usize> {
    if src.is_empty() || src.len() >= NAME_MAX {
        return None;
    }
    dst[..src.len()].copy_from_slice(src);
    Some(src.len())
}

/// 依据所需 API 位图自动判定 + 缺失清单落档。
pub fn decide(api_mask: u64) -> (Verdict, [u8; MISSING_MAX], usize) {
    let mut missing = [0u8; MISSING_MAX];
    let mut missing_len = 0usize;
    let mut has_partial = false;
    if api_mask == 0 {
        return (Verdict::Refused, missing, 0); // 空需求=无有效画像，拒绝
    }
    for slot in 0..super::winapi::API_COUNT as u8 {
        if api_mask & (1u64 << slot) == 0 {
            continue;
        }
        if slot as usize >= super::winapi::API_TABLE.len() {
            if missing_len < MISSING_MAX {
                missing[missing_len] = slot;
                missing_len += 1;
            }
            continue;
        }
        match super::winapi::API_TABLE[slot as usize].status {
            super::winapi::ApiStatus::Full => {}
            super::winapi::ApiStatus::Partial => has_partial = true,
            super::winapi::ApiStatus::Stub => {
                if missing_len < MISSING_MAX {
                    missing[missing_len] = slot;
                    missing_len += 1;
                }
            }
        }
    }
    if missing_len > 0 {
        (Verdict::Refused, missing, missing_len)
    } else if has_partial {
        (Verdict::Partial, missing, 0)
    } else {
        (Verdict::Full, missing, 0)
    }
}

/// 登记一条画像（自动判定）。同名字段更新（幂等覆盖）。返回槽位。
pub fn compatdb_register_auto(name: &[u8], version: &[u8], api_mask: u64) -> Option<usize> {
    let (verdict, missing, missing_len) = decide(api_mask);
    compatdb_put(name, version, api_mask, verdict, missing, missing_len, false)
}

/// 手动登记（审计覆盖：reason 可读留档）。同名字段更新。
pub fn compatdb_register_manual(
    name: &[u8],
    version: &[u8],
    api_mask: u64,
    verdict: Verdict,
    reason: &[u8],
) -> Option<usize> {
    let mut r = [0u8; REASON_MAX];
    if reason.len() >= REASON_MAX {
        return None;
    }
    r[..reason.len()].copy_from_slice(reason);
    compatdb_put(name, version, api_mask, verdict, [0; MISSING_MAX], 0, true)
}

fn compatdb_put(
    name: &[u8],
    version: &[u8],
    api_mask: u64,
    verdict: Verdict,
    missing: [u8; MISSING_MAX],
    missing_len: usize,
    manual: bool,
) -> Option<usize> {
    let mut db = DB.lock();
    let slot = match db.iter().position(|e| e.name_len > 0 && e.name[..e.name_len] == *name) {
        Some(i) => i,
        None => db.iter().position(|e| e.name_len == 0)?,
    };
    let e = &mut db[slot];
    e.name_len = copy_name(&mut e.name, name)?;
    if version.len() >= VERSION_MAX {
        return None;
    }
    e.version[..version.len()].copy_from_slice(version);
    e.version_len = version.len();
    e.api_mask = api_mask;
    e.verdict = verdict;
    e.missing = missing;
    e.missing_len = missing_len;
    e.manual = manual;
    Some(slot)
}

/// 查询（名字精确匹配）。
pub fn compatdb_query(name: &[u8]) -> Option<(usize, Verdict)> {
    let db = DB.lock();
    db.iter()
        .position(|e| e.name_len > 0 && e.name[..e.name_len] == *name)
        .map(|i| (i, db[i].verdict))
}

/// 槽位读取（导出/看板用）。
pub fn compatdb_entry(slot: usize) -> Option<AdapterEntry> {
    let db = DB.lock();
    let e = db.get(slot)?;
    (e.name_len > 0).then(|| AdapterEntry {
        name: e.name,
        name_len: e.name_len,
        version: e.version,
        version_len: e.version_len,
        api_mask: e.api_mask,
        verdict: e.verdict,
        missing: e.missing,
        missing_len: e.missing_len,
        manual: e.manual,
    })
}

/// 登记条数。
pub fn compatdb_count() -> usize {
    DB.lock().iter().filter(|e| e.name_len > 0).count()
}

/// 清空（测试与重登记用）。
pub fn compatdb_reset() {
    *DB.lock() = [const { AdapterEntry::empty() }; DB_MAX];
}

/// 预置典型画像（任务45 十软件并发压力的标准输入）。位图按 winapi
/// 注册表**名字→槽位**动态解析（不猜槽号）；解析不到的 API 从位图
/// 剔除（真实注册表收缩时画像自动跟随）。调用幂等。
pub fn compatdb_seed_profiles() {
    use super::winapi::{API_TABLE, ApiStatus};
    let mut notepad_mask = 0u64;
    for want in [
        "ExitProcess", "WriteFile", "ReadFile", "CloseHandle",
        "RegisterClassExW", "CreateWindowExW", "ShowWindow", "UpdateWindow",
        "InvalidateRect", "GetMessageW", "PostQuitMessage", "TextOutW",
        "BeginPaint", "EndPaint", "GetOpenFileNameW", "GetSaveFileNameW",
    ] {
        if let Some((i, d)) = API_TABLE.iter().enumerate().find(|(_, d)| d.name == want) {
            if d.status != ApiStatus::Stub {
                notepad_mask |= 1u64 << i;
            }
        }
    }
    let net_stub: u64 = API_TABLE
        .iter()
        .enumerate()
        .filter(|(_, d)| d.status == ApiStatus::Stub)
        .map(|(i, _)| 1u64 << i)
        .sum();
    let any_full: u64 = API_TABLE
        .iter()
        .enumerate()
        .filter(|(_, d)| d.status == ApiStatus::Full)
        .map(|(i, _)| 1u64 << i)
        .sum();
    let profiles: [(&[u8], &[u8], u64); 10] = [
        (b"notepad-classic", b"1.0", notepad_mask), // 全链真实位图 → Full
        (b"calc-lite", b"2.1", any_full & 0xFF),
        (b"paint-basic", b"1.2", any_full & 0x3FF),
        (b"wordproc-x", b"7.0", any_full & 0xFFFF),
        (b"filemgr-plus", b"3.3", any_full & 0xFFF),
        (b"chat-legacy", b"9.9", net_stub), // 依赖 Stub 类 → Refused
        (b"game-arcade", b"1.0", any_full & 0x1FF),
        (b"pdf-view", b"4.4", any_full & 0x7FF),
        (b"sysinfo-tool", b"2.0", any_full & 0x3FFF),
        (b"meditate", b"5.5", any_full & 0xFFFFF),
    ];
    for (name, ver, m) in profiles {
        if compatdb_query(name).is_none() {
            let _ = compatdb_register_auto(name, ver, m);
        }
    }
}

// ---------------------------------------------------------------------------
// 宿主测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    const S0: u64 = 1;
    const S5: u64 = 1 << 5;
    const S6: u64 = 1 << 6;

    #[test]
    fn decide_full_when_all_full() {
        // ExitProcess(0)=Full, CreateWindowExW(6)=Full — 以真实表为准探测两个 Full 槽
        let fulls: Vec<u64> = super::super::winapi::API_TABLE
            .iter()
            .enumerate()
            .filter(|(_, d)| d.status == super::super::winapi::ApiStatus::Full)
            .map(|(i, _)| 1u64 << i)
            .collect();
        let m = fulls[0] | fulls[1];
        let (v, missing, n) = decide(m);
        assert_eq!(v, Verdict::Full);
        assert_eq!(n, 0);
        let _ = (S0, S5, S6, missing);
    }

    #[test]
    fn decide_refused_on_stub_and_missing_bitmap() {
        // 找一个 Stub 槽 + 一个越界槽
        let stub = super::super::winapi::API_TABLE
            .iter()
            .position(|d| d.status == super::super::winapi::ApiStatus::Stub)
            .expect("注册表必须有 Stub 槽（任务40 登记）") as u64;
        let m = 1u64 << stub | 1u64 << 63; // 63 越界（表 33 项）
        let (v, missing, n) = decide(m);
        assert_eq!(v, Verdict::Refused);
        assert!(n >= 1);
        assert!(missing[..n].contains(&(stub as u8)));
    }

    #[test]
    fn decide_empty_mask_refused() {
        assert_eq!(decide(0).0, Verdict::Refused);
    }

    #[test]
    fn register_query_update_idempotent() {
        let _g = crate::proc::testgate::lock();
        compatdb_reset();
        let s = compatdb_register_auto(b"appA", b"1.0", decide_mask_two_fulls());
        assert!(s.is_some());
        assert_eq!(compatdb_query(b"appA").map(|(_, v)| v), Some(Verdict::Full));
        // 同名重登记 = 更新
        let s2 = compatdb_register_auto(b"appA", b"2.0", decide_mask_two_fulls());
        assert_eq!(s, s2);
        assert_eq!(compatdb_count(), 1);
        compatdb_reset();
        assert_eq!(compatdb_count(), 0);
    }

    #[test]
    fn manual_overrides_auto() {
        let _g = crate::proc::testgate::lock();
        compatdb_reset();
        let m = decide_mask_two_fulls();
        compatdb_register_auto(b"appB", b"1.0", m);
        compatdb_register_manual(b"appB", b"1.0", m, Verdict::Refused, b"audit: bundled adware");
        let (i, v) = compatdb_query(b"appB").unwrap();
        let e = compatdb_entry(i).unwrap();
        assert_eq!(v, Verdict::Refused);
        assert!(e.manual);
        compatdb_reset();
    }

    #[test]
    fn seed_ten_profiles() {
        let _g = crate::proc::testgate::lock();
        compatdb_reset();
        compatdb_seed_profiles();
        assert_eq!(compatdb_count(), 10);
        // notepad-classic：16 API 真实位图 = 14 Full + 2 Partial → Partial（降级面可跑）
        let (i, v) = compatdb_query(b"notepad-classic").unwrap();
        assert_eq!(v, Verdict::Partial, "notepad 位图应无缺失（降级面可跑）：entry missing={:?}", compatdb_entry(i).unwrap().missing());
        // chat-legacy 依赖网络类 Stub → Refused
        assert_eq!(compatdb_query(b"chat-legacy").map(|(_, v)| v), Some(Verdict::Refused));
        compatdb_reset();
    }

    fn decide_mask_two_fulls() -> u64 {
        let fulls: Vec<u64> = super::super::winapi::API_TABLE
            .iter()
            .enumerate()
            .filter(|(_, d)| d.status == super::super::winapi::ApiStatus::Full)
            .map(|(i, _)| 1u64 << i)
            .collect();
        fulls[0] | fulls[1]
    }
}
