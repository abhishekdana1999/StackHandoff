/**
 * State that has to survive a navigation.
 *
 * Scoped deliberately narrowly. Anything a screen can derive from the backend
 * goes through TanStack Query instead, so this store holds only what the backend
 * cannot be asked again for: the id of a capture in flight, the id of a restore
 * run, and the user's current capture selection.
 *
 * The capture selection is the reason this is a store and not component state.
 * It is built across three screens — the name is typed in Workspaces, the
 * selection in Capture, and the result is confirmed at the end — so lifting it
 * any higher would put a provider around the whole app for one object.
 *
 * Deliberately *not* persisted. A selection is a statement about what the user
 * is doing right now, and restoring a stale one after a restart would re-open
 * a capture the user believed they had finished.
 */

import { create } from 'zustand';
import type {
  CaptureSelection,
  PlanSummary,
  PreflightReport,
  RestoreExecutionReport,
  RestorePlan,
  TransferSession,
  WorkspaceManifest,
} from '../types';

/**
 * An empty selection, with every restrictive policy value already in place.
 *
 * The restrictive values are the floor, not a default the UI is expected to
 * raise: the backend forces `automaticCommandExecution` and `secretValuesIncluded`
 * to false and rejects a manifest claiming otherwise, so starting from anything
 * looser would produce a request the backend silently corrects. Starting strict
 * means the visible value and the effective value are the same.
 */
export function emptySelection(): CaptureSelection {
  return {
    projects: [],
    includeApplications: [],
    browserUrls: [],
    terminalDirs: [],
    terminalCommands: [],
    envVarNames: [],
    policy: {
      fileTransfer: 'none',
      clipboard: 'excluded',
      automaticCommandExecution: false,
      secretValuesIncluded: false,
    },
  };
}

interface CaptureDraft {
  name: string;
  selection: CaptureSelection;
}

interface AppState {
  // --- The capture the user is building -----------------------------------
  draft: CaptureDraft;
  setDraftName: (name: string) => void;
  clearDraft: () => void;

  // --- What a capture produced --------------------------------------------
  /** The id of the workspace just captured. Gates the preflight screen. */
  capturedWorkspaceId: string | null;
  manifest: WorkspaceManifest | null;
  captureWarnings: string[];
  setCapture: (args: {
    workspaceId: string;
    manifest: WorkspaceManifest;
    warnings: string[];
  }) => void;

  // --- A transfer in flight -----------------------------------------------
  transfer: TransferSession | null;
  setTransfer: (transfer: TransferSession | null) => void;

  // --- Preflight ----------------------------------------------------------
  preflight: PreflightReport | null;
  setPreflight: (report: PreflightReport | null) => void;

  // --- A restore run ------------------------------------------------------
  restoreRunId: string | null;
  restorePlan: RestorePlan | null;
  planSummary: PlanSummary | null;
  restoreReport: RestoreExecutionReport | null;
  setRestorePlan: (args: {
    runId: string;
    plan: RestorePlan;
    summary: PlanSummary | null;
  }) => void;
  setRestoreReport: (report: RestoreExecutionReport) => void;
  clearRestore: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  draft: { name: '', selection: emptySelection() },
  setDraftName: (name) => set((state) => ({ draft: { ...state.draft, name } })),
  clearDraft: () => set({ draft: { name: '', selection: emptySelection() } }),

  capturedWorkspaceId: null,
  manifest: null,
  captureWarnings: [],
  setCapture: ({ workspaceId, manifest, warnings }) =>
    set({ capturedWorkspaceId: workspaceId, manifest, captureWarnings: warnings }),

  transfer: null,
  setTransfer: (transfer) => set({ transfer }),

  preflight: null,
  setPreflight: (preflight) => set({ preflight }),

  restoreRunId: null,
  restorePlan: null,
  planSummary: null,
  restoreReport: null,
  setRestorePlan: ({ runId, plan, summary }) =>
    set({ restoreRunId: runId, restorePlan: plan, planSummary: summary, restoreReport: null }),
  setRestoreReport: (restoreReport) => set({ restoreReport }),
  clearRestore: () =>
    set({ restoreRunId: null, restorePlan: null, planSummary: null, restoreReport: null }),
}));
