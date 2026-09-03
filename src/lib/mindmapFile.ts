/**
 * Parser for local `.mindmap` / `.json` map files written by this app
 * (`app: "variable-mindmap"`, plus selection exports). Plain `{nodes, edges}`
 * payloads are accepted too so hand-edited or third-party files still open.
 */
export interface ParsedMindmapFile {
  name: string;
  nodes: unknown[];
  edges: unknown[];
}

const KNOWN_APPS = new Set(["variable-mindmap", "variable-mindmap-selection"]);

export function parseMindmapFile(text: string): ParsedMindmapFile | null {
  try {
    const v = JSON.parse(text) as Record<string, unknown>;
    const nodes = Array.isArray(v.nodes) ? v.nodes : null;
    if (!nodes) return null;
    if (v.app !== undefined && typeof v.app === "string" && !KNOWN_APPS.has(v.app)) return null;
    const name = typeof v.name === "string" ? v.name : "";
    return { name, nodes, edges: Array.isArray(v.edges) ? v.edges : [] };
  } catch {
    return null;
  }
}
