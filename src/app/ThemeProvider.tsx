import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type PropsWithChildren,
} from "react";

export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

type ThemeContextValue = {
  savedTheme: ThemePreference;
  resolvedTheme: ResolvedTheme;
  applySavedTheme: (theme: ThemePreference) => void;
};

const ThemeContext = createContext<ThemeContextValue>({
  savedTheme: "system",
  resolvedTheme: "light",
  applySavedTheme: () => undefined,
});
const SYSTEM_THEME_QUERY = "(prefers-color-scheme: dark)";

function systemTheme(): ResolvedTheme {
  return window.matchMedia(SYSTEM_THEME_QUERY).matches ? "dark" : "light";
}

export function ThemeProvider({ children }: PropsWithChildren) {
  const [savedTheme, setSavedTheme] = useState<ThemePreference>("system");
  const [systemResolvedTheme, setSystemResolvedTheme] = useState<ResolvedTheme>(systemTheme);
  const resolvedTheme = savedTheme === "system" ? systemResolvedTheme : savedTheme;

  const applySavedTheme = useCallback((theme: ThemePreference) => setSavedTheme(theme), []);

  useEffect(() => {
    const media = window.matchMedia(SYSTEM_THEME_QUERY);
    const update = () => setSystemResolvedTheme(media.matches ? "dark" : "light");
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", resolvedTheme === "dark");
    root.style.colorScheme = resolvedTheme;
    if (savedTheme === "system") delete root.dataset.theme;
    else root.dataset.theme = savedTheme;
  }, [resolvedTheme, savedTheme]);

  const value = useMemo(
    () => ({ savedTheme, resolvedTheme, applySavedTheme }),
    [applySavedTheme, resolvedTheme, savedTheme],
  );
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useAppTheme(): ThemeContextValue {
  return useContext(ThemeContext);
}
