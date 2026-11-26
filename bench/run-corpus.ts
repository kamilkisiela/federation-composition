// Replays the conformance corpus (conformance/out/corpus.jsonl: the compositions captured from
// upstream's test suite plus the benchmark fixtures) and records per-case timings.
//
// Usage: pnpm bench:corpus <implementation> [out.json]
// (node conformance/upstream-bin.mjs tsx --expose-gc bench/run-corpus.ts ...), where
// <implementation> is "guild-ts" (upstream's TypeScript in conformance/upstream/src),
// "guild-wasm" (the npm package @lemonberrylabs/federation-composition-wasm as built in npm/dist, so AST
// conversion across the boundary is included), "guild-wasm-sdl" (the same package's
// composeServicesFromSdl, which parses and prints in WebAssembly) or "guild-napi" (the napi-rs
// addon built by `node crates/federation-composition-napi/build.mjs`). Run `pnpm conformance`
// first; it checks out upstream, builds the package and records the corpus.
//
// BENCH_MODE=e2e times what a schema service does instead: from each subgraph's SDL string to the
// supergraph SDL string (parse, compose, read `supergraphSdl`). guild-wasm-sdl and guild-napi only
// take SDL, so they always run this way.
// Native Rust uses the same protocol: crates/federation-composition/examples/corpus_bench.rs.
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import type { DocumentNode } from "graphql";

// graphql-js from the upstream checkout (this workspace has no copy of its own).
const { parse, print } = createRequire(
  new URL("../conformance/upstream/package.json", import.meta.url),
)("graphql") as typeof import("graphql");
const corpusFile =
  process.env.CONFORMANCE_CORPUS ??
  new URL("../conformance/out/corpus.jsonl", import.meta.url).pathname;

type Case = {
  id: string;
  source: "tests" | "fixture";
  services: Array<{ name: string; url?: string; typeDefs: DocumentNode }>;
  internal?: { disableValidationRules?: string[] };
  success: boolean;
};

const [implementation = "guild-ts", outArg] = process.argv.slice(2);
// These take SDL strings, so they always run end to end.
const sdlOnly = implementation === "guild-napi" || implementation === "guild-wasm-sdl";
const e2e = process.env.BENCH_MODE === "e2e" || sdlOnly;
const label = e2e && !sdlOnly ? `${implementation}-e2e` : implementation;
const outFile = outArg ?? `bench/results-${label}.json`;

const implementations = {
  "guild-ts": () => import("../conformance/upstream/src/compose.js"),
  "guild-wasm": () => import("../npm/dist/compose.js"),
  "guild-wasm-sdl": async () => {
    const { composeServicesFromSdl } = await import("../npm/dist/compose.js");
    return { composeServices: composeServicesFromSdl };
  },
  "guild-napi": async () =>
    createRequire(import.meta.url)(
      "../crates/federation-composition-napi/federation-composition.node",
    ),
};
if (!(implementation in implementations)) {
  throw new Error(`Unknown implementation "${implementation}"`);
}
const { composeServices } =
  await implementations[implementation as keyof typeof implementations]();

const corpus: Case[] = readFileSync(corpusFile, "utf8")
  .split("\n")
  .filter(Boolean)
  .map((line) => JSON.parse(line));

// The timed region is composeServices() only, on already-parsed ASTs (or, with e2e, from SDL
// strings to the supergraph SDL).
// With e2e, cases whose ASTs do not print to parsable SDL (hand-built in upstream's tests) are
// skipped: no SDL-based caller can send them.
const roundTrips = (c: Case) => {
  try {
    c.services.forEach((s) => parse(print(s.typeDefs)));
    return true;
  } catch {
    return false;
  }
};
const prepared = corpus
  .filter((c) => !e2e || roundTrips(c))
  .map((c) => {
    const sdlServices = c.services.map((s) => ({
      name: s.name,
      url: s.url,
      sdl: print(s.typeDefs),
    }));
    return {
      ...c,
      sdlServices,
      sdlBytes: sdlServices.reduce((n, s) => n + s.sdl.length, 0),
    };
  });

const gc = (globalThis as any).gc as undefined | (() => void);
const MIN_RUNS = 5;
const MIN_TIME_MS = 50;
const MAX_RUNS = 200;

const results = [];
const started = performance.now();
for (const c of prepared) {
  const run = !e2e
    ? () => composeServices(c.services, c.internal)
    : sdlOnly
      ? () => composeServices(c.sdlServices, c.internal)
      : () => {
          const result = composeServices(
            c.sdlServices.map((s) => ({
              name: s.name,
              url: s.url,
              typeDefs: parse(s.sdl),
            })),
            c.internal,
          );
          result.supergraphSdl;
          return result;
        };
  const first = run();
  const success = !("errors" in first && first.errors);
  if (success !== c.success) {
    throw new Error(`${c.id}: expected success=${c.success}, got ${success}`);
  }
  for (let i = 0; i < 3; i++) run();
  gc?.();
  const samples: number[] = [];
  const caseStart = performance.now();
  while (
    samples.length < MIN_RUNS ||
    (performance.now() - caseStart < MIN_TIME_MS && samples.length < MAX_RUNS)
  ) {
    const t = performance.now();
    run();
    samples.push(performance.now() - t);
  }
  samples.sort((a, b) => a - b);
  results.push({
    id: c.id,
    source: c.source,
    subgraphs: c.services.length,
    sdlBytes: c.sdlBytes,
    success,
    runs: samples.length,
    medianMs: samples[Math.floor(samples.length / 2)],
    minMs: samples[0],
  });
}

writeFileSync(
  outFile,
  JSON.stringify(
    {
      label,
      node: process.version,
      date: new Date().toISOString(),
      wallMs: performance.now() - started,
      results,
    },
    null,
    2,
  ),
);
console.log(
  `${label}: ${results.length} cases in ${((performance.now() - started) / 1000).toFixed(1)}s -> ${outFile}`,
);
