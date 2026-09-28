import { createContext, useContext, useEffect, useState, ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

interface TauriContextValue {
  isTauri: boolean;
  isReady: boolean;
  platform: 'windows' | 'macos' | 'linux' | 'unknown';
  appVersion: string;
  window: ReturnType<typeof getCurrentWindow> | null;
}

const TauriContext = createContext<TauriContextValue | null>(null);

export function TauriProvider({ children }: { children: ReactNode }) {
  const [isTauri, setIsTauri] = useState(false);
  const [isReady, setIsReady] = useState(false);
  const [platform, setPlatform] = useState<'windows' | 'macos' | 'linux' | 'unknown'>('unknown');
  const [appVersion, setAppVersion] = useState('0.1.0');
  const [window, setWindow] = useState<ReturnType<typeof getCurrentWindow> | null>(null);

  useEffect(() => {
    const checkTauri = async () => {
      try {
        // Check if we're running in Tauri
        const result = await invoke<string>('get_app_version');
        setAppVersion(result);
        setIsTauri(true);

        // Get platform
        const platformResult = await invoke<string>('get_platform');
        setPlatform(platformResult as 'windows' | 'macos' | 'linux');

        // Get window reference
        const win = getCurrentWindow();
        setWindow(win);
      } catch {
        // Not running in Tauri (e.g., during development in browser)
        setIsTauri(false);
        setPlatform('unknown');
      } finally {
        setIsReady(true);
      }
    };

    checkTauri();
  }, []);

  return (
    <TauriContext.Provider value={{ isTauri, isReady, platform, appVersion, window }}>
      {children}
    </TauriContext.Provider>
  );
}

export function useTauri() {
  const context = useContext(TauriContext);
  if (!context) {
    throw new Error('useTauri must be used within a TauriProvider');
  }
  return context;
}