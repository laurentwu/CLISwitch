import { create } from "zustand";

export type Navigation = "configuration" | "providers" | "settings";

interface UiState {
  navigation: Navigation;
  configurationId: "current" | string;
  dirty: boolean;
  saveCurrent?: () => Promise<boolean>;
  setNavigation: (navigation: Navigation) => void;
  setConfigurationId: (id: "current" | string) => void;
  setDirty: (dirty: boolean) => void;
  setSaveCurrent: (saveCurrent?: () => Promise<boolean>) => void;
}

export const useUiStore = create<UiState>((set) => ({
  navigation: "configuration",
  configurationId: "current",
  dirty: false,
  setNavigation: (navigation) => set({ navigation }),
  setConfigurationId: (configurationId) => set({ configurationId }),
  setDirty: (dirty) => set({ dirty }),
  setSaveCurrent: (saveCurrent) => set({ saveCurrent }),
}));
