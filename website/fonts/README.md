# Fonts

The site is self-hosted: it loads no font from another origin.

**Geist** (variable, weights 400-700) and **Geist Mono** (variable, weights 400-500), by The Geist Project Authors, under the SIL Open Font License 1.1 (`OFL.txt`, copied from <https://github.com/vercel/geist-font/blob/main/OFL.txt>).

Source: the WOFF2 files served by Google Fonts for the families `Geist` (file version `v5`) and `Geist Mono` (file version `v6`), downloaded on 2026-11-25 from the URLs below. Only the `latin`, `latin-ext` and `vietnamese` subsets are used; the `unicode-range` of each `@font-face` rule in `../index.html` is the one Google Fonts publishes for that subset.

| File | Source URL | SHA-256 |
|---|---|---|
| `geist-latin.woff2` | https://fonts.gstatic.com/s/geist/v5/gyByhwUxId8gMEwcGFU.woff2 | `19f9c92546aa300c312235e3125af1b81394d8db9a4bc4a425cd5b641d2d54e1` |
| `geist-latin-ext.woff2` | https://fonts.gstatic.com/s/geist/v5/gyByhwUxId8gMEwSGFWfOw.woff2 | `824f485b5d26e2f2da3c2b236132ece1bc8e4e43373452950bb0e40548b4313f` |
| `geist-vietnamese.woff2` | https://fonts.gstatic.com/s/geist/v5/gyByhwUxId8gMEwTGFWfOw.woff2 | `8fa40e5d248247735eb97a0bd593b8852440430600d6ba01364c31fe0abc1fe1` |
| `geist-mono-latin.woff2` | https://fonts.gstatic.com/s/geistmono/v6/or3nQ6H-1_WfwkMZI_qYFrcdmg.woff2 | `684ad5b531f81d43c1e8c7038262d5db7cdc1f68006e04d6c7769efa8d33c8cc` |
| `geist-mono-latin-ext.woff2` | https://fonts.gstatic.com/s/geistmono/v6/or3nQ6H-1_WfwkMZI_qYFrkdmgPn.woff2 | `1a189eb997c3e2ece68373e387afaec9e8617424186c4b1ab3cff7c54ba6223b` |
| `geist-mono-vietnamese.woff2` | https://fonts.gstatic.com/s/geistmono/v6/or3nQ6H-1_WfwkMZI_qYFrgdmgPn.woff2 | `d39b60889a94a527a7f73c7988a2d6efb6c081614aef683b0b386c02d74c2175` |

Check: `sha256sum *.woff2`.
