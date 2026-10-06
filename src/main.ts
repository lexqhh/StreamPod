import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";

/* ---------- Types (miroir des structs Rust) ---------- */

interface ObsInfo {
  config_dir: string | null;
  install_dir: string | null;
  version: string | null;
  running: boolean;
}

interface PluginInfo {
  name: string;
  dll: string;
  size: number;
  has_data_dir: boolean;
}

interface BackupPreview {
  config_dir: string;
  obs_version: string | null;
  scene_collections: string[];
  profiles: string[];
  plugins: PluginInfo[];
  asset_count: number;
  asset_total_size: number;
  asset_dirs: number;
  assets: { chemin: string; taille: number }[];
  fonts: string[];
  missing_assets: string[];
  excluded_assets: string[];
  browser_sources: number;
}

interface BackupSummary {
  output_path: string;
  file_size: number;
  scene_collections: number;
  profiles: number;
  plugins: number;
  assets: number;
}

interface Manifest {
  created_at: string;
  obs_version: string | null;
  scene_collections: string[];
  profiles: string[];
  plugins: PluginInfo[];
  assets: { file_name: string; size: number }[];
  asset_dirs: { original_path: string; archive_dir: string }[];
  fonts: string[];
}

interface ServiceProfil {
  profil: string;
  type_service: string;
  service: string | null;
  serveur: string | null;
}

interface RestorePreview {
  manifest: Manifest;
  backup_file_size: number;
  obs_installed: boolean;
  installed_version: string | null;
  config_exists: boolean;
  services: ServiceProfil[];
  missing_fonts: string[];
  warnings: string[];
}

interface RestoreSummary {
  scene_collections: number;
  profiles: number;
  assets_restored: number;
  assets_dir: string | null;
  /** "manual" | "none" - les plugins ne sont jamais installés automatiquement. */
  plugins_status: string;
  plugins: string[];
  previous_config_backup: string | null;
  sources_remappees: number;
  scripts: string[];
}

type Famille = "entree_audio" | "sortie_audio" | "video";

interface Peripherique {
  famille: Famille;
  id: string;
  nom: string;
}

interface SourceConcernee {
  collection: string;
  source: string;
}

interface Association {
  famille: Famille;
  ancien_id: string;
  ancien_nom: string;
  sources: SourceConcernee[];
  occurrences: number;
  candidats: Peripherique[];
}

interface RemapReport {
  a_confirmer: Association[];
  references_valides: number;
  peripheriques: Peripherique[];
}

/** Choix explicite de l'utilisateur pour un ancien identifiant. */
interface RemapChoice {
  famille: Famille;
  ancien_id: string;
  nouveau_id: string;
}

interface CopieSecurite {
  chemin: string;
  nom: string;
  date: string | null;
  taille: number;
}

interface InfosMaj {
  version: string;
  notes: string | null;
  mode: "installe" | "portable";
}

interface ReglageMaj {
  verifier_au_demarrage: boolean;
}

interface Progress {
  step: string;
  message: string;
  current: number;
  total: number;
}

/* ---------- Utilitaires DOM ---------- */

const $ = <T extends HTMLElement = HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

const SCREENS = [
  "home",
  "backup-preview",
  "restore-preview",
  "remap",
  "progress",
  "done",
  "backups",
] as const;
type Screen = (typeof SCREENS)[number];

/** Écran affiché : le verrou OBS a besoin de savoir où poser son motif, et
 *  l'état n'était jusqu'ici lisible que via la classe `hidden` du DOM. */
let ecranCourant: Screen = "home";

function show(screen: Screen) {
  ecranCourant = screen;
  for (const s of SCREENS) {
    $(`screen-${s}`).classList.toggle("hidden", s !== screen);
  }
  // Le motif dépend de l'écran, et les écrans de résumé reconstruisent leurs
  // avertissements juste avant d'appeler show() : on le repose ici.
  appliquerVerrouObs();
  // Jamais de mise à jour pendant une opération (refusée aussi côté Rust).
  $<HTMLButtonElement>("btn-maj-installer").disabled = screen === "progress";
  window.scrollTo({ top: 0, left: 0 });
  // Accessibilité : replacer le focus sur le titre du nouvel écran, sinon
  // il reste sur un élément passé en display:none (clavier/lecteur d'écran perdus).
  const titre = $(`screen-${screen}`).querySelector<HTMLElement>("h2");
  if (titre) {
    titre.tabIndex = -1;
    titre.focus({ preventScroll: true });
  }
}

function showError(message: string) {
  $("error-text").textContent = message;
  $("error-banner").classList.remove("hidden");
}

function hideError() {
  $("error-banner").classList.add("hidden");
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} o`;
  const units = ["Ko", "Mo", "Go", "To"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

function summaryRow(label: string, value: string): HTMLElement {
  const row = document.createElement("div");
  row.className = "summary-row";
  const l = document.createElement("span");
  l.className = "label";
  l.textContent = label;
  const v = document.createElement("span");
  v.className = "value";
  v.textContent = value;
  row.append(l, v);
  return row;
}

/** Panneau d'avertissement ou de note. La marque de préfixe est un élément
 *  distinct : elle seule porte la couleur d'état (charte, § 11). */
function noteItem(text: string, kind: "warning" | "note"): HTMLElement {
  const el = document.createElement("div");
  el.className = kind === "warning" ? "warning-item" : "note-item";
  const mark = document.createElement("span");
  mark.className = "mark";
  mark.setAttribute("aria-hidden", "true");
  mark.textContent = kind === "warning" ? "!" : "✓";
  const body = document.createElement("span");
  body.textContent = text;
  el.append(mark, body);
  return el;
}

/** Ligne de résumé dépliable : libellé et aperçu restent visibles, la liste
 *  complète (avec les tailles le cas échéant) s'ouvre au clic. */
function detailsRow(label: string, items: { nom: string; detail?: string }[]): HTMLElement {
  if (items.length === 0) return summaryRow(label, "-");
  const details = document.createElement("details");
  details.className = "summary-details";
  const resume = document.createElement("summary");
  resume.className = "summary-row";
  const l = document.createElement("span");
  l.className = "label";
  l.textContent = label;
  const v = document.createElement("span");
  v.className = "value";
  v.textContent = listOrDash(items.map((i) => i.nom));
  resume.append(l, v);
  const liste = document.createElement("ul");
  liste.className = "details-list";
  for (const item of items) {
    const li = document.createElement("li");
    const nom = document.createElement("span");
    nom.className = "details-name";
    nom.textContent = item.nom;
    li.append(nom);
    if (item.detail) {
      const detail = document.createElement("span");
      detail.className = "details-meta";
      detail.textContent = item.detail;
      li.append(detail);
    }
    liste.append(li);
  }
  details.append(resume, liste);
  return details;
}

const noms = (items: string[]) => items.map((nom) => ({ nom }));

function listOrDash(items: string[], max = 4): string {
  if (items.length === 0) return "-";
  const shown = items.slice(0, max).join(", ");
  return items.length > max ? `${shown}… (+${items.length - max})` : shown;
}

/* ---------- Statut OBS (en-tête) ---------- */

const MSG_OBS_OUVERT =
  "OBS est en cours d'exécution. Fermez OBS puis réessayez.";

/** Dernier statut connu, rafraîchi toutes les 5 s : c'est lui qui commande
 *  l'activation des commandes, pas seulement le texte d'en-tête. */
let obsRunning = false;
/** Mémorisé depuis l'aperçu de restauration, pour que le sondage puisse
 *  recalculer l'état de « Restaurer maintenant » sans écraser cette règle. */
let restoreObsInstalled = true;

/** Pose ou retire le motif du verrou en tête d'un conteneur d'avertissements.
 *  Le motif est cloné depuis l'accueil : un seul texte à maintenir, dans
 *  index.html. Le marqueur `data-obs-lock` évite les doublons au fil du sondage
 *  et permet de le retirer sans toucher aux avertissements métier. */
function majMotifVerrou(idConteneur: string, visible: boolean) {
  const conteneur = $(idConteneur);
  const existant = conteneur.querySelector("[data-obs-lock]");
  if (visible && !existant) {
    const modele = $("home-obs-lock").querySelector(".warning-item");
    if (!modele) return;
    const motif = modele.cloneNode(true) as HTMLElement;
    motif.dataset.obsLock = "";
    conteneur.prepend(motif);
  } else if (!visible && existant) {
    existant.remove();
  }
}

/** Verrouille tout ce qui mène à une écriture tant qu'OBS est ouvert : refuser
 *  au dernier moment ferait perdre à l'utilisateur ses choix de remappage. */
function appliquerVerrouObs() {
  for (const id of ["card-backup", "card-restore"]) {
    const carte = $<HTMLButtonElement>(id);
    carte.disabled = obsRunning;
    // La classe double l'attribut : sous Blink, basculer `disabled` en JS ne
    // fait pas recalculer le style des descendants de la carte (voir le
    // commentaire de .card.verrouillee dans styles.css).
    carte.classList.toggle("verrouillee", obsRunning);
  }
  $<HTMLButtonElement>("btn-start-backup").disabled = obsRunning;
  $<HTMLButtonElement>("btn-start-restore").disabled =
    obsRunning || !restoreObsInstalled;
  $<HTMLButtonElement>("btn-continue-restore").disabled = obsRunning;
  document
    .querySelectorAll<HTMLButtonElement>(".btn-revenir")
    .forEach((b) => (b.disabled = obsRunning));
  // Le bouton Annuler de l'écran de progression n'est jamais verrouillé ici :
  // il reste utilisable même si OBS est lancé pendant une opération.
  // Le motif suit l'utilisateur : encart de l'accueil, ou note en tête des
  // avertissements de l'écran de résumé, sinon le bouton grisé reste inexpliqué.
  $("home-obs-lock").classList.toggle("hidden", !obsRunning || ecranCourant !== "home");
  majMotifVerrou("backup-warnings", obsRunning && ecranCourant === "backup-preview");
  majMotifVerrou("restore-warnings", obsRunning && ecranCourant === "restore-preview");
  // L'écran de remappage n'est atteignable qu'OBS fermé, mais OBS peut être
  // lancé pendant que l'utilisateur y confirme ses périphériques.
  majMotifVerrou("remap-warnings", obsRunning && ecranCourant === "remap");
  majMotifVerrou("backups-warnings", obsRunning && ecranCourant === "backups");
}

async function refreshObsStatus(): Promise<ObsInfo | null> {
  const status = $("obs-status");
  try {
    const info = await invoke<ObsInfo>("detect_obs");
    if (info.running) {
      status.textContent = "! OBS est ouvert - fermez-le avant toute opération";
      status.className = "obs-status warn";
    } else if (info.config_dir) {
      status.textContent = `✓ OBS ${info.version ?? ""} détecté`.trim();
      status.className = "obs-status ok";
    } else {
      status.textContent = "OBS non détecté sur cet ordinateur";
      status.className = "obs-status warn";
    }
    obsRunning = info.running;
    appliquerVerrouObs();
    return info;
  } catch {
    status.textContent = "Impossible de détecter OBS";
    status.className = "obs-status warn";
    // Détection en échec : ne pas verrouiller l'app sur un doute, les gardes
    // côté Rust restent le dernier rempart.
    obsRunning = false;
    appliquerVerrouObs();
    return null;
  }
}

/** Statut frais juste avant d'agir : le sondage laisse une fenêtre de 5 s
 *  pendant laquelle OBS a pu être lancé. Affiche l'erreur le cas échéant. */
async function obsBloqueLOperation(): Promise<boolean> {
  const info = await refreshObsStatus();
  if (info?.running) {
    showError(MSG_OBS_OUVERT);
    return true;
  }
  return false;
}

/* ---------- Progression ---------- */

// Doit rester identique à backup::MSG_ANNULATION côté Rust : c'est ainsi
// qu'une annulation volontaire est distinguée d'une vraie erreur.
const MSG_ANNULATION = "Opération annulée.";

let annulationDemandee = false;

/** Réinitialise l'écran de progression, bouton Annuler compris. */
function resetProgress(title: string) {
  annulationDemandee = false;
  $("progress-title").textContent = title;
  $("progress-bar").style.width = "0%";
  $("progress-message").textContent = "Préparation…";
  const btn = $<HTMLButtonElement>("btn-cancel-operation");
  btn.disabled = false;
  btn.classList.remove("hidden");
}

const STEP_LABELS: Record<string, string> = {
  scan: "Analyse",
  config: "Configuration",
  assets: "Assets",
  plugins: "Plugins",
  finalize: "Finalisation",
  verify: "Vérification",
  extract: "Extraction",
  rewrite: "Mise à jour des scènes",
  remap: "Périphériques",
  swap: "Mise en place",
  warning: "Avertissement",
  maj: "Mise à jour",
};

listen<Progress>("streampod://progress", (event) => {
  const p = event.payload;
  const percent = p.total > 0 ? Math.min(100, (p.current / p.total) * 100) : 0;
  $("progress-bar").style.width = `${percent}%`;
  // La bascule de configuration est le point de non-retour : l'annulation
  // serait ignorée côté Rust, on retire donc le bouton.
  if (p.step === "swap") {
    $("btn-cancel-operation").classList.add("hidden");
  }
  if (annulationDemandee) {
    return; // Ne pas écraser « Annulation en cours… ».
  }
  const label = STEP_LABELS[p.step] ?? p.step;
  $("progress-message").textContent = `${label} - ${p.message}`;
});

function demanderAnnulation() {
  annulationDemandee = true;
  const btn = $<HTMLButtonElement>("btn-cancel-operation");
  btn.disabled = true;
  $("progress-message").textContent = "Annulation en cours…";
  void invoke("cancel_operation");
}

/* ---------- Parcours : sauvegarde ---------- */

async function startBackupFlow() {
  hideError();
  if (await obsBloqueLOperation()) return;
  try {
    const preview = await invoke<BackupPreview>("backup_preview");
    const summary = $("backup-summary");
    summary.replaceChildren(
      summaryRow("Configuration OBS", preview.config_dir),
      summaryRow("Version d'OBS", preview.obs_version ?? "inconnue"),
      ...(preview.fonts.length > 0
        ? [detailsRow(`Polices utilisées (${preview.fonts.length})`, noms(preview.fonts))]
        : []),
      detailsRow(
        `Collections de scènes (${preview.scene_collections.length})`,
        noms(preview.scene_collections),
      ),
      detailsRow(`Profils (${preview.profiles.length})`, noms(preview.profiles)),
      detailsRow(
        `Plugins tiers (${preview.plugins.length})`,
        preview.plugins.map((p) => ({ nom: p.name, detail: formatBytes(p.size) })),
      ),
      detailsRow(
        `Fichiers d'assets (${preview.assets.length})`,
        preview.assets.map((a) => ({ nom: a.chemin, detail: formatBytes(a.taille) })),
      ),
      summaryRow(
        "Assets (images, vidéos, sons…)",
        `${preview.asset_count} fichier(s)` +
          (preview.asset_dirs > 0
            ? `, dont ${preview.asset_dirs} dossier(s) de diaporama ou de playlist`
            : "") +
          ` - ${formatBytes(preview.asset_total_size)}`,
      ),
    );
    const warnings = $("backup-warnings");
    warnings.replaceChildren();
    if (preview.browser_sources > 0) {
      warnings.append(
        noteItem(
          `${preview.browser_sources} source(s) navigateur (overlays StreamElements, ` +
            `Streamlabs…) seront sauvegardées avec leur URL, qui peut contenir un ` +
            `token privé. Ne partagez cette sauvegarde qu'avec des personnes de confiance.`,
          "warning",
        ),
      );
    }
    if (preview.excluded_assets.length > 0) {
      warnings.append(
        noteItem(
          `Par sécurité, ${preview.excluded_assets.length} fichier(s) référencé(s) par vos ` +
            `scènes ne seront pas inclus (programmes, fichiers de configuration ou de ` +
            `secrets) : ` +
            listOrDash(preview.excluded_assets, 3),
          "warning",
        ),
      );
    }
    if (preview.missing_assets.length > 0) {
      warnings.append(
        noteItem(
          `${preview.missing_assets.length} fichier(s) référencé(s) par vos scènes sont ` +
            `introuvables sur le disque et ne seront pas inclus : ` +
            listOrDash(preview.missing_assets, 3),
          "warning",
        ),
      );
    }
    show("backup-preview");
  } catch (e) {
    showError(String(e));
  }
}

async function runBackup() {
  hideError();
  if (await obsBloqueLOperation()) return;
  const date = new Date().toISOString().slice(0, 10);
  const outputPath = await save({
    title: "Enregistrer la sauvegarde OBS",
    defaultPath: `OBS-${date}.obsbackup`,
    filters: [{ name: "Sauvegarde OBS", extensions: ["obsbackup"] }],
  });
  if (!outputPath) return;

  resetProgress("Sauvegarde en cours…");
  show("progress");

  try {
    const result = await invoke<BackupSummary>("backup_create", { outputPath });
    $("done-title").textContent = "Sauvegarde terminée !";
    $("done-details").replaceChildren(
      summaryRow("Fichier créé", result.output_path),
      summaryRow("Taille", formatBytes(result.file_size)),
      summaryRow("Contenu", `${result.scene_collections} collection(s) de scènes, ` +
        `${result.profiles} profil(s), ${result.plugins} plugin(s), ${result.assets} asset(s)`),
    );
    $("done-notes").replaceChildren(
      noteItem(
        "Votre clé de stream et vos comptes connectés n'ont pas été inclus dans ce fichier.",
        "note",
      ),
      noteItem(
        "Conservez ce fichier sur une clé USB ou un cloud : c'est tout ce qu'il faut pour " +
          "retrouver votre OBS ailleurs.",
        "note",
      ),
    );
    setRevealTarget({ path: result.output_path, kind: "file" });
    show("done");
  } catch (e) {
    show("home");
    // Annulation volontaire : retour à l'accueil sans bandeau d'erreur.
    if (String(e) !== MSG_ANNULATION) {
      showError(String(e));
    }
  }
}

/* ---------- Écran « Terminé » : ouverture dans l'explorateur ---------- */

// L'écran « Terminé » est partagé par les deux parcours et sa barre d'actions
// n'est pas reconstruite : la cible doit être redéfinie à chaque passage, sinon
// le fichier d'une sauvegarde resterait ouvrable après une restauration.
type RevealTarget = { path: string; kind: "file" | "dir" } | null;

let revealTarget: RevealTarget = null;

function setRevealTarget(target: RevealTarget) {
  revealTarget = target;
  $("btn-reveal").classList.toggle("hidden", target === null);
}

/* ---------- Parcours : restauration ---------- */

let selectedBackupPath: string | null = null;

async function startRestoreFlow() {
  hideError();
  if (await obsBloqueLOperation()) return;
  const path = await open({
    title: "Choisir une sauvegarde OBS",
    multiple: false,
    filters: [{ name: "Sauvegarde OBS", extensions: ["obsbackup"] }],
  });
  if (!path || typeof path !== "string") return;
  await ouvrirSauvegarde(path);
}

/** Aperçu de restauration d'une sauvegarde : choisie dans la boîte de
 *  dialogue, ouverte par double-clic ou déposée sur la fenêtre. */
async function ouvrirSauvegarde(path: string) {
  hideError();
  if (await obsBloqueLOperation()) return;
  selectedBackupPath = path;

  try {
    const preview = await invoke<RestorePreview>("restore_preview", {
      backupPath: path,
    });
    const m = preview.manifest;
    const created = m.created_at ? new Date(m.created_at).toLocaleString("fr-FR") : "?";
    const summary = $("restore-summary");
    summary.replaceChildren(
      summaryRow("Sauvegarde", path),
      summaryRow("Créée le", created),
      summaryRow("Version d'OBS d'origine", m.obs_version ?? "inconnue"),
      detailsRow(
        `Collections de scènes (${m.scene_collections.length})`,
        noms(m.scene_collections),
      ),
      detailsRow(`Profils (${m.profiles.length})`, noms(m.profiles)),
      detailsRow(
        `Plugins (${m.plugins.length})`,
        m.plugins.map((p) => ({ nom: p.name, detail: formatBytes(p.size) })),
      ),
      detailsRow(
        `Assets (${m.assets.length})`,
        m.assets.map((a) => ({ nom: a.file_name, detail: formatBytes(a.size) })),
      ),
      summaryRow("Taille du fichier", formatBytes(preview.backup_file_size)),
    );
    for (const s of preview.services) {
      summary.append(
        summaryRow(
          `Diffusion du profil ${s.profil}`,
          [s.service, s.serveur].filter(Boolean).join(" · ") || s.type_service || "-",
        ),
      );
    }
    const warnings = $("restore-warnings");
    warnings.replaceChildren(...preview.warnings.map((w) => noteItem(w, "warning")));
    restoreObsInstalled = preview.obs_installed;
    appliquerVerrouObs();
    show("restore-preview");
  } catch (e) {
    showError(String(e));
  }
}

/* ---------- Remappage des périphériques ---------- */

const FAMILLE_LABELS: Record<Famille, string> = {
  entree_audio: "Entrée audio",
  sortie_audio: "Sortie audio",
  video: "Vidéo / webcam",
};

let currentRemapReport: RemapReport | null = null;
let remapChoices: RemapChoice[] = [];

/** Avant de restaurer : diagnostic en lecture seule des périphériques.
 *  S'il n'y a rien à confirmer, la restauration démarre directement. */
async function prepareRestore() {
  if (!selectedBackupPath) return;
  hideError();
  // Avant le diagnostic : inutile de faire confirmer des périphériques si la
  // restauration sera refusée à l'arrivée.
  if (await obsBloqueLOperation()) return;
  currentRemapReport = null;
  remapChoices = [];

  let report: RemapReport | null = null;
  try {
    report = await invoke<RemapReport>("remap_preview", {
      backupPath: selectedBackupPath,
    });
  } catch (e) {
    showError(
      `L'analyse des périphériques a échoué. La restauration n'a pas démarré : ` +
        `réessayez avant de continuer. (${String(e)})`,
    );
    return;
  }

  if (!report) {
    showError("L'analyse des périphériques n'a retourné aucun résultat.");
    return;
  }
  if (report.a_confirmer.length === 0) {
    await runRestore();
    return;
  }
  currentRemapReport = report;
  renderRemapScreen(report);
  show("remap");
}

function remapItem(assoc: Association, index: number): HTMLElement {
  const item = document.createElement("div");
  item.className = "remap-item";

  const header = document.createElement("div");
  header.className = "remap-item-header";
  const title = document.createElement("span");
  title.className = "remap-item-title";
  title.textContent = assoc.ancien_nom;
  const badge = document.createElement("span");
  badge.className = "remap-badge";
  badge.textContent = FAMILLE_LABELS[assoc.famille];
  header.append(title, badge);

  const usage = document.createElement("p");
  usage.className = "remap-usage";
  const exemples = [...new Set(assoc.sources.map((s) => s.source))];
  usage.textContent =
    (assoc.occurrences === 1
      ? "Utilisé par 1 source : "
      : `Utilisé par ${assoc.occurrences} sources : `) + listOrDash(exemples, 3);

  const choice = document.createElement("div");
  choice.className = "remap-choice";
  const label = document.createElement("label");
  label.className = "label";
  label.textContent = "Utiliser sur ce PC :";
  label.htmlFor = `remap-select-${index}`;
  const select = document.createElement("select");
  select.className = "remap-select";
  select.id = `remap-select-${index}`;
  select.dataset.index = String(index);
  const keep = document.createElement("option");
  keep.value = "";
  keep.textContent = "Laisser cette source inchangée";
  select.append(keep);
  for (const c of assoc.candidats) {
    const opt = document.createElement("option");
    opt.value = c.id;
    opt.textContent = c.nom;
    select.append(opt);
  }
  if (assoc.candidats.length === 0) {
    keep.textContent = "Aucun périphérique compatible détecté - laisser inchangé";
    select.disabled = true;
  }
  choice.append(label, select);

  item.append(header, usage, choice);
  return item;
}

function renderRemapScreen(report: RemapReport) {
  const n = report.a_confirmer.length;
  $("remap-title").textContent =
    n === 1
      ? "1 périphérique est à confirmer sur ce PC"
      : `${n} périphériques sont à confirmer sur ce PC`;
  $("remap-list").replaceChildren(...report.a_confirmer.map(remapItem));
}

/** Lit les listes déroulantes : seuls les remplacements explicitement
 *  choisis sont retenus, « Laisser inchangé » ne produit aucun choix. */
function collectRemapChoices(): RemapChoice[] {
  const report = currentRemapReport;
  if (!report) return [];
  const choices: RemapChoice[] = [];
  document
    .querySelectorAll<HTMLSelectElement>("#remap-list select")
    .forEach((sel) => {
      const assoc = report.a_confirmer[Number(sel.dataset.index)];
      if (assoc && sel.value) {
        choices.push({
          famille: assoc.famille,
          ancien_id: assoc.ancien_id,
          nouveau_id: sel.value,
        });
      }
    });
  return choices;
}

async function runRestore() {
  if (!selectedBackupPath) return;
  if (await obsBloqueLOperation()) return;

  resetProgress("Restauration en cours…");
  show("progress");

  try {
    const result = await invoke<RestoreSummary>("restore_run", {
      backupPath: selectedBackupPath,
      choix: remapChoices,
    });
    $("done-title").textContent = "Restauration terminée !";
    const details = $("done-details");
    details.replaceChildren(
      summaryRow(
        "Restauré",
        `${result.scene_collections} collection(s) de scènes, ${result.profiles} profil(s), ` +
          `${result.assets_restored} asset(s)`,
      ),
    );
    if (result.assets_dir) {
      details.append(summaryRow("Assets déposés dans", result.assets_dir));
    }
    if (result.previous_config_backup) {
      details.append(
        summaryRow("Ancienne configuration conservée dans", result.previous_config_backup),
      );
    }

    const notes = $("done-notes");
    notes.replaceChildren();
    if (result.plugins.length > 0) {
      notes.append(
        noteItem(
          `Par sécurité, les plugins ne sont jamais installés automatiquement. ` +
            `Réinstallez-les depuis obsproject.com/forum/list/plugins.35 : ` +
            result.plugins.join(", "),
          "warning",
        ),
      );
    }
    if (result.scripts.length > 0) {
      notes.append(
        noteItem(
          `Par sécurité, les scripts ne sont jamais réactivés automatiquement. ` +
            `Scripts à réactiver dans Outils → Scripts : ` +
            result.scripts.join(" · "),
          "warning",
        ),
      );
    }
    if (result.sources_remappees > 0) {
      notes.append(
        noteItem(
          result.sources_remappees === 1
            ? "1 source utilise maintenant le périphérique choisi pour ce PC."
            : `${result.sources_remappees} sources utilisent maintenant les périphériques choisis pour ce PC.`,
          "note",
        ),
      );
    }
    // Périphériques laissés inchangés à l'écran de remappage : leur ancien
    // identifiant n'existe pas sur ce PC, ils restent à régler dans OBS.
    const inchanges = (currentRemapReport?.a_confirmer ?? []).filter(
      (a) =>
        !remapChoices.some(
          (c) => c.famille === a.famille && c.ancien_id === a.ancien_id,
        ),
    );
    if (inchanges.length > 0) {
      notes.append(
        noteItem(
          `Restent à vérifier dans OBS (sources laissées inchangées) : ` +
            inchanges.map((a) => a.ancien_nom).join(", ") +
            `. Choisissez un périphérique dans les propriétés de ces sources.`,
          "warning",
        ),
      );
    }
    notes.append(
      noteItem(
        "Dernière étape : ouvrez OBS et reconnectez votre compte ou re-saisissez votre clé " +
          "de stream (Paramètres → Flux). Elle n'est jamais enregistrée dans la sauvegarde.",
        "warning",
      ),
    );
    setRevealTarget(
      result.assets_dir ? { path: result.assets_dir, kind: "dir" } : null,
    );
    show("done");
  } catch (e) {
    show("home");
    // Annulation volontaire : retour à l'accueil sans bandeau d'erreur.
    if (String(e) !== MSG_ANNULATION) {
      showError(String(e));
    }
  }
}

/* ---------- Copies de sécurité ---------- */

function dateCopie(c: CopieSecurite): string {
  return c.date ? new Date(c.date).toLocaleString("fr-FR") : c.nom;
}

function backupItem(c: CopieSecurite): HTMLElement {
  const item = document.createElement("div");
  item.className = "remap-item";
  const header = document.createElement("div");
  header.className = "remap-item-header";
  const titre = document.createElement("span");
  titre.className = "remap-item-title";
  titre.textContent = dateCopie(c);
  const taille = document.createElement("span");
  taille.className = "remap-badge";
  taille.textContent = formatBytes(c.taille);
  header.append(titre, taille);
  const chemin = document.createElement("p");
  chemin.className = "remap-usage";
  chemin.textContent = c.chemin;
  const actions = document.createElement("div");
  actions.className = "backup-actions";
  const revenir = document.createElement("button");
  revenir.type = "button";
  revenir.className = "btn secondary btn-revenir";
  revenir.textContent = "Revenir à cette configuration";
  revenir.disabled = obsRunning;
  revenir.addEventListener("click", () => void revenirACopie(c));
  const jeter = document.createElement("button");
  jeter.type = "button";
  jeter.className = "btn secondary";
  jeter.textContent = "Mettre à la corbeille";
  jeter.addEventListener("click", () => void jeterCopie(c));
  actions.append(revenir, jeter);
  item.append(header, chemin, actions);
  return item;
}

async function afficherCopies(note?: HTMLElement) {
  hideError();
  try {
    const copies = await invoke<CopieSecurite[]>("lister_copies_securite");
    const liste = $("backups-list");
    if (copies.length === 0) {
      const vide = document.createElement("p");
      vide.className = "remap-intro";
      vide.textContent = "Aucune copie de sécurité sur cet ordinateur pour l'instant.";
      liste.replaceChildren(vide);
    } else {
      liste.replaceChildren(...copies.map(backupItem));
    }
    $("backups-warnings").replaceChildren(...(note ? [note] : []));
    show("backups");
  } catch (e) {
    showError(String(e));
  }
}

async function revenirACopie(c: CopieSecurite) {
  hideError();
  if (await obsBloqueLOperation()) return;
  const confirme = await ask(
    `Votre configuration OBS actuelle sera remplacée par celle du ${dateCopie(c)}. ` +
      `Elle sera elle-même conservée comme copie de sécurité.`,
    {
      title: "Revenir à cette configuration ?",
      kind: "warning",
      okLabel: "Revenir",
      cancelLabel: "Annuler",
    },
  );
  if (!confirme) return;
  try {
    const ancienne = await invoke<string | null>("revenir_a_copie", { chemin: c.chemin });
    await afficherCopies(
      noteItem(
        `Configuration du ${dateCopie(c)} remise en place.` +
          (ancienne ? ` L'ancienne configuration est conservée dans ${ancienne}.` : ""),
        "note",
      ),
    );
  } catch (e) {
    showError(String(e));
  }
}

async function jeterCopie(c: CopieSecurite) {
  hideError();
  const confirme = await ask(
    `La copie du ${dateCopie(c)} (${formatBytes(c.taille)}) sera placée dans la corbeille ` +
      `Windows, d'où vous pourrez encore la récupérer.`,
    {
      title: "Mettre cette copie à la corbeille ?",
      kind: "warning",
      okLabel: "Mettre à la corbeille",
      cancelLabel: "Annuler",
    },
  );
  if (!confirme) return;
  try {
    await invoke("jeter_copie", { chemin: c.chemin });
    await afficherCopies(noteItem(`Copie du ${dateCopie(c)} placée dans la corbeille.`, "note"));
  } catch (e) {
    showError(String(e));
  }
}

/* ---------- Ouverture directe : double-clic, glisser-déposer ---------- */

/** Pendant une opération, une nouvelle sauvegarde ne doit rien interrompre. */
const operationEnCours = () => ecranCourant === "progress";

function deposerFichier(chemin: string | undefined) {
  if (!chemin || operationEnCours()) return;
  if (!chemin.toLowerCase().endsWith(".obsbackup")) {
    showError(
      "Ce fichier n'est pas une sauvegarde StreamPod : déposez un fichier .obsbackup.",
    );
    return;
  }
  void ouvrirSauvegarde(chemin);
}

function ecouterGlisserDeposer() {
  const voile = $("drop-overlay");
  void getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "enter" || p.type === "over") {
      voile.classList.toggle("hidden", operationEnCours());
    } else {
      voile.classList.add("hidden");
      if (p.type === "drop") deposerFichier(p.paths[0]);
    }
  });
}

/* ---------- Mise à jour ---------- */

// Début de maj::MSG_LECTURE_SEULE côté Rust : repli vers la page de la release.
const MSG_LECTURE_SEULE = "Le dossier de StreamPod n'est pas accessible en écriture";

let reglageMaj: ReglageMaj = { verifier_au_demarrage: true };

function afficherEtatMaj(texte?: string, succes = false) {
  const actif = reglageMaj.verifier_au_demarrage;
  $("maj-etat-texte").classList.toggle("succes", succes);
  $("maj-etat-texte").textContent =
    texte ??
    `Recherche de mises à jour au démarrage ${actif ? "activée" : "désactivée"}`;
  $("btn-maj-basculer").textContent = actif ? "Désactiver" : "Activer";
}

function afficherBanniereMaj(infos: InfosMaj) {
  $("maj-titre").textContent = `Version ${infos.version} disponible`;
  $("maj-notes").textContent = (infos.notes ?? "").replace(/\*\*/g, "").trim();
  $("btn-maj-installer").classList.remove("hidden");
  $("btn-maj-release").classList.add("hidden");
  $("banniere-maj").classList.remove("hidden");
}

/** Au démarrage (`manuel` faux), un échec réseau reste silencieux. */
async function rechercherMaj(manuel: boolean) {
  const btn = $<HTMLButtonElement>("btn-maj-rechercher");
  btn.disabled = true;
  if (manuel) afficherEtatMaj("Recherche en cours…");
  try {
    const infos = await invoke<InfosMaj | null>("verifier_mise_a_jour", { manuel });
    if (infos) {
      afficherBanniereMaj(infos);
      afficherEtatMaj();
    } else {
      afficherEtatMaj(manuel ? "✓ StreamPod est à jour" : undefined, manuel);
    }
  } catch (e) {
    afficherEtatMaj();
    showError(String(e));
  } finally {
    btn.disabled = false;
  }
}

async function basculerReglageMaj() {
  const reglage = { verifier_au_demarrage: !reglageMaj.verifier_au_demarrage };
  try {
    await invoke("ecrire_reglage_maj", { reglage });
    reglageMaj = reglage;
    afficherEtatMaj();
  } catch (e) {
    showError(String(e));
  }
}

async function installerMaj() {
  if (operationEnCours()) return;
  hideError();
  resetProgress("Mise à jour en cours…");
  $("btn-cancel-operation").classList.add("hidden");
  $("banniere-maj").classList.add("hidden");
  show("progress");
  try {
    // En cas de succès, StreamPod se ferme et la nouvelle version démarre.
    await invoke("appliquer_mise_a_jour");
  } catch (e) {
    const message = String(e);
    show("home");
    showError(message);
    if (message.startsWith(MSG_LECTURE_SEULE)) {
      $("btn-maj-installer").classList.add("hidden");
      $("btn-maj-release").classList.remove("hidden");
      $("banniere-maj").classList.remove("hidden");
    }
  }
}

async function initialiserMaj() {
  try {
    reglageMaj = await invoke<ReglageMaj>("lire_reglage_maj");
  } catch {
    // Réglage illisible : valeur par défaut (activé).
  }
  afficherEtatMaj();
  if (reglageMaj.verifier_au_demarrage) void rechercherMaj(false);
}

/* ---------- Câblage ---------- */

window.addEventListener("DOMContentLoaded", () => {
  $("card-backup").addEventListener("click", startBackupFlow);
  $("card-restore").addEventListener("click", startRestoreFlow);
  $("btn-start-backup").addEventListener("click", runBackup);
  $("btn-start-restore").addEventListener("click", prepareRestore);
  $("btn-continue-restore").addEventListener("click", () => {
    remapChoices = collectRemapChoices();
    void runRestore();
  });
  $("error-close").addEventListener("click", hideError);
  $("btn-open-backups").addEventListener("click", () => void afficherCopies());
  $("btn-cancel-operation").addEventListener("click", demanderAnnulation);
  $("btn-maj-installer").addEventListener("click", () => void installerMaj());
  $("btn-maj-plus-tard").addEventListener("click", () =>
    $("banniere-maj").classList.add("hidden"),
  );
  $("btn-maj-release").addEventListener("click", () => {
    invoke("ouvrir_page_release").catch((e) => showError(String(e)));
  });
  $("btn-maj-basculer").addEventListener("click", () => void basculerReglageMaj());
  $("btn-maj-rechercher").addEventListener("click", () => void rechercherMaj(true));
  $("btn-reveal").addEventListener("click", () => {
    if (!revealTarget) return;
    const { path, kind } = revealTarget;
    // Un dossier s'ouvre directement, un fichier est sélectionné dans son dossier.
    const ouverture = kind === "dir" ? openPath(path) : revealItemInDir(path);
    // Le chemin peut avoir disparu entre-temps : on prévient sans casser l'écran.
    ouverture.catch((e) => showError(String(e)));
  });

  document.querySelectorAll<HTMLElement>("[data-goto]").forEach((el) => {
    el.addEventListener("click", () => {
      hideError();
      show(el.dataset.goto as Screen);
      void refreshObsStatus();
    });
  });

  void refreshObsStatus();
  void initialiserMaj();
  ecouterGlisserDeposer();
  // Double-clic sur un .obsbackup : au lancement, ou renvoyé par une 2e
  // instance vers cette fenêtre (plugin single-instance).
  void invoke<string | null>("fichier_au_lancement").then((chemin) => {
    if (chemin) void ouvrirSauvegarde(chemin);
  });
  void listen<string>("streampod://ouvrir", (event) => {
    if (!operationEnCours()) void ouvrirSauvegarde(event.payload);
  });
  // Le statut OBS (ouvert/fermé) se rafraîchit périodiquement, sauf pendant une
  // sauvegarde ou une restauration : le bandeau est alors masqué par l'écran de
  // progression, et le backend a déjà vérifié qu'OBS était fermé au démarrage.
  setInterval(() => {
    if (!$("screen-progress").classList.contains("hidden")) {
      return;
    }
    void refreshObsStatus();
  }, 5000);
});
