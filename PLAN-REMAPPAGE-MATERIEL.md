# Plan — Remappage simple des périphériques OBS

> **Document de conception — aucune implémentation.** L'objectif est de corriger le
> principal problème d'une migration OBS sans transformer OwBS en moteur universel de
> détection matérielle.

## 1. Problème utilisateur

Après une restauration sur un autre PC, les scènes OBS sont présentes, mais certaines
sources ne fonctionnent plus : leur microphone, leur sortie audio ou leur webcam
pointent encore vers l'identifiant matériel de l'ancien ordinateur.

L'utilisateur doit alors ouvrir chaque source dans OBS et choisir manuellement un
nouveau périphérique. OwBS peut éviter cette étape répétitive puisqu'il manipule déjà
la configuration temporaire avant sa mise en place.

## 2. Solution retenue

Avant de commencer la restauration, OwBS repère les sources audio et vidéo connues
dont l'identifiant n'existe pas sur la nouvelle machine. Pour chacune, il demande
simplement à l'utilisateur quel périphérique compatible utiliser.

Exemple :

> **Confirmez la webcam à utiliser sur ce PC**  
> Périphérique précédent : `Logitech C920`  
> Utiliser : `[Logitech C922 ▼]`  
> `[Laisser cette source inchangée]`

OwBS place les noms les plus ressemblants en haut de la liste, mais ne choisit jamais
silencieusement à la place de l'utilisateur.

## 3. Fonctionnement

Le parcours reste court :

1. l'utilisateur sélectionne son fichier `.obsbackup` ;
2. l'aperçu lit uniquement les JSON nécessaires directement dans l'archive, sans
   extraire de configuration ni restaurer d'asset ;
3. OwBS inventorie les périphériques audio et vidéo présents sur le PC cible ;
4. les périphériques encore valides, dont le périphérique audio `default`, sont
   conservés sans intervention ;
5. pour chaque association à confirmer, OwBS affiche une liste compatible ;
6. l'utilisateur choisit un remplacement ou laisse la source inchangée ;
7. seulement après confirmation, la restauration extrait la configuration, restaure
   les assets et applique les choix dans `obs-studio.tmp-*` ;
8. OwBS effectue la bascule avec le rollback existant ;
9. le bilan final indique ce qui a été remappé et ce qui reste à vérifier dans OBS.

Si aucune association n'est nécessaire, aucun écran supplémentaire n'est affiché.
Une annulation ou une fermeture de l'application avant confirmation ne laisse donc ni
dossier temporaire ni assets restaurés.

## 4. Périmètre du MVP

La première version prend uniquement en charge les sources OBS natives suivantes :

| Famille | Source OBS concernée | Remplacement proposé |
|---|---|---|
| Entrée audio | Capture d'entrée audio | Microphones et entrées audio actives |
| Sortie audio | Capture de sortie audio | Sorties audio actives |
| Vidéo | Périphérique de capture vidéo | Webcams et périphériques vidéo compatibles |

Le MVP analyse les périphériques déclarés comme sources dans les collections de
scènes.

Les périphériques audio globaux (`Mic/Aux`, audio du bureau) sont inclus dans le MVP
**uniquement si** l'étape de vérification confirme qu'OBS les représente avec les
mêmes sources natives et les mêmes champs que ceux déjà pris en charge. Dans ce cas,
leur support ne nécessite pas de deuxième mécanisme.

S'ils sont stockés ailleurs ou demandent un parseur spécifique, ils restent inchangés
dans le MVP et le bilan final affiche clairement : « Les périphériques audio globaux
restent à vérifier dans OBS, dans Paramètres → Audio. »

## 5. Hors périmètre

Pour garder une fonctionnalité simple et fiable, la première version ne gère pas :

- les captures de moniteur ;
- les captures de fenêtre ou de jeu ;
- les sources fournies par des plugins tiers ;
- l'installation de pilotes ou de logiciels manquants ;
- la correction automatique des formats vidéo, résolutions, FPS ou autres réglages
  avancés ;
- un apprentissage automatique ou une base de périphériques en ligne.

Ces cas sont seulement laissés intacts. Ils pourront être étudiés plus tard si les
utilisateurs en ont réellement besoin. L'audio global suit la règle conditionnelle du
§4 : inclusion quasi gratuite avec le même schéma, sinon avertissement explicite.

## 6. Règles de sélection

### Compatibilité stricte

OwBS filtre les choix par famille :

- une entrée audio ne peut être remplacée que par une entrée audio ;
- une sortie audio ne peut être remplacée que par une sortie audio ;
- une webcam ou carte de capture ne peut être remplacée que par un périphérique
  vidéo.

Le backend vérifie cette compatibilité, même si une valeur incorrecte est envoyée par
l'interface.

### Périphérique audio par défaut

Une valeur OBS signifiant « périphérique par défaut du système », telle que `default`,
est une référence valide spéciale et non un identifiant matériel manquant. Elle est
conservée sans afficher l'écran de remappage.

La phase de vérification doit confirmer toutes les valeurs spéciales réellement
utilisées par les versions d'OBS prises en charge. Seules les valeurs documentées sont
traitées ainsi.

### Suggestion simple par nom

Il n'est pas nécessaire de construire un moteur complexe de score ou de confiance.
OwBS normalise les noms pour la comparaison — casse Unicode, ponctuation, espaces et
translittération légère des caractères de marque courants — puis place les candidats
les plus proches en haut de la liste.

La translittération doit rester petite, déterministe et couverte par des tests. Elle
sert notamment à rapprocher `RØDE NT-USB` de `Rode NT USB`, sans modifier les noms
originaux affichés ou écrits.

Exemples :

- `RØDE NT-USB` sera placé avant `Microphone de la webcam` pour remplacer
  `Rode NT USB` ;
- `Logitech C922` sera placé avant une carte de capture générique pour remplacer
  `Logitech C920`.

Cette similarité sert seulement à ordonner les choix. Elle ne déclenche jamais un
remplacement automatique.

### Regroupement

Si le même ancien identifiant apparaît dans plusieurs scènes, OwBS ne pose la
question qu'une fois. L'interface indique combien de sources seront concernées par le
choix.

## 7. Interface minimale

L'écran de remappage contient :

- un titre indiquant le nombre d'associations à confirmer ;
- une ligne par ancien identifiant unique ;
- le nom de la source ou quelques exemples de sources concernées ;
- une liste déroulante contenant uniquement les périphériques compatibles ;
- une option « Laisser inchangé » ;
- un bouton « Continuer la restauration ».

Exemple de contenu :

> **2 périphériques sont à confirmer sur ce PC**
>
> Micro principal — utilisé dans 3 scènes  
> Ancien périphérique : `RØDE NT-USB`  
> Nouveau périphérique : `[HyperX QuadCast ▼]`
>
> Webcam — utilisée dans 2 scènes  
> Ancien périphérique : `Logitech C920`  
> Nouveau périphérique : `[Logitech C922 ▼]`

Il n'est pas nécessaire d'afficher un score numérique, un pourcentage de confiance ou
une longue explication technique.

## 8. Vérification préalable indispensable

Avant d'implémenter la fonctionnalité, il faut confirmer sur des configurations OBS
réelles, en lecture seule :

- les identifiants exacts des trois types de sources du MVP ;
- les champs JSON qui contiennent l'identifiant matériel ;
- les différences éventuelles entre versions majeures d'OBS ;
- la correspondance entre les identifiants enregistrés par OBS et ceux obtenus par
  les API Windows ;
- l'emplacement réel de `Mic/Aux` et de l'audio du bureau selon les versions d'OBS ;
- la représentation OBS d'un périphérique « par défaut », désactivé ou absent ;
- les valeurs spéciales comme `default`, qui doivent rester valides sans correspondre
  à un périphérique physique ;
- le format des identifiants vidéo : ils peuvent combiner un nom convivial et un
  chemin matériel qui change entre deux ports USB ou deux PC.

Cette courte phase de recherche constitue le principal inconnu du chantier. Aucun
champ ne doit être réécrit sur la base d'une supposition ou d'une recherche générique
de clés comme `device_id`.

Le test ignoré `real_machine.rs` peut aider à observer ces structures en lecture
seule. Il ne doit toutefois jamais copier une collection réelle dans le dépôt : noms
de sources, URLs et identifiants peuvent être privés. Après observation, les fixtures
de test sont recréées à la main avec des valeurs entièrement fictives et minimales.

## 9. Architecture envisagée

La fonctionnalité peut rester divisée en quatre petites responsabilités :

### Inventaire Windows

Retourner les entrées audio, sorties audio et périphériques vidéo actifs. Cette couche
doit être remplaçable par un inventaire factice dans les tests.

### Analyse en lecture seule de l'archive

Lire en mémoire uniquement les JSON de collections nécessaires depuis l'archive,
avant toute extraction. Les chemins d'entrée sont validés et une limite de taille
raisonnable évite de charger un contenu anormal. L'analyse reconnaît uniquement les
types OBS explicitement pris en charge.

Pour chaque ancien identifiant, conserver :

- sa famille matérielle ;
- son nom convivial s'il existe ;
- les sources et collections qui l'utilisent ;
- le nombre d'occurrences.

La logique peut reprendre le principe de parcours de `scenes.rs`, mais doit rester
séparée de la réécriture générique des chemins d'assets. Le matériel dépend du type de
source et de champs précis.

### Décisions utilisateur

L'aperçu retourne les associations à confirmer et les candidats compatibles. Aucun
dossier `obs-studio.tmp-*` n'existe encore à ce stade. L'interface transmet ensuite
les choix explicites à la commande de restauration.

### Application des choix

Remplacer uniquement le champ matériel documenté des sources reconnues. Tous les
autres champs et toutes les sources inconnues restent inchangés.

## 10. Intégration à la restauration

Le parcours comporte deux phases sans état temporaire persistant entre elles.

**Phase d'aperçu, sans modification :**

1. valider l'archive et lire en mémoire les JSON nécessaires ;
2. inventorier les périphériques cibles ;
3. détecter les associations valides ou à confirmer ;
4. recueillir les choix de l'utilisateur.

**Phase de restauration, après confirmation :**

1. `restore_run` reçoit le chemin de l'archive et les choix explicites ;
2. le backend revalide l'archive, réénumère les périphériques et contrôle chaque
   association ;
3. il extrait la configuration dans `obs-studio.tmp-*` ;
4. il restaure les assets et réécrit leurs chemins ;
5. il applique les remplacements matériels dans la copie temporaire ;
6. il vérifie le nombre de remplacements ;
7. il lance `basculer_config` avec son rollback existant.

L'archive `.obsbackup` originale n'est jamais modifiée. Une annulation pendant
l'aperçu n'exige aucun nettoyage puisqu'aucune extraction ou copie d'asset n'a encore
eu lieu. La commande de restauration reste une opération complète allant de
l'extraction à la bascule, sans pause interactive au milieu.

Si un périphérique choisi est débranché entre l'analyse et la confirmation, OwBS
arrête la restauration avant la bascule et demande à l'utilisateur de recommencer le
choix. La configuration OBS actuellement installée reste intacte.

## 11. Sécurité et fiabilité

- OBS doit rester fermé pendant la restauration.
- Seuls les types de sources et champs en liste blanche sont analysés et modifiés.
- Une source inconnue ou provenant d'un plugin tiers reste strictement intacte.
- Les choix reçus de l'interface sont revalidés par le backend.
- L'aperçu ne lit que les entrées attendues, après validation de leur chemin et de
  leur taille.
- Aucune information matérielle n'est envoyée sur le réseau.
- Aucun inventaire matériel n'a besoin d'être ajouté à l'archive.
- Aucun rapport ne doit recopier le contenu complet des scènes ou des profils.
- Le remappage intervient avant la bascule et ne modifie pas la garantie de rollback.
- Une erreur de remappage ne doit jamais conduire à une configuration partiellement
  installée.

## 12. Tests nécessaires

### Tests unitaires

- reconnaître les trois familles du MVP ;
- ignorer une source inconnue contenant une clé au nom similaire ;
- regrouper plusieurs occurrences du même identifiant ;
- distinguer entrée audio, sortie audio et vidéo ;
- reconnaître `default` comme une référence audio valide qui ne nécessite aucun
  remappage ;
- détecter l'audio global s'il utilise le même schéma natif que les autres sources ;
- ordonner les noms ressemblants de façon déterministe ;
- rapprocher les variantes Unicode prévues, notamment `RØDE` et `Rode` ;
- proposer une webcam de même nom lorsque son chemin matériel a changé ;
- remplacer uniquement le champ prévu ;
- refuser une association entre familles incompatibles ;
- conserver tous les autres réglages de la source ;
- laisser inchangée une association ignorée par l'utilisateur.

### Test E2E en bac à sable

Ajouter à la configuration OBS factice :

- un ancien microphone ;
- une ancienne sortie audio ;
- une ancienne webcam ;
- une source inconnue représentant un plugin tiers ;
- un inventaire cible factice contenant des périphériques de remplacement ;
- une source audio utilisant la valeur spéciale `default`.

Le test vérifie que :

- les trois sources prises en charge sont détectées ;
- la source `default` reste valide et n'est pas proposée au remappage ;
- les choix explicites sont appliqués après restauration ;
- la source inconnue est inchangée ;
- l'archive originale est inchangée ;
- la configuration réelle de la machine n'est jamais consultée ;
- le rollback existant reste opérationnel ;
- les tests de non-fuite des secrets restent verts ;
- l'analyse seule, puis son annulation, ne crée ni `obs-studio.tmp-*` ni asset.

Le test réel ignoré peut vérifier l'énumération et observer les champs matériels
connus, mais doit rester strictement en lecture seule. Il ne produit ni fixture ni
copie brute d'une collection réelle.

## 13. Découpage du travail

### Étape 1 — Vérifier les formats OBS

- collecter quelques exemples minimaux et anonymisés ;
- confirmer les types et champs du MVP ;
- vérifier leur correspondance avec les API Windows ;
- vérifier où OBS stocke `Mic/Aux` et l'audio du bureau ;
- documenter `default` et le format des identifiants vidéo.

**Résultat :** une petite table de compatibilité fiable, sans modification de config.

### Étape 2 — Diagnostic en lecture seule

- énumérer les périphériques cibles ;
- analyser directement les scènes dans l'archive, sans extraction ;
- distinguer les références valides et absentes ;
- regrouper leurs occurrences.

**Résultat :** OwBS sait quelles associations sont à confirmer sans rien réécrire,
extraire ou copier.

### Étape 3 — Choix utilisateur

- ajouter l'écran minimal de listes déroulantes ;
- trier les choix par ressemblance de nom ;
- permettre « Laisser inchangé ».

**Résultat :** l'utilisateur peut préparer explicitement ses associations.

### Étape 4 — Réécriture et E2E

- valider les associations côté Rust ;
- les appliquer dans `obs-studio.tmp-*` ;
- vérifier les remplacements ;
- intégrer le bilan final ;
- couvrir le parcours complet par des tests en bac à sable.

**Résultat :** remappage audio et vidéo fonctionnel de bout en bout.

## 14. Critères d'acceptation

- Un microphone, une sortie audio ou une webcam absente est détecté avant la bascule.
- La valeur audio `default` est conservée sans question.
- L'audio global est pris en charge s'il utilise le même schéma natif ; sinon le bilan
  demande explicitement de le vérifier dans OBS.
- Un périphérique encore valide n'entraîne aucune question ni modification.
- La liste proposée contient uniquement des périphériques de la bonne famille.
- Les noms ressemblants apparaissent en premier sans être sélectionnés silencieusement.
- L'utilisateur peut choisir un remplacement ou laisser la source inchangée.
- Une seule décision suffit pour toutes les occurrences du même ancien identifiant.
- Seuls les champs documentés des sources OBS prises en charge sont modifiés.
- Les sources inconnues et les plugins tiers restent intacts.
- Le backend refuse une association invalide ou un périphérique devenu absent.
- L'archive originale n'est jamais modifiée.
- Annuler ou fermer l'application avant confirmation ne laisse ni dossier temporaire
  ni asset restauré.
- Toute erreur avant la bascule laisse la configuration actuelle intacte.
- Le rollback automatique reste couvert par les tests existants.
- `cd src-tauri && cargo test` passe sans utiliser les périphériques réels.
- Les tests de non-fuite des secrets restent verts.

## 15. Estimation

Cette version est une fonctionnalité de taille moyenne, mais son périmètre est
maîtrisé. Estimation indicative après validation des formats OBS :

| Travail | Estimation |
|---|---:|
| Recherche sur les identifiants OBS et Windows | 1 à 2 jours |
| Inventaire, analyse et réécriture Rust | 1 à 2 jours |
| Écran de sélection | 1 à 2 jours |
| Tests et cas d'erreur | 1 à 2 jours |

L'ordre de grandeur est donc d'environ **une semaine de travail concentré** pour un
MVP propre. Le principal risque d'estimation reste la correspondance entre les
identifiants stockés par OBS et ceux retournés par Windows.

Les moniteurs et les plugins tiers ne doivent pas être ajoutés au MVP « au passage » :
ils élargiraient sensiblement le chantier et réduiraient la fiabilité de cette
première version.

L'audio global est la seule exception conditionnelle : s'il repose sur le même schéma
natif, il est inclus avec la mécanique existante. S'il demande un nouveau parseur ou
un second parcours de configuration, il est reporté avec un avertissement clair au
lieu d'agrandir le MVP.
