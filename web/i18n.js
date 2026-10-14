// protocol : les langues de l'interface. Le français est la langue de référence ;
// l'anglais se lit dans le dictionnaire ci-dessous, dont chaque clé est la phrase
// française exacte. Une phrase absente du dictionnaire reste en français : jamais de
// texte vide. Le serveur inscrit la langue retenue sur <html lang> ; il traduit lui-même
// ce qui vient de lui (types de notes, côtés, erreurs de l'API, modèles, exports).
//
// Chargé par un <script> classique avant les modules : il expose `I18N` (t, tp, ago,
// fmtSize) et traduit les textes statiques d'index.html marqués `data-i18n` (le texte
// de l'élément) ou `data-i18n-attr="aria-label placeholder"` (ces attributs).
(function () {
  "use strict";
  const lang = document.documentElement.lang === "en" ? "en" : "fr";
  const locale = lang === "en" ? "en-US" : "fr-FR";

  // Une entrée de pluriel (tp) est [singulier, pluriel] ; les autres, un texte.
  const EN = {
    // ── Socle : réseau, nombres, dates ──
    "le carnet ne répond pas": "the notebook is not responding",
    "erreur {status}": "error {status}",
    "{n} Mo": "{n} MB",
    "{n} Ko": "{n} KB",
    "à l'instant": "just now",
    "il y a {n} min": "{n} min ago",
    "il y a {n} h": "{n} h ago",
    hier: "yesterday",
    "il y a {n} j": "{n} days ago",
    "le {date}": "on {date}",

    // ── Page statique (index.html) ──
    "Aller au contenu": "Skip to content",
    "Rechercher dans les notes": "Search notes",
    "Chargement…": "Loading…",
    "Le carnet est fermé : relancez protocol pour continuer.": "The notebook is closed: restart protocol to continue.",
    "Carnet de coach Valorant : protocoles, setups, exécutions et lineups, map par map.":
      "Valorant coaching notebook: protocols, setups, executes and lineups, map by map.",
    "Maps et navigation": "Maps and navigation",
    "protocol, accueil": "protocol, home",
    "rechercher une note, un tag…": "search a note, a tag…",
    "Navigation du carnet": "Notebook navigation",

    // ── Repères d'une note ──
    Attaque: "Attack",
    Défense: "Defense",
    Tous: "All",
    "Les deux": "Both",
    épinglée: "pinned",
    Épinglée: "Pinned",
    "Note vide.": "Empty note.",
    "modifiée {ago}": "edited {ago}",
    "mise à jour {ago}": "updated {ago}",
    "supprimée {ago}": "deleted {ago}",
    "{n} note": ["{n} note", "{n} notes"],
    "{n} map": ["{n} map", "{n} maps"],
    "{n} épinglée": ["{n} pinned", "{n} pinned"],
    "{n} note épinglée": ["{n} pinned note", "{n} pinned notes"],
    "{n} fichier": ["{n} file", "{n} files"],

    // ── Barre, navigation ──
    Accueil: "Home",
    "Fil d'Ariane": "Breadcrumb",
    "Passer en thème clair": "Switch to light theme",
    "Passer en thème sombre": "Switch to dark theme",
    "Les modifications se font sur le poste du coach": "Changes are made on the coach's computer",
    "lecture seule": "read-only",
    "Nouvelle note": "New note",
    "Nouvelle note (touche N)": "New note (N key)",
    '<span class="label">thème </span>{mode}': '{mode}<span class="label"> theme</span>',
    clair: "light",
    sombre: "dark",
    "Fiche match": "Match sheet",
    "pool actif": "active pool",
    "hors pool": "out of pool",
    maps: "maps",
    "Ajouter une map": "Add a map",
    Corbeille: "Bin",
    Aide: "Help",
    Quitter: "Quit",

    // ── Copier ──
    "Pour Discord": "For Discord",
    "Markdown, tableaux alignés en bloc de code": "Markdown, tables aligned in a code block",
    "Pour Word, Docs ou Notion": "For Word, Docs or Notion",
    "Titres, listes et tableaux mis en forme": "Formatted headings, lists and tables",
    "Texte simple": "Plain text",
    "Sans aucune mise en forme": "No formatting at all",
    "Fichier Markdown complet": "Full Markdown file",
    "En-tête compris : pour un autre carnet ou Obsidian": "Front matter included: for another notebook or Obsidian",
    Copier: "Copy",
    "Copier pour Discord, Word ou Docs (touche C)": "Copy for Discord, Word or Docs (C key)",
    "Choisir le format de copie": "Choose the copy format",
    "Copier pour…": "Copy for…",
    "Copié : prêt pour Discord, Word ou Docs.": "Copied: ready for Discord, Word or Docs.",
    "Copié pour Discord.": "Copied for Discord.",
    "Copié avec la mise en forme.": "Copied with formatting.",
    "Texte copié.": "Text copied.",
    "Markdown copié, en-tête compris.": "Markdown copied, front matter included.",
    "Copie impossible : {error}": "Copy failed: {error}",
    "Plus de 2 000 caractères : Discord l'enverra en fichier.": "Over 2,000 characters: Discord will send it as a file.",
    "Copier à la main": "Copy manually",
    "Le navigateur refuse la copie automatique ici. Le texte est sélectionné : faites <kbd>Ctrl</kbd> + <kbd>C</kbd>.":
      "The browser blocks automatic copying here. The text is selected: press <kbd>Ctrl</kbd> + <kbd>C</kbd>.",
    "Texte à copier": "Text to copy",
    Fermer: "Close",
    copie: "copy",
    "ℹ Note": "ℹ Note",
    "✔ Astuce": "✔ Tip",
    "❗ Important": "❗ Important",
    "⚠ Attention": "⚠ Warning",
    "⛔ Danger": "⛔ Caution",

    // ── Accueil ──
    Carnet: "Notebook",
    Compris: "Got it",
    "Les protocoles de l'équipe, rangés map par map : setups, exécutions, retakes, lineups, anti-strats.":
      "The team's protocols, filed map by map: setups, executes, retakes, lineups, anti-strats.",
    "Coller un protocole": "Paste a protocol",
    "/ rechercher": "/ search",
    "n nouvelle note · ctrl+v coller un texte · / rechercher": "n new note · ctrl+v paste text · / search",
    Épinglées: "Pinned",
    "Épinglez les protocoles clés : ils s'affichent ici et dans la fiche match de leur map, prêts pour un temps mort.":
      "Pin the key protocols: they show up here and on their map's match sheet, ready for a timeout.",
    Maps: "Maps",
    Équipe: "Team",
    "Modifiées récemment": "Recently edited",
    "Mini-carte : un radar balaie une map, les contacts apparaissent à son passage":
      "Minimap: a radar sweeps a map, contacts appear as it passes",

    // ── Map ──
    "Aucune note sur {map} pour l'instant": "No notes on {map} yet",
    "Partez d'un modèle (setup, exécution, retake…) ou reprenez un texte existant : copiez-le depuis Discord, Word ou Google Docs, puis faites <kbd>Ctrl</kbd> + <kbd>V</kbd> sur cette page.":
      "Start from a template (setup, execute, retake…) or reuse existing text: copy it from Discord, Word or Google Docs, then press <kbd>Ctrl</kbd> + <kbd>V</kbd> on this page.",
    "Écrire un protocole": "Write a protocol",
    "Coller un texte": "Paste text",
    "Importer des fichiers .md": "Import .md files",
    Présentation: "Overview",
    Modifier: "Edit",
    "Modifier la présentation": "Edit the overview",
    "Pas encore de présentation : défauts, callouts, plan de jeu, compositions.": "No overview yet: defaults, callouts, game plan, comps.",
    "L'écrire": "Write it",
    Tout: "All",
    "Une note à partir d'un texte copié": "A note from copied text",
    "Obsidian, un autre carnet… (ou glissez-les ici)": "Obsidian, another notebook… (or drop them here)",
    "Télécharger la map (.md)": "Download the map (.md)",
    "Présentation et notes en un seul fichier": "Overview and notes in a single file",
    "Retirer du pool": "Remove from pool",
    "Mettre dans le pool": "Add to pool",
    "Les maps du pool passent en tête de la liste": "Pool maps go to the top of the list",
    "Fiche match (touche F)": "Match sheet (F key)",
    "Copier la map": "Copy the map",
    "Modifier la présentation (touche E)": "Edit the overview (E key)",
    "Plus d'actions": "More actions",
    Plus: "More",
    Notes: "Notes",
    Type: "Type",
    Côté: "Side",
    "Aucune note pour ce filtre.": "No notes for this filter.",
    "Tout afficher": "Show all",

    // ── Note ──
    "Télécharger (.md)": "Download (.md)",
    "La note en fichier Markdown, en-tête compris": "The note as a Markdown file, front matter included",
    Supprimer: "Delete",
    "Vers la corbeille, annulable": "To the bin, can be undone",
    "Modifier (touche E)": "Edit (E key)",
    "Les notes épinglées vont sur l'accueil et dans la fiche match": "Pinned notes go on the home page and the match sheet",
    "Notes voisines": "Adjacent notes",
    précédente: "previous",
    suivante: "next",

    // ── Nouvelle note, nouvelle map ──
    "Modèle prêt à remplir : {sections}.": "Ready-to-fill template: {sections}.",
    "Note libre, sans modèle.": "Free-form note, no template.",
    Titre: "Title",
    "Ex. Retake B à trois, exécution rapide A…": "E.g. 3-man B retake, fast A execute…",
    "Facultatif si vous collez un texte : son premier titre sera repris.": "Optional if you paste text: its first heading will be used.",
    Map: "Map",
    Contenu: "Content",
    "Collez ici un protocole existant : Discord, Word, Google Docs, Notion…": "Paste an existing protocol here: Discord, Word, Google Docs, Notion…",
    "Laissez vide pour partir du modèle du type.": "Leave empty to start from the type's template.",
    "Facultatif. Titres, listes, gras et tableaux copiés depuis Word ou Docs sont convertis ; vous pouvez aussi déposer des fichiers .md sur cette page.":
      "Optional. Headings, lists, bold and tables copied from Word or Docs are converted; you can also drop .md files on this page.",
    "Créer la note": "Create note",
    "tab champ suivant · entrée créer": "tab next field · enter create",
    "Brouillon repris": "Draft restored",
    "Le texte saisi {ago} est revenu.": "The text you typed {ago} is back.",
    Effacer: "Clear",
    "Le texte collé remplace le modèle du type.": "The pasted text replaces the type's template.",
    "Nouvelle map": "New map",
    Nom: "Name",
    "Ex. District": "E.g. District",
    "Une map est un dossier du carnet : elle apparaît dans la liste dès sa création.":
      "A map is a folder of the notebook: it shows up in the list as soon as it is created.",
    "Créer la map": "Create map",
    "entrée créer": "enter create",
    "Donnez un titre à la note, ou collez un texte qui en a un.": "Give the note a title, or paste text that has one.",
    "Note créée à partir du texte collé.": "Note created from the pasted text.",

    // ── Éditeur : blocs prêts, barre d'outils ──
    "Tableau des rôles": "Roles table",
    "Joueur, agent, rôle, utilitaire": "Player, agent, role, utility",
    Joueur: "Player",
    Agent: "Agent",
    Rôle: "Role",
    Utilitaire: "Utility",
    Timings: "Timings",
    "Départ, exécution, post-plant": "Start, execute, post-plant",
    Timing: "Timing",
    "Départ :": "Start:",
    "Exécution à :": "Execute at:",
    "Post-plant :": "Post-plant:",
    "Ordre des utilitaires": "Utility order",
    "Des étapes numérotées": "Numbered steps",
    Checklist: "Checklist",
    "Des cases à cocher": "Checkboxes",
    Calls: "Calls",
    "Ce que l'IGL annonce": "What the IGL calls",
    "Encadré « attention »": "“Warning” callout",
    "Un point à ne pas rater": "A point not to miss",
    "Encadré « astuce »": "“Tip” callout",
    "Un conseil": "A tip",
    Séparateur: "Divider",
    "Une ligne entre deux parties": "A line between two parts",
    "Modèle du type": "Type template",
    "Les sections de départ du type choisi": "The starting sections of the chosen type",
    "Mise en forme": "Formatting",
    "Titre de section": "Section heading",
    G: "B",
    "Gras (Ctrl+B)": "Bold (Ctrl+B)",
    Gras: "Bold",
    I: "I",
    "Italique (Ctrl+I)": "Italic (Ctrl+I)",
    Italique: "Italic",
    Liste: "List",
    "Liste à puces": "Bulleted list",
    Étapes: "Steps",
    "Étapes (liste numérotée)": "Steps (numbered list)",
    Cases: "Checks",
    "Cases à cocher": "Checkboxes",
    Tableau: "Table",
    Encadré: "Callout",
    Lien: "Link",
    "Lien vers une autre note": "Link to another note",
    Image: "Image",
    "Image (ou Ctrl+V d'une capture)": "Image (or Ctrl+V a screenshot)",
    "Insérer un bloc prêt": "Insert a ready-made block",
    Insérer: "Insert",
    "Aide-mémoire Markdown": "Markdown cheat sheet",
    "ctrl+b gras · ctrl+i italique · ctrl+entrée terminer": "ctrl+b bold · ctrl+i italic · ctrl+enter done",
    "Insérer un bloc": "Insert a block",
    "Aide-mémoire": "Cheat sheet",
    Rôles: "Roles",
    nuance: "nuance",
    "fumée heaven": "heaven smoke",
    "Étapes numérotées": "Numbered steps",
    "Case à cocher": "Checkbox",
    "Lien vers une note": "Link to a note",
    important: "important",
    "id-de-la-note": "note-id",

    // ── Éditeur : enregistrement ──
    "Enregistrement automatique. Collez une capture (<kbd>Ctrl</kbd> + <kbd>V</kbd>) pour ajouter une image ; un texte copié depuis Word ou Docs garde sa mise en forme. <kbd>Ctrl</kbd> + <kbd>Entrée</kbd> pour terminer.":
      "Saved automatically. Paste a screenshot (<kbd>Ctrl</kbd> + <kbd>V</kbd>) to add an image; text copied from Word or Docs keeps its formatting. <kbd>Ctrl</kbd> + <kbd>Enter</kbd> when done.",
    Enregistré: "Saved",
    "Terminer (Ctrl+Entrée)": "Done (Ctrl+Enter)",
    Terminer: "Done",
    Aperçu: "Preview",
    "Afficher ou masquer l'aperçu": "Show or hide the preview",
    "Modifier : {title}": "Edit: {title}",
    "Titre de la note": "Note title",
    Propriétés: "Properties",
    Tags: "Tags",
    "retake, b, utilitaire": "retake, b, utility",
    "tags séparés par des virgules": "comma-separated tags",
    "Contenu (Markdown)": "Content (Markdown)",
    "Présentation : {map}": "Overview: {map}",
    "Nom de la map": "Map name",
    "Dans le pool": "In the pool",
    "La présentation s'affiche en tête de la map et de sa fiche match : défauts, callouts, plan de jeu, compositions.":
      "The overview is shown at the top of the map and its match sheet: defaults, callouts, game plan, comps.",
    "Supprimer la map": "Delete the map",
    "Présentation (Markdown)": "Overview (Markdown)",
    Brouillon: "Draft",
    "Un brouillon non enregistré existe ({ago}).": "An unsaved draft exists ({ago}).",
    "Le reprendre": "Restore it",
    "L'oublier": "Discard it",
    "Modifications…": "Editing…",
    "Un titre est nécessaire": "A title is required",
    "Enregistrement…": "Saving…",
    "Non enregistré : {error}": "Not saved: {error}",
    Conflit: "Conflict",
    "Cette note a été modifiée ailleurs (autre éditeur, synchronisation) depuis son ouverture.":
      "This note was changed elsewhere (another editor, a sync) since you opened it.",
    "Cette présentation a été modifiée ailleurs (autre éditeur, synchronisation) depuis son ouverture.":
      "This overview was changed elsewhere (another editor, a sync) since you opened it.",
    "Garder ma version": "Keep my version",
    "Prendre la version du disque": "Take the version on disk",
    "Version du disque reprise": "Version on disk restored",
    "Rechargée : modifiée ailleurs": "Reloaded: changed elsewhere",
    "Image trop lourde (15 Mo au plus)": "Image too large (15 MB max)",
    "Envoi de l'image…": "Uploading image…",
    capture: "screenshot",
    "Image envoyée après la fermeture de l'éditeur, à coller dans la note : {md}": "Image uploaded after the editor closed, paste it into the note: {md}",
    "Image refusée : {error}": "Image rejected: {error}",
    "Non enregistré : l'éditeur reste ouvert": "Not saved: the editor stays open",
    "Modifications non enregistrées : un brouillon est gardé sur ce poste.": "Unsaved changes: a draft is kept on this computer.",

    // ── Fiche match ──
    "Fiche match : {map}": "Match sheet: {map}",
    "Fiche match {map}": "Match sheet {map}",
    "imprimable sur une page recto verso": "prints on one double-sided page",
    Imprimer: "Print",
    "Copier la fiche": "Copy the sheet",
    "Retour à la map": "Back to the map",
    "Aucune note épinglée sur {map}. Épinglez les protocoles à garder sous les yeux pendant un temps mort : ils s'afficheront ici en entier.":
      "No pinned notes on {map}. Pin the protocols you want in front of you during a timeout: they will show up here in full.",
    "Autres notes": "Other notes",

    // ── Recherche ──
    'Tapez un mot, ou un filtre : <code>map:ascent</code> <code>type:retake</code> <code>side:def</code> <code>#tag</code> <code>"expression exacte"</code>':
      'Type a word, or a filter: <code>map:ascent</code> <code>type:retake</code> <code>side:def</code> <code>#tag</code> <code>"exact phrase"</code>',
    "{n} résultat pour « {q} »": ["{n} result for “{q}”", "{n} results for “{q}”"],
    "(les {n} meilleurs)": "(top {n})",
    "<kbd>↑</kbd> <kbd>↓</kbd> puis <kbd>Entrée</kbd> pour ouvrir.": "<kbd>↑</kbd> <kbd>↓</kbd> then <kbd>Enter</kbd> to open.",
    "Rien pour « {q} ». Un autre mot ? Un filtre en trop ?": "Nothing for “{q}”. Another word? One filter too many?",
    Recherche: "Search",
    Résultats: "Results",

    // ── Corbeille, import ──
    "Les notes supprimées attendent ici, dans le dossier <code>.trash</code> du carnet. Rien ne s'efface tout seul.":
      "Deleted notes wait here, in the notebook's <code>.trash</code> folder. Nothing is erased on its own.",
    Restaurer: "Restore",
    "La corbeille est vide.": "The bin is empty.",
    "« {title} » est dans la corbeille.": "“{title}” is in the bin.",
    Annuler: "Undo",
    "Seuls les fichiers .md (ou .txt) s'importent ici.": "Only .md (or .txt) files can be imported here.",
    "trop lourd": "too large",
    "{n} note importée dans {map}.": ["{n} note imported into {map}.", "{n} notes imported into {map}."],
    "Aucune note importée.": "No notes imported.",
    "Refusé : {files}.": "Rejected: {files}.",
    "Déposez vos fichiers .md pour les importer dans {map}": "Drop your .md files to import them into {map}",

    // ── Actions ──
    "Configuration copiée.": "Configuration copied.",
    "Le carnet est fermé. À bientôt !": "The notebook is closed. See you soon!",
    "Note désépinglée": "Note unpinned",
    "Note épinglée": "Note pinned",
    "Supprimer cette map vide ?": "Delete this empty map?",
    "Oui, supprimer": "Yes, delete",
    Non: "No",
    "Map supprimée. Sa présentation est dans la corbeille.": "Map deleted. Its overview is in the bin.",
    "Fermer le message": "Close the message",

    // ── Aide ──
    "Aide et raccourcis": "Help and shortcuts",
    "Raccourcis clavier": "Keyboard shortcuts",
    "Rechercher, puis ↑ ↓ et Entrée": "Search, then ↑ ↓ and Enter",
    "Modifier la note ou la présentation affichée": "Edit the note or overview on screen",
    "Fiche match de la map en cours": "Match sheet of the current map",
    "Copier la note, la map ou la fiche affichée": "Copy the note, map or sheet on screen",
    "Map précédente, map suivante": "Previous map, next map",
    "Sur une page : crée une note à partir du texte copié": "On a page: creates a note from the copied text",
    "Enregistrer (c'est aussi automatique)": "Save (it's automatic too)",
    Entrée: "Enter",
    "Terminer l'édition": "Finish editing",
    Échap: "Esc",
    "Revenir en arrière": "Go back",
    "Cette page": "This page",
    Auto: "Auto",
    Interface: "Interface",
    "Copier, coller, importer": "Copy, paste, import",
    "<strong>Copier</strong> une note, une map ou une fiche match met deux versions dans le presse-papiers : Discord reçoit du Markdown (les tableaux deviennent un bloc aligné), Word, Google Docs et Notion reçoivent le texte mis en forme. Le bouton <code>[&nbsp;v&nbsp;]</code> à côté propose aussi le texte simple et le fichier Markdown complet.":
      "<strong>Copying</strong> a note, a map or a match sheet puts two versions on the clipboard: Discord gets Markdown (tables become an aligned block), Word, Google Docs and Notion get formatted text. The <code>[&nbsp;v&nbsp;]</code> button next to it also offers plain text and the full Markdown file.",
    "<strong>Coller</strong> un protocole existant suffit à créer une note : <kbd>Ctrl</kbd> + <kbd>V</kbd> sur une page du carnet, ou dans le champ Contenu d'une nouvelle note. La mise en forme de Word ou Docs est convertie, et le premier titre devient celui de la note.":
      "<strong>Pasting</strong> an existing protocol is enough to create a note: <kbd>Ctrl</kbd> + <kbd>V</kbd> on a notebook page, or in the Content field of a new note. Word or Docs formatting is converted, and the first heading becomes the note's title.",
    "<strong>Importer</strong> des fichiers <code>.md</code> (Obsidian, un autre carnet) : déposez-les sur une map, ou passez par le menu <code>[&nbsp;Plus&nbsp;v&nbsp;]</code> de la map. Leur en-tête est conservé ; une note existante n'est jamais écrasée.":
      "<strong>Import</strong> <code>.md</code> files (Obsidian, another notebook): drop them on a map, or use the map's <code>[&nbsp;More&nbsp;v&nbsp;]</code> menu. Their front matter is kept; an existing note is never overwritten.",
    'Tous les mots doivent apparaître, accents et majuscules ignorés. Filtres : <code>map:ascent</code>, <code>type:retake</code>, <code>side:def</code>, <code>#tag</code>, <code>"expression exacte"</code>.':
      'Every word must appear, ignoring accents and case. Filters: <code>map:ascent</code>, <code>type:retake</code>, <code>side:def</code>, <code>#tag</code>, <code>"exact phrase"</code>.',
    "Votre carnet": "Your notebook",
    "Tout le carnet tient dans un seul fichier, <code>protocol.carnet</code>, rangé à côté du programme : maps, notes, images collées, corbeille et réglages. Pour l'emporter ou le sauvegarder, copiez-le avec <code>protocol.exe</code> (clé USB, dossier partagé, OneDrive).":
      "The whole notebook fits in a single file, <code>protocol.carnet</code>, stored next to the program: maps, notes, pasted images, bin and settings. To take it with you or back it up, copy it along with <code>protocol.exe</code> (USB drive, shared folder, OneDrive).",
    "Rien n'est écrit ailleurs sur le poste : la fenêtre utilise un profil de navigateur temporaire, rangé à côté du carnet et effacé à la fermeture.":
      "Nothing is written anywhere else on the computer: the window uses a temporary browser profile, stored next to the notebook and erased on close.",
    "Pour travailler avec Obsidian, VS Code ou un autre outil, exportez le carnet en fichiers Markdown (une map = un dossier, une note = un fichier <code>.md</code>) ; dans l'autre sens, déposez des fichiers <code>.md</code> sur une map.":
      "To work with Obsidian, VS Code or another tool, export the notebook as Markdown files (one map = one folder, one note = one <code>.md</code> file); the other way round, drop <code>.md</code> files on a map.",
    "Carnet : {path} ({size})": "Notebook: {path} ({size})",
    "Afficher le carnet": "Show the notebook",
    "Exporter en fichiers Markdown (.zip)": "Export as Markdown files (.zip)",
    "Avec d'autres outils": "With other tools",
    "Tout passe par une petite API JSON locale :": "Everything goes through a small local JSON API:",
    "maps, notes épinglées, récentes": "maps, pinned notes, recent",
    "une map et ses notes": "a map and its notes",
    "JSON, ou Markdown brut (text/markdown)": "JSON, or raw Markdown (text/markdown)",
    'créer : {"title": "…", "kind": "retake"} ou un fichier .md': 'create: {"title": "…", "kind": "retake"} or a .md file',
    "la note pour Discord, en texte, en HTML": "the note for Discord, as text, as HTML",
    "tout le carnet": "the whole notebook",
    "En ligne de commande : <code>protocol search retake b</code>, <code>protocol export --map ascent --format discord</code>, <code>protocol serve --lan</code> pour partager le carnet en lecture seule sur le réseau local.":
      "From the command line: <code>protocol search retake b</code>, <code>protocol export --map ascent --format discord</code>, <code>protocol serve --lan</code> to share the notebook read-only on the local network.",
    "C:/chemin/protocol.exe": "C:/path/protocol.exe",
    "Assistant IA (MCP)": "AI assistant (MCP)",
    "Claude Desktop, Claude Code ou tout client MCP peuvent travailler avec ce carnet : chercher, lire une note, sortir la fiche match, et lancer des consignes prêtes (résumer un protocole, en chercher les failles, préparer ou débriefer un match, ranger un texte collé). protocol ne contacte aucune IA : c'est l'assistant installé sur ce poste qui lance <code>protocol mcp</code> et lui parle directement, sans réseau ni port ouvert.":
      "Claude Desktop, Claude Code or any MCP client can work with this notebook: search, read a note, pull the match sheet, and run ready-made prompts (summarize a protocol, find its weak spots, prepare or debrief a match, tidy up pasted text). protocol contacts no AI: the assistant installed on this computer launches <code>protocol mcp</code> and talks to it directly, with no network and no open port.",
    "<strong>Lecture seule</strong> par défaut. <strong>Avec écriture</strong>, l'assistant peut aussi créer, compléter, épingler, déplacer ou retirer des notes, même pendant que cette fenêtre est ouverte. Rien n'est irréversible : une note retirée va à la corbeille, une note réécrite y laisse sa version d'avant, et la corbeille ne se vide qu'ici.":
      "<strong>Read-only</strong> by default. <strong>With write access</strong>, the assistant can also create, extend, pin, move or remove notes, even while this window is open. Nothing is irreversible: a removed note goes to the bin, a rewritten note leaves its previous version there, and the bin is only emptied here.",
    Accès: "Access",
    "Lecture seule": "Read-only",
    "Avec écriture": "With write access",
    "Claude Desktop : ajoutez ceci à son fichier de configuration (<code>claude_desktop_config.json</code>, par Settings › Developer › Edit Config), puis redémarrez-le.":
      "Claude Desktop: add this to its configuration file (<code>claude_desktop_config.json</code>, via Settings › Developer › Edit Config), then restart it.",
    "Copier pour Claude Desktop": "Copy for Claude Desktop",
    "Claude Code : lancez cette commande une fois.": "Claude Code: run this command once.",
    "Copier pour Claude Code": "Copy for Claude Code",
    "Sur une clé USB, la lettre du lecteur change parfois d'un poste à l'autre : revenez alors copier la configuration.":
      "On a USB drive, the drive letter sometimes changes from one computer to another: come back and copy the configuration again.",
    Crédits: "Credits",
    "Direction artistique d'après <a href=\"https://ascii.rest\">ascii.rest</a> (bas3line, licence MIT) : ses pièces « ui » (cadres, contrôles de formulaire, séparateurs, arborescence) inspirent l'interface, ses modules <code>banner</code> et <code>mount</code> dessinent le titre et la mini-carte. Police <a href=\"https://github.com/paper-design/paper-mono\">Paper Mono</a> de paper.design (OFL). Traits et blocs : JetBrains Mono (OFL). Markdown : pulldown-cmark (MIT). <a href=\"/static/licenses.txt\">Licences complètes</a>.":
      "Art direction after <a href=\"https://ascii.rest\">ascii.rest</a> (bas3line, MIT license): its “ui” pieces (frames, form controls, dividers, tree) inspire the interface, its <code>banner</code> and <code>mount</code> modules draw the title and the minimap. <a href=\"https://github.com/paper-design/paper-mono\">Paper Mono</a> typeface by paper.design (OFL). Lines and blocks: JetBrains Mono (OFL). Markdown: pulldown-cmark (MIT). <a href=\"/static/licenses.txt\">Full licenses</a>.",
    "protocol est un logiciel libre (licence MIT). Il n'est ni approuvé ni soutenu par Riot Games et ne reflète pas les opinions de Riot Games ni de quiconque participe officiellement à la production ou à la gestion de ses propriétés. Riot Games, Valorant et les propriétés associées sont des marques commerciales ou déposées de Riot Games, Inc.":
      "protocol is free software (MIT license). protocol isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games, Valorant and all associated properties are trademarks or registered trademarks of Riot Games, Inc.",

    // ── Pages d'exception ──
    "Ce carnet est ouvert en lecture seule ici : les modifications se font sur le poste du coach.":
      "This notebook is open read-only here: changes are made on the coach's computer.",
    Introuvable: "Not found",
    "404 · introuvable": "404 · not found",
    "cette page n'existe pas.": "this page doesn't exist.",
    "Elle a peut-être été renommée, déplacée ou mise à la corbeille.": "It may have been renamed, moved or sent to the bin.",
    "Retour à l'accueil": "Back to home",
    Erreur: "Error",
  };

  /** `{nom}` remplacé par `vars.nom` ; une variable inconnue reste telle quelle. */
  const fill = (s, vars) => (vars ? s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : s);

  /** Le texte dans la langue de la page ; `fr` est la clé, et le repli. */
  function t(fr, vars) {
    let s = fr;
    if (lang === "en") {
      const e = EN[fr];
      if (typeof e === "string" && e) s = e;
      else if (Array.isArray(e) && e[0]) s = e[0];
    }
    return fill(s, vars);
  }

  const num = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });

  /**
   * Un nombre et son nom accordé : `tp(3, "{n} note", "{n} notes")`. En français, 0 et 1
   * prennent le singulier ; en anglais, seul 1 le prend. `{n}` est le nombre mis en forme.
   */
  function tp(n, one, many, vars) {
    const v = { ...vars, n: num.format(n) };
    if (lang === "en") {
      const e = EN[one];
      if (Array.isArray(e) && e[0] && e[1]) return fill(n === 1 ? e[0] : e[1], v);
    }
    return fill(n > 1 ? many : one, v);
  }

  /** 1 536 → « 2 Ko » ; 1,5 Mo → « 1,5 Mo » (« 1.5 MB » en anglais). */
  const fmtSize = (n) =>
    n >= 1 << 20 ? t("{n} Mo", { n: num.format(n / (1 << 20)) }) : t("{n} Ko", { n: num.format(Math.max(1, Math.round(n / 1024))) });

  const fmtDay = new Intl.DateTimeFormat(locale, { day: "numeric", month: "short" });
  const fmtYear = new Intl.DateTimeFormat(locale, { day: "numeric", month: "short", year: "numeric" });

  /** « il y a 5 min », « hier », « le 3 oct. » : en minuscules, pour se glisser dans une phrase. */
  function ago(iso) {
    const at = typeof iso === "number" ? iso : Date.parse(iso);
    if (!Number.isFinite(at)) return "";
    const s = (Date.now() - at) / 1000;
    if (s < 60) return t("à l'instant");
    if (s < 3600) return t("il y a {n} min", { n: Math.floor(s / 60) });
    if (s < 86400) return t("il y a {n} h", { n: Math.floor(s / 3600) });
    if (s < 172800) return t("hier");
    if (s < 7 * 86400) return t("il y a {n} j", { n: Math.floor(s / 86400) });
    const d = new Date(at);
    return t("le {date}", { date: (d.getFullYear() === new Date().getFullYear() ? fmtDay : fmtYear).format(d) });
  }

  /** Les textes statiques de la page : `data-i18n` (le texte), `data-i18n-attr` (des attributs). */
  function apply(root = document) {
    for (const el of root.querySelectorAll("[data-i18n]")) el.textContent = t(el.textContent.trim());
    for (const el of root.querySelectorAll("[data-i18n-attr]")) {
      for (const a of el.getAttribute("data-i18n-attr").split(/\s+/)) {
        const v = el.getAttribute(a);
        if (v != null) el.setAttribute(a, t(v));
      }
    }
  }

  globalThis.I18N = Object.freeze({ lang, locale, dict: EN, t, tp, ago, fmtSize, apply });

  // En français, la page est déjà écrite dans sa langue.
  if (lang !== "fr") {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => apply());
    else apply();
  }
})();
