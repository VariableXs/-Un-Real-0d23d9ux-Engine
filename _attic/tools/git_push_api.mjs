/**
 * github.com:443 不可达时的 REST API 快进推送（非功能性部署工具，归档 _attic/tools/）。
 *
 * 原理：用 API 重建与本地 HEAD **逐字节相同** 的 commit（同树同元数据），
 * 因此得到的 SHA 与本地一致 → 可 force:false 快进推 ref，历史不分叉。
 * 坑位（均由实测踩过，勿改）：
 *   1) 树 sha 用 `git log -1 --format=%T` 取（execSync/cmd 会吃掉 `^{tree}` 的 `^`）。
 *   2) 提交消息逐字节传（API 不补尾换行）。
 *   3) author/committer.date 必须 RFC 3339（+08:00 带冒号），git 内部 `+0800` 会 422。
 *   4) 建完树要断言 sha == 本地 tree sha；建完 commit 要断言 sha == 本地 HEAD。
 *   5) 推完手动 `git update-ref refs/remotes/origin/main <sha>`（fetch 也不可用）。
 */
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const REPO = "VariableXs/-Un-Real-0d23d9ux-Engine";
const git = (...a) => execFileSync("git", a, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });

function ghToken() {
  try {
    return execFileSync("gh", ["auth", "token"], { encoding: "utf8" }).trim();
  } catch {
    const home = process.env.USERPROFILE ?? process.env.HOME ?? "";
    const yml = readFileSync(`${home}/.config/gh/hosts.yml`, "utf8");
    const m = /oauth_token:\s*(\S+)/.exec(yml);
    if (!m) throw new Error("无法取得 gh token");
    return m[1];
  }
}

const TOKEN = ghToken();
async function api(path, method = "GET", body) {
  const res = await fetch(`https://api.github.com${path}`, {
    method,
    headers: {
      Authorization: `Bearer ${TOKEN}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "varix-push-api",
      "Content-Type": "application/json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status} ${text.slice(0, 500)}`);
  return text ? JSON.parse(text) : null;
}

/** `Name <mail> 1757000000 +0800` → API 需要的 {name,email,date(RFC3339)} */
function parseIdent(raw) {
  const m = /^(.*) <(.*)> (\d+) ([+-])(\d{2})(\d{2})$/.exec(raw);
  if (!m) throw new Error(`无法解析身份行: ${raw}`);
  const [, name, email, ts, sign, hh, mm] = m;
  const offSec = (sign === "-" ? -1 : 1) * (Number(hh) * 3600 + Number(mm) * 60);
  const local = new Date((Number(ts) + offSec) * 1000).toISOString().replace(/\.\d{3}Z$/, "");
  return { name, email, date: `${local}${sign}${hh}:${mm}` };
}

const head = git("rev-parse", "HEAD").trim();
const parent = git("rev-parse", `${head}^`).trim();
const raw = execFileSync("git", ["cat-file", "commit", head], { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
const sep = raw.indexOf("\n\n");
const headerLines = raw.slice(0, sep).split("\n");
const message = raw.slice(sep + 2); // 逐字节：保留尾换行
const localTree = git("log", "-1", "--format=%T").trim();
const parentTree = git("log", "-1", "--format=%T", parent).trim();
const author = parseIdent(headerLines.find((l) => l.startsWith("author ")).slice(7));
const committer = parseIdent(headerLines.find((l) => l.startsWith("committer ")).slice(10));
console.log(`HEAD=${head}\nparent=${parent}\ntree=${localTree}\nauthor=${author.date} committer=${committer.date}`);

const diff = git("diff", "--name-status", parent, head).trim().split("\n").filter(Boolean);
console.log(`变更路径 ${diff.length} 条`);

const entries = [];
for (const line of diff) {
  const [status, ...paths] = line.split("\t");
  const p = paths[paths.length - 1];
  if (status.startsWith("D")) {
    entries.push({ path: p, mode: "100644", type: "blob", sha: null });
    console.log(`  D ${p}`);
    continue;
  }
  const mode = git("ls-tree", head, "--", p).trim().split(/\s+/)[0];
  const b64 = execFileSync("git", ["show", `${head}:${p}`], { maxBuffer: 256 * 1024 * 1024 }).toString("base64");
  const blob = await api(`/repos/${REPO}/git/blobs`, "POST", { content: b64, encoding: "base64" });
  entries.push({ path: p, mode, type: "blob", sha: blob.sha });
  console.log(`  ${status} ${p} (${mode}) → ${blob.sha.slice(0, 8)}`);
}

const tree = await api(`/repos/${REPO}/git/trees`, "POST", { base_tree: parentTree, tree: entries });
if (tree.sha !== localTree) throw new Error(`树不一致：远端 ${tree.sha} ≠ 本地 ${localTree}`);
console.log(`树一致 ✓ ${tree.sha}`);

const commit = await api(`/repos/${REPO}/git/commits`, "POST", {
  message, tree: tree.sha, parents: [parent], author, committer,
});
if (commit.sha !== head) throw new Error(`提交不一致：远端 ${commit.sha} ≠ 本地 ${head}`);
console.log(`提交一致 ✓ ${commit.sha}`);

await api(`/repos/${REPO}/git/refs/heads/main`, "PATCH", { sha: commit.sha, force: false });
git("update-ref", "refs/remotes/origin/main", commit.sha);
console.log(`已推送并同步跟踪引用：refs/remotes/origin/main = ${commit.sha}`);
