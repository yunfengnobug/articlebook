import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ARCH_SUFFIXES = [
  ["_x64-setup.exe", "x86_64"],
  ["_arm64-setup.exe", "aarch64"],
  ["_x86-setup.exe", "i686"],
];

export function updaterPlatforms(exeNames, readSignature) {
  const platforms = {};
  for (const exeName of exeNames) {
    const arch = ARCH_SUFFIXES.find(([suffix]) => exeName.endsWith(suffix));
    if (!arch) {
      throw new Error(`Cannot map NSIS installer to an updater arch: ${exeName}`);
    }
    const signature = readSignature(exeName).trim();
    if (!signature) {
      throw new Error(`Signature for ${exeName} is empty`);
    }
    const entry = { signature, exeName, arch: arch[1] };
    const keys = [`windows-${arch[1]}-nsis`, `windows-${arch[1]}`];
    for (const key of keys) {
      if (platforms[key]) {
        throw new Error(`Duplicate updater platform ${key} from ${exeName}`);
      }
      platforms[key] = entry;
    }
  }
  if (Object.keys(platforms).length === 0) {
    throw new Error("No NSIS installers to publish");
  }
  return platforms;
}

export function buildManifest({ version, tag, repo, platforms, pubDate }) {
  const entries = {};
  for (const [key, entry] of Object.entries(platforms)) {
    const url = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(entry.exeName)}`;
    entries[key] = { signature: entry.signature, url };
  }
  return {
    version,
    notes: `ArticleBook ${version}`,
    pub_date: pubDate,
    platforms: entries,
  };
}

function main() {
  const workspace = process.env.GITHUB_WORKSPACE || process.cwd();
  const nsisDir = process.env.NSIS_DIR || join(workspace, "target", "release", "bundle", "nsis");
  const tag = process.env.GITHUB_REF_NAME;
  const repo = process.env.GITHUB_REPOSITORY;
  if (!tag || !repo) {
    console.error("GITHUB_REF_NAME and GITHUB_REPOSITORY are required");
    process.exit(1);
  }
  const conf = JSON.parse(readFileSync(join(workspace, "src-tauri", "tauri.conf.json"), "utf8"));
  if (tag !== `v${conf.version}` && tag !== conf.version) {
    console.warn(
      `Tag ${tag} does not match app version ${conf.version}. latest.json uses the app version, so clients only update when that version is newer.`,
    );
  }
  const exeNames = readdirSync(nsisDir).filter((name) => name.endsWith(".exe") && !name.endsWith(".exe.sig"));
  const platforms = updaterPlatforms(exeNames, (exeName) =>
    readFileSync(join(nsisDir, `${exeName}.sig`), "utf8"),
  );
  const manifest = buildManifest({
    version: conf.version,
    tag,
    repo,
    platforms,
    pubDate: new Date().toISOString(),
  });
  const out = join(nsisDir, "latest.json");
  writeFileSync(out, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`Wrote ${out} for ${Object.keys(manifest.platforms).join(", ")}`);
}

if (process.env.LATEST_JSON_SELF_TEST === "1") {
  const signature = Buffer.from("untrusted comment: signature from tauri secret key\nRUTA\n").toString("base64");
  const platforms = updaterPlatforms(["ArticleBook_0.1.0_x64-setup.exe"], () => `${signature}\n`);
  const manifest = buildManifest({
    version: "0.1.0",
    tag: "v0.1.0",
    repo: "yunfengnobug/articlebook",
    platforms,
    pubDate: "2026-11-04T00:00:00.000Z",
  });
  if (manifest.platforms["windows-x86_64"].signature !== signature) {
    throw new Error("signature was not preserved");
  }
  if (!manifest.platforms["windows-x86_64-nsis"]) {
    throw new Error("missing nsis platform key");
  }
  if (
    manifest.platforms["windows-x86_64"].url !==
    "https://github.com/yunfengnobug/articlebook/releases/download/v0.1.0/ArticleBook_0.1.0_x64-setup.exe"
  ) {
    throw new Error(`unexpected url ${manifest.platforms["windows-x86_64"].url}`);
  }
  let failed = false;
  try {
    updaterPlatforms(["setup.exe"], () => signature);
  } catch {
    failed = true;
  }
  if (!failed) throw new Error("unknown installer name should fail");
  console.log("latest.json self-test ok");
} else {
  main();
}
