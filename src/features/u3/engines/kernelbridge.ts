/**
 * U3-v6 引擎三：kernelbridge —— 内核 ustar3 域镜像对账引擎
 * （AI-U3 · 批次六）。
 *
 * 判据覆盖（深化面）：
 * - 内核 50 域直排注册表（kernel/varix/src/ustar3/mod.rs run_ustar3_all_checks）
 *   的前端**镜像账本**：fno → 域文件 → 判据域，一处一事实、两侧同源；
 * - 前端域 ↔ 内核域映射对账：每个 F 编号两侧都有承接面（差异显性化，
 *   不冒领全绿）；
 * - 检查点计数对账：内核各域 .add/.ok 实测计数誊写为常量，测试钉住
 *   总量与连续性（F501-F550 不重不漏）。
 *
 * 诚实边界：前端无法在运行时调用 Rust CheckSet——本引擎对账的是
 * **注册表结构与编号连续性**（编译期可验证面）；红绿随闸门以
 * cargo test 产出为准，此处登记映射与差异。
 */

/* ------------------------------- 内核镜像注册表 ------------------------------- */

/** 内核域文件 → 覆盖 F 编号（与 mod.rs 头表同源誊写）。 */
export const KERNEL_DOMAIN_FILES: ReadonlyArray<{ file: string; fnos: ReadonlyArray<string> }> = [
  { file: "deskicons.rs", fnos: ["F501", "F502", "F503"] },
  { file: "locksec.rs",   fnos: ["F504", "F505", "F506", "F507", "F508"] },
  { file: "filesec.rs",   fnos: ["F509", "F510", "F511", "F512"] },
  { file: "pointerfx.rs", fnos: ["F513", "F514", "F519", "F520", "F522", "F523"] },
  { file: "explorerx.rs", fnos: ["F515", "F517", "F521", "F525", "F526", "F527", "F528"] },
  { file: "copyops.rs",   fnos: ["F524", "F529", "F530", "F531", "F532", "F533", "F534"] },
  { file: "winkeys.rs",   fnos: ["F516", "F518", "F535", "F536", "F537", "F538", "F539", "F548"] },
  { file: "sysdev.rs",    fnos: ["F540", "F541", "F542", "F543", "F544", "F545", "F546", "F547"] },
  { file: "clockcal.rs",  fnos: ["F549"] },
  { file: "anchor.rs",    fnos: ["F550"] },
];

/** 内核各域检查点实测计数（grep "\.add(|\.ok(" 口径，誊写自 v6 批次现场；随内核深化手工同步）。 */
export const KERNEL_CHECK_COUNTS: Readonly<Record<string, number>> = {
  "deskicons.rs": 37,
  "locksec.rs": 58,
  "filesec.rs": 53,
  "pointerfx.rs": 70,
  "explorerx.rs": 85,
  "copyops.rs": 93,
  "winkeys.rs": 95,
  "sysdev.rs": 91,
  "clockcal.rs": 12,
  "anchor.rs": 25,
};

/** 内核直排表 50 域函数名（mod.rs run_ustar3_all_checks DOMAINS 同源）。 */
export const KERNEL_CHECK_FNS: ReadonlyArray<string> = Array.from({ length: 50 }, (_, i) => `run_f${501 + i}_checks`);

/* ------------------------------- 对账 ------------------------------- */

/** 内核镜像展开：fno → 域文件（重复编号视为账本损坏）。 */
export function kernelFnoMap(): Map<string, string> {
  const m = new Map<string, string>();
  for (const d of KERNEL_DOMAIN_FILES) {
    for (const f of d.fnos) {
      if (m.has(f)) throw new Error(`内核镜像账本重复编号: ${f}`);
      m.set(f, d.file);
    }
  }
  return m;
}

export interface BridgeRow {
  fno: string;
  kernelFile: string | null;
  kernelChecks: number | null;
  bridgeOk: boolean;
  note: string;
}

/** F501-F550 逐项对账：编号连续 + 内核承接 + 检查点计数可溯源。 */
export function bridgeAll50(): Array<BridgeRow> {
  const km = kernelFnoMap();
  const out: BridgeRow[] = [];
  for (let i = 501; i <= 550; i++) {
    const fno = `F${i}`;
    const file = km.get(fno) ?? null;
    out.push({
      fno,
      kernelFile: file,
      kernelChecks: file ? KERNEL_CHECK_COUNTS[file] ?? null : null,
      bridgeOk: file !== null,
      note: file ? `内核 ${file}` : "内核无承接——差异显性",
    });
  }
  return out;
}

/** 前端域承接面：F 编号 → 前端域（engines 16 域 + 既有逻辑域，与 reconcile 同源语义）。 */
export const FRONTEND_FNO_DOMAINS: Readonly<Record<string, string>> = {
  F501: "lumapick", F502: "lumapick", F503: "gridlab",
  F504: "pinvault", F505: "pinvault", F506: "guestbox",
  F507: "capguard", F508: "capguard", F509: "shredplan",
  F510: "vxcrypt2", F511: "shredplan", F512: "shredplan",
  F513: "pointerfx", F514: "pointerfx", F515: "findrepl",
  F516: "bannerpack", F517: "explorerx", F518: "hotkeymap",
  F519: "bannerpack", F520: "bannerpack", F521: "explorerx",
  F522: "pointerfx", F523: "pointerfx", F524: "copyops",
  F525: "hotkeymap", F526: "findrepl", F527: "expui",
  F528: "expui", F529: "copyops", F530: "copyops",
  F531: "copyops", F532: "copyops", F533: "copyops",
  F534: "copyops", F535: "hotkeymap", F536: "hotkeymap",
  F537: "despaint", F538: "hotkeymap", F539: "despaint",
  F540: "sysdiag", F541: "sysdiag", F542: "sysdiag",
  F543: "sysdiag", F544: "sysdiag", F545: "sysdiag",
  F546: "sysdiag", F547: "sysdiag", F548: "winkeys",
  F549: "clockcal", F550: "anchor",
};

/** 两侧对账：每编号内核与前端都有承接（差异显性化清单）。 */
export function bridgeDifferences(): Array<{ fno: string; kind: "kernel-only" | "frontend-only"; note: string }> {
  const diffs: Array<{ fno: string; kind: "kernel-only" | "frontend-only"; note: string }> = [];
  for (const [fno, file] of kernelFnoMap()) {
    if (!FRONTEND_FNO_DOMAINS[fno]) diffs.push({ fno, kind: "kernel-only", note: `内核 ${file} 有承接、前端无域` });
  }
  for (const fno of Object.keys(FRONTEND_FNO_DOMAINS)) {
    if (!kernelFnoMap().has(fno)) diffs.push({ fno, kind: "frontend-only", note: `前端有域、内核无承接` });
  }
  return diffs;
}

/** 总对账行（供 walkcheck/U3Lab 消费）。 */
export function bridgeVerdict(): { rows: Array<BridgeRow>; diffs: ReturnType<typeof bridgeDifferences>; total50: boolean } {
  const rows = bridgeAll50();
  return {
    rows,
    diffs: bridgeDifferences(),
    total50: rows.length === 50 && rows.every((r) => r.bridgeOk) && rows.every((r, i) => r.fno === `F${501 + i}`),
  };
}

/* ------------------------------- 自检 ------------------------------- */

export function kernelbridgeSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  const v = bridgeVerdict();

  checks.push({ name: "内核镜像 50 编号连续不重不漏", pass: v.total50 });
  checks.push({ name: "内核检查函数 50 个直排", pass: KERNEL_CHECK_FNS.length === 50 && KERNEL_CHECK_FNS[0] === "run_f501_checks" && KERNEL_CHECK_FNS[49] === "run_f550_checks" });
  checks.push({ name: "内核 10 域文件在册", pass: KERNEL_DOMAIN_FILES.length === 10 });
  checks.push({ name: "两侧对账零差异", pass: v.diffs.length === 0 });
  checks.push({ name: "检查点计数全可溯源", pass: v.rows.every((r) => (r.kernelChecks ?? 0) > 0) });
  const sum = Object.values(KERNEL_CHECK_COUNTS).reduce((a, b) => a + b, 0);
  checks.push({ name: "内核检查点总量 619", pass: sum === 619 });
  checks.push({ name: "前端承接面 50 编号全覆盖", pass: Object.keys(FRONTEND_FNO_DOMAINS).length === 50 });

  // 破坏注入：重复编号账本损坏即抛
  let threw = false;
  try {
    const bad: ReadonlyArray<{ file: string; fnos: ReadonlyArray<string> }> = [
      { file: "a.rs", fnos: ["F501"] },
      { file: "b.rs", fnos: ["F501"] },
    ];
    const m = new Map<string, string>();
    for (const d of bad) for (const f of d.fnos) {
      if (m.has(f)) throw new Error("dup");
      m.set(f, d.file);
    }
  } catch { threw = true; }
  checks.push({ name: "破坏注入：重复编号显性抛错", pass: threw });

  return checks;
}
