# Conformité du serveur MCP

Deux scénarios pilotent le vrai `protocol.exe` avec les clients MCP officiels. Chaque SDK
valide les messages contre les types du protocole, et la sortie de chaque outil contre le
schéma JSON qu'il annonce : une réponse non conforme fait échouer le scénario.

```powershell
cargo build --release

# SDK Python (paquet `mcp`, 1.28 ou plus) : écriture, lecture seule, fenêtre ouverte
python conformance.py ..\..\target\release\protocol.exe

# SDK TypeScript (@modelcontextprotocol/sdk, celui de Claude Desktop et de Claude Code)
$env:SDK = "C:\chemin\node_modules\@modelcontextprotocol\sdk"
node conformance-ts.mjs ..\..\target\release\protocol.exe
```

Les carnets d'essai sont créés dans un dossier temporaire, puis effacés. Le scénario
« fenêtre ouverte » lance `protocol serve --port 7397` le temps du test.

Le serveur parle la langue du carnet (réglage `lang`, sinon la variable `PROTOCOL_LANG`,
sinon la langue du système). Les scénarios vérifient des textes français : ils lancent le
serveur avec `PROTOCOL_LANG=fr`, quelle que soit la langue de la machine. Le scénario
Python « anglais » le relance avec `PROTOCOL_LANG=en` et vérifie que descriptions,
erreurs, consignes et ressources sont en anglais, sous les mêmes noms, et que les sorties
restent conformes à leurs schémas.
