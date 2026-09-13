# Fix: reserve image height in Markdown table rows

**Status:** ✅ Complete
**Branch:** `fix/124-table-image-height`
**Date:** 2026-09-04
**Issue:** #124

## Summary

A Markdown table cell containing an image reserved only a text line of height, so
the image was clipped to a thin strip. Measured on `main` @ d982ad2 with a
240x180 PNG in a four-column table: **32 px of 180 px visible, 82 % clipped**,
identical across all four rows.

## Two independent causes

### 1. The row-height estimator does not know about images

`table_cell_height` accounts for wrapped text, inline-code chunks
(`cell_visual_lines`) and inline math (`cached_inline_math_height`). It has no
arm for `Event::Start(Tag::Image)`, so an image contributes nothing to the row
height and the row collapses to its text height.

### 2. The first observed image size is not a layout change

`CommonMarkCache::observe_image_size` is already called for every painted image,
and already bumps `layout_revision` when a size *changes*:

```rust
match self.image_sizes.insert(uri.to_owned(), size) {
    Some(previous) if (previous - size).length_sq() > 0.25 => self.mark_layout_changed(),
    _ => {}
}
```

`HashMap::insert` returns `None` the first time a key is written, so the arm that
marks the layout dirty is never reached on first observation. Images load
asynchronously: the first paint measures a row with no size known, the size
arrives on a later frame, and nothing asks for a re-measurement. The row stays at
its wrong first value.

Both had to change. Fixing only the estimator leaves the first paint wrong with
no re-measure; fixing only the revision re-measures a row that still ignores
images.

## Key discoveries

### The cache already recorded what was needed

`Image::end` has always called `cache.observe_image_size(&self.uri, response.rect.size())`.
The data was there; nothing read it back, and the first write did not invalidate
anything. Adding `observed_image_size()` and the `None` arm was enough — no new
plumbing, no texture polling, no extra state.

### The URI has to be resolved the same way the paint path does

`Image::new` applies scheme rules — `file://` for absolute paths,
`options.default_implicit_uri_scheme` for relative ones. Measuring with the raw
`dest_url` would miss the cache on every relative path (the common case) and the
row would silently stay text-height with no error anywhere. The measurement path
therefore calls `crate::Image::new(dest_url, options).uri`, which is why
`table_cell_height` needed an `options` parameter.

### A visual metric can be wrong in a way that looks plausible

First measurement of the fix reported "8 image strips of 32 px" — apparently no
better than the 4×32 px before. The detector keyed on colour saturation, and the
white `IMG1` label painted *through the middle* of each image split every block
into two runs. Measuring against the background colour instead gives 4×68 px.
The wrong metric produced a number that was internally consistent and completely
misleading.

## Verification

| build | image height per row |
|---|---|
| `main` d982ad2 | 34, 35, 34, 34 px (source is 180 px) |
| this branch | 68, 68, 68, 68 px |

68 px is the correct scaled height: 180 x (94 px column / 240) ~= 70. Aspect
ratio was never the problem — the row simply did not reserve room.

A table with no images renders **pixel-identical** between the two builds.

## Future improvements

- The pre-load fallback is `column_width.min(line_height * 8.0)` — a square-ish
  guess. A wide thin image over-reserves for one frame until its real size
  arrives. Reading intrinsic size from the texture loader before first paint
  would avoid the guess, at the cost of duplicating egui's load path.
- HTML tables (`render_html_table`) have no event stream and still use the
  `cell.lines().count() + cell.len()/60` heuristic, so `<img>` inside an HTML
  table is not covered here.
