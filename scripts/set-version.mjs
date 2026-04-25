import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const argv = process.argv.slice(2);
const versionFlag = argv.indexOf("--version");
const dryRun = argv.includes("--dry-run");

if (versionFlag === -1 || !argv[versionFlag + 1]) {
  console.error("Usage: node scripts/set-version.mjs --version <semver> [--dry-run]");
  process.exit(1);
}

const version = argv[versionFlag + 1];
const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const packageJsonPath = path.join(repoRoot, "package.json");
const cargoTomlPath = path.join(repoRoot, "src-tauri", "Cargo.toml");
const tauriConfigPath = path.join(repoRoot, "src-tauri", "tauri.conf.json");

const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8"));
packageJson.version = version;

const cargoToml = await readFile(cargoTomlPath, "utf8");
const nextCargoToml = cargoToml.replace(
  /^version = ".*"$/m,
  `version = "${version}"`
);

if (nextCargoToml === cargoToml) {
  throw new Error("Failed to update version in src-tauri/Cargo.toml");
}

const tauriConfig = JSON.parse(await readFile(tauriConfigPath, "utf8"));
tauriConfig.version = version;

if (dryRun) {
  console.log(`Would set project version to ${version}`);
  process.exit(0);
}

await writeFile(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);
await writeFile(cargoTomlPath, nextCargoToml);
await writeFile(tauriConfigPath, `${JSON.stringify(tauriConfig, null, 2)}\n`);

console.log(`Set project version to ${version}`);
