# protocol — carnet de coach Valorant

[English](README.md) · **Français**

Les protocoles de l'équipe, rangés map par map : setups de défense, exécutions, retakes, lineups, anti-strats, règles de communication. Interface en français et en anglais. Un exécutable de 2,0 Mo, sans installation, et **un seul fichier de carnet** à côté de lui : rien d'autre, et rien d'écrit ailleurs sur le poste. Logiciel libre (MIT), sans compte, sans réseau, sans télémétrie.

![Accueil de protocol, thème sombre](docs/accueil.png)

## Installer

Téléchargez la dernière version sur la [page des releases](https://github.com/IZn0gooD/valorant-protocol/releases/latest) :

| Système | Fichier | Remarque |
|---|---|---|
| Windows 10/11 | `protocol-…-windows-x64.zip` | décompressez, double-cliquez sur `protocol.exe` |
| macOS (Intel et Apple Silicon) | `protocol-…-macos-universal.tar.gz` | bêta ; première ouverture : clic droit → Ouvrir |
| Linux x64 | `protocol-…-linux-x64.tar.gz` | bêta |
| Claude Desktop | `protocol-…-<système>.mcpb` | double-clic : l'assistant IA lit (et, si vous l'autorisez, écrit) votre carnet |

`SHA256SUMS.txt` donne l'empreinte de chaque fichier. Chaque archive contient aussi un mode d'emploi d'une page, `LISEZMOI.txt` en français et `README.txt` en anglais (les mêmes que dans le dossier `dist/` du dépôt), et ce README dans les deux langues. L'interface s'ouvre dans Edge, Chrome, Brave ou Chromium en mode application, sinon dans le navigateur par défaut.

## Démarrer

1. Copiez `protocol.exe` où vous voulez : clé USB, bureau, dossier partagé.
2. Double-cliquez dessus. Le carnet s'ouvre dans une fenêtre dédiée (Edge, Chrome ou Brave en mode application).
3. Fermez la fenêtre quand vous avez fini : le programme s'arrête tout seul.

Au premier lancement, le fichier `protocol.carnet` est créé à côté du programme, avec une note d'accueil, la map **Général** (les règles d'équipe, qui ne compte pas parmi les maps) et les 13 maps de compétition, Summit comprise. Pour emporter ou sauvegarder le carnet, copiez ces deux fichiers ensemble.

Un ancien dossier `notes/` (versions précédentes de protocol) placé à côté du programme est repris automatiquement dans le carnet au premier lancement, sans être modifié ; un message indique ensuite qu'il peut être supprimé.

Relancer le programme pendant qu'il tourne rouvre simplement la fenêtre : il n'y a jamais deux programmes pour un même carnet.

### Français ou anglais

L'interface existe en français et en anglais. Par défaut, elle suit la langue du système : français si Windows (ou macOS, Linux) est en français, anglais sinon.

- **Changer de langue** : *Aide › Langue / Language*, au choix *Auto*, *Français* ou *English*. Le choix est gardé dans le carnet : il suit la clé USB d'un poste à l'autre.
- **Forcer une langue** : la variable `PROTOCOL_LANG=fr` ou `PROTOCOL_LANG=en` s'applique à la ligne de commande et au serveur MCP (le réglage du carnet, s'il est fixé, reste prioritaire).
- **Un carnet neuf** est créé dans la langue retenue : note d'accueil, map d'équipe « Général » (ou « General »), modèles des types de notes. Les notes déjà écrites ne sont jamais traduites.
- **L'assistant IA** reçoit ses outils, ses consignes et ses messages dans la langue du carnet.

### Sans trace sur le poste

- Le carnet, le seul fichier de données, reste à côté du programme. Pas de dossier dans `%APPDATA%`, pas de clé de registre.
- La fenêtre utilise un **profil de navigateur temporaire**, rangé lui aussi à côté du programme (dossier caché `.protocol-session`) et effacé à la fermeture : historique, cache et stockage de la page ne vont jamais dans le profil Edge ou Chrome du poste. Ce navigateur est lancé dans un « job » Windows : fermer la fenêtre arrête protocol, et un arrêt de protocol ferme la fenêtre.
- Le thème et la langue choisis sont gardés dans le carnet, pas dans le navigateur.
- Si le dossier du programme n'est pas modifiable (CD, partage protégé), le carnet existant s'ouvre en lecture seule et le profil temporaire va dans le dossier temporaire de Windows, effacé lui aussi à la fermeture.
- Avec `--browser default`, c'est le navigateur habituel qui s'ouvre, avec son propre historique : à éviter sur un poste qui n'est pas le vôtre.

Windows garde de son côté une trace des programmes lancés (préchargement, liste des récents) : elle ne dépend pas de protocol.

## L'interface

L'interface reprend les pièces « ui » d'[ascii.rest](https://ascii.rest), avec une seule règle de lecture : **ce qui est entre crochets se clique**.

| Pièce | Où | À quoi ça sert |
|---|---|---|
| boutons `[ Copier ]` | partout | inversés au survol ; le bouton principal l'est d'emblée (`[ Modifier ]`, `[ Terminer ]`) |
| cases `[x] Épinglée` | note, éditeur | une bascule : épinglée ou non, aperçu affiché ou non |
| choix `(*) Défense` | filtres, formulaires | un seul choix parmi plusieurs : type, côté |
| champs `[ Ascent  v ]` | formulaires | une saisie ou une liste ; `>` marque la ligne où l'on se trouve |
| cadres titrés | cartes, présentation, éditeur | le type d'une note dans le trait du haut, sa date dans celui du bas ; les raccourcis utiles dans le bas des formulaires |
| séparateurs `── Notes (5) ──` | sections, titres des notes | ouvrent chaque partie, avec son compte et son action |
| arborescence `├── Ascent/` | à gauche | les maps (le pool actif en tête) ; la map ouverte déplie sa **fiche match** et ses notes, à deux clics en temps mort |

- **En haut**, la recherche et `[ + Nouvelle note ]`, toujours à portée de clic.
- **Les noms de maps** s'écrivent en texte simple, dans la police Paper Mono de [paper.design](https://paper.design) qui sert à toute l'interface. Seul le titre de l'accueil garde ses lettres-blocs, à côté de la mini-carte radar.
- **Les messages** s'affichent en bas, comme un journal de démarrage : `[ OK ] Texte copié.`, `[ !! ]` en cas d'échec.
- **Thèmes clair et sombre**, et en petite fenêtre (à côté du jeu, par exemple) la navigation passe dans un tiroir.
- **Accessible** : tout se fait au clavier (les raccourcis accélèrent, ils ne remplacent rien), focus toujours visible, de vrais boutons radio et de vraies bascules derrière les crochets, animations coupées si le système le demande, contrastes WCAG AA mesurés sur 112 éléments dans les deux thèmes (`tests/ui/ui-contrast.cjs`).

## Au quotidien

| Pour… | Faire |
|---|---|
| écrire un protocole | **Nouvelle note** (en haut à droite, ou `N`) : un titre, une map, un type. Le modèle du type est pré-rempli (rôles, timings, utilitaires…). |
| reprendre un texte existant | copiez-le dans Discord, Word, Google Docs ou Notion, puis `Ctrl+V` sur une page du carnet : une nouvelle note s'ouvre, déjà remplie, titre compris. Ou **Coller un protocole**, sur l'accueil ou dans le menu d'une map. |
| retrouver une info | la recherche en haut de l'écran (ou `/`), puis quelques mots. Accents et majuscules ignorés. Filtres : `map:ascent`, `type:retake`, `side:def`, `#tag`, `"expression exacte"`. ↑ ↓ puis Entrée. |
| préparer un match | épinglez les protocoles clés, puis **Fiche match** (ou `F`) : une page condensée de la map, imprimable, à garder sous les yeux pendant un temps mort. |
| partager avec l'équipe | **Copier** (ou `C`) sur une note, une map ou la fiche match, puis collez dans Discord ou dans un document. |
| illustrer un lineup | collez une capture (`Ctrl+V`) ou glissez une image dans l'éditeur. |
| changer de map | la barre latérale, ou `[` et `]`. Les maps du pool actif sont regroupées en tête. |

L'éditeur guide la mise en forme : une barre d'outils (titre, gras, listes, cases à cocher, tableau, encadré, lien, image), un menu **Insérer** de blocs prêts (tableau des rôles, timings, ordre des utilitaires, checklist, calls, encadrés) et un aide-mémoire. L'aperçu se met à jour pendant la frappe, l'enregistrement est automatique. `Ctrl+S` force l'enregistrement, `Ctrl+Entrée` ou `Échap` referme l'édition. Une note supprimée part dans la corbeille, et le message qui le confirme propose d'annuler.

![Une map : présentation, filtres par type et par côté, notes](docs/map.png)

![L'éditeur guidé et son aperçu](docs/edition.png)

## Copier, coller, importer

**Copier** met deux versions dans le presse-papiers, d'un seul clic :

- du **Markdown pour Discord** : titres, gras et listes passent tels quels ; ce que Discord n'affiche pas est converti (un tableau devient un bloc de code aux colonnes alignées, une case à cocher ☐ ou ☑, un encadré une citation titrée) ;
- du **texte mis en forme** pour Word, Google Docs ou Notion, images comprises.

Le bouton `[ v ]` à côté choisit un seul format : Discord, Word/Docs/Notion, **texte simple** (sans aucune marque, pour un SMS ou un mail) ou **fichier Markdown complet** (en-tête compris, pour un autre carnet ou Obsidian). Le menu `[ Plus v ]` d'une note la télécharge en `.md` ; celui d'une map télécharge la map entière.

![Le choix du format de copie](docs/copier.png)

**Coller** un protocole existant suffit à créer une note : `Ctrl+V` n'importe où sur une page du carnet, ou dans le champ *Contenu* d'une nouvelle note. La mise en forme de Word, Google Docs, Notion, Excel ou d'une page web est convertie en Markdown (titres, gras, italique, listes, y compris les listes numérotées de Word, tableaux, liens), et le premier titre du texte devient celui de la note. Dans l'éditeur, un collage riche est converti de la même façon ; `Ctrl+Maj+V` colle le texte brut.

**Importer** des fichiers `.md` (Obsidian, un autre carnet, un export) : déposez-les sur une map, ou passez par son menu. L'en-tête du fichier est conservé, clés inconnues comprises, et une note existante n'est jamais écrasée.

**Exporter tout le carnet** en fichiers Markdown : *Aide › Votre carnet › Exporter*, ou `protocol export --format zip -o carnet.zip`. L'archive ZIP s'ouvre partout (Explorateur Windows, macOS, Obsidian) : une map = un dossier, une note = un fichier `.md`, les images dans `assets/`.

![Un texte collé depuis Google Docs, converti en Markdown](docs/coller.png)

## Le carnet

Tout tient dans `protocol.carnet` : maps et présentations, notes, images collées, corbeille, réglages. Chaque note y reste du Markdown précédé d'un petit en-tête, le format qu'Obsidian, VS Code ou GitHub lisent :

```markdown
---
title: Retake B à trois
type: retake
side: def
tags: [b, retake]
pin: true
updated: 2026-10-12T14:03:09Z
---

## Conditions
- 3 joueurs en vie, au moins une fumée
```

Les clés d'en-tête inconnues (ajoutées par Obsidian, par exemple) sont conservées. Liens entre notes : `[[retake-b]]`, ou `[[bind/setup-a]]` vers une autre map.

Le fichier est un journal : chaque enregistrement s'ajoute à la fin, avec une somme de contrôle, et rien n'est réécrit en place. Une coupure de courant ou une clé USB arrachée en pleine écriture fait perdre au plus la toute dernière modification, jamais le reste. Les versions périmées sont régulièrement éliminées en arrière-plan : le fichier est réécrit à côté puis mis en place d'un seul geste, en gardant l'ancien jusqu'au dernier moment. Si une partie du fichier devient illisible (support abîmé), elle est écartée au lancement et une copie du fichier d'origine est gardée.

Si une même note est modifiée dans deux fenêtres en même temps, le carnet ne l'écrase pas : il signale le conflit et vous laisse choisir la version à garder.

## Avec d'autres outils

**API JSON** locale, sur `http://127.0.0.1:7373` :

| Requête | Effet |
|---|---|
| `GET /api/state` | maps, notes épinglées et récentes |
| `GET /api/maps/ascent` | une map, sa présentation et ses notes |
| `GET /api/notes/ascent/retake-b` | une note (Markdown et HTML rendu) |
| `PUT /api/notes/ascent/retake-b` | modifier : JSON (`title`, `kind`, `side`, `tags`, `pin`, `body`, `baseVersion`) ou le texte Markdown complet, en-tête compris (`Content-Type: text/markdown`) |
| `POST /api/notes/ascent` | créer : `{"title": "…", "kind": "retake", "body": "…"}`, ou importer un fichier Markdown complet (`Content-Type: text/markdown`, `?name=` pour le nom du fichier) |
| `DELETE /api/notes/ascent/retake-b` | mettre à la corbeille |
| `GET /api/search?q=retake+b` | rechercher |
| `GET /api/share/ascent/retake-b` | une note prête à partager : `discord`, `text`, `markdown`, `html` |
| `GET /api/share/ascent?scope=pinned` | la fiche match (sans `scope` : toute la map), mêmes formats |
| `GET /api/export.json` | tout le carnet en un seul JSON |
| `GET /api/export.zip` | tout le carnet en fichiers Markdown (depuis le poste du coach) |
| `GET /api/export/ascent.md` | une map en un seul Markdown |
| `POST /api/settings` | réglages gardés dans le carnet : `{"theme": "dark", "lang": "en"}` (`lang` : `fr`, `en` ou `auto`) |

Exemple en PowerShell :

```powershell
Invoke-RestMethod "http://127.0.0.1:7373/api/search?q=retake" | Select-Object -ExpandProperty hits
```

Une application web locale (un tableau de bord, par exemple) peut appeler l'API depuis le navigateur si on l'autorise : `protocol --cors http://localhost:5173`.

**Ligne de commande** :

```text
protocol serve [--lan]            serveur seul (pas de fenêtre, pas d'arrêt automatique)
protocol search retake b          recherche dans le terminal
protocol export -o carnet.json    tout le carnet en JSON
protocol export --format zip -o carnet.zip
                                  tout le carnet en fichiers Markdown (Obsidian, sauvegarde lisible)
protocol export --map ascent -o ascent.md
protocol export --map ascent --pinned --format discord
                                  la fiche match, prête à coller sur Discord (--format texte : sans marques)
protocol mcp [--write]            serveur MCP pour un assistant IA (voir plus bas)
```

Options communes : `--carnet <fichier>` (ou la variable `PROTOCOL_CARNET`), `--port <n>`, `--read-only`, `--browser app|default|none`. La langue des messages suit celle du carnet ; `PROTOCOL_LANG=fr|en` la force. La recherche et les exports en ligne de commande lisent le carnet sans le verrouiller : la fenêtre peut rester ouverte.

**Bootcamp, LAN** : `protocol serve --lan` partage le carnet en lecture seule aux joueurs du réseau local. Au lancement s'affiche un lien d'invitation (`http://192.168.…:7373/?t=…`) : sans lui, un autre poste n'accède à rien, ce qui compte sur un réseau de tournoi partagé avec d'autres équipes. Seul le poste du coach modifie, et la corbeille lui reste réservée.

## Assistant IA (MCP)

`protocol mcp` fait du carnet un serveur [MCP](https://modelcontextprotocol.io) (spécification 2025-11-25, et les versions 2025-06-18, 2025-03-26 et 2024-11-05) pour Claude Desktop, Claude Code ou tout autre client : l'assistant installé sur le poste lance le programme et lui parle par l'entrée et la sortie standard. protocol ne contacte aucune IA lui-même, n'ouvre aucun port et ne demande aucune clé. Le serveur parle la langue du carnet (descriptions des outils, consignes prêtes, messages d'erreur) ; `PROTOCOL_LANG=fr|en` dans la configuration du client la fixe pour un carnet resté en automatique.

La configuration exacte du poste (chemins réels, lecture seule ou écriture) est dans **Aide › Assistant IA (MCP)**, prête à copier. À la main, avec la clé USB en `E:` :

```json
{ "mcpServers": { "protocol": { "command": "E:/protocol.exe", "args": ["mcp", "--carnet", "E:/protocol.carnet"] } } }
```

```text
claude mcp add --scope user protocol -- E:/protocol.exe mcp --carnet E:/protocol.carnet
```

`--write` à la fin des arguments autorise l'écriture.

| Ce que l'assistant trouve | |
|---|---|
| **Outils de lecture** | `list_maps`, `get_map`, `list_notes` (par map, type, côté, étiquette, épinglage ; paginé), `read_note`, `search_notes` (les filtres de la recherche), `match_sheet`, `list_tags`, `recent_notes`, `format_note` (Discord, texte, Markdown), `list_trash` |
| **Outils d'écriture** (`--write`) | `create_note`, `update_note`, `append_to_note`, `set_pinned`, `move_note`, `trash_note`, `restore_from_trash`, `create_map`, `update_map` |
| **Ressources** | `protocol://note/{map}/{id}`, `protocol://map/{map}`, `protocol://sheet/{map}` (fiche match), `protocol://search/{requête}`, `protocol://guide`, `protocol://types` ; liste paginée et abonnements (une note suivie qui change, dans la fenêtre ou ailleurs, est signalée) |
| **Consignes prêtes** | Résumer un protocole, Trouver les failles, Proposer une variante, Préparer un match, Débriefer un match, Ranger un texte dans le carnet : chacune joint au message les notes concernées |
| **Et aussi** | complétion des arguments (maps, notes), journal (`logging/setLevel`), sorties structurées conformes à leur schéma, annotations (lecture seule, destructif, idempotent) |

Ce qui protège le carnet, par construction :

- **Aucun port** : le transport stdio réserve l'accès au client qui a lancé le serveur, comme le recommande la spécification pour un serveur local.
- **Lecture seule par défaut** : sans `--write`, les outils d'écriture n'existent pas, et un appel est refusé.
- **Rien d'irréversible** : `trash_note` met à la corbeille. `update_note` et `update_map` exigent la version lue (rien n'est écrasé si la note a changé entre-temps) et laissent la version d'avant à la corbeille, d'où « Restaurer » la remet à côté. Aucun outil ne supprime définitivement ni n'agit en masse, et la corbeille ne se vide que dans la fenêtre.
- **Écritures rythmées** (30 par minute) et **entrées bornées** (8 Mo par message, 1 Mo par note, 500 caractères par recherche).
- **Le texte des notes est de la donnée** : les consignes données à l'assistant le disent, pour qu'un texte collé piégé ne soit pas pris pour un ordre.
- **Aucun verrou gardé** : un assistant peut tourner des heures sans bloquer la fenêtre. Le carnet est suivi en lecture ; une écriture passe par la fenêtre si elle est ouverte (son API locale, sur 127.0.0.1, après avoir vérifié qu'elle sert bien ce carnet), sinon le carnet est ouvert le temps de l'opération, puis relâché.

Vérifié avec les deux clients officiels : `tests/mcp/conformance.py` (SDK Python) et `tests/mcp/conformance-ts.mjs` (SDK TypeScript, celui de Claude Desktop et de Claude Code) valident chaque message et chaque sortie d'outil contre les schémas du protocole.

## Sécurité

Le carnet est un serveur local ; il est construit pour qu'aucun site web ouvert dans le navigateur ne puisse lire ou modifier les notes :

- l'en-tête `Host` est vérifié (pas de « DNS rebinding ») ;
- une modification n'est acceptée que depuis ce poste, depuis l'interface elle-même (ou une origine autorisée par `--cors`), et avec un type de contenu qu'un formulaire tiers ne peut pas envoyer ;
- en mode `--lan`, les autres postes doivent présenter le jeton du lien d'invitation (gardé ensuite dans un cookie `HttpOnly`), et ne font que lire ;
- le Markdown est rendu sans HTML actif : une balise `<script>` s'affiche comme du texte, les liens `javascript:` sont neutralisés ;
- la page impose une politique de sécurité de contenu stricte (aucun script extérieur ni en ligne) ;
- le serveur ne lit aucun fichier du poste : notes et images viennent du carnet, et seules les images sont servies ;
- les requêtes sont bornées en taille et en durée, et le nombre de connexions est limité ;
- un seul programme écrit dans un carnet (fichier verrouillé), chaque écriture est vérifiée par une somme de contrôle et ajoutée à la fin du fichier : une coupure ne laisse jamais une note à moitié écrite.

## Performances

Mesuré sur un carnet de 520 notes (2,1 Mo de Markdown), Windows 11, binaire de production :

| Mesure | Résultat |
|---|---|
| taille du programme | 2,0 Mo, interface, polices, deux langues et serveur MCP compris |
| taille du carnet | 2,2 Mo |
| lancement, lecture du carnet et recherche (`protocol search`) | 65 ms |
| recherche plein texte | 1 à 5 ms |
| affichage d'une note, d'une map | 1 ms |
| export complet : JSON (2,3 Mo) / fichiers Markdown (ZIP) | 9 ms / 17 ms |
| mémoire | 17 Mo |

## Pourquoi Rust

Le cahier des charges : léger, portable, compatible avec d'autres outils, optimisé. Parmi les experts de langage disponibles (Ballerina, COBOL, C/C++, Crystal, C#, Elixir, Gleam, Hare, Java, JavaScript, Julia, Koka, Mojo, MoonBit, Nim, Odin, Python, Rust, Zig), **Rust** est le seul à cocher toutes les cases à la fois :

| Langage | Un seul exécutable, sans runtime | Taille | Sûreté mémoire | Outils sur ce poste | Écosystème pour ce besoin |
|---|---|---|---|---|---|
| **Rust** | oui | ~1 Mo | oui | oui (1.94, MSVC) | mûr (rendu Markdown, tests) |
| Zig | oui | < 1 Mo | partielle | portable, hors PATH | jeune, bibliothèque standard instable (0.16) |
| C/C++ | oui | < 1 Mo | non | aucun compilateur dans le PATH | tout à faire à la main |
| Nim, Odin, Crystal | oui | ~1 Mo | variable | non installés ; Crystal encore fragile sous Windows | restreint |
| C# (.NET 10 AOT) | oui | 10 à 30 Mo | oui | oui | mûr, mais lourd pour ce besoin |
| JavaScript (Node) | non (ou 80 Mo empaqueté) | — | oui | oui | mûr |
| Python | non (ou 15 Mo empaqueté) | — | oui | oui | mûr, démarrage lent |
| Java, Elixir, Gleam, Julia, Ballerina | non (JVM, BEAM…) | lourd | oui | — | hors sujet |
| Hare, Koka, MoonBit, Mojo, COBOL | — | — | — | pas de Windows natif, ou langage de recherche | hors sujet |

L'interface, elle, est en HTML, CSS et JavaScript sans framework : le navigateur est déjà là, inutile d'embarquer Electron (plus de 100 Mo).

Le code a ensuite été relu par deux agents experts, chacun dans son langage, chaque constat reproduit avant d'être rapporté :

- **Rust**, avec le guide de programmation sécurisée de l'ANSSI : le modèle de sécurité tient (47 charges XSS sans effet, traversées de chemin, CSRF et rebinding bloqués). Une douzaine de défauts réels ont été corrigés : ETag réutilisé d'un lancement à l'autre, panique possible dans la corbeille, compteur de connexions qui fuyait, écrasements possibles sous verrou Windows, partage LAN sans authentification…
- **JavaScript** : aucune injection, CSP respectée. Le cycle de vie de l'éditeur a été repris : un éditeur quitté n'écrit plus jamais, une image collée ne peut plus atterrir dans une autre note, une vue lente ne recouvre plus la suivante, les brouillons ne donnent plus de fausses alertes.

La refonte de l'interface et le copier-coller ont eu leur propre relecture, dans les mêmes conditions :

- **JavaScript** : aucune faille d'échappement, CSP respectée. Une vingtaine de défauts réels corrigés : Échap dans l'aide-mémoire qui fermait l'éditeur, Entrée sur « Importer » qui soumettait le formulaire, le changement de thème qui vidait une nouvelle note (elle est désormais gardée en brouillon), le focus déplacé par un rafraîchissement, des propriétés de Word mal lues au collage, des images téléchargées à chaque affichage (elles ne le sont plus qu'au moment de copier).
- **Rust** : aucune panique ni faille à l'import (760 000 documents générés au hasard). Corrigés : un bloc de code non refermé qui avalait la note suivante dans l'export d'une map, un temps de traitement quadratique sur une ligne piégée qui pouvait geler le carnet (désormais linéaire, et la mise en forme se fait hors du verrou), une ligne `---` avalée après un tableau.

Chaque correctif est couvert par un test : unitaire, de bout en bout, ou scénario dans un navigateur piloté.

## Construire depuis les sources

```powershell
cargo build --release      # target\release\protocol.exe
cargo test                 # 108 tests : unitaires, de bout en bout (vrai serveur HTTP) et ligne de commande
```

Le code est portable : `cargo build --release` sur Linux ou macOS produit l'équivalent. Une seule dépendance, `pulldown-cmark` pour le Markdown. Le serveur HTTP, le JSON, le format du carnet et l'archive ZIP sont écrits à la main pour rester légers et lisibles.

| Fichier | Rôle |
|---|---|
| `src/main.rs` | ligne de commande, lancement, reprise d'un ancien dossier `notes/` |
| `src/platform.rs` | la fenêtre (navigateur au profil temporaire, dans un job Windows), l'explorateur |
| `src/api.rs` | routes de l'API et règles de sécurité |
| `src/store.rs` | le carnet : maps, notes, images, corbeille, réglages, compactage en arrière-plan |
| `src/carnet.rs` | le fichier du carnet : journal, sommes de contrôle, reprise après coupure, verrou |
| `src/zip.rs` | l'export en fichiers Markdown (archive ZIP) |
| `src/note.rs` | l'en-tête YAML des notes et les modèles par type |
| `src/i18n.rs` | la langue retenue (réglage du carnet, `PROTOCOL_LANG`, langue du système) |
| `src/render.rs` | Markdown vers HTML assaini |
| `src/share.rs` | une note mise en forme pour Discord ou en texte brut |
| `src/search.rs` | recherche et extraits |
| `src/http.rs`, `src/json.rs` | serveur HTTP et JSON minimaux |
| `src/mcp.rs`, `src/mcp/` | serveur MCP : transport et cycle de vie, outils, ressources, consignes prêtes, écritures sans verrou (`link.rs`) |
| `web/` | l'interface, embarquée dans l'exécutable à la compilation (`app.js` les vues, `clip.js` le presse-papiers, `i18n.js` le dictionnaire anglais) |
| `tests/ui/` | 6 scénarios navigateur (Puppeteer) : parcours, copier-coller, contrastes, interface en anglais (`ui-i18n.cjs`) ; plus le contrôle statique des traductions (`i18n-check.cjs`). Mode d'emploi : [`tests/ui/LISEZMOI.md`](tests/ui/LISEZMOI.md) |
| `tests/mcp/` | conformité du serveur MCP, vérifiée par les SDK officiels Python et TypeScript |

## Crédits

Direction artistique d'après [ascii.rest](https://ascii.rest) (bas3line, licence MIT) : ses pièces « ui » (cadres, contrôles de formulaire, séparateurs, arborescence, journal de démarrage) inspirent l'interface, ses modules `banner` et `mount` dessinent le titre de l'accueil ; la mini-carte animée suit le format de ses pièces. Police [Paper Mono](https://github.com/paper-design/paper-mono) (OFL), traits et blocs tirés de JetBrains Mono (OFL), rendu Markdown par [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark) (MIT). Textes complets dans [`web/LICENSES.txt`](web/LICENSES.txt), aussi servis par l'application sur `/static/licenses.txt`.

## Licence et mentions légales

protocol est distribué sous [licence MIT](LICENSE).

protocol isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games, and all associated properties are trademarks or registered trademarks of Riot Games, Inc.

*protocol n'est ni approuvé ni soutenu par Riot Games et ne reflète pas les opinions de Riot Games ni de quiconque participe officiellement à la production ou à la gestion de ses propriétés. Riot Games, Valorant et toutes les propriétés associées sont des marques commerciales ou des marques déposées de Riot Games, Inc.*
