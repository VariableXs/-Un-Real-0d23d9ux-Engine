/**
 * Binary file anatomy (PE / ELF / Mach-O). Parses headers, section tables,
 * export and import tables purely from bytes — fully offline, no execution,
 * no injection. Output feeds the same drill-down UI as source analysis:
 * every section / import module / export symbol is a node you can expand.
 */

export interface BinarySection {
  name: string;
  vaddr: number;
  vsize: number;
  rawSize: number;
  flags: string[];
}

export interface BinaryImport {
  module: string;
  names: string[];
}

export interface BinaryInfo {
  format: "PE" | "ELF" | "Mach-O" | "Unknown";
  /** Human machine label, e.g. x86-64 / ARM64 / i386. */
  arch: string;
  bits: 32 | 64 | 0;
  /** Subsystem / type label (GUI, console, dylib, exec…). */
  kind: string;
  entryPoint?: string;
  timestamp?: string;
  sections: BinarySection[];
  imports: BinaryImport[];
  exports: string[];
  size: number;
  truncated: boolean;
  notes: string[];
}

const BIN_EXT = new Set(["dll", "exe", "sys", "ocx", "cpl", "efi", "so", "dylib", "a", "lib", "bin", "node"]);

export function isBinaryExt(ext: string | null | undefined): boolean {
  return !!ext && BIN_EXT.has(ext);
}

const u16 = (b: Uint8Array, o: number): number => b[o]! | (b[o + 1]! << 8);
const u32 = (b: Uint8Array, o: number): number => (b[o]! | (b[o + 1]! << 8) | (b[o + 2]! << 16) | (b[o + 3]! << 24)) >>> 0;

const PE_MACHINE: Record<number, string> = {
  0x14c: "i386", 0x8664: "x86-64", 0x1c0: "ARM", 0xaa64: "ARM64",
  0x1c4: "ARMNT", 0x200: "IA-64", 0x5032: "RISC-V 32", 0x5064: "RISC-V 64",
};

const ELF_MACHINE: Record<number, string> = {
  3: "i386", 62: "x86-64", 40: "ARM", 183: "AArch64", 243: "RISC-V", 8: "MIPS", 22: "SPARC", 2: "SPARC", 20: "PowerPC", 21: "PowerPC64",
};

const PE_SECTION_FLAGS: Array<[number, string]> = [
  [0x20, "code"], [0x40, "idata"], [0x80, "udata"], [0x02000000, "discard"],
  [0x04000000, "nocache"], [0x10000000, "shared"], [0x20000000, "exec"],
  [0x40000000, "read"], [0x80000000, "write"],
];

function cstr(b: Uint8Array, off: number, max = 256): string {
  let end = off;
  const stop = Math.min(off + max, b.length);
  while (end < stop && b[end] !== 0) end++;
  return new TextDecoder("latin1").decode(b.subarray(off, end));
}

function rvaToOffset(sections: Array<{ va: number; vsize: number; raw: number; rawSize: number }>, rva: number): number {
  for (const s of sections) {
    if (rva >= s.va && rva < s.va + Math.max(s.vsize, s.rawSize)) {
      return s.raw + (rva - s.va);
    }
  }
  return 0;
}

/** Parse a Windows PE file (dll/exe/sys/ocx/efi). Returns null if malformed. */
export function parsePE(b: Uint8Array, size: number, truncated: boolean): BinaryInfo | null {
  if (b.length < 0x40 || u16(b, 0) !== 0x5a4d) return null; // "MZ"
  const notes: string[] = [];
  const peOff = u32(b, 0x3c);
  if (peOff <= 0 || peOff + 24 > b.length || u32(b, peOff) !== 0x00004550) return null; // "PE\0\0"
  const coff = peOff + 4;
  const machine = u16(b, coff);
  const nSections = u16(b, coff + 2);
  const timestamp = u32(b, coff + 4);
  const optSize = u16(b, coff + 16);
  const opt = coff + 20;
  if (opt + 2 > b.length) return null;
  const magic = u16(b, opt);
  const plus = magic === 0x20b;
  const bits: 32 | 64 = plus ? 64 : 32;
  const entryRva = u32(b, opt + (plus ? 16 : 16));
  const subsystem = u16(b, opt + (plus ? 68 : 68));
  const kind = subsystem === 2 ? "GUI app" : subsystem === 3 ? "Console" : subsystem === 1 ? "Native driver" : `Subsystem ${subsystem}`;
  const ddStart = opt + (plus ? 112 : 96);
  const expRva = u32(b, ddStart);
  const impRva = u32(b, ddStart + 8);

  const secTable = opt + optSize;
  const secs: Array<{ name: string; va: number; vsize: number; raw: number; rawSize: number; flags: number }> = [];
  for (let i = 0; i < nSections; i++) {
    const o = secTable + i * 40;
    if (o + 40 > b.length) break;
    const name = new TextDecoder("latin1").decode(b.subarray(o, o + 8)).replace(/\0+$/, "");
    secs.push({
      name,
      vsize: u32(b, o + 8),
      va: u32(b, o + 12),
      rawSize: u32(b, o + 16),
      raw: u32(b, o + 20),
      flags: u32(b, o + 36),
    });
  }
  const sections: BinarySection[] = secs.map((s) => ({
    name: s.name,
    vaddr: s.va,
    vsize: s.vsize,
    rawSize: s.rawSize,
    flags: PE_SECTION_FLAGS.filter(([m]) => (s.flags & m) !== 0).map(([, n]) => n),
  }));

  // export table → exported symbol names
  const exports: string[] = [];
  if (expRva) {
    const eo = rvaToOffset(secs, expRva);
    if (eo && eo + 40 <= b.length) {
      const nameCount = u32(b, eo + 24);
      const namesRva = u32(b, eo + 32);
      const no = rvaToOffset(secs, namesRva);
      for (let i = 0; i < nameCount && i < 400; i++) {
        if (!no || no + i * 4 + 4 > b.length) break;
        const pRva = u32(b, no + i * 4);
        const p = rvaToOffset(secs, pRva);
        if (p) exports.push(cstr(b, p));
      }
    }
  }

  // import table → dll module + imported names
  const imports: BinaryImport[] = [];
  if (impRva) {
    let io = rvaToOffset(secs, impRva);
    for (let d = 0; d < 96; d++) {
      if (!io || io + 20 > b.length) break;
      const nameRva = u32(b, io + 12);
      const thunkRva = u32(b, io) || u32(b, io + 16);
      if (!nameRva && !thunkRva) break;
      const modName = cstr(b, rvaToOffset(secs, nameRva));
      const names: string[] = [];
      if (thunkRva) {
        let to = rvaToOffset(secs, thunkRva);
        const step = bits === 64 ? 8 : 4;
        for (let f = 0; f < 512; f++) {
          if (!to || to + step > b.length) break;
          const val = bits === 64 ? Number(BigInt.asUintN(64, BigInt(u32(b, to)) | (BigInt(u32(b, to + 4)) << 32n))) : u32(b, to);
          if (val === 0) break;
          const ordinalFlag = bits === 64 ? 1n << 63n : 0x80000000;
          if (BigInt(val) & BigInt(ordinalFlag)) {
            names.push(`#ord${BigInt(val) & (bits === 64 ? 0xffffn : 0xffffn)}`);
          } else {
            const hn = rvaToOffset(secs, bits === 64 ? Number(BigInt(val) & 0x7fffffffn) + 2 : (val & 0x7fffffff) + 2);
            if (hn) names.push(cstr(b, hn));
          }
          to += step;
        }
      }
      if (modName) imports.push({ module: modName, names: names.slice(0, 128) });
      io += 20;
    }
  }

  if (truncated) notes.push("Truncated read: tables near the file tail may be missing.");
  if (!exports.length && !imports.length) notes.push("No import/export tables resolved (static library, stripped, or packed binary).");
  return {
    format: "PE",
    arch: PE_MACHINE[machine] ?? `Machine 0x${machine.toString(16)}`,
    bits,
    kind,
    entryPoint: entryRva ? `0x${entryRva.toString(16)}` : undefined,
    timestamp: timestamp ? new Date(timestamp * 1000).toISOString().slice(0, 10) : undefined,
    sections,
    imports,
    exports,
    size,
    truncated,
    notes,
  };
}

/** Parse an ELF file (.so). Returns null if malformed. */
export function parseELF(b: Uint8Array, size: number, truncated: boolean): BinaryInfo | null {
  if (b.length < 0x34 || b[0] !== 0x7f || b[1] !== 0x45 || b[2] !== 0x4c || b[3] !== 0x46) return null;
  const is64 = b[4] === 2;
  const le = b[5] !== 2;
  if (!le) return null; // big-endian unsupported in this light parser
  const rd16 = (o: number): number => is64 ? u16(b, o) : u16(b, o);
  const rd32 = (o: number): number => u32(b, o);
  const rd64 = (o: number): number => u32(b, o + 4); // low 32 bits suffice for display
  const machine = rd16(0x12);
  const shoff = is64 ? rd64(0x28) : rd32(0x20);
  const shentsize = rd16(is64 ? 0x3a : 0x2e);
  const shnum = rd16(is64 ? 0x3c : 0x30);
  const shstrndx = rd16(is64 ? 0x3e : 0x32);
  const sections: BinarySection[] = [];
  const secs: Array<{ name: string; va: number; vsize: number; rawSize: number }> = [];
  if (shoff && shnum > 0 && shnum < 96 && shentsize >= (is64 ? 64 : 40)) {
    const strOff = shoff + shstrndx * shentsize;
    const strNameOff = is64 ? rd64(strOff + 24) : rd32(strOff + 16);
    const strSize = is64 ? rd64(strOff + 32) : rd32(strOff + 20);
    for (let i = 0; i < shnum; i++) {
      const o = shoff + i * shentsize;
      if (o + shentsize > b.length) break;
      const nameOff = is64 ? rd32(o) : rd32(o);
      const type = rd32(o + 4);
      const va = is64 ? rd64(o + 16) : rd32(o + 12);
      const vsize = is64 ? rd64(o + 32) : rd32(o + 20);
      const nm = cstr(b, Math.min(Number(strNameOff) + nameOff, b.length - 1), 24);
      secs.push({ name: nm || `section ${i}`, va, vsize, rawSize: vsize });
      void type;
      sections.push({ name: nm || `section ${i}`, vaddr: va, vsize, rawSize: vsize, flags: [] });
    }
    void strSize;
  }
  const type = rd16(0x10);
  return {
    format: "ELF",
    arch: ELF_MACHINE[machine] ?? `Machine ${machine}`,
    bits: is64 ? 64 : 32,
    kind: type === 2 ? "Executable" : type === 3 ? "Shared library" : `Type ${type}`,
    sections,
    imports: [],
    exports: [],
    size,
    truncated,
    notes: ["ELF: header + section table shown; dynamic symbol tables not expanded for stripped/obfuscated builds."].concat(truncated ? ["Truncated read."] : []),
  };
}

/** Parse a Mach-O file (.dylib). Header-level anatomy only. */
export function parseMachO(b: Uint8Array, size: number, truncated: boolean): BinaryInfo | null {
  if (b.length < 32) return null;
  const magic = u32(b, 0);
  const is64 = magic === 0xfeedfacf;
  const swap = magic === 0xcffaedfe || magic === 0xcefaedfe;
  if (!swap) return null;
  const rd32 = (o: number): number => swap ? ((b[o]! << 24) | (b[o + 1]! << 16) | (b[o + 2]! << 8) | b[o + 3]!) >>> 0 : u32(b, o);
  const cputype = rd32(4);
  const ncmds = rd32(16);
  const cpu = cputype === 0x0100000c ? "ARM64" : cputype === 0x01000007 ? "x86-64" : cputype === 7 ? "i386" : cputype === 12 ? "ARM" : `cpu ${cputype}`;
  const ftype = rd32(12);
  return {
    format: "Mach-O",
    arch: cpu,
    bits: is64 ? 64 : 32,
    kind: ftype === 6 ? "Dynamic library" : ftype === 2 ? "Executable" : `Type ${ftype}`,
    sections: [],
    imports: [],
    exports: [],
    size,
    truncated,
    notes: [`Mach-O header parsed: ${ncmds} load commands. Deep symbol tables not expanded.`],
  };
}

export function parseBinary(bytes: Uint8Array, size: number, truncated: boolean): BinaryInfo | null {
  return parsePE(bytes, size, truncated) ?? parseELF(bytes, size, truncated) ?? parseMachO(bytes, size, truncated);
}
