// Une mini-carte balayée par un radar, au format des pièces ascii.rest :
// `meta`, plus une fonction qui renvoie le dessin de l'instant `t` (secondes).
// Map imaginaire à trois voies ; les contacts n'apparaissent qu'au passage du
// faisceau, puis s'effacent jusqu'au tour suivant.

export const meta = {
  name: "minimap",
  category: "scenes",
  note: "une map à trois voies balayée par le radar",
  cols: 64,
  rows: 27,
  fps: 15,
  still: 2.6,
  ground: "#0b0e14",
  palette: [
    "#151a21", // 0 mur épais
    "#1f252e", // 1 bord de mur
    "#343c48", // 2 mur éclairé par le faisceau
    "#363d48", // 3 sol de site
    "#4a5361", // 4 traînée
    "#b4bcc7", // 5 faisceau
    "#7d8796", // 6 texte
    "#e8ebef", // 7 lettres de site
    "#ff4655", // 8 attaque
    "#2dd4bf", // 9 défense
  ],
};

const W = meta.cols, H = meta.rows;
const TAU = Math.PI * 2;
const TURN = 5; // secondes par tour de faisceau
const FADE = 2.2; // secondes pour qu'un contact passe au tiers de son éclat

// Le sol : des rectangles [x0, y0, x1, y1] creusés dans la masse.
const FLOOR = [
  [27, 2, 36, 4], // spawn défense
  [27, 21, 36, 24], // spawn attaque
  [3, 6, 17, 12], // site A
  [46, 6, 60, 12], // site B
  [30, 7, 33, 22], // mid
  [31, 4, 32, 7], // défense → mid
  [17, 9, 30, 10], // mid → A
  [33, 9, 46, 10], // mid → B
  [7, 12, 10, 20], // A main
  [7, 19, 28, 21], // A lobby
  [53, 12, 56, 20], // B main
  [35, 19, 56, 21], // B lobby
  [14, 3, 27, 4], // rotation défense → A
  [14, 3, 15, 6],
  [36, 3, 49, 4], // rotation défense → B
  [48, 3, 49, 6],
];
const SITES = [[3, 6, 17, 12], [46, 6, 60, 12]];
const LABELS = [[10, 9, "A", 7], [53, 9, "B", 7], [30, 3, "DEF", 9], [30, 23, "ATK", 8], [30, 15, "MID", 6]];
// Trajets aller-retour : [équipe (8 attaque, 9 défense), vitesse en cases/s, décalage, points…]
const TRACKS = [
  [8, 1.7, 0, [28, 23], [28, 20], [8, 20], [8, 12], [9, 8]],
  [8, 1.5, 9, [31, 23], [31, 10], [24, 10], [14, 9]],
  [8, 1.6, 4, [35, 23], [35, 20], [55, 20], [55, 12], [54, 8]],
  [8, 1.4, 14, [32, 22], [32, 10], [40, 10], [50, 9]],
  [9, 1.2, 2, [29, 3], [15, 3], [15, 6], [11, 8], [8, 11]],
  [9, 1.1, 7, [34, 3], [48, 3], [48, 6], [52, 8], [55, 11]],
  [9, 0.9, 5, [31, 4], [31, 7], [31, 12], [31, 17]],
];

const CX = 31.5, CY = 13.5;
const bearing = (x, y) => (Math.atan2(x - CX, (CY - y) * 2) + TAU) % TAU;
const inRect = ([x0, y0, x1, y1], x, y) => x >= x0 && x <= x1 && y >= y0 && y <= y1;

export default function minimap() {
  const floor = new Uint8Array(W * H);
  for (let y = 1; y < H - 1; y++)
    for (let x = 0; x < W; x++)
      if (FLOOR.some((r) => inRect(r, x, y))) floor[y * W + x] = SITES.some((r) => inRect(r, x, y)) ? 2 : 1;
  const isFloor = (x, y) => x >= 0 && x < W && y >= 0 && y < H && floor[y * W + x] > 0;

  // Les murs : des blocs pleins et sombres sur deux cases autour du sol ; les
  // pièces se lisent en creux, comme sur une mini-carte.
  const near = (x, y, d) => {
    for (let dy = -d; dy <= d; dy++) for (let dx = -d; dx <= d; dx++) if (isFloor(x + dx, y + dy)) return true;
    return false;
  };
  const back = new Array(W * H).fill(" ");
  const ink = new Uint8Array(W * H);
  const cells = []; // [index, relèvement] des cases de sol, pour le faisceau
  const walls = [];
  for (let y = 1; y < H - 1; y++)
    for (let x = 0; x < W; x++) {
      const i = y * W + x;
      if (floor[i]) {
        cells.push([i, bearing(x, y)]);
        if (floor[i] === 2 && (x + 2 * y) % 4 === 0) {
          back[i] = "·";
          ink[i] = 3;
        }
      } else if (near(x, y, 2)) {
        back[i] = "█";
        ink[i] = near(x, y, 1) ? 1 : 0;
        walls.push([i, bearing(x, y), ink[i]]);
      }
    }
  for (const [x, y, text, c] of LABELS)
    for (let k = 0; k < text.length; k++) {
      back[y * W + x + k] = text[k];
      ink[y * W + x + k] = c;
    }
  const labelled = new Set(LABELS.flatMap(([x, y, text]) => [...text].map((_, k) => y * W + x + k)));

  // Chaque trajet : ses segments et sa longueur, parcourus en aller-retour.
  const tracks = TRACKS.map(([team, speed, offset, ...pts]) => {
    const segs = [];
    let len = 0;
    for (let k = 1; k < pts.length; k++) {
      const [x0, y0] = pts[k - 1], [x1, y1] = pts[k];
      const l = Math.hypot(x1 - x0, y1 - y0);
      segs.push([x0, y0, x1, y1, len, l]);
      len += l;
    }
    return { team, speed, offset, segs, len };
  });
  const where = (tr, t) => {
    let u = ((t * tr.speed + tr.offset) % (2 * tr.len) + 2 * tr.len) % (2 * tr.len);
    if (u > tr.len) u = 2 * tr.len - u;
    for (const [x0, y0, x1, y1, from, l] of tr.segs)
      if (u <= from + l) {
        const f = l ? (u - from) / l : 0;
        return [x0 + (x1 - x0) * f, y0 + (y1 - y0) * f];
      }
    const last = tr.segs[tr.segs.length - 1];
    return [last[2], last[3]];
  };

  const out = new Array(W * H);
  return (t, env = {}) => {
    const color = env.color;
    const sweep = ((t / TURN) * TAU) % TAU;
    const ago = (b) => ((((sweep - b) % TAU) + TAU) % TAU) / TAU * TURN;
    for (let i = 0; i < out.length; i++) out[i] = back[i];
    if (color) color.set(ink);
    // Le faisceau, puis sa traînée qui s'estompe.
    for (const [i, b] of cells) {
      if (labelled.has(i)) continue;
      const a = (ago(b) / TURN) * TAU;
      if (a < 0.9) {
        out[i] = a < 0.07 ? "▓" : a < 0.35 ? "▒" : "░";
        if (color) color[i] = a < 0.07 ? 5 : 4;
      }
    }
    if (color) for (const [i, b, k] of walls) if (ago(b) < 0.45) color[i] = k ? 2 : 1;
    // Un contact se montre là où il était quand le faisceau l'a touché.
    let seen = 0;
    for (const tr of tracks) {
      const [x0, y0] = where(tr, t);
      const s = ago(bearing(x0, y0));
      const [x, y] = where(tr, t - s);
      const i = Math.round(y) * W + Math.round(x);
      if (!floor[i] || labelled.has(i)) continue;
      const g = Math.exp(-s / FADE);
      if (g < 0.2) continue;
      seen++;
      out[i] = g > 0.7 ? "●" : g > 0.42 ? "•" : "·";
      if (color) color[i] = tr.team;
    }
    const deg = String(Math.round((sweep * 180) / Math.PI) % 360).padStart(3, "0");
    const round = String((Math.floor(t / 25) % 24) + 1).padStart(2, "0");
    // x < 0 : aligné à droite ; x === null : centré.
    const put = (row, text, x, c = 6) => {
      const x0 = x === null ? Math.floor((W - text.length) / 2) : x < 0 ? W - text.length + x + 1 : x;
      for (let k = 0; k < text.length; k++) {
        out[row * W + x0 + k] = text[k];
        if (color) color[row * W + x0 + k] = c;
      }
    };
    put(0, ` brg ${deg}°`, 0);
    put(0, `round ${round} `, -1);
    put(H - 1, " ● atk", 0, 8);
    put(H - 1, `contacts ${seen}`, null);
    put(H - 1, "def ● ", -1, 9);
    const lines = [];
    for (let r = 0; r < H; r++) lines.push(out.slice(r * W, (r + 1) * W).join(""));
    return lines.join("\n");
  };
}
