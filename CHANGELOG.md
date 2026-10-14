# Journal des changements

Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/), les versions
suivent [la gestion sémantique de version](https://semver.org/lang/fr/).

## [1.0.0] — 2026-10-14

Première version publique. Interface, modèles de notes, messages, exports et serveur MCP en français et en anglais (langue du système, réglable dans l'aide).

### Le carnet
- Un seul exécutable et un seul fichier de carnet (`protocol.carnet`) à côté de lui : rien n'est écrit ailleurs sur le poste.
- Notes rangées map par map (les 13 maps de compétition, dont Summit, plus « général » pour l'équipe), en Markdown, avec type, côté, étiquettes et épinglage.
- Fiche match (les notes épinglées d'une map), recherche instantanée avec filtres, corbeille.
- Copier pour Discord, Word, Google Docs ou Notion ; coller un protocole existant pour créer une note ; import et export en fichiers Markdown (Obsidian).
- Journal à sommes de contrôle : une coupure ne laisse jamais une note à moitié écrite ; compactage en arrière-plan.
- Partage en lecture seule sur le réseau local (bootcamp, LAN), par lien d'invitation.

### Assistant IA (MCP)
- `protocol mcp` : serveur MCP (spécification 2025-11-25) pour Claude Desktop, Claude Code ou tout client : 19 outils, ressources suivies, 6 consignes prêtes, complétion.
- Lecture seule par défaut ; avec `--write`, rien d'irréversible (corbeille, version d'avant gardée) et écriture possible même fenêtre ouverte.
- Paquet MCPB pour une installation en un double-clic dans Claude Desktop.
