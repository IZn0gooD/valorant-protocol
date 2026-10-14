# Publier protocol

Les gestes à faire une fois sur ton compte GitHub, puis à chaque version. Tout le reste
(tests, builds Windows/Linux/macOS, paquets MCPB, empreintes, fiche du registre) est fait
par les builds automatiques de `.github/workflows/`.

> Le compte utilisé partout est **IZn0gooD** et le dépôt **valorant-protocol**. Si tu en
> choisis d'autres, remplace-les dans `Cargo.toml`, `packaging/mcpb/manifest.json` et
> `packaging/registry/server.json` (le nom du registre doit commencer par `io.github.<compte>/`).

## 1. Le dépôt (une fois, fait le 2026-10-14)

Le dossier du projet a son propre dépôt Git, branche `main`, relié à
`https://github.com/IZn0gooD/valorant-protocol`. Le `.gitignore` écarte ce qui ne doit
jamais partir : `target/`, les exécutables, tout fichier `.carnet`, **`dist/notes`** (tes
notes d'essai) et les caches d'outils.

Avant chaque envoi, `git status` doit ne montrer ni `.carnet`, ni `.exe`, ni `dist/notes`.

Onglet **Actions** : le build `ci` doit passer au vert sur Windows, Linux et macOS.

## 2. Une version (à chaque fois)

1. Mettre à jour `version` dans `Cargo.toml` et `packaging/mcpb/manifest.json`, et ajouter
   la section de la version dans `CHANGELOG.md`.
2. Étiqueter et envoyer :

```powershell
git commit -am "protocol 1.0.0"
git tag v1.0.0
git push origin main v1.0.0
```

3. Le build `release` crée une **release en brouillon** avec : les archives des trois systèmes,
   les paquets `.mcpb`, `SHA256SUMS.txt` et `server.json` (la fiche du registre, empreintes
   remplies). Relis-la, puis **Publish release**.

## 3. Le registre officiel MCP (après chaque release publiée)

```powershell
# Une fois : installer mcp-publisher (voir https://github.com/modelcontextprotocol/registry, bloc Windows)
mcp-publisher login github          # ouvre github.com/login/device
# Télécharger le server.json de la release, puis, dans son dossier :
mcp-publisher publish
```

Vérification : `https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.IZn0gooD/valorant-protocol`

## 4. Signature Windows (recommandé avant une large diffusion)

Sans signature, Windows SmartScreen avertit au premier lancement. Pour un projet open source,
la [SignPath Foundation](https://signpath.org) signe gratuitement : faire la demande avec
l'adresse du dépôt public ; une fois acceptée, ils fournissent l'étape à ajouter au build
`release` (signature de `protocol.exe` avant la mise en archive).

## 5. Plus tard

- **winget** : une fois les releases signées, proposer un manifeste à `microsoft/winget-pkgs`
  (`winget install protocol`).
- **macOS** : sans notarisation Apple (compte développeur payant), macOS bloque l'ouverture ;
  clic droit → Ouvrir la première fois, ou `xattr -d com.apple.quarantine protocol`.
