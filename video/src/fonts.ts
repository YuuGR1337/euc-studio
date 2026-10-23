import { continueRender, delayRender, staticFile } from "remotion";

// JetBrains Mono, the studio's number face, copied from the app's bundle by
// scripts/prepare-assets.sh. The interface face is the system's, which Chrome already has.
const FACES: Array<[weight: string, file: string]> = [
  ["400", "fonts/jetbrains-mono-latin-400-normal.woff2"],
  ["500", "fonts/jetbrains-mono-latin-500-normal.woff2"],
  ["700", "fonts/jetbrains-mono-latin-700-normal.woff2"],
];

// Hold the first frame until every face is loaded, or the render would flash fallback type.
const handle = delayRender("Loading fonts");
Promise.all(
  FACES.map(([weight, file]) => {
    const face = new FontFace("JetBrains Mono", `url(${staticFile(file)}) format('woff2')`, {
      weight,
    });
    return face.load().then((f) => document.fonts.add(f));
  }),
)
  .then(() => continueRender(handle))
  .catch((err) => {
    console.error(err);
    continueRender(handle);
  });
