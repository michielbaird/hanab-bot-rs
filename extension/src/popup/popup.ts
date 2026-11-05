// The toolbar popup: the extension's settings.

import { CONVENTIONS } from "../conventions";
import { PRIVACY_PAGE, getReportUrl } from "../links";
import { DEFAULT_SETTINGS, loadSettings, saveSettings } from "../settings";
import { DISCLAIMER } from "../text";

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error(`Missing #${id}`);
  }
  return element as T;
}

const enabled = byId<HTMLInputElement>("enabled");
const slowDevice = byId<HTMLInputElement>("slowDevice");
const mistakesOnly = byId<HTMLInputElement>("mistakesOnly");
const convention = byId<HTMLSelectElement>("convention");

for (const [value, label] of CONVENTIONS) {
  convention.append(new Option(label, value));
}
byId("disclaimer").textContent = DISCLAIMER;
byId<HTMLAnchorElement>("privacy").href = PRIVACY_PAGE;
byId<HTMLAnchorElement>("report").href = getReportUrl({
  gameID: null,
  variant: null,
  convention: null,
});

function show(settings: typeof DEFAULT_SETTINGS) {
  enabled.checked = settings.enabled;
  slowDevice.checked = settings.slowDevice;
  mistakesOnly.checked = settings.mistakesOnly;
  convention.value = settings.convention;
}
show(DEFAULT_SETTINGS);
loadSettings()
  .then(show)
  .catch(() => undefined);

const save = () => {
  saveSettings({
    enabled: enabled.checked,
    slowDevice: slowDevice.checked,
    mistakesOnly: mistakesOnly.checked,
    convention: convention.value,
  }).catch(() => undefined);
};
for (const input of [enabled, slowDevice, mistakesOnly, convention]) {
  input.addEventListener("change", save);
}
