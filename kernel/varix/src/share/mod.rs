//! TRINITY-500 · AI-04 共享卷数据协议域（F076~F100，W1）
//!
//! 子模块：[`layout`] 定义目录树与各类契约，[`consistency`] 定义协议层
//! （版本/校验和/写锁/仲裁/审计/测试夹具）。本文件做域自检收口。

pub mod consistency;
pub mod layout;

use crate::checks::CheckSet;

/// AI-04 域自检：F076~F100 逐项登记。
pub fn run_share_checks() -> CheckSet {
    let mut set = CheckSet::new("share");

    set.add(
        "F076 directory tree",
        layout::tree_complete(&layout::TREE) && layout::TreeKind::Docs.path() == "/docs",
        "docs/media/config/apps/vault/trash",
    );

    let mut p = [0u8; 64];
    let n = layout::doc_path(layout::DocKind::Mind, "m1", &mut p).unwrap_or(0);
    set.add(
        "F077 document contract",
        n > 0 && core::str::from_utf8(&p[..n]).unwrap() == "/docs/minds/m1.vmind",
        "records/minds/fates/code",
    );

    let mut mp = [0u8; 96];
    set.add(
        "F078 media contract",
        layout::media_path(layout::MediaKind::Image, 0x12, ".png", &mut mp).is_some()
            && layout::MediaKind::Video.needs_large_fs(),
        "sharded media paths",
    );

    let mut st = layout::SettingsTable::new();
    st.set(0, 1);
    set.add(
        "F079 74 shared settings",
        layout::SettingsTable::capacity_ok()
            && layout::SETTING_COUNT == 74
            && st.get(0) == Some(1)
            && st.get(74).is_none(),
        "group capacity sums to 74",
    );

    let mut ad = [0u8; 64];
    let an = layout::app_data_dir("gimp", &mut ad).unwrap_or(0);
    set.add(
        "F080 portable app data dir",
        an > 0 && core::str::from_utf8(&ad[..an]).unwrap() == "/apps/gimp/data",
        "only data crosses the bridge",
    );

    set.add(
        "F081 executable manifest",
        layout::binary_allowed(layout::BinaryHost::Windows, ".exe")
            && layout::binary_allowed(layout::BinaryHost::Varix, ".elf")
            && !layout::binary_allowed(layout::BinaryHost::Varix, ".exe")
            && !layout::binary_allowed(layout::BinaryHost::Windows, ".elf"),
        "abi wall",
    );

    let rec = consistency::SharedRecord::new(3, 1, 0xAA, 4, 0);
    let mut buf = [0u8; consistency::RECORD_LEN];
    rec.encode(&mut buf);
    set.add(
        "F082 consistency protocol",
        consistency::SharedRecord::decode(&buf) == Some(rec) && consistency::version_compatible(consistency::PROTOCOL_VERSION),
        "version + crc + seq",
    );

    set.add(
        "F083 conflict arbitration",
        consistency::arbitrate(2, 1, 0, 0) == consistency::Resolution::KeepLocal
            && consistency::arbitrate(1, 1, 1, 1) == consistency::Resolution::ForkBoth,
        "seq then timestamp",
    );

    let mut bus = consistency::ChangeBus::new();
    let seq = bus.publish(7, consistency::ChangeKind::Created);
    let mut evs = [consistency::ChangeEvent { path_hash: 0, kind: consistency::ChangeKind::Created, seq: 0 }; 4];
    set.add("F084 change notification", bus.drain(seq, &mut evs) == 0 && bus.drain(0, &mut evs) == 1, "drain since seq");

    let t = layout::TrashEntry { path_hash: 1, size: 1, deleted_ms: 0, owner: 2 };
    set.add(
        "F085 recycle bin alignment",
        layout::trash_bucket(2) == "/$RECYCLE.BIN/variable" && layout::trash_expired(&t, 86_400_000u64 * 31),
        "per-system buckets",
    );

    let env = consistency::VaultEnvelope::new(consistency::ALG_AES_256_GCM, [0u8; 12], [0u8; 16], 64);
    let mut vbuf = [0u8; consistency::VAULT_HEADER_LEN];
    env.encode(&mut vbuf);
    set.add(
        "F086 vault ciphertext format",
        consistency::VaultEnvelope::decode(&vbuf).unwrap().acceptable(),
        "aes-256-gcm envelope",
    );

    set.add(
        "F087 clipboard protocol",
        consistency::clipboard_allowed(consistency::ClipKind::Text, 8)
            && !consistency::clipboard_allowed(consistency::ClipKind::Image, consistency::CLIPBOARD_MAX_BYTES as u32 + 1),
        "4MiB cap, no silent truncation",
    );

    let mut recent = consistency::RecentList::new();
    recent.push(1, 1);
    recent.push(2, 2);
    let mut top = [0u64; 4];
    set.add("F088 recent files sync", recent.top(1, &mut top) == 1 && top[0] == 2, "newest first");

    let chain = [
        consistency::ChainNode { crc: 11, parent: 0 },
        consistency::ChainNode { crc: 22, parent: 11 },
    ];
    set.add(
        "F089 checksum trust chain",
        consistency::chain_ok(&chain) && consistency::chain_tip(&chain) == 22,
        "parent links previous crc",
    );

    set.add(
        "F090 migration wizard",
        layout::may_commit(&layout::MigrationPlan { items: 1, bytes: 1, conflict: false }, true)
            && !layout::may_commit(&layout::MigrationPlan { items: 1, bytes: 1, conflict: true }, true)
            && layout::MigrateStep::Scan.next() == Some(layout::MigrateStep::Plan),
        "verify before commit",
    );

    set.add("F091 protocol self-check", set.all_passed(), "entry point");

    let mut audit = consistency::AuditLog::new();
    audit.append(consistency::AuditEntry { who: 1, path_hash: 0, stamp_ms: 0, allowed: false });
    set.add("F092 audit log", audit.len() == 1 && audit.denied_count() == 1, "denials recorded");

    set.add(
        "F093 access boundary",
        layout::access_for("/vault/x", false) == layout::Access::Read
            && layout::access_for("/vault/x", true) == layout::Access::ReadWrite
            && layout::access_for("/apps-bin/varix/a.elf", false) == layout::Access::Read,
        "ciphertext-only vault",
    );

    let mut sr = consistency::StreamReader::new(3_000_000, 1_000_000);
    set.add(
        "F094 streaming IO",
        sr.chunk_count() == 3 && sr.next_chunk() == 1_000_000 && !sr.done(),
        "fixed chunk walk",
    );

    set.add(
        "F095 version compat matrix",
        consistency::compat(1, 1) == consistency::Compat::Full
            && consistency::compat(1, 2) == consistency::Compat::ReadOnly
            && consistency::compat(1, 101) == consistency::Compat::Incompatible,
        "major/minor rules",
    );

    set.add(
        "F096 performance budget",
        consistency::io_verdict(consistency::IO_REDLINE_US) == consistency::IoVerdict::Within
            && consistency::p95_us(&[1, 2, 3, 25_000]) == 25_000,
        "20ms random read redline",
    );

    let mut journal = [
        consistency::journal_entry(1),
        consistency::journal_entry(2),
        consistency::journal_entry(3),
    ];
    journal[2].committed = false;
    let mut applied = [0u32; consistency::MAX_JOURNAL];
    set.add(
        "F097 power-loss consistency",
        consistency::replay(&journal, &mut applied) == 2,
        "committed prefix only",
    );

    let stress = consistency::stress_round(3, 24, 4);
    set.add(
        "F098 multi-system stress",
        stress.corrupt == 0 && stress.applied == 20 && stress.conflicts > 0,
        "no corruption under contention",
    );

    let mut diag = consistency::ShareDiag::new();
    diag.inspect(true, &chain, &audit, 10_000, true);
    set.add("F099 share diagnostics", diag.len() == 0, "clean run reports nothing");

    set.add("F100 share domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f100_domain_self_test_is_green() {
        let set = run_share_checks();
        assert_eq!(set.len(), 25);
        assert!(set.all_passed(), "share domain self-test must pass");
    }
}
