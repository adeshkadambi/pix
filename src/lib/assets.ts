// Client asset paths are served by the backend's `lcu-asset` protocol (SPEC §6).
// On Windows a custom protocol is reached at http://<scheme>.localhost/<path>.
export function assetUrl(path: string | null | undefined): string | undefined {
  return path ? `http://lcu-asset.localhost${path}` : undefined;
}
