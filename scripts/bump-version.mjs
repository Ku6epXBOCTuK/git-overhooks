import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];

if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
	console.error("usage: node scripts/bump-version.mjs <version>");
	process.exit(1);
}

function replace(path, pattern, label) {
	const file = new URL(path, import.meta.url);
	const content = readFileSync(file, "utf8");
	const updated = content.replace(pattern, `$1${version}$2`);
	if (updated === content && !content.includes(version)) {
		throw new Error(`no ${label} to update in ${path}`);
	}
	writeFileSync(file, updated);
	console.log(`${path} -> ${version}`);
}

replace("../package.json", /^([ \t]*"version": ")[^"]+(",?\r?)$/m, "package version");
replace("../Cargo.toml", /^(version = ")[^"]+("\r?)$/m, "cargo version");
replace(
	"../Cargo.lock",
	/(name = "git-overhooks"\r?\nversion = ")[^"]+(")/,
	"lockfile version",
);
