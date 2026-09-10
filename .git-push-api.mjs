// Rebuild remote tree exactly from local tree object (b03d517's tree), then fix ref (force).
import { execSync } from "node:child_process";

const REPO = "VariableXs/-Un-Real-0d23d9ux-Engine";
const LOCAL = execSync("git rev-parse HEAD").toString().trim();
const TREE = execSync("git log -1 --format=%T").toString().trim();
const RAW = execSync("git cat-file commit HEAD").toString("binary");
function run(c, opts = {}) { return execSync(c, { maxBuffer: 1 << 28, ...opts }); }

const credOut = execSync("git credential fill", { input: "protocol=https\nhost=github.com\n\n" }).toString();
const TOKEN = credOut.split(/\r?\n/).find(l => l.startsWith("password=")).slice(9).replace(/[^ -~]/g, "");
const H = { "Authorization": `Bearer ${TOKEN}`, "Accept": "application/vnd.github+json", "Content-Type": "application/json", "User-Agent": "varix-fix" };

async function api(path, method = "GET", body) {
  const res = await fetch(`https://api.github.com/repos/${REPO}/${path}`, { method, headers: H, body: body ? JSON.stringify(body) : undefined });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${text.slice(0, 400)}`);
  return text ? JSON.parse(text) : {};
}

// full flat listing from local tree; blobs already exist remotely (parent pushed + we uploaded)
const raw = run(`git ls-tree -r -z ${TREE}`).toString("binary");
console.log("raw len", raw.length, "nul count", raw.split("\0").length, "first", JSON.stringify(raw.slice(0, 60)));
const items = [];
for (const line of raw.split("\0").filter(Boolean)) {
  const sp = line.indexOf(" ");
  const tab = line.indexOf("\t", sp);
  const mode = line.slice(0, sp);
  const type = line.slice(sp + 1, tab);
  const rest = line.slice(tab + 1);
  const t2 = rest.indexOf("\t");
  const sha = rest.slice(0, t2);
  const path = Buffer.from(rest.slice(t2 + 1), "binary").toString("utf8");
  if (type !== "blob") continue;
  items.push({ path, mode, type: "blob", sha });
}
console.log("full tree entries:", items.length);

const tree = await api("git/trees", "POST", { tree: items });
console.log("tree:", tree.sha, tree.sha === TREE ? "MATCH" : `DIFF local=${TREE}`);
if (tree.sha !== TREE) process.exit(1);

// message bytes
const msgStart = RAW.indexOf("\n\n");
const MESSAGE = Buffer.from(RAW.slice(msgStart + 2), "binary").toString("utf8").replace(/\n$/, "");
function parsePerson(line) {
  const m = line.match(/^(author|committer) (.*?) <(.*?)> (\d+) ([+-]\d{4})$/);
  if (!m) { console.error("PERSON_PARSE_FAIL"); process.exit(1); }
  const [, , name, email, epoch, off] = m;
  const sign = off[0], oh = parseInt(off.slice(1, 3), 10), om = parseInt(off.slice(3, 5), 10);
  const wall = new Date(parseInt(epoch, 10) * 1000 + (sign === "+" ? 1 : -1) * (oh * 3600 + om * 60) * 1000);
  const p = n => String(n).padStart(2, "0");
  const iso = `${wall.getUTCFullYear()}-${p(wall.getUTCMonth() + 1)}-${p(wall.getUTCDate())}T${p(wall.getUTCHours())}:${p(wall.getUTCMinutes())}:${p(wall.getUTCSeconds())}`;
  return { name, email, date: `${iso}${sign}${p(oh)}:${p(om)}` };
}
const lines = RAW.split("\n");
const AUTHOR = parsePerson(Buffer.from(lines.find(l => l.startsWith("author ")), "binary").toString("utf8"));
const COMMITTER = parsePerson(Buffer.from(lines.find(l => l.startsWith("committer ")), "binary").toString("utf8"));
const PARENT = execSync("git log -1 --format=%P").toString().trim().split(" ")[0];

const commit = await api("git/commits", "POST", { message: MESSAGE, tree: tree.sha, parents: [PARENT], author: AUTHOR, committer: COMMITTER });
console.log("commit:", commit.sha, commit.sha === LOCAL ? "SHA-MATCH" : `DIFF local=${LOCAL}`);

await api("git/refs/heads/main", "PATCH", { sha: commit.sha, force: true }); // fix my own broken push
console.log("REF-FIXED", commit.sha);
