// Self-contained build: inline the bundled JS into index.html so the page
// works when opened directly from the filesystem (file://), where external
// module scripts are blocked. Sample auto-load still needs a server; the
// fetch failure is caught and drag-drop remains available.

const build = await Bun.build({
  entrypoints: ["src/main.ts"],
  target: "browser",
  minify: true,
});
if (!build.success) {
  console.error(...build.logs);
  process.exit(1);
}
const js = await build.outputs[0].text();
const html = await Bun.file("index.html").text();
const marker = '<script type="module" src="main.js"></script>';
if (!html.includes(marker)) {
  console.error("index.html lost the main.js script marker");
  process.exit(1);
}
await Bun.write("dist/index.html", html.replace(marker, `<script type="module">${js}</script>`));
console.log("dist/index.html (self-contained)");
