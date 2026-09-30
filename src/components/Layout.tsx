import { ReactNode } from 'react';
import { Outlet, NavLink, useLocation, useNavigate } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  Monitor,
  FolderKanban,
  Settings as SettingsIcon,
  ChevronLeft,
  PanelLeftClose,
  PanelLeftOpen,
} from 'lucide-react';
import { clsx } from 'clsx';
import { getAppVersion, getDeviceFingerprint, listPairedDevices } from '../lib/ipc';
import { AppearanceControl } from './AppearanceControl';
import { useThemeStore } from '../store/useThemeStore';

const navigation = [
  { name: 'Workspaces', href: '/workspaces', icon: FolderKanban },
  { name: 'Devices', href: '/devices', icon: Monitor },
  { name: 'Settings', href: '/settings', icon: SettingsIcon },
];

/**
 * The three top-level sections, and nothing else.
 *
 * Everything else in the app is a step inside one of them, reached by a button
 * on that section's screen. Treating them as siblings of the sections would
 * mean a sidebar that changes shape as you move, which is a browser pattern.
 */
const SECTIONS = new Set(['/workspaces', '/devices', '/settings', '/welcome']);

function titleFor(path: string): string {
  if (path === '/devices') return 'Devices';
  if (path === '/settings') return 'Settings';
  if (path === '/welcome') return 'Welcome';
  if (path.startsWith('/capture')) return 'Capture Workspace';
  if (path.startsWith('/transfer')) return 'Send Workspace';
  if (path.startsWith('/preflight')) return 'Preflight Check';
  if (path.startsWith('/prepare')) return 'Prepare Destination';
  if (path.startsWith('/restore-preview')) return 'Restore Plan';
  if (path.startsWith('/restore-report')) return 'Restore Report';
  return 'Workspaces';
}

/**
 * The window's status bar.
 *
 * Every value here is real state the user would otherwise have to go looking
 * for: which key this machine is identified by, how many devices it trusts, and
 * what build is running. The previous version of this bar was a single hardcoded
 * "Local Mode" pill with a green dot that meant nothing — the listener is up in
 * exactly the same circumstances where that pill was not there at all.
 */
function StatusBar() {
  const { data: version } = useQuery({
    queryKey: ['app-version'],
    queryFn: getAppVersion,
    staleTime: Infinity,
    retry: false,
  });
  const { data: fingerprint } = useQuery({
    queryKey: ['device-fingerprint'],
    queryFn: getDeviceFingerprint,
    staleTime: Infinity,
    retry: false,
  });
  const { data: paired } = useQuery({
    queryKey: ['paired-count'],
    queryFn: listPairedDevices,
    retry: false,
  });

  // The fingerprint is a base64 public key. Shown shortened, because the full
  // 27 characters do not fit a status bar and a truncated-looking one invites
  // the reader to believe the key is shorter than it is.
  const short = fingerprint ? `${fingerprint.slice(0, 8)}…${fingerprint.slice(-4)}` : '—';

  return (
    <div className="statusbar font-mono" data-testid="status-bar">
      <span className="flex items-center gap-1.5" title={`This device: ${fingerprint ?? 'unknown'}`}>
        <span
          className={clsx(
            'h-1.5 w-1.5 rounded-full',
            fingerprint ? 'bg-success' : 'bg-neutral-border'
          )}
          aria-hidden="true"
        />
        {short}
      </span>
      <span className="text-fg-faint">·</span>
      <span data-testid="status-paired">
        {paired ? `${paired.length} paired` : '— paired'}
      </span>
      <span className="flex-1" />
      <span className="text-fg-faint">StackHandoff</span>
      <span className="text-fg-faint">·</span>
      <span data-testid="status-version">{version ? `v${version}` : '—'}</span>
    </div>
  );
}

export function Layout({ children }: { children?: ReactNode }) {
  const location = useLocation();
  const navigate = useNavigate();
  const isSection = SECTIONS.has(location.pathname);
  const collapsed = useThemeStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useThemeStore((s) => s.toggleSidebar);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-surface-canvas font-sans antialiased">
      <div className="flex min-h-0 flex-1">
        {/*
          The sidebar is always present. It used to be `hidden lg:flex`, which
          meant that in a window narrower than Tailwind's 1024px `lg` breakpoint
          — the shipped default was 800px — the entire navigation vanished and
          the hamburger that replaced it had no click handler at all. A desktop
          window is not a phone: it can be resized, but it should not lose its
          navigation to do it. It collapses to an icon rail instead.
        */}
        <aside
          data-testid="sidebar"
          data-collapsed={collapsed}
          className={clsx(
            'flex shrink-0 flex-col border-r border-border bg-surface-sunken',
            collapsed ? 'w-11' : 'w-52'
          )}
        >
          <div
            className={clsx(
              'flex h-10 shrink-0 items-center border-b border-border',
              collapsed ? 'justify-center px-1' : 'gap-2 px-3'
            )}
          >
            <div className="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-primary text-primary-foreground">
              <Monitor className="h-3.5 w-3.5" aria-hidden="true" />
            </div>
            {!collapsed && <span className="truncate text-[13px] font-semibold">StackHandoff</span>}
          </div>

          <nav className="flex-1 space-y-0.5 overflow-y-auto p-1.5 scrollbar-thin">
            {navigation.map((item) => (
              <NavLink
                key={item.href}
                to={item.href}
                title={item.name}
                className={({ isActive }) =>
                  clsx(
                    'flex h-7 items-center rounded text-[13px] transition-colors',
                    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                    collapsed ? 'justify-center px-0' : 'gap-2 px-2',
                    isActive
                      ? 'bg-surface-active font-medium text-foreground'
                      : 'text-fg-muted hover:bg-surface-hover hover:text-foreground'
                  )
                }
              >
                <item.icon className="h-4 w-4 shrink-0" aria-hidden="true" />
                {!collapsed && item.name}
              </NavLink>
            ))}
          </nav>

          <div className="shrink-0 space-y-2 border-t border-border p-1.5">
            {!collapsed && <AppearanceControl />}
            <button
              type="button"
              onClick={toggleSidebar}
              aria-label={collapsed ? 'Show sidebar' : 'Hide sidebar'}
              title={collapsed ? 'Show sidebar' : 'Hide sidebar'}
              className={clsx(
                'flex h-7 w-full items-center rounded text-[12px] text-fg-subtle transition-colors',
                'hover:bg-surface-hover hover:text-foreground',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring',
                collapsed ? 'justify-center' : 'gap-2 px-2'
              )}
            >
              {collapsed ? (
                <PanelLeftOpen className="h-4 w-4" aria-hidden="true" />
              ) : (
                <>
                  <PanelLeftClose className="h-4 w-4" aria-hidden="true" />
                  Hide sidebar
                </>
              )}
            </button>
          </div>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header className="flex h-10 shrink-0 items-center gap-2 border-b border-border bg-surface px-3">
            {/*
              Every screen other than the three sections is a step inside one,
              and the only way out of it used to be the sidebar — which is
              exactly the control you cannot find when the sidebar is hidden.
            */}
            {!isSection && (
              <button
                type="button"
                onClick={() => navigate('/workspaces')}
                aria-label="Back to workspaces"
                title="Back to workspaces"
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded text-fg-muted transition-colors hover:bg-surface-hover hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <ChevronLeft className="h-4 w-4" aria-hidden="true" />
              </button>
            )}
            {/*
              The page title lives in the toolbar, on every route, and it is the
              only <h1> the app renders.

              This is where a desktop window puts it: pinned in the chrome,
              present at every scroll position, next to the back button on a
              step. The alternative — a heading in the scrollable content — is a
              web layout. It also duplicated: the toolbar title and each screen's
              own <h1> were the same words 20px apart on all nine routes, and on
              /capture and /restore-report they matched character for character,
              so no amount of styling the difference away would have helped.

              Screens keep their explanatory line under where the heading was, so
              the page still says what it is for.
            */}
            <h1
              className="truncate text-[13px] font-semibold"
              data-testid="toolbar-title"
            >
              {titleFor(location.pathname)}
            </h1>
          </header>

          <main className="min-h-0 flex-1 overflow-y-auto scrollbar-thin">
            <div className="mx-auto max-w-4xl p-4">{children ?? <Outlet />}</div>
          </main>
        </div>
      </div>

      <StatusBar />
    </div>
  );
}
