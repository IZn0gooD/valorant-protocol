// Thème posé avant le premier affichage (pas de flash). Le serveur l'inscrit dans
// la page quand il est choisi (il est gardé dans le carnet) ; sinon, celui du système.
(function () {
  var r = document.documentElement;
  if (r.dataset.theme === "light" || r.dataset.theme === "dark") return;
  r.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
})();
