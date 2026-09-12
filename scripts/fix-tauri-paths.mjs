#!/usr/bin/env node
/**
 * Rewrite absolute asset paths so the UI loads under Tauri's custom protocol.
 */
import fs from "node:fs";
import path from "node:path";

const buildDir = path.resolve("build");

function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (/\.(html|js|css)$/.test(entry.name)) rewrite(full);
  }
}

function rewrite(file) {
  const before = fs.readFileSync(file, "utf8");
  let after = before
    // HTML/CSS/JS absolute app assets
    .replaceAll('"/_app/', '"./_app/')
    .replaceAll("'/ _app/", "'./_app/")
    .replaceAll("'/ _app/", "'./_app/")
    .replaceAll("'/_app/", "'./_app/")
    .replaceAll("`/_app/", "`./_app/")
    .replaceAll('href="/favicon', 'href="./favicon')
    .replaceAll("href='/favicon", "href='./favicon")
    .replaceAll('url(/_app/', 'url(./_app/')
    .replaceAll("url(/_app/", "url(./_app/");

  // import("/_app/...") in inline scripts
  after = after.replaceAll('import("/_app/', 'import("./_app/');
  after = after.replaceAll("import('/_app/", "import('./_app/");

  if (after !== before) {
    fs.writeFileSync(file, after);
    console.log("fixed", path.relative(process.cwd(), file));
  }
}

if (!fs.existsSync(buildDir)) {
  console.error("build/ missing — run vite build first");
  process.exit(1);
}
walk(buildDir);
console.log("Tauri asset paths normalized.");
