export const REPOSITORY = "https://github.com/michielbaird/hanab-bot-rs";
export const EXTENSION_PAGE = `${REPOSITORY}/tree/main/extension`;
export const PRIVACY_PAGE = `${EXTENSION_PAGE}/PRIVACY.md`;

/** A new issue with what is needed to look into it already filled in. */
export function getReportUrl(details: {
  gameID: number | null;
  variant: string | null;
  convention: string | null;
}): string {
  const lines = [
    `Extension: ${chrome.runtime.getManifest().version}`,
    `Game: ${details.gameID === null ? "-" : `https://hanab.live/replay/${details.gameID}`}`,
    `Variant: ${details.variant ?? "-"}`,
    `Convention: ${details.convention ?? "-"}`,
    "",
    "What happened?",
  ];
  const query = new URLSearchParams({
    title: "Hanab Analysis: ",
    body: lines.join("\n"),
  });
  return `${REPOSITORY}/issues/new?${query.toString()}`;
}
