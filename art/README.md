# gflow art

Source of truth: `SVG/Artboard *.svg`. Every other file here is derived from them.

| Artboard | Content |
|---|---|
| 1 | White icon + wordmark on gradient, with drop shadow |
| 3 | Dark icon + wordmark |
| 4 | Gradient icon + dark wordmark |
| 5 | Dark icon only |
| 6 | Gradient icon only (same as `packaging/chocolatey/icon.png`) |

Wordmark: Titillium Web Bold, sheared 10°, converted to outlines. The F, L and
hyphen were hand-adjusted by the original designer. The G was generated from
the font at the B's origin, then the word was re-centred on the icon.

Derived files: PNG 1x/2x/3x (513/1025/1537 px) and JPG (quality 100) are
rasterized from the SVGs. `PDF/gflow.pdf` (one page per artboard) and
`gflow.eps` (artboards stacked, 512 x 2706 pt) are vector, except the
artboard 1 background + drop shadow, which is an embedded raster layer.
