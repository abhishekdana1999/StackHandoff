import React from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TauriProvider } from '@hooks/useTauri';
import { initTheme } from './store/useThemeStore';
import App from './App';
import './styles/index.css';

// Apply the stored theme before the first render, and keep following the OS
// while the mode is `system`. index.html has already set the class to avoid a
// flash; this is what keeps it correct afterwards.
initTheme();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5, // 5 minutes
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <TauriProvider>
          <App />
        </TauriProvider>
      </BrowserRouter>
    </QueryClientProvider>
  </React.StrictMode>
);