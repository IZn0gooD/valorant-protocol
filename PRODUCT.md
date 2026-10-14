# Product

## Register

product

## Users

Le coach d'une équipe Valorant, sur PC. Deux moments d'usage :

- **Préparation et review**, au calme sur grand écran : il écrit, relit et réorganise les protocoles de l'équipe map par map (setups de défense, exécutions, retakes, lineups, anti-strats, règles de communication), souvent à partir de textes déjà rédigés ailleurs (Discord, Word, Google Docs).
- **Pendant un match**, au temps mort (environ une minute) : il doit retrouver et lire le bon protocole en quelques secondes, d'un coup d'œil.

Les joueurs reçoivent les protocoles partagés par le coach, surtout par copier-coller dans Discord ; ils ne sont pas la cible principale de l'interface.

## Product Purpose

Un carnet local et portable (un seul exécutable) qui range les protocoles de l'équipe par map, en fichiers Markdown lisibles partout (Obsidian, VS Code, Git). Il réussit quand le coach trouve, écrit et partage un protocole sans penser à l'outil, et quand, en temps mort, le protocole utile est à l'écran en moins de cinq secondes.

## Brand Personality

Concentré, net, tactique. L'identité vient d'ascii.rest, et d'abord de ses pièces « ui » : une seule police à chasse fixe (Paper Mono, de paper.design), des cadres titrés en traits fins, des séparateurs, une arborescence pour naviguer, des contrôles écrits comme dans un terminal (`[ bouton ]`, `[x] case`, `(*) choix`, `[ champ  v ]`), l'inversion vidéo au survol, des messages façon journal de démarrage (`[ OK ]`). Les lettres-blocs et la mini-carte radar signent l'accueil, et seulement lui. Palette sombre ou claire, un éclat orange pour l'action et la sélection.

La grammaire est stricte, c'est elle qui rend l'interface évidente : ce qui est entre crochets se clique, `[x]` bascule, `(*)` choisit, un cadre regroupe, un séparateur ouvre une section.

## Anti-references

- La première version de protocol, jugée « trop éthérée » : texte gris pâle, crochets employés pour des liens comme pour des actions, minuscules partout, actions cachées dans les coins, vignettes de Markdown brut au lieu de résumés lisibles.
- La deuxième version, solide mais générique : police sans-serif, boutons arrondis, badges ; elle avait perdu l'esprit ascii.rest. Les noms de maps en lettres-blocs, eux, se lisaient mal.
- Le décor qui passe avant le contenu : un grand visuel animé qui repousse les maps et les protocoles sous la ligne de flottaison.
- Les outils administratifs froids : formulaires sans hiérarchie, jargon technique (« front matter », « slug ») visible par l'utilisateur.
- L'esthétique « gamer » surchargée : néons, dégradés, textures, angles agressifs.

## Design Principles

1. **Le contenu d'abord.** Maps, protocoles et actions sont visibles sans défiler ; le décor ASCII structure (cadres, séparateurs, arborescence) au lieu de décorer.
2. **Des affordances évidentes.** Ce qui se clique est entre crochets et s'inverse au survol, ce qui s'édite est un champ `[ … ]`. Aucune action essentielle n'est réservée au clavier : les raccourcis accélèrent, ils ne remplacent pas.
3. **La vitesse au temps mort.** Recherche toujours visible, notes épinglées et fiche de match à un clic, lecture d'un coup d'œil grâce aux titres forts et aux repères de type et de côté.
4. **Portable et sans trace.** Un exécutable et un seul fichier de carnet, à côté l'un de l'autre ; rien n'est écrit ailleurs sur le poste (la fenêtre a son profil de navigateur temporaire). Le contenu reste du Markdown propre : il s'exporte d'un clic en fichiers `.md` (Obsidian) et un `.md` se réimporte en le déposant. L'éditeur guide (barre d'outils, blocs prêts, aperçu) sans enfermer.
5. **Partager sans friction.** Un clic copie un protocole prêt pour Discord ou Word ; coller un texte existant suffit à créer une note.

## Accessibility & Inclusion

WCAG 2.2 niveau AA :

- **Contraste :** au moins 4,5:1 pour le texte et 3:1 pour les grands textes et les composants.
- **Clavier :** navigation complète, avec un focus toujours visible.
- **Mouvement :** `prefers-reduced-motion` est respecté, le radar et les reflets se figent.
- **Thèmes :** clair et sombre.
- **Couleur :** l'information n'est jamais portée par la seule couleur ; attaque et défense sont écrits en toutes lettres en plus de leur teinte.
