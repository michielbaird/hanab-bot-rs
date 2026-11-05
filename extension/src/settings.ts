export interface Settings {
  /** Whether the robot is shown at all. */
  enabled: boolean;
  /** The convention the box starts on (the one used last). */
  convention: string;
  /** Gives each endgame search more time, for a device that is slower than a desktop. */
  slowDevice: boolean;
  /** Hides the suggestions and keeps the clear mistakes. */
  mistakesOnly: boolean;
  /** Whether the user has read that the extension is unofficial and the bot often wrong. */
  acknowledged: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  enabled: true,
  convention: "HGroup3",
  slowDevice: false,
  mistakesOnly: false,
  acknowledged: false,
};

const KEY = "settings";

function withDefaults(stored: unknown): Settings {
  return { ...DEFAULT_SETTINGS, ...(stored as Partial<Settings> | undefined) };
}

export async function loadSettings(): Promise<Settings> {
  const stored = await chrome.storage.local.get(KEY);
  return withDefaults(stored[KEY]);
}

export async function saveSettings(change: Partial<Settings>): Promise<void> {
  const current = await loadSettings();
  await chrome.storage.local.set({ [KEY]: { ...current, ...change } });
}

export function onSettingsChanged(listener: (settings: Settings) => void) {
  chrome.storage.onChanged.addListener((changes, area) => {
    const change = changes[KEY];
    if (area === "local" && change !== undefined) {
      listener(withDefaults(change.newValue));
    }
  });
}
