declare module "@finos/perspective/dist/esm/perspective.inline.js" {
  export function worker(): Promise<import("@finos/perspective").Client>;
  export function websocket(url: string | URL): Promise<import("@finos/perspective").Client>;
  const _default: {
    worker: typeof worker;
    websocket: typeof websocket;
  };
  export default _default;
}

declare module "@finos/perspective-viewer/dist/esm/perspective-viewer.inline.js" {
  // Side-effect import — registers <perspective-viewer> custom element
}
