# Plan de correctifs critiques — OwBS

> **Rédigé le 2026-07-18.** Trois correctifs de sécurité/fiabilité bloquants pour la
> release, identifiés lors de l'audit pré-release. **Une étape = une session dédiée**,
> dans l'ordre ci-dessous (l'étape 2 est la plus sensible, l'étape 1 la plus simple).
> Chaque étape est autonome : tout le contexte nécessaire est inclus.

## Règles communes à toutes les étapes

- Lire `CLAUDE.md` du projet avant de commencer (conventions, contraintes produit).
- **Promesse produit non négociable** : aucune clé de stream, token, cookie ni mot de
  passe ne doit se retrouver dans une archive `.obsbackup`.
- Tests : ne **jamais** toucher la vraie config OBS de la machine. Toujours rediriger
  via `OWBS_CONFIG_DIR`, `OWBS_INSTALL_DIR`, `OWBS_ASSETS_DIR`, `OWBS_OBS_VERSION`.
- UI, messages d'erreur Rust et noms de tests : **en français**.
- Erreurs internes : `Result<T, String>` avec message français prêt à afficher.
- Validation systématique en fin d'étape : `cd src-tauri && cargo test` (tous verts),
  puis relecture du diff complet avant de conclure.

---

## Étape 1 — Boucher la fuite du mot de passe obs-websocket

### Problème
`plugin_config/obs-websocket/config.json` contient `server_password` (mot de passe du
contrôle à distance d'OBS). Or dans `src-tauri/src/backup.rs:388-409`, seuls
`service.json` (via `sanitize::sanitize_json`) et les `.ini` (via
`sanitize::sanitize_ini`) sont assainis ; tout autre fichier — dont ce JSON — est copié
tel quel dans l'archive (`zip_file_from_disk`, branche `else` ligne 407-408).
Quiconque récupère l'archive peut piloter l'OBS de la victime à distance.

Le test E2E ne détecte rien car la config factice écrit
`{"server_enabled":true}` **sans** mot de passe (`src-tauri/tests/e2e.rs:58`).

### Correctif attendu
1. Dans `backup.rs`, étendre la sanitization : tout fichier `.json` sous
   `plugin_config/` passe par `sanitize::sanitize_json` (même traitement que
   `service.json`). S'appuyer sur `rel_lower` déjà calculé (ligne 372).
   - La denylist existante (`SENSITIVE_JSON_KEYS` + le test `k.contains("password")`
     dans `sanitize.rs:33`) couvre déjà `server_password` — vérifier par un test
     unitaire, ne pas dupliquer la logique.
   - **Ne surtout pas** étendre aux JSON de scènes (`basic/scenes/**`) : ils
     contiennent des champs `key` légitimes (raccourcis clavier). C'est une décision
     d'architecture actée, documentée dans `CLAUDE.md`.
2. Cas d'un JSON de `plugin_config/` illisible ou invalide : ne **pas** retomber sur
   la copie brute (ce serait une fuite potentielle). Recommandation : exclure le
   fichier de l'archive et le signaler (avertissement dans le résumé de sauvegarde ou
   via l'event `owbs://progress`). Ne pas faire échouer toute la sauvegarde pour un
   JSON de plugin corrompu.
3. Vérifier qu'il n'existe pas d'autres emplacements de secrets du même genre sous
   `plugin_config/` (rapide inventaire : obs-websocket est le cas connu ;
   `obs-browser` est déjà exclu en bloc). En cas de doute, la denylist par clé couvre
   le cas général puisqu'on sanitize désormais tous les JSON de `plugin_config/`.

### Tests à ajouter (obligatoires)
- **E2E** (`src-tauri/tests/e2e.rs`) : enrichir la config factice ligne 58 avec un
  mot de passe sentinelle, p. ex.
  `{"server_enabled":true,"server_password":"WSPASS_ULTRASECRET"}`, déclarer une
  constante `FAKE_WS_PASSWORD` à côté de `FAKE_STREAM_KEY` (ligne 9), puis :
  - assertion « aucun secret dans l'archive » (même mécanique que la vérification
    `FAKE_STREAM_KEY` existante, lignes ~110-131) ;
  - assertion « aucun secret dans la config restaurée » (pendant des lignes ~209-218) ;
  - vérifier que le fichier `plugin_config/obs-websocket/config.json` est bien
    **présent** dans l'archive avec `server_enabled` conservé (on assainit, on
    n'exclut pas).
- **Unitaire** (`sanitize.rs`, module tests existant) : `sanitize_json` sur un
  document contenant `server_password` le supprime et laisse les autres champs.

### Critères de fin
- `cargo test` vert, y compris les nouvelles assertions de non-fuite.
- Le diff ne touche que `backup.rs`, `sanitize.rs` (si besoin) et les tests.
- Passage par `code-reviewer` sur le diff avant de conclure.

---

## Étape 2 — Corriger l'écriture de fichier arbitraire à la restauration (zip-slip)

### Problème (faille confirmée par PoC)
`src-tauri/src/restore.rs:261` ne bloque que les noms contenant `..`. Une entrée
d'archive nommée `config/C:\Users\Public\evil.dll` passe le filtre ; ensuite
`tmp_config.join(rel.split('/').collect::<PathBuf>())` (ligne 267) produit
`C:\Users\Public\evil.dll`, car `PathBuf::join` **abandonne la base** dès qu'un
composant est absolu (préfixe de lecteur Windows). Un `.obsbackup` piégé — et le
format est précisément conçu pour s'échanger entre PC — peut donc écrire n'importe où
sur le disque.

**Surface complète à couvrir, pas seulement `config/`** (vérifié dans le code) :
1. `config/…` → `tmp_config.join(...)` (restore.rs:266-268) ;
2. **`assets/…` via le manifest** : `asset_dest_by_archive_path` est construit à
   partir de `asset.archive_path` **lu depuis le manifest de l'archive**, donc
   contrôlé par l'attaquant (restore.rs:240-244, `assets_dir.join(rel)`) — même
   vecteur ;
3. `plugins/64bit/…` et `plugins/data/…` → dossier de transit (restore.rs:282-294).

### Correctif attendu
1. Écrire une fonction de validation unique, p. ex.
   `fn chemin_relatif_sur(base: &Path, rel: &str) -> Result<PathBuf, String>` (ou un
   helper `composants_surs(rel) -> Option<PathBuf>`), qui découpe sur `/` et
   **n'accepte que des composants `Component::Normal`** ; rejeter tout composant :
   - vide, `.` ou `..` ;
   - contenant `\`, `:` (préfixe de lecteur, flux NTFS ADS) ou un caractère de
     contrôle ;
   - dont le `Path` résultant est absolu ou possède un préfixe (`Component::Prefix`).
2. Appliquer cette validation aux **trois** points de jointure listés ci-dessus (y
   compris les chemins issus du manifest, pas seulement `entry.name()`).
3. Politique en cas d'entrée invalide : **refuser toute la restauration** avec une
   erreur française explicite (p. ex. « Archive invalide ou malveillante : chemin
   suspect "…" ») plutôt que d'ignorer l'entrée en silence comme aujourd'hui. Une
   archive piégée ne doit pas être restaurée à moitié. Supprimer le filtre
   `name.contains("..")` devenu redondant (ligne 261) au profit de la validation
   centrale.
4. Le crate `zip` offre `enclosed_name()` : utilisable comme première barrière, mais
   **ne pas s'y fier seul** — garder la validation maison des composants (`:` et `\`
   dans un composant, chemins issus du manifest que `enclosed_name` ne voit jamais).
5. Vérifier au passage `extract_entry` : s'assurer que la création des dossiers
   parents part bien du chemin validé.

### Tests à ajouter (obligatoires)
- **Unitaires** sur le validateur : acceptés (`a/b/c.json`) ; rejetés :
  `C:\Users\Public\evil.dll`, `C:/x`, `..\\x`, `a/../b`, `a/b:ads`, `/etc/x`, `\\x`,
  composant vide (`a//b`), `.` seul.
- **E2E adversarial** (nouveau test dans `e2e.rs` ou fichier dédié
  `src-tauri/tests/zip_slip.rs`) : construire à la main, avec le crate `zip`, une
  archive `.obsbackup` piégée contenant :
  - une entrée `config/C:\Users\Public\evil.dll` (reproduit le PoC) ;
  - une entrée `config/..\..\evil2.txt` ;
  - un manifest dont un asset a `archive_path` piégé (p. ex.
    `assets/C:\Users\Public\evil3.txt`).
  Lancer la restauration dans un bac à sable (`OWBS_CONFIG_DIR` etc.), vérifier :
  restauration **refusée** avec message français, **aucun fichier créé hors du bac à
  sable** (utiliser un chemin sentinelle dans le tempdir du test, jamais un vrai
  chemin système), et config d'origine intacte.
- Le round-trip existant (`backup_puis_restore_round_trip`) doit rester vert : la
  validation ne doit rejeter aucune archive légitime produite par `backup.rs`.

### Critères de fin
- PoC neutralisé : le test adversarial passe, aucun fichier hors bac à sable.
- `cargo test` vert au complet.
- Passage par `code-reviewer` sur le diff, avec attention aux contournements
  (encodages, casse, ADS `fichier:flux`, préfixes `\\?\`).

---

## Étape 3 — Rendre la bascule de configuration récupérable (rollback)

### Problème
`src-tauri/src/restore.rs:334-349` : la bascule se fait en deux `rename` :
1. `target_config` → `obs-studio.bak-<stamp>` ;
2. `tmp_config` → `target_config`.

Si le rename n°2 échoue (verrou antivirus, dossier ouvert dans l'Explorateur, disque
plein…), l'utilisateur n'a **plus de config OBS du tout** — juste un `.bak`. Le
commentaire parle de bascule « atomique » alors qu'elle ne l'est pas : deux renames ne
seront jamais atomiques ensemble ; l'objectif réaliste est **jamais d'état sans
config + rollback automatique**.

### Correctif attendu
1. Extraire la bascule dans une fonction dédiée et testable, p. ex.
   `fn basculer_config(target: &Path, tmp: &Path, bak: &Path) -> Result<Option<String>, String>`
   (retourne le chemin du `.bak` créé, comme aujourd'hui `previous_backup`).
2. Logique :
   - rename n°1 échoue → abandon propre, rien n'a bougé (comportement actuel
     correct : message « Impossible de mettre de côté la configuration existante
     (OBS ouvert ?) ») ;
   - rename n°2 échoue → **rollback automatique** : re-renommer le `.bak` vers
     `target_config`, puis retourner une erreur française expliquant que la
     restauration a échoué mais que la configuration d'origine a été remise en place ;
   - si le rollback échoue **aussi** (cas extrême) : erreur détaillée donnant les
     **chemins absolus** du `.bak` et du dossier temporaire, avec instruction
     manuelle (« renommez X en Y ») — ne jamais avaler cette information.
3. Corriger le commentaire ligne 334-335 : ne plus prétendre à l'atomicité ; décrire
   la vraie garantie (« bascule avec rollback : en cas d'échec, l'ancienne
   configuration est remise en place »).
4. Nettoyage : en cas d'échec avec rollback réussi, supprimer (best effort) le
   dossier `obs-studio.tmp-*` orphelin pour ne pas laisser traîner des données.

### Tests à ajouter (obligatoires)
- **Unitaires sur `basculer_config`** dans un tempdir :
  - cas nominal : tmp devient target, l'ancien devient bak ;
  - cible inexistante (première restauration) : pas de bak, pas d'erreur ;
  - **échec du rename n°2 avec rollback** : provoquer l'échec de façon
    déterministe — piste simple sous Windows : après le rename n°1, créer un
    **fichier** au chemin `target_config` (un rename de dossier vers un chemin déjà
    occupé échoue) ; sinon, injecter la fonction de rename (paramètre
    `rename_fn: impl Fn(&Path, &Path) -> std::io::Result<()>` ou hook `#[cfg(test)]`).
    Vérifier : erreur retournée, config d'origine **de nouveau à sa place**, contenu
    intact.
- E2E existant : `backup_puis_restore_round_trip` doit rester vert (la bascule
  nominale ne change pas de comportement observable).

### Critères de fin
- Plus aucun scénario d'échec du rename n°2 ne laisse l'utilisateur sans config.
- Le mot « atomique » a disparu des commentaires ou est remplacé par la garantie
  réelle.
- `cargo test` vert ; revue du diff par `code-reviewer` ; si possible, vérification
  réelle du chemin nominal via `verifier` (restauration bac à sable de bout en bout).

---

## Après les trois étapes

- Relancer la suite complète : `cd src-tauri && cargo test`.
- Test lecture seule sur la machine réelle :
  `cargo test --test real_machine -- --ignored`.
- Mettre à jour `CLAUDE.md` (section Contraintes) : mentionner la sanitization des
  JSON de `plugin_config/` et la garantie de rollback de la bascule.
- Reconsidérer alors le verdict « Ne pas livrer » de l'audit pré-release.
