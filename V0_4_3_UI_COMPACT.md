# HearthCoach V0.4.3 — Compact UI / Top Guide

## UI changes

- Bottom-right overlay is now a compact **decision card**.
- Panel opacity default increased to `248/255`.
- Composition guide moved to a separate horizontal top overlay.
- Current-stage watchlist is laid out left-to-right instead of vertically.
- The guide attempts to load CardId-named PNG/JPG/WebP artwork from common HDT cache directories and optional `overlay.card_art_dirs`; text tiles are used when art is unavailable.
- During an active Trinket choice, the decision card replaces normal tactical actions with the Trinket role-priority plan. After `ChoiceResolved`, normal actions return automatically.

## Planner robustness

`RoundPlan.target_round` and Trinket plan target fields now deserialize with defaults. Rust overwrites target round after parsing, so a model response that omits this mechanically-known field no longer forces a full local fallback.

## New overlay config

```json
{
  "panel_width_ratio": 0.235,
  "panel_height_ratio": 0.30,
  "panel_alpha": 248,
  "guide_width_ratio": 0.76,
  "guide_height_ratio": 0.14,
  "guide_top_margin_ratio": 0.012,
  "guide_alpha": 246,
  "guide_card_limit": 7,
  "card_art_dirs": []
}
```

If HDT card artwork is not found automatically, put an artwork directory in `card_art_dirs`. Image filenames should begin with the CardId, for example `BG31_818.png`.
