import { Routes, Route, Navigate } from 'react-router-dom';
import { useTauri } from '@hooks/useTauri';
import { Layout } from '@components/Layout';
import { WelcomeScreen } from '@screens/WelcomeScreen';
import { DevicesScreen } from '@screens/DevicesScreen';
import { WorkspacesScreen } from '@screens/WorkspacesScreen';
import { CaptureScreen } from '@screens/CaptureScreen';
import { TransferScreen } from '@screens/TransferScreen';
import { PreflightScreen } from '@screens/PreflightScreen';
import { PrepareScreen } from '@screens/PrepareScreen';
import { RestorePreviewScreen } from '@screens/RestorePreviewScreen';
import { RestoreReportScreen } from '@screens/RestoreReportScreen';
import { SettingsScreen } from '@screens/SettingsScreen';
import { LoadingScreen } from '@components/LoadingScreen';

function AppRoutes() {
  const { isReady } = useTauri();

  if (!isReady) {
    return <LoadingScreen />;
  }

  return (
    <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<Navigate to="/workspaces" replace />} />
        <Route path="welcome" element={<WelcomeScreen />} />
        <Route path="devices" element={<DevicesScreen />} />
        <Route path="workspaces" element={<WorkspacesScreen />} />
        <Route path="capture" element={<CaptureScreen />} />
        <Route path="transfer/:workspaceId" element={<TransferScreen />} />
        {/*
          The param is the *workspace* id, not a transfer id.

          It was called `:transferId` while the screens read
          `useParams<{ workspaceId }>()`, which types to `string` and so compiled
          cleanly while resolving to `undefined` at runtime -- every one of these
          three screens would have opened showing "the workspace could not be
          read". TypeScript cannot catch a mismatch between a route pattern and
          a param name; only a test that loads each route can. See
          `src/test/routes.test.tsx`.

          Preflight, prepare and restore-preview all run against a workspace's
          requirements and plan, not against a transfer record: the transfer is
          complete by the time any of them is reachable, and on the sending
          machine none of them apply at all.
        */}
        <Route path="preflight/:workspaceId" element={<PreflightScreen />} />
        <Route path="prepare/:workspaceId" element={<PrepareScreen />} />
        <Route path="restore-preview/:workspaceId" element={<RestorePreviewScreen />} />
        <Route path="restore-report/:restoreRunId" element={<RestoreReportScreen />} />
        <Route path="settings" element={<SettingsScreen />} />
      </Route>
      <Route path="*" element={<Navigate to="/workspaces" replace />} />
    </Routes>
  );
}

function App() {
  return <AppRoutes />;
}

export default App;