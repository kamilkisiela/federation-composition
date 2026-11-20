// Builds the addon (release) and copies it next to this file as federation-composition.node,
// the file name Node loads with require().
import { spawnSync } from "node:child_process";
import { copyFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const { status } = spawnSync("cargo", ["build", "--release", "-p", "federation-composition-napi"], {
  cwd: root,
  stdio: "inherit",
});
if (status !== 0) process.exit(status ?? 1);

const lib = {
  darwin: "libfederation_composition_napi.dylib",
  win32: "federation_composition_napi.dll",
}[process.platform] ?? "libfederation_composition_napi.so";
const out = fileURLToPath(new URL("federation-composition.node", import.meta.url));
copyFileSync(`${root}target/release/${lib}`, out);
console.log(`-> ${out}`);
