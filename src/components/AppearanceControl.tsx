import { Sun, Moon, Laptop } from 'lucide-react';
import { clsx } from 'clsx';
import { useThemeStore, type ThemeMode } from '../store/useThemeStore';

const OPTIONS: { mode: ThemeMode; label: string; icon: typeof Sun }[] = [
  { mode: 'light', label: 'Light', icon: Sun },
  { mode: 'dark', label: 'Dark', icon: Moon },
  { mode: 'system', label: 'Auto', icon: Laptop },
];

/**
 * The appearance control: Light / Dark / Auto.
 *
 * A three-way segmented control rather than a switch, for a reason that is about
 * correctness rather than taste. A switch is two-state, so "follow the system" —
 * the right default for a desktop app, and the only option that survives being
 * handed to someone else — would be unreachable once the user had touched it.
 * With three states the user can always get back to following the OS.
 *
 * It lives in its own file because it appears in two places: the sidebar, where
 * it is always one click away, and Settings, where it can be explained. Importing
 * it from `Layout` into a screen would make every screen depend on the app shell,
 * which is backwards.
 *
 * `Auto` says which mode it resolved to, because "Auto" while the window is
 * plainly dark is otherwise indistinguishable from a broken toggle.
 */
export function AppearanceControl() {
  const mode = useThemeStore((s) => s.mode);
  const resolved = useThemeStore((s) => s.resolved);
  const setMode = useThemeStore((s) => s.setMode);

  return (
    <div
      role="radiogroup"
      aria-label="Appearance"
      className="flex h-7 w-full items-center gap-0.5 rounded border border-border bg-surface p-0.5"
    >
      {OPTIONS.map(({ mode: value, label, icon: Icon }) => {
        const selected = mode === value;
        return (
          <button
            key={value}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => setMode(value)}
            title={
              value === 'system'
                ? `Follow the system appearance (currently ${resolved})`
                : `${label} appearance`
            }
            className={clsx(
              'flex h-5 flex-1 items-center justify-center gap-1 rounded-sm text-[11px] transition-colors',
              'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
              selected
                ? 'bg-surface-active font-medium text-foreground'
                : 'text-fg-subtle hover:text-foreground'
            )}
          >
            <Icon className="h-3 w-3" aria-hidden="true" />
            {label}
          </button>
        );
      })}
    </div>
  );
}
