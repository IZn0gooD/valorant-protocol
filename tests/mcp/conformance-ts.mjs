// Conformité du serveur MCP de protocol, vérifiée par le SDK TypeScript officiel
// (@modelcontextprotocol/sdk, celui de Claude Desktop et de Claude Code) : chaque réponse
// est validée contre les types du protocole, chaque sortie d'outil contre son schéma.
//
//   SDK=C:/chemin/node_modules/@modelcontextprotocol/sdk node conformance-ts.mjs C:/chemin/protocol.exe
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const sdk = (p) => import(pathToFileURL(join(process.env.SDK, "dist/esm", p)).href);
const { Client } = await sdk("client/index.js");
const { StdioClientTransport, getDefaultEnvironment } = await sdk("client/stdio.js");

const exe = process.argv[2];
let failures = 0;
const check = (ok, label) => {
  console.log(`${ok ? "ok   " : "ÉCHEC"} ${label}`);
  if (!ok) failures++;
};

const dir = mkdtempSync(join(tmpdir(), "protocol-mcp-ts-"));
// La langue de référence, fixée : les textes vérifiés ici sont en français.
const transport = new StdioClientTransport({
  command: exe,
  args: ["mcp", "--write", "--carnet", join(dir, "essai.carnet"), "--port", "1"],
  env: { ...getDefaultEnvironment(), PROTOCOL_LANG: "fr" },
});
const client = new Client({ name: "conformite", version: "1.0.0" });
const updated = [];
client.fallbackNotificationHandler = async (n) => {
  if (n.method === "notifications/resources/updated") updated.push(n.params.uri);
};
try {
  await client.connect(transport);
  check(client.getServerVersion()?.name === "protocol", `serveur : ${client.getServerVersion()?.title}`);
  const tools = (await client.listTools()).tools;
  check(tools.length === 19, `${tools.length} outils`);
  const call = async (name, args) => {
    const r = await client.callTool({ name, arguments: args });
    if (r.isError) throw new Error(`${name} : ${r.content[0].text}`);
    return r.structuredContent;
  };
  const n = await call("create_note", { map: "ascent", title: "Retake B à trois", type: "retake", side: "def", tags: ["b"], pinned: true, body: "- fumée heaven\n" });
  check(n.note.id === "retake-b-a-trois", "create_note");
  const read = await call("read_note", { map: "ascent", id: "retake-b-a-trois" });
  for (const [name, args] of [
    ["list_maps", {}],
    ["get_map", { map: "ascent" }],
    ["list_notes", { map: "ascent", limit: 1 }],
    ["search_notes", { query: "fumee" }],
    ["match_sheet", { map: "ascent" }],
    ["list_tags", {}],
    ["recent_notes", {}],
    ["format_note", { map: "ascent", id: "retake-b-a-trois", format: "discord" }],
    ["list_trash", {}],
    ["update_note", { map: "ascent", id: "retake-b-a-trois", base_version: read.version, side: "both" }],
    ["append_to_note", { map: "ascent", id: "retake-b-a-trois", text: "- go" }],
    ["set_pinned", { map: "ascent", id: "retake-b-a-trois", pinned: true }],
    ["create_map", { title: "Carte TS" }],
    ["update_map", { map: "carte-ts", in_pool: false }],
  ]) {
    await call(name, args);
    check(true, `${name} (sortie validée par le SDK)`);
  }
  const res = (await client.listResources()).resources;
  check(res.some((r) => r.uri === "protocol://note/ascent/retake-b-a-trois"), `resources/list (${res.length}${res.length === 100 ? ", page pleine" : ""})`);
  const text = (await client.readResource({ uri: "protocol://sheet/ascent" })).contents[0].text;
  check(text.includes("Retake B à trois"), "resources/read : fiche match");
  await client.subscribeResource({ uri: "protocol://note/ascent/retake-b-a-trois" });
  await call("append_to_note", { map: "ascent", id: "retake-b-a-trois", text: "- encore" });
  for (let i = 0; i < 40 && !updated.length; i++) await new Promise((r) => setTimeout(r, 200));
  check(updated.includes("protocol://note/ascent/retake-b-a-trois"), "abonnement : mise à jour reçue");
  const prompts = (await client.listPrompts()).prompts;
  check(prompts.length === 6, `${prompts.length} consignes prêtes`);
  const p = await client.getPrompt({ name: "preparer_match", arguments: { map: "ascent" } });
  check(p.messages.some((m) => m.content.type === "resource"), "prompts/get : fiche match jointe");
  const c = await client.complete({ ref: { type: "ref/prompt", name: "resumer" }, argument: { name: "map", value: "asc" } });
  check(c.completion.values[0] === "ascent", "completion/complete");
  await client.setLoggingLevel("info");
  check(true, "logging/setLevel");
} catch (e) {
  check(false, String(e?.stack || e));
} finally {
  await client.close().catch(() => {});
  rmSync(dir, { recursive: true, force: true });
}
console.log(failures ? `${failures} échec(s)` : "conforme pour le SDK TypeScript aussi");
process.exit(failures ? 1 : 0);
