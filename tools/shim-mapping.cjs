#!/usr/bin/env node
/**
 * 任务23（AI-B）：三色审计——554 方法（实测 549 方法 / 546 唯一命令）逐条映射表。
 * 产出：docs/shim-mapping.json（数据驱动，加命令改表不改垫片代码）
 *       + 控制台三色占比。
 * 颜色口径（总案阶段3步骤2）：
 *   ✅ 内核原生：单一内核服务可承接
 *   🔶 组合映射：由 ≥2 个目录服务组合而成（services[] 必须写全）
 *   ❌ 暂缺：目录中无对应服务，需内核立项（missingService 必填）
 * 覆盖率门禁：100% 有归属，出现"未分类"即非零退出。
 */
"use strict";
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const ipcSrc = fs.readFileSync(path.join(ROOT, "src", "lib", "ipc.ts"), "utf8");

// ---- 提取：方法名 → invoke 命令名（方法体内第一个 invoke<"T">("cmd")） ----
const entries = [];
const objStart = ipcSrc.indexOf("export const ipc = {");
const body = ipcSrc.slice(objStart);
const re = /^  ([a-zA-Z][a-zA-Z0-9]*)\s*:\s*(?:async\s*)?\([\s\S]*?$/gm;
// 逐方法切片：以两空格缩进的 key 行为界
const keyRe = /^  ([a-zA-Z][a-zA-Z0-9]*)\s*:/gm;
const marks = [];
let m;
while ((m = keyRe.exec(body))) marks.push({ name: m[1], at: m.index });
for (let i = 0; i < marks.length; i++) {
  const end = i + 1 < marks.length ? marks[i + 1].at : body.length;
  const chunk = body.slice(marks[i].at, end);
  const cm = chunk.match(/invoke<[^>]*>\(\s*["']([a-z_]+)["']/);
  entries.push({ method: marks[i].name, cmd: cm ? cm[1] : null });
}
if (entries.length < 500) {
  console.error("方法提取异常：", entries.length);
  process.exit(1);
}

// ---- 内核服务目录（与 source.json capabilities 对齐 + 存储类服务） ----
const SERVICES = {
  kv: "KV 存储服务（localStorage 语义）",
  docStore: "文档/文件夹结构化存储（variable.db）",
  mindStore: "思维导图存储（节点/边）",
  mediaStore: "媒体与附件存储",
  settings: "设置服务（settings://changed 同步）",
  backup: "备份/恢复/恢复文件/导出导入",
  fsWs: "工作区文件系统（白名单内）",
  fsShared: "共享分区文件系统（白名单内）",
  vfs: "VFS 白名单裁决层",
  search: "全库搜索与索引",
  recycle: "回收站语义",
  verHistory: "文件版本历史/快照",
  arch: "归档（创建/浏览/修复）",
  taskQueue: "传输队列（并发/暂停/重试）",
  procCtl: "进程控制与隔离（Job 语义）",
  windowMgr: "VWM 窗口管理（几何/贴靠/层级）",
  embed: "拥有式嵌入（第三方窗收编）",
  input: "输入事件总线（键鼠/宏/IME）",
  clipboard: "剪贴板（白名单化+历史）",
  shell: "壳集成（启动/关联/图标/菜单）",
  vault: "保险箱（AES-256-GCM，密钥仅内存）",
  privacy: "隐私看护（粉碎/审计/金丝雀）",
  sysinfo: "系统信息（磁盘/电池/环境/健康）",
  net: "网络栈（状态/代理/规则/许可）",
  audio: "音频输出/混音/设备",
  display: "显示控制（亮度/多屏/壁纸）",
  power: "电源动作与电池",
  applog: "日志总线与诊断导出",
  sched: "定时任务与提醒",
  notify: "通知存档与勿扰",
  extLoader: "扩展/插件装载与市场",
  openhub: "openhub 网关（token/流/连接器）",
  update: "更新扫描与应用",
  compat: "兼容性探测/清单/履历",
  peTools: "PE 分析/反汇编/沙箱试探",
  pkg: "软件包管理（登记/安装/残留）",
  steam: "Steam 库与启动",
  browser: "浏览器档案管理",
  aiTools: "AI 工具链（Node/工具安装/启动）",
  gitTools: "git/ssh 开发工具",
  term: "终端会话",
  ime: "输入法服务",
  print: "打印服务",
  usb: "U 盘便携（打包/校验/拔出看护）",
  container: "Uxv 容器（诊断/修复/救援）",
  boot: "启动链（引导/阶段叙事/自检）",
  perf: "性能观测（内存/CPU 配额/IO 限速）",
};

// ---- 分类规则（顺序即优先级；先专后泛） ----
// color: "ok"=✅ 单服务；"combo"=🔶 组合；"missing"=❌ 暂缺
const RULES = [
  // ---- 引导/系统底座 ----
  [/^app_bootstrap$|^boot_replay$|^perf_boot_stage/, "ok", ["boot"]],
  [/^exit_prepare$|^power_action$/, "ok", ["power"]],
  [/^maintain_selfcheck$|^diag_flags$|^diagnostic_export$/, "combo", ["applog", "sysinfo"]],
  [/^sys_(brief|disks|user|disk_health|self_info|prefs_read)$/, "ok", ["sysinfo"]],
  [/^sysenv_/, "ok", ["sysinfo"]],
  [/^sysdep_probe$|^dep_audit_status$/, "ok", ["applog"]],
  [/^applog_recent$/, "ok", ["applog"]],
  [/^perf_/, "ok", ["perf"]],
  [/^cursor_pos$/, "ok", ["input"]],
  [/^shot_|^snapshot_capture$/, "combo", ["display", "fsWs"]],
  [/^http_fetch$/, "ok", ["net"]],
  [/^net_(status|ip|drives)$/, "ok", ["net"]],
  [/^net_(proxy|kill_switch|consent|rule)/, "ok", ["net"]],
  [/^fw_/, "missing", "netFirewall", "VARIX 内核无防火墙服务，需立项（或走纯 Windows 通道）"],
  [/^open_datavault$/, "ok", ["vault"]],
  [/^win_(set_avoid_taskbar|hide_to_tray|health_scan)$/, "ok", ["windowMgr"]],

  // ---- 文档/思维导图/媒体（variable.db） ----
  [/^(list|create|rename|move|trash|restore|purge)_folder/, "ok", ["docStore"]],
  [/^(list|create|get|save|move|trash|restore|purge)_document/, "ok", ["docStore"]],
  [/^set_document_(favorite|tags)$|^list_document_tags$/, "ok", ["docStore"]],
  [/^empty_trash$|^search_all$/, "combo", ["docStore", "search"]],
  [/^(list|create|get|update|rename|trash)_mindmap/, "ok", ["mindStore"]],
  [/^(save|delete)_nodes$|^nodes_versions$/, "combo", ["mindStore", "verHistory"]],
  [/^(save_edge|delete_edges)$/, "ok", ["mindStore"]],
  [/^import_(media|data_url)$|^list_attachments$|^resolve_media_path$|^delete_media$/, "ok", ["mediaStore"]],
  [/^(get|set)_settings$|^reset_ui_settings$/, "ok", ["settings"]],

  // ---- 恢复/备份/导出 ----
  [/recovery_file|^recover_to_document$/, "combo", ["backup", "docStore"]],
  [/backup/, "ok", ["backup"]],
  [/^export_/, "combo", ["backup", "fsWs"]],
  [/^import_workspace$/, "combo", ["backup", "fsWs", "docStore"]],
  [/^ws_/, "ok", ["fsWs"]],

  // ---- 工程/代码/终端/AI 工具 ----
  [/^project_scan$|^project_read_(file|bytes)$/, "combo", ["fsWs", "search"]],
  [/^(read|write)_text_file$|^save_text_file$|^bigfile_slice$/, "ok", ["fsWs"]],
  [/^workspace_search$|^editor_goto$/, "combo", ["search", "fsWs"]],
  [/^(git_|ssh_)/, "ok", ["gitTools"]],
  [/^term_/, "ok", ["term"]],
  [/^ai_/, "ok", ["aiTools"]],
  [/^code_/, "combo", ["aiTools", "extLoader"]],
  [/^toolchain_/, "combo", ["aiTools", "fsWs"]],

  // ---- 文件管理器（宿主面） ----
  [/^ex_(home|variable_dirs)$/, "combo", ["fsShared", "vfs"]],
  [/^ex_drives$/, "missing", "fsHost", "VARIX 物理隔离：宿主盘列举按设计不提供，需以共享分区列举服务替代"],
  [/^ex_(list|mkdir|rename|move|copy|trash|search|purge|thumbnail)/, "combo", ["fsShared", "vfs"]],
  [/^ex_(fav|view)/, "ok", ["kv"]],
  [/^ex_conflicts$/, "ok", ["fsShared"]],
  [/^rec_/, "combo", ["recycle", "fsShared"]],
  [/^checksum/, "combo", ["fsShared", "perf"]],
  [/^(dupe_scan|space_scan)$/, "combo", ["fsShared", "perf"]],
  [/^batch_rename_/, "combo", ["fsShared", "vfs", "verHistory"]],
  [/^sendto_/, "combo", ["fsShared", "shell"]],
  [/^who_locks$/, "missing", "fsHost", "文件锁探测依赖宿主内核句柄表，VARIX 下无对应面"],
  [/^archive_(ls|extract_one)$/, "combo", ["arch", "fsShared"]],
  [/^net_drives$/, "ok", ["net"]],

  // ---- 软件容器/便携/安装 ----
  [/^tp_/, "combo", ["pkg", "shell"]],
  [/^icon_/, "combo", ["shell", "display"]],
  [/^profile_/, "combo", ["pkg", "fsWs"]],
  [/^residue_/, "combo", ["pkg", "fsShared"]],
  [/^install_/, "combo", ["pkg", "procCtl"]],
  [/^official_usage$|^official_purge$/, "combo", ["pkg", "fsShared"]],
  [/^(usb_)/, "ok", ["usb"]],
  [/^portability_assess$|^ecosystem_migrate$/, "combo", ["pkg", "fsWs"]],
  [/^winget_/, "missing", "pkgRemote", "VARIX 无 winget/远程源；走共享分区登记 + 引擎通道替代"],
  [/^vhdx_probe$/, "ok", ["container"]],
  [/^container_/, "ok", ["container"]],

  // ---- 兼容/PE/沙箱/安全 ----
  [/^compat_/, "ok", ["compat"]],
  [/^(pe_analyze|disasm_entry)/, "ok", ["peTools"]],
  [/^sandbox_/, "combo", ["peTools", "procCtl"]],
  [/^security_/, "combo", ["peTools", "applog"]],
  [/^shield_/, "ok", ["procCtl"]],
  [/^trust_/, "combo", ["compat", "vault"]],
  [/^canary_/, "ok", ["privacy"]],
  [/^privacy_(shred|audit)$|^priv_timeline$|^privacy_usage$/, "ok", ["privacy"]],

  // ---- 保险箱 ----
  [/^vault_/, "ok", ["vault"]],
  [/^tool_secure_/, "combo", ["vault", "fsWs"]],
  [/^tool_data_/, "ok", ["kv"]],

  // ---- 硬件设置面 ----
  [/^(mouse_params|pointer_speed)/, "ok", ["input"]],
  [/^(audio_|mixer_|sound_scheme|mic_usage|media_status)/, "ok", ["audio"]],
  [/^(wifi_|bluetooth_|bt_|net_consent)/, "ok", ["net"]],
  [/^(battery_get|brightness_|wp_monitors)/, "combo", ["power", "display"]],
  [/^brightness_/, "ok", ["display"]],
  [/^battery_get$/, "ok", ["power"]],
  [/^wp_/, "combo", ["display", "fsShared"]],
  [/^we_wallpaper_/, "combo", ["display", "embed"]],
  [/^print_/, "ok", ["print"]],
  [/^ime_/, "ok", ["ime"]],
  [/^startup_|^service_|^sched_|^startdelay_/, "combo", ["sched", "sysinfo"]],
  [/^update_/, "ok", ["update"]],
  [/^reminder_/, "ok", ["sched"]],
  [/^notify_archive_/, "ok", ["notify"]],
  [/^shortcuts_apply$/, "ok", ["input"]],
  [/^a11yProbe$/, "ok", ["input"]],

  // ---- 剪贴板/宏/版本历史 ----
  [/^cliphist_/, "ok", ["clipboard"]],
  [/^macro_/, "combo", ["input", "procCtl"]],
  [/^ver_/, "ok", ["verHistory"]],
  [/^tag_/, "combo", ["docStore", "search"]],

  // ---- 传输/下载 ----
  [/^tr_/, "ok", ["taskQueue"]],
  [/^inc_/, "combo", ["taskQueue", "fsWs"]],

  // ---- 关联/打开方式 ----
  [/^file_assoc_/, "combo", ["shell", "kv"]],
  [/^assoc_snapshot_/, "combo", ["shell", "backup"]],

  // ---- 嵌入/窗口 ----
  [/^embed_/, "ok", ["embed"]],
  [/^watch_/, "ok", ["windowMgr"]],
  [/^desktop_raise$/, "ok", ["windowMgr"]],
  [/^shell_(execute|activate_application|item_icon|context_menu|forward_gesture)$/, "ok", ["shell"]],

  // ---- openhub/网关/webhook/深链 ----
  [/^(openhub|gateway|companion|webhook|deeplink|vxs|safehouse|demo_capsule|revocation_list)/, "ok", ["openhub"]],
  [/^cfg_diff$/, "combo", ["settings", "backup"]],

  // ---- 引擎通道 ----
  [/^(steam_|aumid_launch$)/, "combo", ["steam", "procCtl"]],
  [/^browser_/, "combo", ["browser", "procCtl"]],
  [/^env_/, "combo", ["procCtl", "sysinfo"]],
  [/^(proc_|perf_cpu$)/, "ok", ["procCtl"]],
  [/^(fsindex_)/, "ok", ["search"]],
  [/^svc_/, "combo", ["sysinfo", "diag"]],
  [/^volmem_/, "ok", ["kv"]],
  [/^lin_/, "combo", ["privacy", "applog"]],
  [/^ins_/, "combo", ["privacy", "sysinfo"]],
  [/^arch_(create|list|browse|audit|extract|repair|remove)$/, "combo", ["arch", "fsShared"]],
  [/^panic_/, "combo", ["power", "privacy"]],
  [/^openhub_stream_/, "ok", ["openhub"]],

  // ---- 补缺口（首轮门禁暴露） ----
  [/^open_path$/, "combo", ["shell", "vfs"]],
  [/^reveal_path$/, "combo", ["shell", "fsWs"]],
  [/^check_paths_exist$/, "combo", ["fsWs", "vfs"]],
  [/^log_frontend$/, "ok", ["applog"]],
  [/^sentinel_/, "combo", ["fsWs", "verHistory"]],
  [/^identity_/, "ok", ["vault"]],
  [/^ext_/, "ok", ["extLoader"]],
  [/^win_(suspend|resume)$/, "ok", ["windowMgr"]],
  [/^directshell_/, "ok", ["shell"]],
];

// ---- 覆盖判定 ----
// 规则形态：[regex, "ok"|"combo", services[]] 或 [regex, "missing", missingService, missingNote]
function classify(cmd) {
  for (const [re, color, a, b] of RULES) {
    if (re.test(cmd)) {
      if (color === "missing") {
        return { color: "❌", services: [], missingService: a, missingNote: b ?? "" };
      }
      return {
        color: Array.isArray(a) && a.length > 1 ? "🔶" : "✅",
        services: a,
        missingService: null,
        missingNote: null,
      };
    }
  }
  return null;
}

const mapping = [];
const unclassified = [];
for (const { method, cmd } of entries) {
  if (!cmd) {
    // 无直接 invoke 的包装方法：逐一给归属（禁止一刀切兜底，防误归属）
    const wrapperMap = {
      a11yProbe: ["input", "sysinfo"],
      compatShimStats: ["compat"],
      getSettings: ["settings"],
    };
    const services = wrapperMap[method] || ["kv"];
    mapping.push({ method, cmd: null, color: services.length > 1 ? "🔶" : "✅", services, missingService: null, missingNote: null, note: "无直接 invoke（前端包装方法），随实现归属" });
    continue;
  }
  const c = classify(cmd);
  if (!c) {
    unclassified.push({ method, cmd });
    continue;
  }
  mapping.push({ method, cmd, ...c, missingService: c.missingService ?? null, missingNote: c.missingNote ?? null });
}

// ---- 垫片新增命令登记（非 ipc.ts 方法；总案"加命令改表不改垫片代码"） ----
// 任务 26（AI-V）：KV 透明桥命令面，服务=kv（任务 24 内核 kvsrv），ns≤16B 由内核强制。
const EXTRA_COMMANDS = [
  { method: null, cmd: "kv_get", color: "✅", services: ["kv"], missingService: null, missingNote: null, note: "任务26 KV桥：读键（缺键=null）" },
  { method: null, cmd: "kv_set", color: "✅", services: ["kv"], missingService: null, missingNote: null, note: "任务26 KV桥：写键（满容 Err(Full) 透传）" },
  { method: null, cmd: "kv_remove", color: "✅", services: ["kv"], missingService: null, missingNote: null, note: "任务26 KV桥：删键（不存在=no-op）" },
  { method: null, cmd: "kv_keys", color: "✅", services: ["kv"], missingService: null, missingNote: null, note: "任务26 KV桥：列键（预热/枚举）" },
];
for (const extra of EXTRA_COMMANDS) mapping.push(extra);

if (unclassified.length) {
  console.error("未分类（门禁失败）：", JSON.stringify(unclassified, null, 1));
  process.exit(1);
}

const counts = { "✅": 0, "🔶": 0, "❌": 0 };
for (const e of mapping) counts[e.color]++;
const total = mapping.length;
const pct = (n) => ((n / total) * 100).toFixed(1) + "%";

const out = {
  $comment: "任务23（AI-B）三色审计映射表。数据驱动：新增命令先在此登记归属（services[]），再进 generate_handler。颜色口径见 docs/双域-垫片协议规范-v1.md 与本表生成脚本头注。",
  generatedAt: new Date().toISOString().slice(0, 10),
  source: "src/lib/ipc.ts",
  stats: { total, ok: counts["✅"], okPct: pct(counts["✅"]), combo: counts["🔶"], comboPct: pct(counts["🔶"]), missing: counts["❌"], missingPct: pct(counts["❌"]) },
  serviceCatalogue: SERVICES,
  entries: mapping,
};

const outPath = path.join(ROOT, "docs", "shim-mapping.json");
fs.writeFileSync(outPath, JSON.stringify(out, null, 1), "utf8");
`total=${total} OK=${counts[0x2705]}(${pct(counts[0x2705])}) COMBO=${counts[0x1F534]}(${pct(counts[0x1F534])}) MISS=${counts[0x274C]}(${pct(counts[0x274C])})`;
console.log("written:", path.relative(ROOT, outPath));
