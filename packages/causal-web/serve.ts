const build = await Bun.build({ entrypoints: ["src/main.ts"], target: "browser" });
if (!build.success) {
  console.error(...build.logs);
  process.exit(1);
}
const js = await build.outputs[0].text();
const root = import.meta.dir;

Bun.serve({
  port: 3777,
  async fetch(req) {
    const path = new URL(req.url).pathname;
    if (path === "/main.js")
      return new Response(js, {
        headers: { "content-type": "text/javascript; charset=utf-8" },
      });
    const rel = path === "/" ? "index.html" : path.slice(1);
    if (rel.includes("..")) return new Response("no", { status: 400 });
    const file = Bun.file(`${root}/${rel}`);
    if (await file.exists()) return new Response(file);
    return new Response("not found", { status: 404 });
  },
});
console.log("causal-web → http://localhost:3777");
