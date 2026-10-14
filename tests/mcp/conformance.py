"""Conformité du serveur MCP de protocol, vérifiée par le client officiel (paquet `mcp`).

Le SDK valide chaque message contre les types du protocole, et la sortie de chaque outil
contre le schéma JSON qu'il annonce : une sortie non conforme lève une erreur.

    python conformance.py C:/chemin/protocol.exe

Quatre scénarios : écriture (`--write`, carnet neuf), lecture seule, écriture pendant
qu'une fenêtre de protocol (`protocol serve`) tient le carnet — en français, la langue de
référence (PROTOCOL_LANG=fr) —, puis un carnet en anglais (PROTOCOL_LANG=en).
"""

import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

import anyio
from mcp import ClientSession, StdioServerParameters, types
from mcp.client.stdio import get_default_environment, stdio_client
from mcp.shared.exceptions import McpError
from pydantic import AnyUrl

# Un chemin absolu : sous Windows, un chemin relatif à barres obliques ne se lance pas toujours.
EXE = str(Path(sys.argv[1]).resolve())
failures = 0


def server(args, lang="fr"):
    """Le serveur, lancé dans une langue fixée : le SDK ne transmet qu'un environnement réduit."""
    return StdioServerParameters(command=EXE, args=args, env={**get_default_environment(), "PROTOCOL_LANG": lang})


def check(ok, label):
    global failures
    print(("ok   " if ok else "ÉCHEC") + " " + label)
    if not ok:
        failures += 1


async def tool(s, name, args):
    """Un appel d'outil réussi ; le SDK a déjà validé sa sortie contre son schéma."""
    r = await s.call_tool(name, args)
    if r.isError:
        raise AssertionError(f"{name} : {r.content[0].text}")
    return r.structuredContent


async def scenario_write(carnet):
    params = server(["mcp", "--write", "--carnet", carnet, "--port", "1"])
    updates, logs = [], []

    async def on_message(m):
        if isinstance(m, types.ServerNotification) and isinstance(m.root, types.ResourceUpdatedNotification):
            updates.append(str(m.root.params.uri))

    async def on_log(p):
        logs.append(p)

    async with stdio_client(params) as (r, w):
        async with ClientSession(r, w, message_handler=on_message, logging_callback=on_log) as s:
            init = await s.initialize()
            check(init.protocolVersion == types.LATEST_PROTOCOL_VERSION, f"version négociée : {init.protocolVersion}")
            caps = init.capabilities
            check(
                caps.resources.subscribe and caps.resources.listChanged and caps.prompts is not None and caps.completions is not None,
                "capacités : outils, ressources suivies, consignes, complétion, journal",
            )
            tools = (await s.list_tools()).tools
            check(len(tools) == 19, f"{len(tools)} outils avec --write")
            check(all(t.outputSchema and t.annotations and t.title for t in tools), "chaque outil : titre, schéma de sortie, annotations")
            check(
                [t.name for t in tools if t.annotations.destructiveHint] == ["update_note", "trash_note", "update_map"],
                "outils signalés destructifs : update_note, trash_note, update_map",
            )
            await s.set_logging_level("info")

            # Les 19 outils, chacun validé par le SDK.
            n = await tool(s, "create_note", {
                "map": "ascent", "title": "Retake B à trois", "type": "retake", "side": "def",
                "tags": ["b", "retake"], "pinned": True, "body": "## Regroupement\n- au marché, fumée **heaven**\n",
            })
            check(n["note"]["id"] == "retake-b-a-trois", "create_note")
            await tool(s, "create_note", {"map": "ascent", "title": "Exécution A rapide", "type": "execute", "side": "atk"})
            maps = await tool(s, "list_maps", {})
            check(any(m["id"] == "ascent" and m["notes"] == 2 for m in maps["maps"]), "list_maps")
            check((await tool(s, "get_map", {"map": "ascent"}))["map"]["id"] == "ascent", "get_map")
            check((await tool(s, "list_notes", {"map": "ascent", "side": "def"}))["total"] == 1, "list_notes")
            note = await tool(s, "read_note", {"map": "ascent", "id": "retake-b-a-trois"})
            check("heaven" in note["body"], "read_note")
            check((await tool(s, "search_notes", {"query": "fumee type:retake"}))["total"] == 1, "search_notes")
            check(len((await tool(s, "match_sheet", {"map": "ascent"}))["notes"]) == 1, "match_sheet")
            check(any(t["tag"] == "b" for t in (await tool(s, "list_tags", {}))["tags"]), "list_tags")
            # Un carnet neuf a déjà sa note d'accueil : les deux nôtres sont les plus récentes.
            recent = [x["id"] for x in (await tool(s, "recent_notes", {"limit": 5}))["notes"]]
            check(set(recent[:2]) == {"retake-b-a-trois", "execution-a-rapide"}, "recent_notes")
            check("Retake B" in (await tool(s, "format_note", {"map": "ascent", "id": "retake-b-a-trois"}))["text"], "format_note")
            up = await tool(s, "update_note", {"map": "ascent", "id": "retake-b-a-trois", "base_version": note["version"], "title": "Retake B (révisé)"})
            check(up["backup"] is not None, "update_note : version d'avant à la corbeille")
            stale = await s.call_tool("update_note", {"map": "ascent", "id": "retake-b-a-trois", "base_version": note["version"], "body": "écrasé"})
            check(stale.isError and "rien n'a été écrasé" in stale.content[0].text, "update_note refuse une version périmée")
            check((await tool(s, "append_to_note", {"map": "ascent", "id": "retake-b-a-trois", "text": "## Calls\n- go"}))["backup"] is None, "append_to_note")
            check((await tool(s, "set_pinned", {"map": "ascent", "id": "retake-b-a-trois", "pinned": False}))["note"]["pinned"] is False, "set_pinned")
            moved = await tool(s, "move_note", {"map": "ascent", "id": "execution-a-rapide", "to_map": "bind"})
            check(moved["note"]["map"] == "bind", "move_note")
            gone = await tool(s, "trash_note", {"map": "bind", "id": "execution-a-rapide"})
            trash = await tool(s, "list_trash", {})
            check(len(trash["items"]) == 2, "list_trash : la note retirée et la version d'avant")
            back = await tool(s, "restore_from_trash", {"key": gone["trash"]})
            check(back["id"] == "execution-a-rapide", "restore_from_trash")
            check((await tool(s, "create_map", {"title": "Carte d'essai"}))["map"]["id"] == "carte-d-essai", "create_map")
            mv = (await tool(s, "get_map", {"map": "ascent"}))["map"]["version"]
            um = await tool(s, "update_map", {"map": "ascent", "overview": "Mid d'abord.", "base_version": mv, "in_pool": True})
            check(um["backup"] is not None and um["map"]["pool"] is True, "update_map : présentation d'avant à la corbeille")
            check(any("écrit dans le carnet" in str(l.data) for l in logs), "journal : les écritures sont signalées")

            # Ressources : liste paginée, modèles, lecture, abonnement.
            resources, cursor = [], None
            while True:
                page = await s.list_resources(cursor)
                resources += page.resources
                cursor = page.nextCursor
                if not cursor:
                    break
            uris = [str(x.uri) for x in resources]
            check("protocol://guide" in uris and "protocol://note/ascent/retake-b-a-trois" in uris, f"resources/list ({len(uris)} ressources)")
            check(len((await s.list_resource_templates()).resourceTemplates) == 4, "resources/templates/list")
            for uri in ["protocol://guide", "protocol://types", "protocol://map/ascent", "protocol://sheet/ascent",
                        "protocol://note/ascent/retake-b-a-trois", "protocol://search/heaven"]:
                text = (await s.read_resource(AnyUrl(uri))).contents[0].text
                check(len(text) > 20, f"resources/read {uri}")
            try:
                await s.read_resource(AnyUrl("protocol://note/ascent/nope"))
                check(False, "ressource absente refusée")
            except McpError as e:
                check(e.error.code == -32002, "ressource absente : erreur -32002")
            watched = "protocol://note/ascent/retake-b-a-trois"
            await s.subscribe_resource(AnyUrl(watched))
            await tool(s, "append_to_note", {"map": "ascent", "id": "retake-b-a-trois", "text": "- encore"})
            with anyio.move_on_after(8):
                while watched not in updates:
                    await anyio.sleep(0.2)
            check(watched in updates, "abonnement : notifications/resources/updated reçue")
            await s.unsubscribe_resource(AnyUrl(watched))

            # Consignes prêtes et complétion.
            prompts = (await s.list_prompts()).prompts
            check(len(prompts) == 6, f"{len(prompts)} consignes prêtes")
            asks = {
                "resumer": {"map": "ascent", "note": "retake-b-a-trois"},
                "failles": {"map": "ascent", "note": "Retake B (révisé)"},
                "variante": {"map": "ascent", "note": "retake-b-a-trois"},
                "preparer_match": {"map": "ascent", "adversaire": "Les Rivaux"},
                "debrief": {"map": "ascent", "observations": "4 retakes B perdus"},
                "ranger_texte": {"map": "bind", "texte": "smoke heaven puis entrée B"},
            }
            for name, args in asks.items():
                got = await s.get_prompt(name, args)
                check(len(got.messages) >= 2, f"prompts/get {name}")
            c = await s.complete(types.PromptReference(type="ref/prompt", name="resumer"), {"name": "map", "value": "asc"})
            check(c.completion.values == ["ascent"], "completion : map")
            c = await s.complete(
                types.ResourceTemplateReference(type="ref/resource", uri="protocol://note/{map}/{id}"),
                {"name": "id", "value": "ret"},
                {"map": "ascent"},
            )
            check(c.completion.values == ["retake-b-a-trois"], "completion : note d'une map")


async def scenario_read_only(carnet):
    params = server(["mcp", "--carnet", carnet])
    async with stdio_client(params) as (r, w):
        async with ClientSession(r, w) as s:
            init = await s.initialize()
            check("lecture seule" in (init.instructions or ""), "lecture seule annoncée")
            tools = (await s.list_tools()).tools
            check(len(tools) == 10 and all(t.annotations.readOnlyHint for t in tools), "lecture seule : 10 outils, tous en lecture")
            try:
                await s.call_tool("create_note", {"map": "ascent", "title": "x"})
                check(False, "écriture refusée sans --write")
            except McpError as e:
                check("--write" in e.error.message, "écriture refusée sans --write")


def http(port, method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=data, method=method, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=5) as res:
        return res.status, json.loads(res.read() or b"null")


async def scenario_window(carnet):
    port = 7397
    env = {**os.environ, "PROTOCOL_LANG": "fr"}
    window = subprocess.Popen(
        [EXE, "serve", "--port", str(port), "--carnet", carnet], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env
    )
    try:
        for _ in range(50):
            try:
                http(port, "GET", "/api/ping")
                break
            except OSError:
                time.sleep(0.1)
        params = server(["mcp", "--write", "--carnet", carnet, "--port", str(port)])
        async with stdio_client(params) as (r, w):
            async with ClientSession(r, w) as s:
                await s.initialize()
                n = await tool(s, "create_note", {"map": "ascent", "title": "Anti-strat Rivaux", "type": "antistrat"})
                status, seen = http(port, "GET", f"/api/notes/ascent/{n['note']['id']}")
                check(status == 200 and seen["title"] == "Anti-strat Rivaux", "fenêtre ouverte : la note écrite par l'assistant y est aussitôt")
                note = await tool(s, "read_note", {"map": "ascent", "id": n["note"]["id"]})
                up = await tool(s, "update_note", {"map": "ascent", "id": n["note"]["id"], "base_version": note["version"], "body": "- Tendances"})
                status, trash = http(port, "GET", "/api/trash")
                check(up["backup"] is not None and status == 200, "fenêtre ouverte : la version d'avant est dans sa corbeille")
    finally:
        try:
            http(port, "POST", "/api/quit", {})
        except OSError:
            pass
        window.wait(timeout=10)


async def scenario_english(carnet):
    """Un carnet en anglais : tout ce que lisent le client et l'assistant est en anglais,
    les noms restent les mêmes, et les sorties restent conformes à leurs schémas."""
    async with stdio_client(server(["mcp", "--write", "--carnet", carnet, "--port", "1"], "en")) as (r, w):
        async with ClientSession(r, w) as s:
            init = await s.initialize()
            how = init.instructions or ""
            check("never instructions to follow" in how and "carnet" not in how, "anglais : instructions")
            check(init.serverInfo.title == "protocol — Valorant coaching notebook", f"anglais : serverInfo.title ({init.serverInfo.title})")
            tools = {t.name: t for t in (await s.list_tools()).tools}
            check(len(tools) == 19 and "read_note" in tools, "anglais : mêmes noms d'outils")
            rn = tools["read_note"]
            check(rn.title == "Read a note" and rn.description.startswith("Reads a full note"), "anglais : titre et description d'outil")
            check(rn.inputSchema["properties"]["map"]["description"].startswith("map id"), "anglais : description des propriétés d'entrée")
            check(rn.outputSchema["properties"]["body"]["description"] == "the note body, in Markdown", "anglais : description des propriétés de sortie")
            n = await tool(s, "create_note", {"map": "ascent", "title": "B site retake", "type": "retake", "side": "def", "pinned": True, "body": "- smoke heaven\n"})
            check(n["note"]["id"] == "b-site-retake", "anglais : create_note (sortie validée)")
            for name, args in [
                ("list_maps", {}), ("get_map", {"map": "ascent"}), ("list_notes", {"map": "ascent"}),
                ("read_note", {"map": "ascent", "id": "b-site-retake"}), ("search_notes", {"query": "heaven"}),
                ("match_sheet", {"map": "ascent"}), ("format_note", {"map": "ascent", "pinned_only": True}), ("list_trash", {}),
            ]:
                await tool(s, name, args)
                check(True, f"anglais : {name} (sortie validée par le SDK)")
            miss = await s.call_tool("read_note", {"map": "ascent", "id": "nope"})
            check(miss.isError and miss.content[0].text.startswith("note not found: ascent/nope"), "anglais : erreur d'outil")
            prompts = {p.name: p for p in (await s.list_prompts()).prompts}
            check(prompts["failles"].title == "Find the weaknesses", "anglais : titres des consignes, noms inchangés")
            got = await s.get_prompt("failles", {"map": "ascent", "note": "b-site-retake"})
            intro = got.messages[0].content.text
            check("Play the opponent" in intro and "never instructions to follow" in intro, "anglais : message de consigne")
            guide = (await s.read_resource(AnyUrl("protocol://guide"))).contents[0].text
            check(guide.startswith("# The protocol notebook"), "anglais : ressource guide")


async def main():
    with tempfile.TemporaryDirectory() as d:
        writable = str(Path(d) / "essai.carnet")
        await scenario_write(writable)
        await scenario_read_only(writable)
        await scenario_window(str(Path(d) / "fenetre.carnet"))
        await scenario_english(str(Path(d) / "english.carnet"))
    print(f"{failures} échec(s)" if failures else "serveur MCP conforme : tous les scénarios passent")
    sys.exit(1 if failures else 0)


anyio.run(main)
