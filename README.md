# OwBS — Votre OBS complet, dans un seul fichier

OwBS sauvegarde l'intégralité d'une installation OBS Studio (Windows) dans un
fichier unique `.obsbackup`, puis la restaure sur n'importe quel PC :
changement d'ordinateur, stream en déplacement, réinstallation de Windows.

## Ce qui est sauvegardé

- **Collections de scènes** (`basic\scenes`) — avec réécriture automatique des
  chemins d'assets à la restauration
- **Profils** (`basic\profiles`) — paramètres d'encodage, de sortie, etc.
- **Paramètres globaux** (`global.ini`, `user.ini`)
- **Configuration des plugins** (`plugin_config`)
- **Plugins tiers** — les DLL et leurs données sont incluses ; à la
  restauration elles sont copiées si la version majeure d'OBS correspond,
  sinon la liste des plugins à réinstaller est affichée
- **Assets** — images, vidéos, sons, fichiers HTML… référencés par les scènes
  sont embarqués dans l'archive et déposés dans `Documents\OBS-Backup-Assets`
  à la restauration

## Ce qui n'est JAMAIS sauvegardé (par conception)

- La **clé de stream** (Twitch, Kick, YouTube…) et les champs de comptes
  connectés de `service.json` (y compris les copies `.bak`)
- Les tokens OAuth et lignes sensibles des fichiers `.ini`
- Les **cookies des docks navigateur** (`plugin_config\obs-browser`), qui
  contiennent les sessions Twitch/YouTube — c'est aussi ~700 Mo de cache
  évités
- Les logs, rapports de crash et données de profilage

Après une restauration, il suffit de re-saisir sa clé de stream ou de
reconnecter son compte dans OBS (Paramètres → Flux). Tout le reste fonctionne.

100 % local : aucune donnée ne quitte l'ordinateur.

## Développement

```bash
npm install
npm run tauri dev      # lance l'app en mode développement
npm run tauri build    # produit l'exécutable + installateur (src-tauri/target/release)
```

### Tests

```bash
cd src-tauri
cargo test                                        # unitaires + E2E sur config factice
cargo test --test real_machine -- --ignored       # test réel (lecture seule sur votre OBS)
```

Le test E2E fabrique une fausse configuration OBS, la sauvegarde, la restaure
dans un bac à sable et vérifie qu'aucun secret ne fuit dans l'archive. Les
variables d'environnement `OWBS_CONFIG_DIR`, `OWBS_INSTALL_DIR`,
`OWBS_ASSETS_DIR` et `OWBS_OBS_VERSION` permettent de rediriger tous les
chemins pour les tests.

## Format `.obsbackup`

Archive ZIP :

```
manifest.json      # version du format, version d'OBS, plugins, table des assets
config/            # copie nettoyée de %APPDATA%\obs-studio
plugins/64bit/     # DLL des plugins tiers
plugins/data/      # données des plugins tiers
assets/<n>/        # fichiers médias référencés par les scènes
```

## Architecture

- `src-tauri/src/obs.rs` — détection d'OBS (config, installation, version, processus)
- `src-tauri/src/sanitize.rs` — suppression des secrets (clé de stream, tokens, cookies)
- `src-tauri/src/scenes.rs` — extraction et réécriture des chemins d'assets
- `src-tauri/src/backup.rs` — pipeline de sauvegarde → `.obsbackup`
- `src-tauri/src/restore.rs` — restauration avec copie de sécurité automatique
  de la configuration existante (`obs-studio.bak-<date>`) et rollback en cas
  d'échec de la bascule
- `src/` — interface (Vite + TypeScript, en français)
