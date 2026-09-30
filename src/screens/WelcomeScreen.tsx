import {
  ArrowRight,
  CheckCircle,
  CircleSlash,
  Key,
  Monitor,
  Shield,
  Zap,
} from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import { Button } from '@components/ui/Button';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@components/ui/Card';
import { Badge } from '@components/ui/Badge';
import { listPairedDevices, getPlatform, getAppVersion } from '@lib/ipc';
import { useNavigate } from 'react-router-dom';

/**
 * The first screen, and the one that is easiest to get wrong.
 *
 * It used to promise things this build does not do: QR codes, capture of open
 * browser tabs, a "Development Status: Phase 0" badge that was three phases out
 * of date, and a tracking-board link to `href="#"`. A welcome screen is a
 * promise about what the app will do, and one that describes a different product
 * than the one installed is worse than no screen — a user pairs devices, expects
 * their tabs to arrive, and concludes the app is broken when it is working
 * exactly as built.
 *
 * So the claims here are drawn from the adapters that exist. The preflight
 * engine probes `node`, `pnpm`, `git`, `gh`, `aws`, `supabase` and `docker` by
 * running their version flags; the editor adapter locates a VS Code `code`
 * binary; the browser adapter detects an installed browser and knows the
 * platform's URL-opening command. There is no adapter that reads an open tab,
 * no camera access, and no relay.
 */
export function WelcomeScreen() {
  const navigate = useNavigate();
  const platform = useQuery({ queryKey: ['platform'], queryFn: getPlatform });
  const version = useQuery({ queryKey: ['app-version'], queryFn: getAppVersion });
  const paired = useQuery({ queryKey: ['paired-devices'], queryFn: listPairedDevices });

  const pairedCount = paired.data?.filter((d) => !d.revoked).length ?? 0;

  return (
    <div className="max-w-4xl mx-auto space-y-8">
      <div className="text-center space-y-6">
        <div className="w-20 h-20 rounded-2xl bg-primary/10 flex items-center justify-center mx-auto">
          <Monitor className="w-10 h-10 text-primary" />
        </div>
        <div>
          {/*
            Not an <h1>. The toolbar owns the page title on every route in this
            app, and a 36px hero word beneath a 13px toolbar title was the most
            web-page thing in the build. The app's name is in the window title
            and the sidebar; repeating it at display size adds nothing and
            competes with the one thing a visitor needs to read here.
          */}
          <p className="text-[15px] font-semibold text-foreground">StackHandoff</p>
          {/*
            The tagline. It sits directly under the name, in muted rather than
            full-strength text, so the pair reads as one lockup: what this is,
            then what it promises. Set at 15px to match the name — a tagline at
            the app's dense 13px default disappears into the toolbar title above
            it, and this is the one screen where being slightly larger than the
            rest of the app is the point rather than an accident.
          */}
          <p className="mt-1 text-[15px] text-muted-foreground">Pick up right where you left off.</p>
          {/*
            The lead paragraph. It deliberately does not restate the tagline —
            the tagline is the promise, this is the mechanism, and a first-run
            screen that says the same thing twice reads as marketing copy.
          */}
          <p className="mt-3 text-[15px] leading-relaxed text-foreground max-w-2xl mx-auto">
            Move the <em>intent</em> of a work session between your own machines. Capture what a
            project needs, check the other machine is ready, then rebuild it there — without
            carrying a single credential across.
          </p>
          <p className="mt-3 text-sm text-muted-foreground">
            v{version.data ?? '…'}
            {platform.data ? ` · ${platform.data}` : ''}
          </p>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-3">
        <Card>
          <CardHeader>
            <Shield className="w-10 h-10 text-primary mb-2" />
            <CardTitle>Direct, and authenticated</CardTitle>
            <CardDescription>
              Devices find each other on the local network over mDNS and talk over a Noise_IK
              session, so every byte is encrypted and both ends are authenticated. There is no
              server in the middle — and no account to create.
            </CardDescription>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader>
            <Key className="w-10 h-10 text-primary mb-2" />
            <CardTitle>Pairing you verify</CardTitle>
            <CardDescription>
              A device only becomes a destination after you compare a safety number with the
              person at the other machine, out loud. The number is derived from both Noise keys, so
              a machine that intercepts the connection cannot produce a matching one.
            </CardDescription>
          </CardHeader>
        </Card>
        <Card>
          <CardHeader>
            <Zap className="w-10 h-10 text-primary mb-2" />
            <CardTitle>Checked before anything happens</CardTitle>
            <CardDescription>
              The receiving machine runs the checks itself and tells you what is missing before any
              state-changing action runs. What it found is shown with the evidence, and anything it
              could not check is reported as unknown rather than assumed fine.
            </CardDescription>
          </CardHeader>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>What actually travels</CardTitle>
          <CardDescription>
            Work intent and reconstructable state. The second column is a list of things this
            build never reads, not merely declines to send.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-6 md:grid-cols-2">
            <div className="space-y-3">
              <h4 className="font-medium flex items-center gap-2">
                <CheckCircle className="w-5 h-5 text-success" />
                Captured
              </h4>
              <ul className="space-y-2 text-sm text-muted-foreground pl-4 list-disc">
                <li>Project paths, and per repository: branch, remote, and whether it is dirty</li>
                <li>Editor references — this build locates a VS Code installation and its CLI</li>
                <li>Browser URLs you paste in yourself</li>
                <li>Terminal working directories, and the commands the tool adapters suggest</li>
                <li>
                  Toolchain requirements declared by the project, such as a Node version range
                  read from <span className="font-mono">.nvmrc</span>
                </li>
                <li>Environment variable <em>names</em> only — presence, never values</li>
                <li>
                  Whether a provider CLI answers a version probe: <span className="font-mono">gh</span>
                  , <span className="font-mono">aws</span>,{' '}
                  <span className="font-mono">supabase</span>,{' '}
                  <span className="font-mono">docker</span>
                </li>
              </ul>
            </div>
            <div className="space-y-3">
              <h4 className="font-medium flex items-center gap-2">
                <CircleSlash className="w-5 h-5 text-danger" />
                Never read
              </h4>
              <ul className="space-y-2 text-sm text-muted-foreground pl-4 list-disc">
                <li>Passwords, tokens, cookies, passkeys, recovery codes</li>
                <li>Private keys, SSH agent state, and the contents of any .env file</li>
                <li>Browser tabs, history, profiles, or session databases</li>
                <li>Clipboard contents — there is no opt-in to accept yet</li>
                <li>The OS credential store, where this app keeps its own keys</li>
                <li>Terminal scrollback, shell history, or command output</li>
              </ul>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Where this build stands</CardTitle>
          <CardDescription>
            Stated as built, so nothing here has to be taken on trust later.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3 text-sm">
          <ul className="space-y-2 text-muted-foreground list-disc pl-4">
            <li>
              Built and run on <strong>macOS</strong>. The code has Windows and Linux paths in it,
              but neither has been exercised end to end, so this build does not claim them.
            </li>
            <li>
              Pairing is by <strong>pasted key or a pairing link</strong>. There is no camera
              access, so no QR image is generated.
            </li>
            <li>
              Transfers are <strong>device to device over the local network</strong>. There is no
              relay, so two machines on different networks cannot reach each other yet.
            </li>
            <li>
              The manifest is <strong>sealed with a local key</strong> before it is written to
              disk. The database that stores it is not itself encrypted.
            </li>
          </ul>
          <div className="flex flex-wrap gap-2 pt-1">
            <Badge variant="outline">macOS — running</Badge>
            <Badge variant="outline">Windows — unverified</Badge>
            <Badge variant="outline">Linux — not started</Badge>
            <Badge variant="outline">Relay — not started</Badge>
          </div>
        </CardContent>
      </Card>

      <div className="text-center pt-4 space-y-3">
        <Button size="lg" onClick={() => navigate('/devices')} className="w-full sm:w-auto">
          <ArrowRight className="w-4 h-4 mr-2" />
          {pairedCount === 0
            ? 'Pair your first device'
            : `Manage your ${pairedCount} paired device${pairedCount === 1 ? '' : 's'}`}
        </Button>
        {pairedCount === 0 && (
          <p className="text-sm text-muted-foreground">
            You need the app open on the other machine too, or its pairing code.
          </p>
        )}
        <p className="text-sm text-muted-foreground">
          <button
            onClick={() => navigate('/capture')}
            className="underline underline-offset-2 hover:text-foreground"
          >
            Or skip ahead and capture a workspace
          </button>{' '}
          — you can pair later.
        </p>
      </div>
    </div>
  );
}
