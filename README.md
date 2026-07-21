<div align="center">

<img src="src-tauri/icons/icon.png" alt="Logo OwBS" width="128" height="128" />

# OwBS

**Votre OBS complet, dans un seul fichier.**

OwBS sauvegarde l'intégralité d'une installation OBS Studio dans un fichier
`.obsbackup` unique, puis la restaure sur n'importe quel PC — sans jamais
embarquer votre clé de stream.

</div>

---

Changer d'ordinateur, streamer en déplacement, réinstaller Windows : OwBS
réunit vos **scènes, profils, paramètres, plugins et assets** dans une seule
archive transportable sur clé USB, et remet tout en place de l'autre côté.
Aucune connexion réseau, aucune télémétrie : **100 % local**.

## Sommaire

- [Fonctionnalités](#fonctionnalités)
- [Confidentialité](#confidentialité)
- [Utilisation](#utilisation)
- [Installation](#installation)
- [Le format `.obsbackup`](#le-format-obsbackup)
- [Développement](#développement)
- [Architecture](#architecture)

## Fonctionnalités

- **Sauvegarde complète en un fichier** — collections de scènes, profils,
  paramètres globaux, configuration des plugins et tous les assets référencés,
  réunis dans une archive `.obsbackup` prête pour une clé USB.
- **Restauration transactionnelle** — l'ancienne configuration est mise de côté
  (`obs-studio.bak-<date>`) avant la bascule ; en cas d'échec, elle est remise
  en place automatiquement. Jamais d'OBS sans configuration.
- **Assets embarqués et rechemins automatiques** — images, vidéos, sons et
  overlays sont déposés dans `Documents\OBS-Backup-Assets`, et les chemins sont
  réécrits dans les scènes pour pointer au bon endroit sur le nouveau PC.
- **Remappage du matériel** — quand un micro, une webcam ou une sortie audio
  n'existe pas sur la machine cible, OwBS propose des remplaçants classés par
  pertinence. Vous confirmez chaque association ; rien n'est choisi à votre
  place.
- **Secrets exclus par conception** — clé de stream, tokens OAuth et cookies de
  session ne sont jamais écrits dans l'archive (voir [Confidentialité](#confidentialité)).
- **Léger et portable** — application Tauri d'environ 10 Mo, sans runtime lourd
  à installer.

## Confidentialité

C'est la promesse centrale du produit : **une sauvegarde ne doit jamais fuiter
vos secrets.**

Ne sont **jamais** enregistrés dans l'archive :

- la **clé de stream** (Twitch, Kick, YouTube…) et les champs de comptes
  connectés de `service.json` — y compris ses copies `.bak` ;
- les **tokens OAuth** et les lignes sensibles des fichiers `.ini` ;
- les **cookies des docks navigateur** (`plugin_config\obs-browser`), qui
  contiennent vos sessions Twitch/YouTube — c'est aussi ~700 Mo de cache évités ;
- les logs, rapports de crash et données de profilage.

Les fichiers de configuration des plugins sont assainis par liste blanche : seuls
les `.json` et `.ini` nettoyés sont archivés (par exemple le mot de passe
d'obs-websocket est retiré), tout autre format opaque est exclu avec un
avertissement.

> [!TIP]
> Après une restauration, il suffit de re-saisir votre clé de stream ou de
> reconnecter votre compte dans OBS (**Paramètres → Flux**). Tout le reste
> fonctionne immédiatement.

> [!CAUTION]
> Les **sources navigateur** (overlays StreamElements, Streamlabs…) conservent
> leur URL dans les scènes, car OBS en a besoin pour les réafficher. Or ces URL
> incluent parfois un **token privé** (`.../overlay/<id>/<TOKEN>`). Ne partagez
> donc un `.obsbackup` qu'avec des personnes de confiance.

## Utilisation

L'interface tient en deux boutons.

**Sauvegarder** — OwBS détecte votre installation OBS, affiche un résumé de ce
qui sera inclus (scènes, profils, plugins, taille des assets), puis vous
demande où écrire le fichier `.obsbackup`.

**Restaurer** — choisissez un `.obsbackup`, vérifiez le résumé, confirmez le
remappage du matériel si nécessaire, et OwBS remet votre configuration en place.

> [!IMPORTANT]
> OBS doit être **fermé** pendant une sauvegarde ou une restauration. OwBS
> refuse d'agir tant qu'`obs64.exe` est en cours d'exécution, pour éviter toute
> corruption des fichiers en cours d'écriture par OBS.

À propos des **plugins tiers** : la liste des plugins installés est enregistrée,
mais leurs DLL ne sont **jamais** réinstallées depuis l'archive — une DLL issue
d'un fichier est du code non vérifié. OwBS affiche simplement la liste des
plugins à réinstaller manuellement depuis leurs sites officiels.

## Installation

OwBS est une application **Windows** (10/11). OBS Studio doit avoir été lancé au
moins une fois sur la machine pour que son dossier de configuration existe.

Le plus simple est de récupérer l'exécutable portable produit par la
compilation (voir [Développement](#développement)) : `OwBS.exe` se lance sans
installation, y compris depuis une clé USB.

> [!NOTE]
> L'exécutable n'étant pas encore signé, Windows SmartScreen peut afficher un
> avertissement au premier lancement. Choisissez **Informations complémentaires
> → Exécuter quand même**. OwBS ne fait aucune requête réseau et ne se met pas à
> jour tout seul : revenez sur la page de téléchargement pour obtenir une
> nouvelle version.

## Le format `.obsbackup`

Un `.obsbackup` est une simple archive ZIP :

```
manifest.json      # version du format, version d'OBS, plugins, table des assets
config/            # copie assainie de %APPDATA%\obs-studio
plugins/64bit/     # DLL des plugins tiers (jamais extraites à la restauration)
plugins/data/      # données des plugins tiers (jamais extraites à la restauration)
assets/<n>/        # fichiers médias référencés par les scènes
```

Les entrées `plugins/` ne servent qu'à l'inventaire : la restauration ne les
extrait jamais, seule la liste des plugins à réinstaller est présentée.

## Développement

Prérequis : [Node.js](https://nodejs.org/), la [toolchain Rust](https://rustup.rs/)
et les [prérequis Tauri pour Windows](https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev      # lance l'app en mode développement
npm run tauri build    # produit l'exécutable + installateur (src-tauri/target/release)
```

### Tests

```bash
cd src-tauri
cargo test                                    # unitaires + E2E sur config factice
cargo test --test real_machine -- --ignored   # test réel (lecture seule sur votre OBS)
```

La suite E2E fabrique une fausse configuration OBS, la sauvegarde, la restaure
dans un bac à sable et **vérifie qu'aucun secret ne fuit** dans l'archive
(zip-slip, non-installation des DLL et rollback sont également couverts).

> [!WARNING]
> Les tests ne touchent **jamais** votre vraie configuration OBS. Les variables
> d'environnement `OWBS_CONFIG_DIR`, `OWBS_INSTALL_DIR`, `OWBS_ASSETS_DIR` et
> `OWBS_OBS_VERSION` redirigent tous les chemins vers des dossiers temporaires.

## Architecture

OwBS est bâti sur **Tauri 2** (backend Rust) avec un frontend **Vite +
TypeScript** vanilla, en français.

| Fichier | Rôle |
|---|---|
| `src-tauri/src/obs.rs` | Détection d'OBS (config, installation, version, processus) |
| `src-tauri/src/sanitize.rs` | Suppression des secrets (clé de stream, tokens, cookies) |
| `src-tauri/src/scenes.rs` | Extraction et réécriture des chemins d'assets |
| `src-tauri/src/backup.rs` | Pipeline de sauvegarde → `.obsbackup` |
| `src-tauri/src/restore.rs` | Restauration avec sauvegarde de secours et rollback automatique |
| `src-tauri/src/devices.rs` | Inventaire du matériel Windows (audio, vidéo) |
| `src-tauri/src/remap.rs` | Diagnostic et application du remappage matériel |
| `src-tauri/src/lib.rs` | Commandes exposées au frontend |
| `src/main.ts` | Toute la logique de l'interface |

Le format des identifiants matériels d'OBS (audio WASAPI, vidéo DirectShow) est
documenté dans [`docs/FORMATS-OBS.md`](docs/FORMATS-OBS.md).
