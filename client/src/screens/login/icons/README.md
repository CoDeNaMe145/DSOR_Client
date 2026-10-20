# GitHub mark

`github.svg` is `mark-github-16.svg` from GitHub's
[Octicons](https://github.com/primer/octicons/blob/main/icons/mark-github-16.svg),
distributed under the included MIT `LICENSE`.

`github.png` is the same vector rendered at 96 × 96 with a white fill and a
transparent background. The login footer tints it at runtime. It is embedded in
the client binary and does not depend on the game asset conversion.

To regenerate it with `resvg-py`:

```python
from pathlib import Path
import resvg_py

icons = Path("client/src/screens/login/icons")
png = resvg_py.svg_to_bytes(
    svg_string=(icons / "github.svg").read_text(encoding="utf-8"),
    width=96,
    height=96,
    style_sheet="path { fill: white; }",
)
(icons / "github.png").write_bytes(png)
```
