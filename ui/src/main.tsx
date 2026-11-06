import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "@fontsource-variable/inter";
import "@fontsource/jetbrains-mono/400.css";
// Colour emoji everywhere, bundled and split by unicode-range so only used glyphs load.
// macOS prefers Apple Color Emoji (earlier in the font stack).
import "@fontsource/noto-color-emoji";
import "leaflet/dist/leaflet.css";
import "./index.css";
import { native, errorText } from "./lib/bb";
import { AppProvider } from "./lib/app";
import { App } from "./App";

const queryClient = new QueryClient({
  defaultOptions: {
    // Server events keep caches fresh; no refetch storms on focus.
    queries: { staleTime: 60_000, refetchOnWindowFocus: false, retry: 1 },
  },
});

const root = createRoot(document.getElementById("root")!);

native
  .bootstrap()
  .then((boot) => {
    if (boot.dev_force_width) {
      // Developer aid: emulate a narrow window on a tiling WM.
      document.getElementById("root")!.style.maxWidth = `${boot.dev_force_width}px`;
    }
    root.render(
      <StrictMode>
        <QueryClientProvider client={queryClient}>
          <AppProvider boot={boot}>
            <App />
          </AppProvider>
        </QueryClientProvider>
      </StrictMode>,
    );
  })
  .catch((err) => {
    root.render(<pre style={{ color: "#e5787a", padding: 16 }}>BlackBubbles failed to start: {errorText(err)}</pre>);
  });
