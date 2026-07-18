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
  plugins_compatible: boolean;
  config_exists: boolean;
  warnings: string[];
}

interface RestoreSummary {
  scene_collections: number;
  profiles: number;
  assets_restored: number;
  assets_dir: string | null;
  plugins_status: string;
  plugins: string[];
  previous_config_backup: string | null;
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
  "progress",
  "done",
] as const;
type Screen = (typeof SCREENS)[number];

function show(screen: Screen) {
  for (const s of SCREENS) {
    $(`screen-${s}`).classList.toggle("hidden", s !== screen);
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

async function runRestore() {
  if (!selectedBackupPath) return;
  hideError();

  $("progress-title").textContent = "Restauration en cours…";
  $("progress-bar").style.width = "0%";
  $("progress-message").textContent = "Préparation…";
  show("progress");

  try {
    const result = await invoke<RestoreSummary>("restore_run", {
      backupPath: selectedBackupPath,
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
      if (result.plugins_status === "copied" || result.plugins_status === "copied_elevated") {
        notes.append(
          noteItem(`✔ ${result.plugins.length} plugin(s) installé(s) : ${result.plugins.join(", ")}`, "note"),
        );
      } else {
        notes.append(
          noteItem(
            `Les plugins suivants n'ont pas pu être installés automatiquement — ` +
              `réinstallez-les depuis obsproject.com/forum/list/plugins.35 : ` +
              result.plugins.join(", "),
            "warning",
          ),
        );
      }
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
  $("btn-start-restore").addEventListener("click", runRestore);
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
