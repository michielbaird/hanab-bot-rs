export interface SiteRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type ReplayKind = "finished" | "game" | "leader" | "follower";

export function getSiteLayout(
  winW: number,
  winH: number,
  kind: ReplayKind,
): {
  canvas: SiteRect;
  elements: Record<string, SiteRect>;
  bottomRow: Record<string, SiteRect>;
  controls: SiteRect[];
};
