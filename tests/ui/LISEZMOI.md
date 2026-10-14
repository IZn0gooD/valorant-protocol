# Scénarios navigateur

Six scripts Puppeteer qui pilotent l'interface dans Chrome headless, contre un carnet de test, et un contrôle statique des traductions.

```powershell
# 1. Un carnet jetable et un serveur sans fenêtre, en français (les cinq premiers scénarios
#    lisent l'interface en français ; sans la variable, c'est la langue du système)
$env:PROTOCOL_LANG = "fr"
protocol serve --port 7399 --carnet $env:TEMP\test.carnet

# 2. Les scénarios (PUPPETEER : chemin d'un module puppeteer installé, sinon `npm i puppeteer`).
#    Tout passe par l'API : les scénarios ne lisent jamais le fichier du carnet.
$env:PUPPETEER = "C:\chemin\vers\node_modules\puppeteer"
node ui-check.cjs      # remplit le carnet, captures d'écran dans ./shots, erreurs console et réseau
node ui-flows.cjs      # création, épinglage, suppression + annulation, conflits, thème, raccourcis
node ui-regress.cjs    # non-régressions de la revue JavaScript (réseau ralenti à la demande)
node ui-share.cjs      # copier (presse-papiers relu), coller depuis Word / Google Docs, import et dépôt de .md,
                       # téléchargement, menus au clavier, tiroir de navigation mobile
node ui-contrast.cjs   # contrastes WCAG AA mesurés sur les couleurs réelles, thèmes clair et sombre
node ui-i18n.cjs       # le carnet passé en anglais (API) : chaque vue, textes clés anglais, aucun reste de
                       # français hors contenu des notes, 0 erreur console ; le sélecteur de l'aide le
                       # repasse en français

# 3. Les traductions, sans navigateur ni serveur
node i18n-check.cjs    # chaque t("…") / tp(n, "…", "…") de web/ et chaque data-i18n d'index.html a son
                       # anglais dans web/i18n.js ; aucune entrée inutilisée ; mêmes variables {nom}
```

`ui-check.cjs` doit passer en premier : les autres scénarios s'appuient sur les notes qu'il crée.

## Traductions

L'interface est en français (langue de référence) et en anglais. Chaque texte passe par `t("phrase française", { variable })` ou, pour un nombre, `tp(n, "{n} note", "{n} notes")` ; la phrase française est la clé du dictionnaire anglais de `web/i18n.js` (une entrée de pluriel y est `[singulier, pluriel]`). Une phrase absente reste en français. Les textes fixes d'`index.html` portent `data-i18n` (le texte de l'élément) ou `data-i18n-attr="aria-label placeholder"` (ces attributs). Après tout ajout de texte : `node i18n-check.cjs`.
