# Étape 1 — Table de compatibilité des formats OBS (remappage matériel)

> Résultat de la vérification préalable (§8 et §13 du plan). Observations faites en
> **lecture seule** sur une config OBS Studio **32.1.2** réelle (Windows 11), croisées
> avec le code source d'OBS. Tous les exemples ci-dessous sont **fictifs et
> anonymisés** — aucun extrait de collection réelle n'est recopié.

## 1. Les trois familles du MVP — types et champs confirmés

| Famille | `id` OBS | Champ matériel | Format de la valeur |
|---|---|---|---|
| Entrée audio | `wasapi_input_capture` | `settings.device_id` | ID d'endpoint MMDevice `{0.0.1.00000000}.{guid}` ou `default` |
| Sortie audio | `wasapi_output_capture` | `settings.device_id` | ID d'endpoint MMDevice `{0.0.0.00000000}.{guid}` ou `default` |
| Vidéo | `dshow_input` | `settings.video_device_id` | `NomConvivial:CheminEncodé` (voir §4) |

- Sur OBS 32, `versioned_id` est présent et **identique** à `id` pour ces trois types
  (aucune variante versionnée n'existe pour eux). Recommandation : matcher sur `id`.
- Les sources vivent dans le tableau racine `sources[]` de chaque
  `basic/scenes/<collection>.json` — **et aussi** aux clés racine d'audio global (§2).
- `dshow_input` possède aussi `last_video_device_id` (même valeur que
  `video_device_id`, maintenu par OBS). À l'étape 4, réécrire **les deux champs à
  l'identique** pour éviter toute incohérence.
- `dshow_input` peut avoir un `audio_device_id` (audio custom de la webcam, observé au
  format `Nom encodé:` avec chemin vide). **Hors MVP** : ne pas le toucher.

## 2. Audio global (`Mic/Aux`, audio du bureau) — même schéma natif ✅

La condition du §4 du plan est **remplie** : l'audio global est stocké dans le **même
JSON de collection de scènes**, avec les **mêmes types et le même champ** que les
sources de scène. Il est inclus dans le MVP sans mécanisme supplémentaire.

- Clés racine de la collection : `DesktopAudioDevice1`, `DesktopAudioDevice2`
  (sorties, `wasapi_output_capture`) et `AuxAudioDevice1` … `AuxAudioDeviceN`
  (entrées, `wasapi_input_capture`). Observé : 2 + 2 ; OBS permet jusqu'à 4 Aux selon
  les versions → **parcourir par préfixe** (`DesktopAudioDevice*`, `AuxAudioDevice*`),
  ne pas coder en dur les indices.
- Un périphérique global désactivé dans OBS = **clé absente** du JSON.
- Chaque valeur est un objet source complet (`name`, `id`, `settings.device_id`,
  volume, `muted`, etc.) — seul `settings.device_id` est concerné par le remappage.

Exemple anonymisé :

```json
{
  "name": "Ma collection",
  "AuxAudioDevice1": {
    "id": "wasapi_input_capture",
    "name": "Micro",
    "settings": { "device_id": "{0.0.1.00000000}.{aaaaaaaa-1111-2222-3333-444444444444}" }
  },
  "DesktopAudioDevice1": {
    "id": "wasapi_output_capture",
    "name": "Audio du bureau",
    "settings": { "device_id": "default" }
  },
  "sources": [ "…sources de scènes, dont dshow_input…" ]
}
```

## 3. Valeurs spéciales documentées

- **`default`** est la seule valeur spéciale confirmée pour `wasapi_*` :
  « périphérique par défaut du système ». C'est la valeur d'une collection vierge.
  Référence **toujours valide**, jamais proposée au remappage.
- Il n'existe **pas** de valeur « par défaut » pour `dshow_input` (la vidéo pointe
  toujours vers un périphérique précis).
- Un périphérique **absent/débranché** n'a aucune représentation spéciale : le JSON
  conserve simplement l'ancien identifiant tel quel (c'est le cas que le remappage
  détecte).
- Hors MVP mais observé : `basic.ini` des profils contient
  `[Audio] MonitoringDeviceId` au même format MMDevice, avec `default` aussi. À
  laisser intact (le bilan final peut le mentionner comme « à vérifier dans OBS »).

## 4. Format des identifiants vidéo `dshow_input`

`video_device_id` = `NomConvivial:CheminInterface`, où **les deux parties** sont
passées par `encode_dstr` du plugin `win-dshow` d'OBS :

- `#` → `#22`
- `:` → `#3A`
- le `:` **non encodé** restant est l'unique séparateur nom/chemin.

Le chemin est le lien symbolique d'interface DirectShow du périphérique
(casse en minuscules), qui contient l'InstanceId PnP Windows. Exemple anonymisé :

```
Webcam Exemple C900:\\?\usb#22vid_1234&pid_5678&mi_00#227&abcdef1&0&0000#22{65e8773d-8f56-11d0-a3b9-00a0c9223196}\global
```

décodé → `\\?\usb#vid_1234&pid_5678&mi_00#7&abcdef1&0&0000#{65e8773d-…}\global`,
soit l'InstanceId `USB\VID_1234&PID_5678&MI_00\7&ABCDEF1&0&0000` + le GUID de
catégorie « capture vidéo » (`65e8773d-8f56-11d0-a3b9-00a0c9223196`).

Conséquences confirmées :

- la portion `7&abcdef1&0&0000` (instance) **change entre deux ports USB ou deux
  PC** → comparer sur l'identifiant complet pour la validité, mais suggérer par
  **nom convivial** (la partie avant le premier `:` non encodé, après décodage) ;
- pour construire l'inventaire cible, énumérer DirectShow
  (`CLSID_VideoInputDeviceCategory`, propriétés `FriendlyName` + `DevicePath` du
  moniker) puis appliquer le même encodage `nom:chemin` qu'OBS.

## 5. Correspondance avec les API Windows — vérifiée sur machine réelle

| Côté OBS | Côté Windows | Correspondance |
|---|---|---|
| `wasapi_*` → `settings.device_id` | `IMMDevice::GetId()` (MMDevice API, `eCapture`/`eRender`, `DEVICE_STATE_ACTIVE`) | **Identité exacte** (comparer sans tenir compte de la casse des GUID) |
| idem | `Win32_PnPEntity` classe `AudioEndpoint` : `PNPDeviceID = SWD\MMDEVAPI\<id>` | Id OBS = suffixe après `SWD\MMDEVAPI\` |
| `dshow_input` → `video_device_id` | Moniker DirectShow `FriendlyName` + `DevicePath` | Après encodage §4, identité exacte |

Les 4 périphériques audio globaux et les 2 périphériques vidéo de la machine
d'observation ont tous été retrouvés à l'identique — aucune divergence.

Note : `{0.0.0.00000000}.` = flux de **rendu** (sortie), `{0.0.1.00000000}.` = flux de
**capture** (entrée). Cela permet une vérification de famille indépendante de
l'énumération.

## 6. Différences entre versions d'OBS

- Encodage `encode_dstr` et clés `wasapi_*`/`dshow_input` : identiques sur la branche
  master d'`obsproject/obs-studio` (juillet 2026) et sur la 32.1.2 observée ; ces
  identifiants sont considérés comme stables (pas de variante versionnée connue).
- `versioned_id` existe depuis OBS 28 ; sur les versions plus anciennes il peut être
  absent → matcher sur `id` couvre tous les cas.
- OBS 32 ajoute une clé racine `canvases` dans les collections : sans impact, le
  parcours doit simplement ignorer les clés inconnues.
- Rappel produit : OwBS ne copie automatiquement les plugins qu'à version majeure
  identique ; la même prudence s'applique ici, mais les trois types du MVP sont
  stables sur toutes les versions visées.

## 7. Décisions pour l'étape 2

1. Parcourir dans chaque collection : `sources[]` **et** les clés racine
   `AuxAudioDevice*` / `DesktopAudioDevice*` (l'audio global est inclus au MVP).
2. Reconnaître uniquement `wasapi_input_capture`, `wasapi_output_capture`,
   `dshow_input` via `id` ; famille déduite du type (et recoupée par le préfixe
   `{0.0.X.…}` côté audio).
3. `default` (audio) = référence valide, jamais remappée.
4. Inventaire cible : MMDevice API pour l'audio, DirectShow pour la vidéo, avec
   ré-encodage `encode_dstr` côté vidéo pour comparer à l'identique.
5. Comparaisons d'identifiants insensibles à la casse.
6. Champs réécrits à l'étape 4 : `settings.device_id` (wasapi) ;
   `settings.video_device_id` **et** `settings.last_video_device_id` (dshow). Rien
   d'autre.

Sources code OBS consultées :
[win-dshow.cpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-dshow/win-dshow.cpp)
(construction `nom:chemin`) et
[encode-dstr.hpp](https://github.com/obsproject/obs-studio/blob/master/plugins/win-dshow/encode-dstr.hpp)
(`#`→`#22`, `:`→`#3A`).
