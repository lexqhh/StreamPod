# StreamPod — Direction artistique

Charte de l'identité visuelle StreamPod. Document **autoportant** : tout ce qui est nécessaire pour reproduire l'identité (couleurs, typographie, grille, motifs, mouvement) est écrit ici en valeurs exactes. Pas besoin d'ouvrir le CSS du site.

**Portée** : web (site vitrine), réseaux sociaux, interface de l'application Windows.
**Source de vérité** : ce document pour l'identité ; `styles.css` en est l'implémentation web de référence (les deux doivent rester alignés — toute évolution de l'un se répercute dans l'autre).

**Verrouillé** (ne se négocie pas) : le couple `#0a0a0a` / `#74d8a0`, les deux familles typographiques, le ratio du logo, la grille de croix 64 px, la courbe d'animation unique.
**Libre** : les compositions, les mises en page, le choix des formats, les illustrations produit (captures, schémas) tant qu'elles respectent la palette et le ton.

---

## 1. Positionnement & ton

StreamPod est un outil : léger, local, sans superflu. L'identité traduit cette promesse par une esthétique **de terminal** — fond quasi noir, texte monospace, arborescences de fichiers, points de statut, une seule couleur d'accent qui ne sert qu'à confirmer.

**Ton rédactionnel** : français, phrases courtes, factuel. On annonce ce que le produit fait et ce qu'il ne fait pas. Les promesses sont chiffrées (`100 % local`, `0 requête réseau`, `~10 Mo`) plutôt que qualifiées.

**Ce que l'identité n'est pas** :
- pas de néon gaming (violet/cyan saturés, glow épais, italiques agressives) ;
- pas de dégradés multicolores ni de mesh gradients — le système ne contient que 3 dégradés, tous quasi invisibles ;
- pas de skeuomorphisme, pas d'ombres portées dures, pas de bordures épaisses ;
- pas d'illustration 3D, de mascotte ni d'emoji dans les visuels de marque ;
- pas de superlatifs marketing (« révolutionnaire », « ultime »).

---

## 2. Logo

| Fichier | Dimensions | Usage |
|---|---|---|
| `assets/streampod-logo.png` | 431 × 512 px | logo principal (web, wordmark, `og:image`) |
| `assets/streampod-favicon.png` | 256 × 256 px | tous les usages carrés : favicon, avatar, icône |
| `assets/streampod-icon.png` | 256 × 256 px | **ancienne icône, ne pas utiliser** (conservée en référence) |

**Règles**

- **Ratio natif `431 / 512` (≈ 0,842) inviolable.** Toujours redimensionner par la hauteur, largeur `auto`.
- Hauteurs de référence web : **28 px** (marque principale), **21 px** (marque secondaire / footer). Hauteur minimale lisible : **16 px**.
- **Halo double**, subtil, toujours présent quand le fond est sombre :
  ```css
  filter: drop-shadow(0 0 3px rgba(255, 255, 255, 0.18))
          drop-shadow(0 0 7px rgba(116, 216, 160, 0.42));
  ```
  Un voile blanc très serré (3 px) + un voile vert un peu plus large (7 px). À l'échelle : les rayons se multiplient proportionnellement à la hauteur du logo (3 px et 7 px pour 28 px de haut ≈ 0,11 × h et 0,25 × h).
- **Zone de respiration** : au moins la moitié de la hauteur du logo, libre de tout élément, sur les quatre côtés.
- Le logo est **décoratif** : il accompagne toujours le nom en texte (`alt=""` en HTML), il ne porte jamais seul l'identification hors contexte carré (avatar, icône).

**Interdits** : recolorer, appliquer un dégradé, déformer, faire pivoter, ajouter un contour, une ombre portée dure ou un cadre, poser le logo sur une photo chargée, ou sur fond clair (aucune variante fond clair n'existe — voir § 15).

---

## 3. Nom & wordmark

- Orthographe : **`StreamPod`** — un seul mot, deux capitales, sans espace ni tiret. Jamais « Streampod », « StreamPOD », « Stream Pod ».
- Wordmark : **Space Grotesk 700**, `20 px`, `letter-spacing: -0.01em`. Variante secondaire (footer) : `15 px`.
- **Règle de l'accent** : le fragment « Pod » se colore en `#74d8a0` **uniquement** en position de marque principale (en-tête, avatar, visuel d'accroche). Partout ailleurs — footer, corps de texte, mentions légales, documentation — le wordmark est monochrome (`#ededed`).
- Jamais deux wordmarks accentués dans un même visuel.

```html
<!-- marque principale -->
<span class="brand-name">Stream<span class="brand-accent">Pod</span></span>
<!-- marque secondaire -->
<span class="brand-name">StreamPod</span>
```

---

## 4. Palette

Le couple identitaire est **fond `#0a0a0a` + accent `#74d8a0`**. Tout le reste est une échelle de gris strictement neutres.

### Accent

| Valeur | Rôle |
|---|---|
| `#74d8a0` | accent de marque, confirmations, micro-signaux. Contraste 11,4:1 sur `#0a0a0a`. |
| `rgba(116, 216, 160, 0.42)` | halo vert du logo |
| `rgba(116, 216, 160, 0.18)` | ligne de balayage du panneau archive |
| `rgba(190, 235, 210, 0.32)` | croix éclairées du halo curseur (vert désaturé) |

**Règle du vert** : réservé à l'accent de marque, aux états de succès, et aux micro-signaux (points de statut, curseur clignotant, numéro de carte au survol, barre d'accent 2 px). **Jamais** en fond de bloc, jamais en couleur de texte courant, jamais en fond de bouton.

### États — extension de palette (application uniquement)

Le web reste strictement vert + gris. **Dans l'application**, où un avertissement et une erreur doivent se distinguer d'un coup d'œil pendant une opération destructive, deux teintes d'état sont ajoutées. Elles sont dérivées du vert de marque : même saturation, même luminosité, seule la teinte tourne — `#74d8a0` est `hsl(150, 55 %, 65 %)`.

| Valeur | Rôle | Contraste sur `#0a0a0a` |
|---|---|---|
| `#d8a674` (h 35) | avertissement | ≈ 9,4:1 |
| `#d87474` (h 0) | erreur bloquante | ≈ 7,4:1 |

**Mêmes restrictions que le vert** : jamais en fond de bloc, jamais en fond de bouton, jamais en couleur de texte courant. La teinte ne porte que la marque de préfixe (`✕`, `!`) et la bordure gauche de 2 px du panneau, qui reste `#0d0d0d`. Ces deux valeurs n'existent pas hors interface applicative : aucun visuel de marque, aucun support social, aucune page web ne les emploie.

### Surfaces

| Valeur | Rôle |
|---|---|
| `#0a0a0a` | fond de page / fenêtre — la surface par défaut |
| `#0d0d0d` | panneau, carte survolée : un cran au-dessus du fond, presque imperceptible |
| `#1a1a1a` | pastille inline, champ de saisie |
| `rgba(255, 255, 255, 0.08)` | filet standard (séparateurs, bordures de grille) |
| `rgba(255, 255, 255, 0.10)` | filet renforcé (bordure de panneau), trait de la grille de croix |
| `rgba(255, 255, 255, 0.16)` | bordure de bouton fantôme au repos |
| `rgba(255, 255, 255, 0.40)` | bordure de bouton fantôme au survol |
| `rgba(255, 255, 255, 0.04)` | fond de bouton fantôme au survol, cœur du halo curseur |
| `rgba(0, 0, 0, 0.60)` | ombre de panneau (`0 20px 60px`) |

### Texte (contrastes calculés sur `#0a0a0a`)

| Valeur | Rôle | Contraste |
|---|---|---|
| `#ffffff` | état survolé des surfaces claires uniquement | 19,7:1 |
| `#ededed` | texte principal, titres | 16,9:1 |
| `#c9c9c9` | texte technique (nom de fichier, extension) | 12,0:1 |
| `#9a9a9a` | texte secondaire (chapeaux, réponses) | 7,0:1 |
| `#8a8a8a` | texte tertiaire (descriptions de cartes), liens de nav au repos | 6,4:1 |
| `#7a7a7a` | barre de statut | 4,6:1 |
| `#6f6f6f` | labels discrets, liens de footer, éléments exclus | 3,9:1 — **texte ≥ 24 px ou non essentiel** |
| `#5f5f5f` | mentions basses, métadonnées | 3,1:1 — **décoratif / non essentiel** |
| `#4f4f4f` | copyright | 2,4:1 — **jamais pour une information utile** |
| `#3a3a3a` | chiffres décoratifs de grande taille | — purement graphique |

À partir de `#6f6f6f`, la valeur ne porte plus d'information nécessaire à la compréhension. En interface applicative, ne descendez pas sous `#8a8a8a` pour un texte que l'utilisateur doit lire.

**Aucune autre teinte n'existe dans le système**, à la seule exception des deux valeurs d'état ci-dessus, réservées à l'application. Pas de bleu, pas de violet, aucune teinte saturée, nulle part.

---

## 5. Typographie

Deux familles, aucune autre.

| Famille | Rôle | Graisses chargées |
|---|---|---|
| **Space Grotesk** | titres, wordmark, chiffres décoratifs | 400, 500, 600, 700 |
| **JetBrains Mono** | tout le reste : corps de texte, labels, boutons, données, UI | 400, 500, 600 |

```css
--font-display: "Space Grotesk", sans-serif;
--font-mono: "JetBrains Mono", ui-monospace, monospace;
```

Chargement web (Google Fonts) :
```
https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap
```

Le monospace en corps de texte est un choix identitaire fort, pas un accident : il porte l'esthétique outil. Ne pas le remplacer par une sans-serif « plus lisible ».

### Échelle

| Rôle | Famille | Graisse | Taille | Interlignage | Interlettrage |
|---|---|---|---|---|---|
| Titre principal (H1) | Display | 700 | `clamp(42px, 6.5vw, 82px)` | 0.98 | −0.035em |
| Titre de conclusion | Display | 700 | `clamp(36px, 5.5vw, 56px)` | 1.0 | −0.035em |
| Titre de section (H2) | Display | 600 | `clamp(30px, 5vw, 46px)` | 1.04 | −0.03em |
| Wordmark | Display | 700 | 20 px (15 px secondaire) | — | −0.01em |
| Question / sous-titre (H3) | Display | 500 | 20 px | — | −0.01em |
| Titre de carte (H3) | Display | 600 | 18 px | — | — |
| Chiffre décoratif | Display | 500 | 34 px | — | — |
| Chapeau (lead) | Mono | 400 | 15 px | 1.7 | — |
| Réponse FAQ | Mono | 400 | 14 px | 1.75 | — |
| Bouton | Mono | 600 (500 fantôme) | 13 px | — | 0.05em + capitales |
| Description de carte | Mono | 400 | 13 px | 1.65 | — |
| Données / arborescence | Mono | 400 | 12,5 px | 2.05 | — |
| Label, lien de nav, méta | Mono | 400 | 12 px | — | 0.08em + capitales |
| Kicker de section | Mono | 400 | 12 px | — | 0.16em + capitales |
| Mention basse | Mono | 400 | 12 px | — | 0.06em + capitales |
| Barre de statut | Mono | 400 | 11,5 px | — | 0.08em + capitales |

### Règles

- **Titres** : interlettrage négatif serré (−0,03 à −0,035em), interlignage sous 1,05, ponctuation finale conservée (`Votre OBS complet, dans un seul fichier.`). Équilibrer les retours à la ligne (`text-wrap: balance`).
- **Labels et kickers** : capitales + interlettrage large. Plus le label est petit, plus il est espacé (11,5 px → 0,08em ; 12 px de kicker → 0,16em).
- **Corps de texte** : jamais plus de **52 ch** (colonne de hero) à **56 ch** (chapeau de section) de large. Interlignage généreux (1,65 à 1,75).
- Les titres de section sont bridés à **20 ch** pour forcer un retour à la ligne franc.
- Ne jamais mettre un titre Display en capitales, ni un label mono en bas de casse.

---

## 6. Grille & espacement

| Élément | Valeur |
|---|---|
| Largeur de contenu max | 1280 px |
| Gouttières latérales | 48 px (24 px sous 900 px) |
| Rythme de section | 104 px haut et bas (72 px sous 900 px) |
| Bloc de conclusion | 96 px haut et bas |
| Colonnes du hero | `1.15fr 0.85fr`, gouttière 64 px (1 colonne sous 900 px) |
| Grille de cartes | 3 colonnes (2 entre 640 et 900 px, 1 en dessous) |
| Colonnes FAQ | `0.9fr 1.1fr`, gouttière 48 px |
| Breakpoints | 900 px (principal), 640 px (grille de cartes) |

**Échelle d'espacement utilisée** : 8 / 10 / 11 / 12 / 14 / 16 / 18 / 20 / 22 / 24 / 26 / 30 / 34 / 36 / 38 / 48 / 64 / 72 / 96 / 104 px. Rester sur ces paliers.

**Rayons de bordure** : `3px` pastille inline · `4px` bouton · `8px` panneau · `50%` point de statut. Aucun autre rayon, jamais de rayon supérieur à 8 px.

**Ombre** : une seule dans tout le système — `0 20px 60px rgba(0, 0, 0, 0.6)`, réservée au panneau principal.

---

## 7. Motifs signature

Trois éléments portent l'identité à eux seuls. À reproduire en priorité sur tout nouveau support.

### 7.1 Grille de croix 64 px

Le fond n'est jamais un noir plat : il est parcouru d'une trame de petites croix. Cellule de 64 px, croix de 6 × 6 px centrée, trait de 1 px à 10 % de blanc.

```css
background-color: #0a0a0a;
background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='64' height='64'%3E%3Cpath d='M32 29v6M29 32h6' stroke='rgba(255,255,255,0.10)' stroke-width='1'/%3E%3C/svg%3E");
background-size: 64px 64px;
```

SVG lisible :
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
  <path d="M32 29v6M29 32h6" stroke="rgba(255,255,255,0.10)" stroke-width="1"/>
</svg>
```

**Mise à l'échelle** : 64 px est la valeur pour un affichage à 1× sur écran. Sur un support d'une autre densité ou d'un autre format, viser **15 à 22 cellules sur la plus grande dimension** et garder le trait à 1 px de rendu final. La trame doit rester à la limite du perceptible : si on « voit un quadrillage », c'est trop fort.

### 7.2 Halo curseur

Un disque de 480 px suit le pointeur et éclaire localement la trame : les croix y passent d'un blanc à 10 % à un vert désaturé à 32 %, sur un cœur blanc à 4 %.

```css
width: 480px; height: 480px;
background-image:
  radial-gradient(closest-side, rgba(255, 255, 255, 0.04), transparent),
  url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='64' height='64'%3E%3Cpath d='M32 29v6M29 32h6' stroke='rgba(190,235,210,0.32)' stroke-width='1'/%3E%3C/svg%3E");
background-size: auto, 64px 64px;
mask-image: radial-gradient(closest-side, #000, transparent);
```

Le halo est déplacé par `transform: translate3d(...)` et centré sur le pointeur ; sa position de fond est recalée modulo 64 px (scroll inclus) pour que les croix éclairées restent **alignées sur celles du fond**. Apparition en fondu de 500 ms au premier mouvement.

**Sur un support fixe** (visuel social, splash, capture) : reprendre le halo en **version statique**, posé derrière l'élément le plus important (wordmark ou titre), même géométrie et mêmes opacités.

### 7.3 Filets par gouttière (« faux borders »)

Les grilles ne portent pas de bordures : la grille a un fond `rgba(255,255,255,0.08)` et une gouttière de 1 px, les cellules ont le fond de la page. Les séparateurs sont donc parfaitement continus et d'une seule épaisseur.

```css
.grid { display: grid; gap: 1px; background: rgba(255,255,255,0.08);
        border: 1px solid rgba(255,255,255,0.08); }
.cell { background: #0a0a0a; }
```

### 7.4 Les seuls dégradés autorisés

1. `radial-gradient(closest-side, rgba(255,255,255,0.04), transparent)` — cœur du halo
2. `radial-gradient(closest-side, #000, transparent)` — masque du halo
3. `linear-gradient(90deg, transparent, rgba(116,216,160,0.18), transparent)` — ligne de balayage

Aucun autre dégradé ne doit apparaître dans un visuel StreamPod.

---

## 8. Composants de référence

### Bouton primaire (inversé)

Mono 600, 13 px, capitales, `letter-spacing: 0.05em` · texte `#0a0a0a` sur fond `#ededed` · rayon 4 px · padding `16px 24px` (26 px en bloc de conclusion) · icône 16 px à gauche, gouttière 10 px.
**Survol / focus** : fond `#ffffff`, `translateY(-2px)` — et l'icône descend de `translateY(2px)`. Le bouton monte, la flèche descend : mouvement en ciseaux, c'est la micro-interaction signature.

### Bouton fantôme

Mono 500, 13 px, capitales · texte `#ededed`, fond transparent · bordure `1px rgba(255,255,255,0.16)` · padding `16px 22px` · gouttière 9 px.
**Survol / focus** : bordure `rgba(255,255,255,0.4)`, fond `rgba(255,255,255,0.04)`. Aucun déplacement.

### Bouton compact (barre de navigation)

Mono 600, 12 px, capitales, `0.08em` · `#0a0a0a` sur `#ededed` · rayon 4 px · padding `11px 18px` · survol `#ffffff` + `translateY(-1px)`.

### Lien de navigation

Mono 12 px, capitales, `0.08em`, `#8a8a8a`. Soulignement de 1 px en `#ededed` à 5 px sous la ligne de base, révélé par `scaleX(0 → 1)` depuis la gauche ; la couleur passe à `#ededed`.

### Carte

Fond `#0a0a0a`, padding `34px 30px`, séparée par les gouttières de 1 px de la grille. Contient : un numéro à deux chiffres (Display 500, 34 px, `#3a3a3a`, marge basse 22 px), un titre (Display 600, 18 px), une description (Mono 13 px, `#8a8a8a`).
**Survol** : fond `#0d0d0d`, et le numéro passe en `#74d8a0`. C'est la seule récompense de survol de la carte — pas d'élévation, pas de bordure.

### Ligne de question (FAQ)

Deux colonnes `0.9fr 1.1fr`, gouttière 48 px, padding vertical 30 px, filet haut de 1 px. Au survol : une barre verticale de **2 px en `#74d8a0`** apparaît 18 px à gauche de la ligne, en `scaleY(0 → 1)` depuis le haut.

### Barre de statut

Filet haut de 1 px, padding `16px 48px`, Mono 11,5 px capitales `0.08em` en `#7a7a7a`. Chaque item est précédé d'un point de 6 px en `#74d8a0` qui pulse (`opacity: 1 → 0.35`, 3,6 s, `ease-in-out`, en boucle) avec des délais désynchronisés (`0`, `-1.2s`, `-2.4s`) pour que les points ne battent jamais ensemble. Mention de droite en `#5f5f5f`.

### Panneau de données (archive)

Bordure `1px rgba(255,255,255,0.10)`, rayon 8 px, fond `#0d0d0d`, ombre `0 20px 60px rgba(0,0,0,0.6)`, `overflow: hidden`, perspective du parent 900 px.
- **En-tête** : padding `13px 16px`, filet bas, 12 px — nom de fichier en `#c9c9c9` à gauche, taille en `#5f5f5f` à droite.
- **Corps** : padding `20px 18px`, Mono 12,5 px, interlignage 2.05. Arborescence en caractères de dessin (`│`, `├─`, `└─`) en `#5f5f5f`, libellés en `#ededed`.
- **Pied** : filet haut, padding `16px 18px`, 12 px, interlignage 1.9 — une ligne `✓` en `#74d8a0` (ce qui est inclus), une ligne `✕` en `#6f6f6f` (ce qui est exclu). Ce couple inclus/exclu est un motif rhétorique de la marque : on montre toujours les deux.
- **Balayage** : bande de 2 px en dégradé horizontal `transparent → rgba(116,216,160,0.18) → transparent` qui traverse le panneau de haut en bas en 2,5 s, puis attend 6,5 s (cycle de 9 s, `linear`, en boucle).

### Pastille inline

Texte `#ededed` sur `#1a1a1a`, padding `2px 7px`, rayon 3 px. Sert à isoler un terme technique dans une phrase (une extension de fichier, un chemin).

### Curseur clignotant

Bloc de `0.55em × 1em` en `#74d8a0`, 10 px après le texte, aligné sur le bas de la casse, clignotant en `steps(1)` sur 1,1 s. Se place en fin de kicker d'accroche pour évoquer une invite de commande.

### Icône

Style **Lucide** : `24 × 24` de viewBox, rendu à 16 px, `fill: none`, `stroke: currentColor`, `stroke-width: 2`, `stroke-linecap` et `stroke-linejoin` en `round`. Aucune icône pleine, aucune icône colorée.

---

## 9. Mouvement

### Constantes

| Constante | Valeur | Emploi |
|---|---|---|
| Courbe | `cubic-bezier(0.22, 1, 0.36, 1)` | **toutes** les transitions du système |
| Durée micro | `180 ms` | survol, focus, changements d'état |
| Durée d'apparition | `600 ms` | entrées à l'écran |
| Durée de fondu de ligne | `150 ms` | apparition d'une ligne de données |
| Fondu du halo | `500 ms` | apparition du décor réactif |

Une seule courbe pour tout le système : une sortie très amortie, presque sans rebond. Ne pas introduire de `ease-in-out`, de `linear` (hors boucles ambiantes) ni de spring.

### Apparition standard

`opacity: 0 → 1` et `translateY(12px) → 0`, sur 600 ms. Déclenchée à 15 % de visibilité de l'élément, une seule fois (jamais rejouée).

**Stagger** :
- en pile verticale : **60 ms** par élément (kicker → titre → chapeau → actions → mention) ;
- en grille : **80 ms** par colonne (0 / 80 / 160 ms), annulé quand la grille passe en colonne unique ;
- le panneau de données entre en dernier, après la colonne de texte.

### Boucles ambiantes

| Animation | Cycle | Courbe | Détail |
|---|---|---|---|
| Pulsation des points | 3,6 s | `ease-in-out` | `opacity: 1 → 0.35`, décalages `0 / -1.2s / -2.4s` |
| Curseur clignotant | 1,1 s | `steps(1)` | `opacity: 0` à 50 % |
| Balayage du panneau | 9 s | `linear` | traversée sur 28 % du cycle, départ retardé de 1 s |

### Interactions continues

- **Halo curseur** : suit le pointeur à chaque frame, sans lissage (`requestAnimationFrame`, une frame en vol maximum).
- **Parallaxe du panneau** : très faible — translation max **± 6 px**, `rotateX ≈ ± 1,5°`, `rotateY ≈ ± 1,8°`, perspective 900 px, lissage exponentiel de 8 % par frame. L'effet doit être perçu comme une matière, pas comme un mouvement.
- **Construction séquentielle** : les lignes du panneau apparaissent une par une, pas de **220 ms**, fondu de 150 ms chacune (≈ 1,8 s au total), avec un compteur de taille qui monte de `0.0 GB` à la valeur finale.

### Règles absolues

1. **N'animer que `transform` et `opacity`.** Jamais de largeur, hauteur, marge, couleur de fond animée sur de longues durées.
2. **Tout doit être lisible à l'arrêt.** Aucune information ne dépend d'une animation : les états masqués sont conditionnés à la présence de JS, et le rendu par défaut est l'état final.
3. **`prefers-reduced-motion: reduce` fait autorité** : les apparitions passent en état final sans transition, les déplacements de survol disparaissent (les changements de couleur restent), le halo est retiré, les boucles ambiantes sont coupées, le défilement redevient instantané. Le curseur clignotant reste affiché, fixe.
4. Les animations d'entrée ne se rejouent pas au retour dans le champ de vision.

---

## 10. Déclinaison réseaux sociaux

### Principes communs

- **Fond sombre uniquement** : `#0a0a0a` + trame de croix. Aucun visuel de marque sur fond clair.
- **Trame** : mise à l'échelle selon le § 7.1 — 15 à 22 cellules sur la plus grande dimension, trait 1 px de rendu final.
- **Halo statique** derrière l'élément principal (wordmark ou titre), diamètre ≈ 40 % de la plus grande dimension, croix éclairées en `rgba(190,235,210,0.32)` alignées sur la trame du fond.
- **Hiérarchie type** : un kicker mono en capitales (`0.16em`, `#6f6f6f`) → un titre Space Grotesk 700 en interlettrage `-0.035em` → éventuellement une mention basse (`0.06em`, `#5f5f5f`). Jamais deux titres.
- **Un seul accent vert par visuel** : soit le « Pod » du wordmark, soit une coche, soit une barre de 2 px. Pas les trois.
- Pas de bordure, pas de cadre, pas de coin arrondi sur l'image elle-même.

### Formats

| Format | Dimensions | Recette |
|---|---|---|
| **Avatar** | 512 × 512 (min. 256) | `streampod-favicon.png` centré, occupant 62 à 70 % du côté, halo double conservé, fond `#0a0a0a` + trame (cellule ≈ 32 px à 512). Pas de texte. |
| **Bannière X** | 1500 × 500 | Wordmark (logo 72 px de haut + nom Display 700 ≈ 52 px) aligné à gauche à 96 px du bord, tagline mono en capitales dessous. Zone sûre : garder 220 px libres en bas à gauche (avatar) et 130 px de marge verticale (recadrage mobile). |
| **Bannière Twitch** | 1200 × 480 | Wordmark centré, titre court dessous (Display 700, ≈ 44 px), points de statut sur une ligne. Zone sûre : 80 px de marge sur les quatre côtés. |
| **Bannière YouTube** | 2560 × 1440 | Zone sûre TV **1546 × 423 centrée** : n'y placer que le wordmark et la tagline. Le reste = fond + trame + halo, rien d'autre. |
| **Post carré** | 1080 × 1080 | Une seule idée : un titre Display 700 (56–72 px) sur 3 lignes max, kicker au-dessus, wordmark discret en bas (logo 32 px). Marges 96 px. |
| **Miniature 16:9** | 1280 × 720 | Titre Display 700 sur 2 lignes max, aligné à gauche, marges 80 px ; wordmark en bas à droite. Le texte doit rester lisible à 320 × 180 : jamais sous 44 px. |
| **Image de partage (OG)** | 1200 × 630 | Wordmark centré + titre + tagline. Zone sûre 1200 × 600 (les aperçus rognent le bas). |

### Taille minimale du wordmark

Le nom « StreamPod » ne descend pas sous **16 px de hauteur de casse** en rendu final ; en dessous, n'utiliser que l'icône carrée.

### Textes à réutiliser

Tagline : `WINDOWS 10/11 · GRATUIT · ~10 MO PORTABLE`
Promesses : `100 % local` · `0 requête réseau` · `secrets jamais sauvegardés` · `Gratuit · Portable`

---

## 11. Déclinaison app Windows / UI

Transposition directe des tokens vers une interface applicative. Aucun thème clair.

### Surfaces

| Élément | Valeur |
|---|---|
| Fond de fenêtre | `#0a0a0a` (+ trame de croix optionnelle, uniquement sur les écrans peu denses : accueil, fin d'opération) |
| Panneau, carte, liste | `#0d0d0d`, bordure `1px rgba(255,255,255,0.10)`, rayon 8 px |
| Champ de saisie, pastille | `#1a1a1a`, rayon 3 à 4 px |
| Séparateur | `1px rgba(255,255,255,0.08)` |
| Élément de liste survolé | `#0d0d0d` |
| Ombre | `0 20px 60px rgba(0,0,0,0.6)` — dialogues modaux uniquement |

### Texte

| Rôle | Valeur |
|---|---|
| Titre de fenêtre / d'écran | Space Grotesk 600, 24–30 px, `-0.03em`, `#ededed` |
| Titre de section | Space Grotesk 600, 18 px, `#ededed` |
| Corps, libellés de contrôles | JetBrains Mono 400, 13 px, interlignage 1.65, `#ededed` |
| Texte secondaire, aide | JetBrains Mono 400, 13 px, `#9a9a9a` |
| Label de champ, en-tête de colonne | JetBrains Mono 400, 12 px, capitales, `0.08em`, `#8a8a8a` |
| Chemin, nom de fichier, valeur technique | JetBrains Mono 400, 12,5 px, `#c9c9c9` |
| Métadonnée, taille, horodatage | JetBrains Mono 400, 12 px, `#6f6f6f` |

### Contrôles

- **Bouton primaire** : `#0a0a0a` sur `#ededed`, Mono 600, 13 px, capitales `0.05em`, rayon 4 px, padding `12px 20px` (hauteur cible 40 px). Survol `#ffffff`. Pas de `translateY` dans une app.
- **Bouton secondaire** : transparent, bordure `rgba(255,255,255,0.16)`, texte `#ededed`, Mono 500. Survol : bordure `rgba(255,255,255,0.4)` + fond `rgba(255,255,255,0.04)`.
- **Bouton désactivé** : bordure et texte en `#5f5f5f`, fond transparent, pas de survol.
- **Focus clavier** : contour de 1 px en `#74d8a0` avec 2 px de décalage. Le focus est **le seul** endroit où le vert borde un élément interactif.
- **Case à cocher / interrupteur actif** : marque en `#74d8a0` sur `#1a1a1a`. Jamais de fond plein vert.

### États

Le vert est le **succès**. L'avertissement et l'erreur emploient les deux teintes d'état du § 4 — extension décidée pour l'application, où la gravité doit se lire instantanément pendant une opération qui touche la configuration OBS de l'utilisateur. Le reste se dit en gris et en typographie.

| État | Traitement |
|---|---|
| Succès / inclus | `✓` + texte en `#74d8a0` |
| Exclu / ignoré volontairement | `✕` + texte en `#6f6f6f` |
| En cours | texte `#ededed` + point pulsé `#74d8a0` (3,6 s) |
| Avertissement | texte `#ededed` sur panneau `#0d0d0d`, préfixe `!` en `#d8a674`, bordure gauche 2 px `#d8a674` |
| Erreur | texte `#ededed` sur panneau `#0d0d0d`, préfixe `✕` en `#d87474`, bordure gauche 2 px `#d87474` |

Le panneau reste `#0d0d0d` dans les deux cas : la teinte est un liseré et une marque, jamais un fond. Sur le web, ces deux états conservent le traitement monochrome (préfixe `#9a9a9a` / `#ededed`, liseré blanc).

### Progression & journaux

- **Barre de progression** : rail `#1a1a1a`, hauteur 2 à 4 px, rayon 0 ou 2 px, remplissage `#74d8a0`. Pas de rayures, pas d'animation de brillance. Le pourcentage à côté en Mono 12 px `#9a9a9a`.
- **Journal / arborescence de fichiers** : Mono 12,5 px, interlignage **2.05**, panneau `#0d0d0d`, caractères de dessin (`│ ├─ └─`) en `#5f5f5f`, libellés en `#ededed`, valeurs techniques en `#c9c9c9`. C'est la reprise exacte du panneau du site — c'est l'écran qui doit le plus ressembler au site.
- **Compteur** : un volume ou un décompte qui progresse est un motif de marque. L'animer numériquement (comme le compteur de taille du site) plutôt que d'afficher un spinner.

### Barre de statut basse

Filet haut `rgba(255,255,255,0.08)`, hauteur ≈ 40 px, Mono 11,5 px capitales `0.08em` en `#7a7a7a`, items précédés d'un point de 6 px `#74d8a0` pulsé avec des délais désynchronisés. Y afficher les invariants du produit : `100 % local`, `0 requête réseau`.

### Icône & écran de démarrage

- Icône d'application : `streampod-favicon.png` (256 × 256), déclinée en 16 / 32 / 48 / 256 pour le `.ico`.
- Splash : fond `#0a0a0a` + trame, logo centré 96 px de haut avec son halo, wordmark en dessous (Display 700, 28 px, « Pod » en `#74d8a0`), version en Mono 12 px `#5f5f5f`.

### Mouvement en app

Mêmes constantes : courbe `cubic-bezier(0.22, 1, 0.36, 1)`, 180 ms pour les états, 600 ms pour les entrées de panneau. Pas de halo curseur, pas de parallaxe. Respecter le réglage système d'animations réduites.

---

## 12. Textes canoniques

À réutiliser tels quels, ce sont des éléments d'identité.

| Élément | Texte |
|---|---|
| Titre | `Votre OBS complet, dans un seul fichier.` |
| Kicker | `Sauvegarde & restauration - OBS Studio` |
| Chapeau | `Scènes, profils, paramètres, plugins et assets - réunis dans une archive .obsbackup transportable, restaurée à l'identique sur n'importe quel PC. Aucune connexion réseau, jamais.` |
| Tagline | `WINDOWS 10/11 · GRATUIT · ~10 MO PORTABLE` |
| Promesses | `100 % local` · `0 requête réseau` · `secrets jamais sauvegardés` |
| Mention courte | `Gratuit · Portable` |
| Inclus | `✓ Tout ce qu'il faut pour retrouver votre OBS` |
| Exclu | `✕ Jamais votre clé de stream ni vos mots de passe` |
| Appel final | `Prêt à changer de PC ?` |
| Action | `Télécharger StreamPod.exe` |
| Copyright | `© 2026 StreamPod` |

### Conventions d'écriture

- Espace insécable avant `%` et avant les ponctuations doubles : `100 % local`, `Prêt à changer de PC ?`.
- Séparateur de liste inline : **`·`** entouré d'espaces insécables. Jamais `|`, `/` ou `—`.
- Incise : **tiret court `-`** entouré d'espaces (convention du site), pas de cadratin.
- Marques de statut : **`✓`** (U+2713) et **`✕`** (U+2715). Pas d'emoji, pas de ✅/❌.
- Arborescences : `│` `├─` `└─` (caractères de dessin de boîte), suivis d'une espace.
- Extensions et chemins toujours en pastille ou en Mono `#c9c9c9` : `.obsbackup`.
- Unités : `Mo` en français courant, `GB` toléré dans un affichage technique reproduisant une sortie de programme.
- Ponctuation finale des titres conservée (point ou point d'interrogation).

---

## 13. Annexe — tokens

Bloc à copier tel quel dans un projet web (identique à l'implémentation de référence) :

```css
:root {
  --bg: #0a0a0a;
  --panel: #0d0d0d;
  --fg: #ededed;
  --muted: #9a9a9a;
  --muted-2: #8a8a8a;
  --muted-3: #7a7a7a;
  --muted-4: #6f6f6f;
  --muted-5: #5f5f5f;
  --accent: #74d8a0;
  --line: rgba(255, 255, 255, 0.08);
  --line-strong: rgba(255, 255, 255, 0.1);
  --font-mono: "JetBrains Mono", ui-monospace, monospace;
  --font-display: "Space Grotesk", sans-serif;
  --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  --dur-fast: 180ms;   /* micro-interactions (hover, focus) */
  --dur-reveal: 600ms; /* apparitions au scroll */
  --grid: 64px;        /* pas de la grille de croix (doit correspondre aux SVG inline) */
  --brand-logo-height: 28px;
  --brand-logo-small-height: 21px;
  --brand-glow-light: rgba(255, 255, 255, 0.18);
  --brand-glow-accent: rgba(116, 216, 160, 0.42);
}
```

### Correspondance pour les environnements non-CSS

| Token | Valeur | Rôle |
|---|---|---|
| `bg` | `#0a0a0a` | fond de page / fenêtre |
| `panel` | `#0d0d0d` | panneau, carte, élément survolé |
| `field` | `#1a1a1a` | champ, pastille |
| `fg` | `#ededed` | texte principal |
| `fg-strong` | `#ffffff` | état survolé des surfaces claires |
| `fg-tech` | `#c9c9c9` | valeur technique |
| `muted` | `#9a9a9a` | texte secondaire |
| `muted-2` | `#8a8a8a` | texte tertiaire, label |
| `muted-3` | `#7a7a7a` | barre de statut |
| `muted-4` | `#6f6f6f` | label discret, élément exclu |
| `muted-5` | `#5f5f5f` | métadonnée, état désactivé |
| `muted-6` | `#4f4f4f` | copyright |
| `decor` | `#3a3a3a` | chiffre décoratif |
| `accent` | `#74d8a0` | accent, succès, focus |
| `line` | blanc 8 % | filet standard |
| `line-strong` | blanc 10 % | filet renforcé, trait de trame |
| `line-btn` | blanc 16 % | bordure de bouton au repos |
| `line-btn-hover` | blanc 40 % | bordure de bouton au survol |
| `overlay-soft` | blanc 4 % | fond survolé, cœur du halo |
| `shadow` | `0 20px 60px` noir 60 % | ombre de panneau |
| `radius-sm` / `md` / `lg` | `3px` / `4px` / `8px` | pastille / bouton / panneau |
| `grid-step` | `64px` | pas de la trame |
| `ease` | `cubic-bezier(0.22, 1, 0.36, 1)` | courbe unique |
| `dur-fast` / `dur-reveal` | `180ms` / `600ms` | micro-interaction / apparition |

---

## 14. Limites connues

À traiter avant d'étendre l'identité à de nouveaux supports :

- **Aucune variante de logo sur fond clair.** Tout visuel doit rester sur fond sombre. Un fond clair nécessiterait une version dédiée (sans halo, avec un contraste retravaillé) qui n'existe pas.
- **Aucune version monochrome / une couleur** du logo — impression, gravure, tampon, filigrane sont hors périmètre pour l'instant.
- **Pas de favicon SVG** ni de logo vectoriel : les assets sont en PNG, `streampod-logo.png` plafonne à 431 × 512. Toute utilisation grand format (affiche, écran très dense) demande un ré-export vectoriel.
- **Aucun asset social existant** : les formats du § 10 sont des recettes, pas des fichiers livrés.
- **Dépendance Google Fonts** pour le web. L'application Windows, elle, embarque les fichiers de police via `@fontsource` (SIL OFL, redistribuables) : sa CSP n'autorise que `font-src 'self'` et le produit ne fait aucune requête réseau.
- **Les couleurs d'état sont réservées à l'app.** L'ambre et le rouge du § 4 n'ont pas d'équivalent web ni social : un avertissement dans un visuel de marque reste monochrome.
- **Aucune marque `✓` / `✕` dans les polices embarquées** : ces glyphes retombent sur la police système. Acceptable en app ; sur un visuel de marque, les composer en vectoriel.
