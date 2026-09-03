/**
 * 第三章：项目语义与领域识别引擎（规范 2.2）。
 * 通过"入口文件 + 配置文件 + 目录特征"三维证据链推断项目类型与领域。
 */

import type { ProjectDetect, ProjectTypeId, ProjectScanResult, SourceFile } from "./types";

interface TypeRule {
  typeId: ProjectTypeId;
  label: string;
  domain: string;
  primaryLang: ProjectDetect["primaryLang"];
  /** Any of these files/dirs (root or depth ≤ 2) is strong evidence. */
  markers: string[];
  /** Extra markers searched in file CONTENTS of matched sources. */
  contentMarkers?: Array<[string, string]>;
  stack: string[];
  /** Entry-file candidates by relPath suffix. */
  entries: string[];
}

const RULES: TypeRule[] = [
  {
    typeId: "next", label: "一个可以让搜索引擎找到的网站（Next.js）", domain: "web",
    primaryLang: "ts", markers: ["next.config.js", "next.config.ts", "next.config.mjs"], stack: ["Next.js", "React", "TypeScript"],
    entries: ["app/page.tsx", "pages/index.tsx", "app/page.jsx", "pages/index.jsx"],
  },
  {
    typeId: "react", label: "一个用网页技术做的应用（React）", domain: "web",
    primaryLang: "ts", markers: ["package.json"], stack: ["React", "JavaScript/TypeScript"],
    entries: ["src/main.tsx", "src/index.tsx", "src/App.tsx", "src/main.jsx", "src/index.jsx", "index.html"],
    contentMarkers: [["package.json", "\"react\""]],
  },
  {
    typeId: "vue", label: "一个用网页技术做的应用（Vue）", domain: "web",
    primaryLang: "ts", markers: ["package.json"], stack: ["Vue", "JavaScript/TypeScript"],
    entries: ["src/main.ts", "src/main.js"],
    contentMarkers: [["package.json", "\"vue\""]],
  },
  {
    typeId: "rust", label: "一个跑得飞快的电脑程序（Rust）", domain: "desktop",
    primaryLang: "rust", markers: ["Cargo.toml"], stack: ["Rust"],
    entries: ["src/main.rs", "src/lib.rs"],
  },
  {
    typeId: "python", label: "一个做数据分析或自动化的工具（Python）", domain: "data",
    primaryLang: "python", markers: ["requirements.txt", "pyproject.toml", "setup.py"], stack: ["Python"],
    entries: ["main.py", "app.py", "run.py", "src/main.py"],
  },
  {
    typeId: "go", label: "一个云端服务或命令行工具（Go）", domain: "cli",
    primaryLang: "go", markers: ["go.mod"], stack: ["Go"],
    entries: ["main.go", "cmd/main.go"],
  },
  {
    typeId: "java", label: "一个给别人提供服务的大后台（Java）", domain: "web",
    primaryLang: "java", markers: ["pom.xml", "build.gradle", "settings.gradle"], stack: ["Java"],
    entries: ["src/main/java"],
  },
  {
    typeId: "android", label: "一个安卓手机上用的应用（Android）", domain: "mobile",
    primaryLang: "kotlin", markers: ["AndroidManifest.xml", "build.gradle.kts"], stack: ["Android SDK", "Kotlin/Java"],
    entries: ["app/src/main"],
  },
  {
    typeId: "ios", label: "一个苹果手机上用的应用（iOS）", domain: "mobile",
    primaryLang: "swift", markers: ["Podfile", "*.xcodeproj", "*.xcworkspace"], stack: ["Swift", "iOS SDK"],
    entries: [],
  },
  {
    typeId: "dotnet", label: "一个微软生态的程序（C#/.NET）", domain: "desktop",
    primaryLang: "csharp", markers: ["*.csproj", "*.sln"], stack: ["C#", ".NET"],
    entries: ["Program.cs"],
  },
  {
    typeId: "php", label: "一个老牌网站建设语言写的项目（PHP）", domain: "web",
    primaryLang: "php", markers: ["composer.json", "artisan"], stack: ["PHP"],
    entries: ["index.php", "public/index.php"],
  },
  {
    typeId: "ruby", label: "一个简洁优雅的网站项目（Ruby）", domain: "web",
    primaryLang: "ruby", markers: ["Gemfile", "config.ru"], stack: ["Ruby"],
    entries: ["config/application.rb"],
  },
  {
    typeId: "flutter", label: "一个跨手机平台的界面应用（Flutter）", domain: "mobile",
    primaryLang: "dart", markers: ["pubspec.yaml"], stack: ["Flutter", "Dart"],
    entries: ["lib/main.dart"],
  },
  {
    typeId: "unity", label: "一个电脑或手机游戏（Unity）", domain: "game",
    primaryLang: "csharp", markers: ["ProjectSettings/ProjectVersion.txt", "Assets"], stack: ["Unity", "C#"],
    entries: ["Assets"],
  },
  {
    typeId: "node", label: "一个跑在服务器/命令行的 JavaScript 程序（Node.js）", domain: "cli",
    primaryLang: "js", markers: ["package.json"], stack: ["Node.js", "JavaScript"],
    entries: ["src/index.js", "index.js", "server.js"],
  },
  {
    typeId: "docker", label: "一个可以放到云端服务器运行的打包程序（容器化）", domain: "web",
    primaryLang: "generic", markers: ["Dockerfile", "docker-compose.yml"], stack: ["Docker"],
    entries: [],
  },
];

const DOMAIN_LABEL: Record<string, string> = {
  web: "网站/网页 —— 在浏览器里访问的服务",
  desktop: "桌面 App —— 装在电脑上双击打开的软件",
  mobile: "手机 App —— 装在手机上的软件",
  game: "游戏项目 —— 画面 + 玩法 + 用户输入",
  data: "数据/AI 项目 —— 从一堆数据里找规律",
  cli: "命令行工具 —— 在黑窗口里输入指令来使用",
};

/** L7 模式识别：扫描源码内容，命中常见架构/风格/流程关键词（8.3）。 */
const PATTERN_MARKERS: Array<[string, RegExp]> = [
  ["redux", /\bfrom\s+["']redux|createStore|configureStore\b/],
  ["vuex", /\bfrom\s+["']vuex|useStore\(\)/],
  ["rest", /\b(?:express|koa|app\.get\(|app\.post\(|@RestController)\b/],
  ["graphql", /\b(?:graphql|ApolloServer|gql`)/],
  ["websocket", /\b(?:WebSocket|socket\.io|tungstenite)\b/],
  ["cicd", /\b(?:github-actions|\.gitlab-ci|docker build|kubectl apply)\b/],
  ["mvc", /\b(?:Controller|Model|View).{0,40}(?:Controller|Model|View)/],
  ["microservice", /\b(?:microservice|grpc|tonic)\b/i],
  ["spa", /\b(?:createRoot|createApp)\s*\(/],
];

export function detectPatterns(sources: SourceFile[]): string[] {
  const joined = sources.map((s) => s.content.slice(0, 4000)).join("\n").slice(0, 200000);
  const out: string[] = [];
  for (const [name, re] of PATTERN_MARKERS) {
    if (re.test(joined) && !out.includes(name)) out.push(name);
    if (out.length >= 6) break;
  }
  return out;
}

function fileNameSet(scan: ProjectScanResult): Map<string, string> {
  // lower name → first relPath with that name (root-area files win)
  const m = new Map<string, string>();
  const sorted = [...scan.entries].sort((a, b) => a.depth - b.depth);
  for (const e of sorted) {
    const key = e.name.toLowerCase();
    if (!m.has(key)) m.set(key, e.path);
  }
  return m;
}

function hasMarker(files: Map<string, string>, marker: string): string | null {
  if (marker.startsWith("*")) {
    const suffix = marker.slice(1).toLowerCase();
    for (const [name, path] of files) {
      if (name.endsWith(suffix)) return path;
    }
    return null;
  }
  return files.get(marker.toLowerCase()) ?? null;
}

function sourceOf(sources: SourceFile[], relPath: string | null): SourceFile | null {
  if (!relPath) return null;
  return sources.find((s) => s.relPath === relPath) ?? null;
}

export function detectProject(scan: ProjectScanResult): ProjectDetect {
  const files = fileNameSet(scan);
  const evidence: string[] = [];

  // Special compound rules first.
  const pkgPath = files.get("package.json") ?? null;
  if (pkgPath) {
    const pkgSrc = sourceOf(scan.sources, pkgPath);
    const depsText = pkgSrc?.content ?? "";
    if (/["']next["']/.test(depsText)) {
      return build("next", evidence.concat(`发现 next.config / package.json 依赖 next`), scan, files, depsText);
    }
    if (/["']react["']/.test(depsText)) {
      return build("react", evidence.concat("package.json + react 依赖 → 用网页积木搭的应用"), scan, files, depsText);
    }
    if (/["']vue["']/.test(depsText)) {
      return build("vue", evidence.concat("package.json + vue 依赖 → 用网页积木搭的应用"), scan, files, depsText);
    }
    evidence.push("package.json + src 源码 → Node.js / 前端项目");
    return build("node", evidence, scan, files, depsText);
  }

  for (const rule of RULES) {
    if (rule.typeId === "node") continue; // handled above / fallback
    const hits: string[] = [];
    for (const mk of rule.markers) {
      const p = hasMarker(files, mk);
      if (p !== null) hits.push(`${mk} → ${rule.typeId} 的标志性文件`);
    }
    if (hits.length > 0) {
      return build(rule.typeId, evidence.concat(hits), scan, files, null);
    }
  }

  // Fallback: majority language among parsed sources.
  evidence.push("没有找到标志性配置文件 → 按源代码语言推断");
  const counts = new Map<string, number>();
  for (const s of scan.sources) {
    const name = s.relPath.split("/").pop() ?? "";
    const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
    counts.set(ext, (counts.get(ext) ?? 0) + 1);
  }
  let bestExt = "";
  let bestN = 0;
  for (const [ext, n] of counts) {
    if (n > bestN) { bestExt = ext; bestN = n; }
  }
  const extType: Record<string, ProjectTypeId> = {
    py: "python", rs: "rust", go: "go", java: "java", cs: "dotnet",
    swift: "ios", kt: "android", php: "php", rb: "ruby", dart: "flutter",
  };
  return build(extType[bestExt] ?? "generic", evidence, scan, files, null);
}

function build(
  typeId: ProjectTypeId,
  evidence: string[],
  scan: ProjectScanResult,
  files: Map<string, string>,
  pkgContent: string | null,
): ProjectDetect {
  if (typeId === "generic") {
    evidence.push("按通用项目处理：仍然展示目录结构与文件职责");
  }
  const rule = RULES.find((r) => r.typeId === typeId);
  const label = rule?.label ?? "一个用代码写成的项目";
  const domainKey = rule?.domain ?? "web";
  const stack = rule?.stack ?? [];
  if (pkgContent) {
    try {
      const pkg = JSON.parse(stripJsonComments(pkgContent)) as { dependencies?: Record<string, string>; devDependencies?: Record<string, string> };
      const keys = Object.keys({ ...pkg.dependencies, ...pkg.devDependencies });
      for (const fw of ["react", "vue", "svelte", "next", "vite", "webpack", "typescript", "tauri"]) {
        if (keys.includes(fw)) stack.push(fw);
      }
    } catch {
      // corrupt package.json — stack stays as-is
    }
  }
  // Entry candidates: rule-specified relPaths that exist, else entry-role sources.
  const entries = (rule?.entries ?? []).filter((e) => files.has(e.toLowerCase()) || scan.entries.some((x) => x.path === e));
  if (entries.length === 0) {
    for (const s of scan.sources) {
      const name = (s.relPath.split("/").pop() ?? "").toLowerCase();
      const stem = name.replace(/\.[^.]+$/, "");
      if (["main", "index", "app"].includes(stem) && /\.(ts|tsx|js|jsx|py|rs|go|java|cs|dart)$/.test(name)) {
        entries.push(s.relPath);
        if (entries.length >= 3) break;
      }
    }
  }
  return {
    typeId,
    label,
    domain: DOMAIN_LABEL[domainKey] ?? domainKey,
    domainKey,
    evidence,
    stack: [...new Set(stack)],
    entryCandidates: entries.slice(0, 4),
    primaryLang: rule?.primaryLang ?? "generic",
    patterns: detectPatterns(scan.sources),
  };
}

/** Tolerate package.json with // comments (some tools write them). */
function stripJsonComments(text: string): string {
  return text.replace(/^\s*\/\/.*$/gm, "");
}
