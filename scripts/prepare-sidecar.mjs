// Builds the `aiannotations` CLI in release mode and copies it to where Tauri's `externalBin`
// expects it (crates/app/binaries/aiannotations-<target-triple>[.exe]). The installer then
// ships it next to the app executable, where the app looks for it to write the hooks.
//
// Usage: node scripts/prepare-sidecar.mjs [target-triple]   (default: the host triple)
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const hostTriple = () =>
  execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n')
    .find((l) => l.startsWith('host: '))
    .slice('host: '.length)
    .trim();

const target = process.argv[2] || hostTriple();
const exe = target.includes('windows') ? '.exe' : '';
const args = ['build', '--release', '-p', 'aiannotations-cli', '--bin', 'aiannotations'];
if (process.argv[2]) args.push('--target', target);
execFileSync('cargo', args, { cwd: root, stdio: 'inherit' });

const built = process.argv[2]
  ? join(root, 'target', target, 'release', `aiannotations${exe}`)
  : join(root, 'target', 'release', `aiannotations${exe}`);
const dest = join(root, 'crates', 'app', 'binaries', `aiannotations-${target}${exe}`);
mkdirSync(dirname(dest), { recursive: true });
copyFileSync(built, dest);
console.log(`sidecar: ${dest}`);
