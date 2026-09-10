# gflow art

Source of truth: `SVG/Artboard *.svg`. Every other file here is derived from them.

| Artboard | Content |
|---|---|
| 1 | White icon + wordmark on gradient, with drop shadow |
| 3 | Ink icon + wordmark |
| 4 | Gradient icon + ink wordmark |
| 5 | Ink icon only |
| 6 | Gradient icon only (same as `packaging/chocolatey/icon.png`) |

## Colors

Four colors do all the work. The gradient is radial: start at the light corner
(top-left on the icon, center on artboard 1), end at the far corner.

| Role | Hex |
|---|---|
| Gradient start | `#ff9424` |
| Gradient end | `#ef5a17` |
| Ink | `#1b2a4a` |
| Drop shadow (artboard 1) | `#5a2306` at 65 % |

## Wordmark

`GFLOW` in Titillium Web Bold (Google Fonts, OFL), sheared 10°, font-native
spacing and kerning plus 1 unit of tracking, converted to outlines. Every
letter is grown by a uniform 1.75 unit outward offset, with the base glyph
scaled so the cap height stays 60.45 units. That reproduces the stem weight
of the original hand-adjusted wordmark without any hand adjustment. The
lockup is centered on the artboard.

## Derived files

PNG 1x/2x/3x (512/1024/1536 px, transparent background) and JPG 1x/2x
(quality 100, white background) are rasterized from the SVGs with headless
Chromium. `PDF/gflow.pdf` (one page per artboard) is printed from the SVGs
with headless Chromium: paths and gradients stay vector, the artboard 1 drop
shadow is rasterized.

`gflow.eps` still holds the previous purple `G-FLOW` art. It came from
Illustrator and has not been regenerated.
