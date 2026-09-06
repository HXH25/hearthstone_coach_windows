# HearthCoach V0.5.0.6 — Robust Card Art + Auto Guide

This release addresses two issues that can look related in-game but are actually separate.

## 1. `libpng warning: iCCP: known incorrect sRGB profile`

That message means a PNG contains a malformed embedded ICC color profile. The pixels are normally still usable. V0.5.0.6 avoids relying on that metadata:

- prefer HDT `Images/CardPortraits/*.jpg` and `Images/CardTiles/*.jpg`;
- use full-card PNG only as a later fallback;
- strip the optional PNG `iCCP` chunk before Rust-side decode;
- try every candidate file for a CardId before falling back to a text tile.

The guide therefore remains usable even if one machine has a stale/corrupt PNG cache.

## 2. Blank composition guide

A black guide with only `阵容指南` + an AI status line means there is no selected composition/watchlist yet; it is not an image-decoder failure.

New behavior:

- `agent.auto_prepare_guide` defaults to `true`;
- once live Battlegrounds available tribes arrive, composition analysis starts automatically;
- the first HDT-validated composition is selected automatically and its stage watchlist is generated;
- users can still use `换阵容` afterwards;
- if prerequisites are missing, the top guide explicitly says whether it is waiting for tribes, model/API configuration, composition analysis, or watchlist generation.

Set `"auto_prepare_guide": false` to restore the manual analyze/select flow.
