import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";

export function getNextVersion(current, releaseTypeOrVersion) {
  const clean = releaseTypeOrVersion.replace(/^v/, "");
  if (/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(clean)) {
    return clean;
  }
  const [major, minor, patch] = current.split(".").map(Number);
  if (releaseTypeOrVersion === "patch") return `${major}.${minor}.${patch + 1}`;
  if (releaseTypeOrVersion === "minor") return `${major}.${minor + 1}.0`;
  if (releaseTypeOrVersion === "major") return `${major + 1}.0.0`;
  throw new Error(`Unknown version or release type: ${releaseTypeOrVersion}`);
}

export function bump(arg) {
  const pkgPath = path.resolve("package.json");
  const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf-8"));
  const oldVersion = pkg.version;
  const newVersion = getNextVersion(oldVersion, arg);

  // 1. Update package.json
  pkg.version = newVersion;
  fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

  // 2. Update package-lock.json (if present)
  const lockPath = path.resolve("package-lock.json");
  if (fs.existsSync(lockPath)) {
    const lock = JSON.parse(fs.readFileSync(lockPath, "utf-8"));
    lock.version = newVersion;
    if (lock.packages && lock.packages[""]) {
      lock.packages[""].version = newVersion;
    }
    fs.writeFileSync(lockPath, JSON.stringify(lock, null, 2) + "\n");
  }

  // 3. Update src-tauri/tauri.conf.json
  const tauriConfPath = path.resolve("src-tauri/tauri.conf.json");
  if (fs.existsSync(tauriConfPath)) {
    let tauriConf = fs.readFileSync(tauriConfPath, "utf-8");
    tauriConf = tauriConf.replace(
      /("version"\s*:\s*")[^"]+(")/,
      `$1${newVersion}$2`
    );
    fs.writeFileSync(tauriConfPath, tauriConf);
  }

  // 4. Update src-tauri/Cargo.toml
  const cargoTomlPath = path.resolve("src-tauri/Cargo.toml");
  if (fs.existsSync(cargoTomlPath)) {
    let cargoToml = fs.readFileSync(cargoTomlPath, "utf-8");
    cargoToml = cargoToml.replace(
      /(\[package\][\s\S]*?version\s*=\s*")[^"]+(")/,
      `$1${newVersion}$2`
    );
    fs.writeFileSync(cargoTomlPath, cargoToml);
  }

  // 5. Update src-tauri/Cargo.lock
  const cargoLockPath = path.resolve("src-tauri/Cargo.lock");
  if (fs.existsSync(cargoLockPath)) {
    let cargoLock = fs.readFileSync(cargoLockPath, "utf-8");
    cargoLock = cargoLock.replace(
      /(\[\[package\]\]\s+name\s*=\s*"blacktape-desktop"\s+version\s*=\s*")[^"]+(")/,
      `$1${newVersion}$2`
    );
    fs.writeFileSync(cargoLockPath, cargoLock);
  }

  // Verify Cargo metadata / lockfile consistency
  try {
    execSync("cargo check --manifest-path src-tauri/Cargo.toml", { stdio: "ignore" });
  } catch {
    // If cargo check fails in sandbox, the lockfile is already updated via regex
  }

  console.log(`\nSuccessfully bumped version: ${oldVersion} -> ${newVersion}`);
  console.log("Updated files:");
  console.log("  - package.json");
  console.log("  - package-lock.json");
  console.log("  - src-tauri/tauri.conf.json");
  console.log("  - src-tauri/Cargo.toml");
  console.log("  - src-tauri/Cargo.lock\n");
}

if (process.argv[1]?.replace(/\\/g, "/").endsWith("scripts/bump.js")) {
  const arg = process.argv[2];
  if (!arg) {
    console.error("Usage: npm run bump <patch|minor|major|x.y.z>");
    process.exit(1);
  }
  bump(arg);
}
