import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

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
  missing_assets: string[];
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
}

interface RestorePreview {
  manifest: Manifest;
  backup_file_size: number;
  obs_installed: boolean;
  installed_version: string | null;
  config_exists: boolean;
  warnings: string[];
}

interface RestoreSummary {
  scene_collections: number;
  profiles: number;
  assets_restored: number;
  assets_dir: string | null;
  /** "manual" | "none" — les plugins ne sont jamais installés automatiquement. */
  plugins_status: string;
  plugins: string[];
  previous_config_backup: string | null;
  sources_remappees: number;
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
] as const;
type Screen = (typeof SCREENS)[number];

function show(screen: Screen) {
  for (const s of SCREENS) {
    $(`screen-${s}`).classList.toggle("hidden", s !== screen);
  }
  window.scrollTo({ top: 0, left: 0 });
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

function noteItem(text: string, kind: "warning" | "note"): HTMLElement {
  const el = document.createElement("div");
  el.className = kind === "warning" ? "warning-item" : "note-item";
  el.textContent = text;
  return el;
}

function listOrDash(items: string[], max = 4): string {
  if (items.length === 0) return "—";
  const shown = items.slice(0, max).join(", ");
  return items.length > max ? `${shown}… (+${items.length - max})` : shown;
}

/* ---------- Statut OBS (en-tête) ---------- */

async function refreshObsStatus(): Promise<ObsInfo | null> {
  const status = $("obs-status");
  try {
    const info = await invoke<ObsInfo>("detect_obs");
    if (info.running) {
      status.textContent = "⚠ OBS est ouvert — fermez-le avant toute opération";
      status.className = "obs-status warn";
    } else if (info.config_dir) {
      status.textContent = `✔ OBS ${info.version ?? ""} détecté`.trim();
      status.className = "obs-status ok";
    } else {
      status.textContent = "OBS non détecté sur cet ordinateur";
      status.className = "obs-status warn";
    }
    return info;
  } catch {
    status.textContent = "Impossible de détecter OBS";
    status.className = "obs-status warn";
    return null;
  }
}

/* ---------- Progression ---------- */

const STEP_LABELS: Record<string, string> = {
  scan: "Analyse",
  config: "Configuration",
  assets: "Assets",
  plugins: "Plugins",
  finalize: "Finalisation",
  extract: "Extraction",
  rewrite: "Mise à jour des scènes",
  swap: "Mise en place",
};

listen<Progress>("owbs://progress", (event) => {
  const p = event.payload;
  const percent = p.total > 0 ? Math.min(100, (p.current / p.total) * 100) : 0;
  $("progress-bar").style.width = `${percent}%`;
  const label = STEP_LABELS[p.step] ?? p.step;
  $("progress-message").textContent = `${label} — ${p.message}`;
});

/* ---------- Parcours : sauvegarde ---------- */

async function startBackupFlow() {
  hideError();
  try {
    const preview = await invoke<BackupPreview>("backup_preview");
    const summary = $("backup-summary");
    summary.replaceChildren(
      summaryRow("Configuration OBS", preview.config_dir),
      summaryRow("Version d'OBS", preview.obs_version ?? "inconnue"),
      summaryRow(
        `Collections de scènes (${preview.scene_collections.length})`,
        listOrDash(preview.scene_collections),
      ),
      summaryRow(`Profils (${preview.profiles.length})`, listOrDash(preview.profiles)),
      summaryRow(
        `Plugins tiers (${preview.plugins.length})`,
        listOrDash(preview.plugins.map((p) => p.name)),
      ),
      summaryRow(
        "Assets (images, vidéos, sons…)",
        `${preview.asset_count} fichier(s) — ${formatBytes(preview.asset_total_size)}`,
      ),
    );
    const warnings = $("backup-warnings");
    warnings.replaceChildren();
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
  const date = new Date().toISOString().slice(0, 10);
  const outputPath = await save({
    title: "Enregistrer la sauvegarde OBS",
    defaultPath: `OBS-${date}.obsbackup`,
    filters: [{ name: "Sauvegarde OBS", extensions: ["obsbackup"] }],
  });
  if (!outputPath) return;

  $("progress-title").textContent = "Sauvegarde en cours…";
  $("progress-bar").style.width = "0%";
  $("progress-message").textContent = "Préparation…";
  show("progress");

  try {
    const result = await invoke<BackupSummary>("backup_create", { outputPath });
    $("done-title").textContent = "Sauvegarde terminée !";
    $("done-details").replaceChildren(
      summaryRow("Fichier créé", result.output_path),
      summaryRow("Taille", formatBytes(result.file_size)),
      summaryRow("Contenu", `${result.scene_collections} collection(s) de scènes, ` +
        `${result.profiles} profil(s), ${result.plugins} plugin(s), ${result.assets} asset(s)`),
    );
    $("done-notes").replaceChildren(
      noteItem(
        "🔒 Votre clé de stream et vos comptes connectés n'ont pas été inclus dans ce fichier.",
        "note",
      ),
      noteItem(
        "Conservez ce fichier sur une clé USB ou un cloud : c'est tout ce qu'il faut pour " +
          "retrouver votre OBS ailleurs.",
        "note",
      ),
    );
    show("done");
  } catch (e) {
    show("home");
    showError(String(e));
  }
}

/* ---------- Parcours : restauration ---------- */

let selectedBackupPath: string | null = null;

async function startRestoreFlow() {
  hideError();
  const path = await open({
    title: "Choisir une sauvegarde OBS",
    multiple: false,
    filters: [{ name: "Sauvegarde OBS", extensions: ["obsbackup"] }],
  });
  if (!path || typeof path !== "string") return;
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
      summaryRow(
        `Collections de scènes (${m.scene_collections.length})`,
        listOrDash(m.scene_collections),
      ),
      summaryRow(`Profils (${m.profiles.length})`, listOrDash(m.profiles)),
      summaryRow(
        `Plugins (${m.plugins.length})`,
        listOrDash(m.plugins.map((p) => p.name)),
      ),
      summaryRow("Assets", `${m.assets.length} fichier(s)`),
      summaryRow("Taille du fichier", formatBytes(preview.backup_file_size)),
    );
    const warnings = $("restore-warnings");
    warnings.replaceChildren(...preview.warnings.map((w) => noteItem(w, "warning")));
    $<HTMLButtonElement>("btn-start-restore").disabled = !preview.obs_installed;
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
    keep.textContent = "Aucun périphérique compatible détecté — laisser inchangé";
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

  $("progress-title").textContent = "Restauration en cours…";
  $("progress-bar").style.width = "0%";
  $("progress-message").textContent = "Préparation…";
  show("progress");

  try {
    const result = await invoke<RestoreSummary>("restore_run", {
      backupPath: selectedBackupPath,
      choix: remapChoices,
    });
    $("done-title").textContent = "Restauration terminée !";
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
    if (result.sources_remappees > 0) {
      notes.append(
        noteItem(
          result.sources_remappees === 1
            ? "✔ 1 source utilise maintenant le périphérique choisi pour ce PC."
            : `✔ ${result.sources_remappees} sources utilisent maintenant les périphériques choisis pour ce PC.`,
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
    show("done");
  } catch (e) {
    show("home");
    showError(String(e));
  }
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

  document.querySelectorAll<HTMLElement>("[data-goto]").forEach((el) => {
    el.addEventListener("click", () => {
      hideError();
      show(el.dataset.goto as Screen);
      void refreshObsStatus();
    });
  });

  void refreshObsStatus();
  // Le statut OBS (ouvert/fermé) se rafraîchit périodiquement.
  setInterval(() => void refreshObsStatus(), 5000);
});
