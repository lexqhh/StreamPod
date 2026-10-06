<div align="center">

<img src="src/assets/streampod-logo.png" alt="Logo StreamPod" width="108" />

# StreamPod

**Votre OBS complet, dans un seul fichier.**

StreamPod sauvegarde l'intégralité d'une installation OBS Studio dans un fichier
`.obsbackup` unique, puis la restaure sur n'importe quel PC - sans jamais
embarquer votre clé de stream.

**[Site officiel : stream-pod.fr](https://stream-pod.fr/)**

</div>

---

Changer d'ordinateur, streamer en déplacement, réinstaller Windows : StreamPod
réunit vos **scènes, profils, paramètres, assets et la liste de vos plugins** dans une seule
archive transportable sur clé USB, et remet tout en place de l'autre côté.
Vos données restent **100 % locales** : une seule requête, vers GitHub, pour
vérifier les mises à jour - désactivable. Aucune télémétrie, aucune donnée
envoyée.

## Sommaire

- [Fonctionnalités](#fonctionnalités)
- [Confidentialité](#confidentialité)
- [Utilisation](#utilisation)
- [Installation](#installation)
- [Mises à jour](#mises-à-jour)
- [Le format `.obsbackup`](#le-format-obsbackup)
- [Compiler depuis les sources](#compiler-depuis-les-sources)
- [Architecture](#architecture)
- [Notes de version](#notes-de-version)

## Fonctionnalités

- **Sauvegarde complète en un fichier** - collections de scènes, profils,
  paramètres globaux, configuration des plugins et tous les assets référencés,
  réunis dans une archive `.obsbackup` prête pour une clé USB.
- **Restauration transactionnelle** - l'ancienne configuration est mise de côté
  (`obs-studio.bak-<date>`) avant la bascule ; en cas d'échec, elle est remise
  en place automatiquement. Jamais d'OBS sans configuration.
- **Assets embarqués et rechemins automatiques** - images, vidéos, sons et
  overlays sont déposés dans `Documents\OBS-Backup-Assets\<date>`, un dossier
  propre à chaque restauration (rien n'est jamais écrasé), et les chemins sont
  réécrits dans les scènes pour pointer au bon endroit sur le nouveau PC. Les
  dossiers d'un diaporama ou d'une playlist VLC sont embarqués eux aussi.
- **Polices signalées** - les polices utilisées par vos textes sont comparées à
  celles du nouveau PC : l'aperçu liste celles à installer (les fichiers de
  police ne sont pas embarqués, pour des raisons de licence).
- **Archive vérifiée** - après l'écriture, chaque fichier de l'archive est relu
  et contrôlé avant d'annoncer « Sauvegarde terminée ».
- **Remappage du matériel** - quand un micro, une webcam ou une sortie audio
  n'existe pas sur la machine cible, StreamPod propose des remplaçants classés par
  pertinence. Vous confirmez chaque association ; rien n'est choisi à votre
  place.
- **Secrets exclus par conception** - clé de stream, tokens OAuth et cookies de
  session ne sont jamais écrits dans l'archive (voir [Confidentialité](#confidentialité)).
- **Léger et portable** - application Tauri d'environ 10 Mo, sans runtime lourd
  à installer.

## Confidentialité

C'est la promesse centrale du produit : **une sauvegarde ne doit jamais fuiter
vos secrets.**

Ne sont **jamais** enregistrés dans l'archive :

- la **clé de stream** (Twitch, Kick, YouTube…) et les champs de comptes
  connectés de `service.json` - y compris ses copies `.bak` ;
- les **tokens OAuth** et les lignes sensibles des fichiers `.ini` ;
- les **cookies des docks navigateur** (`plugin_config\obs-browser`), qui
  contiennent vos sessions Twitch/YouTube - c'est aussi ~700 Mo de cache évités ;
- les logs, rapports de crash et données de profilage.

Les fichiers de configuration des plugins sont assainis par liste blanche : seuls
les `.json` et `.ini` nettoyés sont archivés (par exemple le mot de passe
d'obs-websocket est retiré), tout autre format opaque est exclu avec un
avertissement. Les réglages des scripts OBS sont nettoyés de la même façon, et
les fichiers référencés par vos scènes qui ressemblent à des secrets ou à des
programmes (`.bak`, `.env`, `.json`, `.ini`, `.exe`, `.dll`…) ne sont jamais
embarqués : l'aperçu les liste.

À la restauration, une archive est traitée comme une donnée non fiable :

- les **scripts** (Lua, Python) sont restaurés mais **désactivés** - la liste
  des scripts à réactiver dans **Outils → Scripts** s'affiche à la fin ;
- le serveur **obs-websocket** est désactivé (il régénère son mot de passe au
  prochain lancement) ;
- l'aperçu affiche le **serveur de diffusion** de chaque profil et signale un
  serveur personnalisé.

> [!TIP]
> Après une restauration, il suffit de re-saisir votre clé de stream ou de
> reconnecter votre compte dans OBS (**Paramètres → Flux**). Tout le reste
> fonctionne immédiatement.

> [!CAUTION]
> Les **sources navigateur** (overlays StreamElements, Streamlabs…) conservent
> leur URL dans les scènes, car OBS en a besoin pour les réafficher. Or ces URL
> incluent parfois un **token privé** (`.../overlay/<id>/<TOKEN>`). Ne partagez
> donc un `.obsbackup` qu'avec des personnes de confiance.

> [!NOTE]
> Le manifeste et les scènes conservent les **chemins d'origine** de vos assets
> (par exemple `C:\Users\<votre nom>\…`) : ils sont nécessaires pour réécrire
> les scènes à la restauration. Un `.obsbackup` révèle donc le nom de votre
> session Windows - à garder en tête si vous partagez le fichier.

## Utilisation

L'interface tient en deux boutons.

**Sauvegarder** - StreamPod détecte votre installation OBS, affiche un résumé de ce
qui sera inclus (scènes, profils, plugins, taille des assets), puis vous
demande où écrire le fichier `.obsbackup`.

**Restaurer** - choisissez un `.obsbackup` (ou double-cliquez dessus, ou
déposez-le sur la fenêtre), vérifiez le résumé, confirmez le remappage du
matériel si nécessaire, et StreamPod remet votre configuration en place. Les
listes du résumé se déplient pour voir chaque collection, profil, plugin et
asset.

**Copies de sécurité** - chaque restauration conserve votre configuration
précédente (`obs-studio.bak-<date>`). L'écran « Copies de sécurité » les liste
avec leur date et leur taille : revenez à l'une d'elles en un clic (la
configuration actuelle devient à son tour une copie), ou placez celles devenues
inutiles dans la corbeille Windows.

> [!IMPORTANT]
> OBS doit être **fermé** pendant une sauvegarde ou une restauration. StreamPod
> refuse d'agir tant qu'`obs64.exe` est en cours d'exécution, pour éviter toute
> corruption des fichiers en cours d'écriture par OBS.

Une opération longue (grosses collections d'assets) peut être **annulée** en
cours de route : les fichiers temporaires sont nettoyés et rien n'est modifié.
Pendant une restauration, l'annulation n'est plus possible une fois la mise en
place de la configuration engagée - l'opération va alors jusqu'au bout pour ne
jamais laisser OBS sans configuration.

À propos des **plugins tiers** : les plugins sont repérés dans le dossier d'OBS
**et** dans `C:\ProgramData\obs-studio\plugins`. Leur liste est enregistrée,
mais leurs DLL ne sont **jamais** réinstallées depuis l'archive - une DLL issue
d'un fichier est du code non vérifié. StreamPod affiche simplement la liste des
plugins à réinstaller manuellement depuis leurs sites officiels.

## Installation

StreamPod est une application **Windows** (10/11). OBS Studio doit avoir été lancé au
moins une fois sur la machine pour que son dossier de configuration existe.
Testé avec OBS Studio 32.2.2.

**[Télécharger l'installateur](https://github.com/lexqhh/StreamPod/releases/latest/download/StreamPod-setup.exe)**
(`StreamPod-setup.exe`) : StreamPod s'installe pour votre session Windows, sans
droits administrateur, et s'associe aux fichiers `.obsbackup`.

**Version portable** - pour une clé USB ou sans installation :
[`StreamPod.exe`](https://github.com/lexqhh/StreamPod/releases/latest/download/StreamPod.exe)
se lance directement, sans rien installer.

Les empreintes SHA-256 de chaque fichier sont publiées avec
[la release](https://github.com/lexqhh/StreamPod/releases/latest)
(`SHA256SUMS.txt`).

> [!NOTE]
> L'exécutable n'étant pas encore signé, Windows SmartScreen peut afficher un
> avertissement au premier lancement. Choisissez **Informations complémentaires
> → Exécuter quand même**.

## Mises à jour

Au démarrage, StreamPod vérifie s'il existe une nouvelle version : c'est sa
**seule requête réseau**, vers GitHub, sans aucune donnée envoyée. Elle se
désactive en un clic depuis l'accueil (« Recherche de mises à jour au démarrage
· Désactiver »), et « Rechercher maintenant » lance une vérification à la
demande. Hors ligne, rien ne s'affiche.

Quand une version est disponible, un bandeau présente ses notes. Au clic sur
**Mettre à jour**, le fichier est téléchargé et sa **signature vérifiée** avant
toute installation :

- **version installée** : l'installateur s'exécute sans question, puis
  StreamPod redémarre ;
- **version portable** : `StreamPod.exe` est remplacé sur place (son nom est
  conservé) et relancé. L'ancienne version est gardée jusqu'à ce que la
  nouvelle démarre ; en cas d'échec, elle est remise en place.

Si la mise à jour échoue (dossier non modifiable, clé USB protégée, antivirus),
StreamPod l'explique et propose d'ouvrir la page de téléchargement pour
récupérer la nouvelle version à la main.

La mise à jour est refusée pendant une sauvegarde ou une restauration.

> [!NOTE]
> Les versions 0.2.0 et antérieures n'intègrent pas cette fonction : téléchargez
> la 0.3.0 une dernière fois à la main. Le réglage est mémorisé par PC : une
> version portable sur clé USB le retrouve activé sur chaque nouvelle machine.

## Le format `.obsbackup`

Un `.obsbackup` est une simple archive ZIP :

```
manifest.json      # version du format, version d'OBS, plugins, assets, polices
config/            # copie assainie de %APPDATA%\obs-studio
assets/<n>/        # fichiers médias référencés par les scènes
assets/d<n>/       # contenu des dossiers de diaporama ou de playlist
```

Les plugins tiers ne sont **pas** embarqués : leurs DLL ne seraient de toute
façon jamais réinstallées depuis l'archive, seule la liste du manifeste sert
(réinstallation manuelle). Les archives plus anciennes qui contiennent un
dossier `plugins/` restent restaurables : ces entrées sont simplement ignorées.

## Compiler depuis les sources

Prérequis : [Node.js](https://nodejs.org/), la [toolchain Rust](https://rustup.rs/)
et les [prérequis Tauri pour Windows](https://tauri.app/start/prerequisites/).

```bash
npm install
npm run tauri dev                    # lance l'app en mode développement
npm run tauri build -- --no-bundle   # exécutable seul (src-tauri/target/release)
```

`npm run tauri build` sans `--no-bundle` produit aussi l'installateur et sa
signature de mise à jour : il exige la clé privée de signature
(`TAURI_SIGNING_PRIVATE_KEY`), réservée aux releases officielles.

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
> d'environnement `STREAMPOD_CONFIG_DIR`, `STREAMPOD_INSTALL_DIR`, `STREAMPOD_ASSETS_DIR`,
> `STREAMPOD_PLUGINS_DIR`, `STREAMPOD_OBS_VERSION` et `STREAMPOD_POLICES` redirigent
> tous les chemins et détections vers des valeurs de test ;
> `STREAMPOD_UPDATER_DESACTIVE=1` coupe la recherche de mises à jour.

## Architecture

StreamPod est bâti sur **Tauri 2** (backend Rust) avec un frontend **Vite +
TypeScript** vanilla, en français.

| Fichier | Rôle |
|---|---|
| `src-tauri/src/obs.rs` | Détection d'OBS (config, installation, version, processus) |
| `src-tauri/src/sanitize.rs` | Suppression des secrets (clé de stream, tokens, cookies) |
| `src-tauri/src/scenes.rs` | Analyse des scènes (chemins, dossiers, polices, scripts) et réécriture |
| `src-tauri/src/backup.rs` | Pipeline de sauvegarde → `.obsbackup` |
| `src-tauri/src/restore.rs` | Restauration avec sauvegarde de secours et rollback automatique |
| `src-tauri/src/devices.rs` | Inventaire du matériel Windows (audio, vidéo) |
| `src-tauri/src/remap.rs` | Diagnostic et application du remappage matériel |
| `src-tauri/src/polices.rs` | Polices installées (DirectWrite) |
| `src-tauri/src/copies.rs` | Copies de sécurité : liste, retour, corbeille |
| `src-tauri/src/maj.rs` | Mise à jour : mode installé/portable, réglage, signature, remplacement de l'exe |
| `src-tauri/src/lib.rs` | Commandes exposées au frontend |
| `src/main.ts` | Toute la logique de l'interface |

## Notes de version

### 0.3.0

- **Mise à jour intégrée** : recherche au démarrage (désactivable), bandeau avec
  les notes de version, téléchargement signé puis installation en un clic -
  installateur relancé en mode passif, ou remplacement de l'exécutable
  portable avec retour arrière en cas d'échec.
- **Distribution** : installateur `StreamPod-setup.exe` en téléchargement
  principal, exécutable portable en alternative ; l'installateur MSI n'est
  plus publié.
- **Démarrage** : la fenêtre s'ouvre sur un fond sombre au lieu d'un écran
  blanc pendant le chargement.

### 0.2.0

- **Sécurité** : toutes les pistes de l'audit du 2026-09-18 sont corrigées.
  Scripts OBS désactivés à la restauration, programmes jamais embarqués,
  obs-websocket désactivé, serveur de diffusion affiché dans l'aperçu,
  fichiers de secrets exclus des assets, décompression bornée, noms de fichiers
  Windows piégés refusés, assets jamais écrasés, OBS revérifié avant la mise
  en place.
- **Complétude** : dossiers de diaporama et de playlist VLC embarqués, polices
  manquantes signalées, archive relue et contrôlée après l'écriture.
- **Ergonomie** : ouverture d'un `.obsbackup` par double-clic ou
  glisser-déposer, écran « Copies de sécurité » (retour à une configuration
  précédente, corbeille), résumés dépliables.
- **Compatibilité** : une sauvegarde créée par une version plus récente de
  StreamPod est refusée avec un message clair ; une sauvegarde d'un OBS plus
  récent que celui installé est signalée.
- **Outillage** : CI GitHub Actions et release automatisée avec empreintes
  SHA-256.

## Droits d'auteur

Copyright © 2026 MUDE. Tous droits réservés.

Le code source est rendu public uniquement à des fins de consultation. Aucune
licence open source n'est accordée. Toute réutilisation, modification,
redistribution ou commercialisation du code nécessite l'autorisation écrite
préalable de MUDE.
