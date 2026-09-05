import { describe, expect, it } from "vitest";
import { langFromExt, parseSource, roleOf } from "../parsers";

const TS_SAMPLE = `// app entry
import React, { useState as st } from "react";
import type { Foo } from "./types";
import "./side-effect.css";

/** docstring comment */
const helper = (a: number, b: string) => {
  return compose(a, b);
};

function compose(x: number, y: string): string {
  if (x > 0) {
    return y.repeat(x);
  }
  return "";
}

export class Widget {
  render(): void {
    helper(1, "x");
  }
}

export default Widget;
`;

const PY_SAMPLE = `"""Module docstring."""
import os
from dataclasses import dataclass, field

class Miner:
    def run(self, amount: int = 3) -> None:
        prepare(amount)
        print(amount)

def prepare(n):
    return n * 2
`;

const RS_SAMPLE = `use std::collections::HashMap;
use crate::widget::{Box, Label};

pub struct Miner {
    depth: u32,
}

enum Mode { Fast, Slow }

pub fn dig(depth: u32) -> u32 {
    load(depth)
}

fn load(depth: u32) -> u32 {
    depth + 1
}
`;

describe("langFromExt / roleOf", () => {
  it("maps extensions to languages", () => {
    expect(langFromExt("tsx")).toBe("ts");
    expect(langFromExt("py")).toBe("python");
    expect(langFromExt("rs")).toBe("rust");
    expect(langFromExt("md")).toBe("markdown");
    expect(langFromExt("png")).toBeNull();
  });

  it("classifies file roles", () => {
    expect(roleOf("README.md", "md")).toBe("doc");
    expect(roleOf("config/app.toml", "toml")).toBe("config");
    expect(roleOf("assets/logo.png", "png")).toBe("asset");
    expect(roleOf("src/main.tsx", "tsx")).toBe("entry");
    expect(roleOf("src/util.ts", "ts")).toBe("source");
  });
});

describe("parseSource: TypeScript", () => {
  const a = parseSource("src/main.tsx", TS_SAMPLE);

  it("extracts imports with names", () => {
    const react = a.imports.find((i) => i.from === "react");
    expect(react).toBeDefined();
    expect(react!.names).toContain("React");
    expect(react!.names).toContain("st");
    expect(a.imports.some((i) => i.from === "./types")).toBe(true);
    expect(a.imports.some((i) => i.from === "./side-effect.css")).toBe(true);
  });

  it("extracts classes, functions and variables", () => {
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Widget")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "compose")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "helper")).toBe(true);
  });

  it("builds intra-file call chains (helper calls compose)", () => {
    expect(a.calls.some((c) => c.from === "helper" && c.to === "compose")).toBe(true);
  });

  it("collects exports", () => {
    expect(a.exports).toContain("Widget");
  });

  it("keeps original line numbers despite stripped comments", () => {
    const compose = a.symbols.find((s) => s.name === "compose")!;
    expect(compose.line).toBe(TS_SAMPLE.split("\n").findIndex((l) => l.startsWith("function compose")) + 1);
  });
});

describe("parseSource: Python", () => {
  const a = parseSource("src/miner.py", PY_SAMPLE);

  it("extracts from-imports with names", () => {
    const imp = a.imports.find((i) => i.from === "dataclasses");
    expect(imp).toBeDefined();
    expect(imp!.names).toEqual(expect.arrayContaining(["dataclass", "field"]));
    expect(a.imports.some((i) => i.from === "os")).toBe(true);
  });

  it("extracts classes and methods", () => {
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Miner")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "run")).toBe(true);
    expect(a.symbols.find((s) => s.name === "run")!.params).toEqual(["amount"]);
  });

  it("detects call run→prepare", () => {
    expect(a.calls.some((c) => c.from === "run" && c.to === "prepare")).toBe(true);
  });
});

describe("parseSource: Rust", () => {
  const a = parseSource("src/miner.rs", RS_SAMPLE);

  it("extracts use items", () => {
    expect(a.imports.some((i) => i.from === "std::collections::HashMap")).toBe(true);
    expect(a.imports.some((i) => i.from === "crate::widget")).toBe(true);
  });

  it("extracts structs/enums as class-kind symbols and fns", () => {
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Miner")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Mode")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
  });

  it("pub symbols become exports and calls chain dig→load", () => {
    expect(a.exports).toContain("dig");
    expect(a.calls.some((c) => c.from === "dig" && c.to === "load")).toBe(true);
  });
});

// —— 第四轮扩容：8 种新语言（与既有语言零重复） ——

describe("parseSource: 8 new languages", () => {
  it("maps the new extensions", () => {
    expect(langFromExt("lua")).toBe("lua");
    expect(langFromExt("pl")).toBe("perl");
    expect(langFromExt("pm")).toBe("perl");
    expect(langFromExt("scala")).toBe("scala");
    expect(langFromExt("hs")).toBe("haskell");
    expect(langFromExt("ex")).toBe("elixir");
    expect(langFromExt("exs")).toBe("elixir");
    expect(langFromExt("zig")).toBe("zig");
    expect(langFromExt("jl")).toBe("julia");
    expect(langFromExt("r")).toBe("r");
  });

  it("parses Lua modules (require + functions)", () => {
    const a = parseSource("src/plumber.lua", `-- helper\nlocal cfg = require("config")\nfunction M.turn(val)\n  return helper(val)\nend\nlocal function helper(x)\n  return x + 1\nend\n`);
    expect(a.lang).toBe("lua");
    expect(a.imports.some((i) => i.from === "config")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "M.turn")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "helper")).toBe(true);
    expect(a.calls.some((c) => c.from === "M.turn" && c.to === "helper")).toBe(true);
  });

  it("parses Perl subs and use-items", () => {
    const a = parseSource("lib/Dig.pm", `use strict;\nuse warnings;\nuse Data::Dumper;\nsub dig {\n  my ($depth) = @_;\n  return load($depth);\n}\nsub load { return 42; }\n`);
    expect(a.lang).toBe("perl");
    expect(a.imports.some((i) => i.from === "Data::Dumper")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "load")).toBe(true);
    expect(a.calls.some((c) => c.from === "dig" && c.to === "load")).toBe(true);
  });

  it("parses Scala defs / case classes", () => {
    const a = parseSource("src/main/scala/Mine.scala", `import scala.collection.mutable\ncase class Vein(depth: Int)\nobject Mine {\n  def dig(depth: Int): Int = load(depth)\n  def load(depth: Int): Int = depth + 1\n}\n`);
    expect(a.lang).toBe("scala");
    expect(a.imports.some((i) => i.from === "scala.collection.mutable")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Vein")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Mine")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
  });

  it("parses Haskell signatures and data types", () => {
    const a = parseSource("src/Dig.hs", `module Dig where\nimport qualified Data.Map as M\ndata Vein = Deep | Shallow\ndig :: Int -> Int\ndig n = load n\nload :: Int -> Int\nload = (+1)\n`);
    expect(a.lang).toBe("haskell");
    expect(a.imports.some((i) => i.from === "Data.Map")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Vein")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
  });

  it("parses Elixir defmodule/def and @import-free alias imports", () => {
    const a = parseSource("lib/dig.ex", `defmodule Dig do\n  alias Dig.Helper\n  def dig(depth) do\n    load(depth)\n  end\n  defp load(depth), do: depth + 1\nend\n`);
    expect(a.lang).toBe("elixir");
    expect(a.imports.some((i) => i.from === "Dig.Helper")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Dig")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
  });

  it("parses Zig fns, structs, pub exports and @import", () => {
    const a = parseSource("src/main.zig", `const std = @import("std");\npub const Vein = struct {\n    depth: u32,\n};\npub fn dig(depth: u32) u32 {\n    return load(depth);\n}\nfn load(depth: u32) u32 {\n    return depth + 1;\n}\n`);
    expect(a.lang).toBe("zig");
    expect(a.imports.some((i) => i.from === "std")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Vein")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
    expect(a.exports).toContain("dig");
    expect(a.calls.some((c) => c.from === "dig" && c.to === "load")).toBe(true);
  });

  it("parses Julia functions and structs", () => {
    const a = parseSource("src/Dig.jl", `using Statistics\nimport LinearAlgebra\nstruct Vein\n    depth::Int\nend\nfunction dig(depth::Int)\n    return load(depth)\nend\nload(depth) = depth + 1\n`);
    expect(a.lang).toBe("julia");
    expect(a.imports.some((i) => i.from === "Statistics")).toBe(true);
    expect(a.imports.some((i) => i.from === "LinearAlgebra")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "class" && s.name === "Vein")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "load")).toBe(true);
  });

  it("parses R functions assigned via <- and library() imports", () => {
    const a = parseSource("analysis/dig.R", `library(stats)\nsource("helpers.R")\ndig <- function(depth) {\n  load(depth)\n}\nload = function(depth) depth + 1\n`);
    expect(a.lang).toBe("r");
    expect(a.imports.some((i) => i.from === "stats")).toBe(true);
    expect(a.imports.some((i) => i.from === "helpers.R")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "dig")).toBe(true);
    expect(a.symbols.some((s) => s.kind === "function" && s.name === "load")).toBe(true);
    expect(a.calls.some((c) => c.from === "dig" && c.to === "load")).toBe(true);
  });
});
