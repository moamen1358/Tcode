# Sample fixtures

A grab-bag of files in common formats, used to exercise the viewer's inline
rendering, panning/zoom, and drag-and-drop path insertion across the file types
it claims to support. Nothing here is application logic — they exist only as test
inputs. Drop one onto a pane (or click it in the sidebar) and confirm it renders,
plays, or inserts its path as expected.

## Layout

| Folder                | What's in it                                              |
|-----------------------|----------------------------------------------------------|
| `code/`               | Source/text files in many languages and config formats   |
| `documents/`          | Office + PDF documents (rendered off the UI thread)       |
| `images/`             | Raster and vector images in assorted encodings            |
| `media-and-binaries/` | Audio, video, fonts, archives, and opaque binaries        |

## Contents

### `code/`
Plain-text and source files for syntax/preview handling:
`app.js`, `build.sh`, `config.toml`, `data.json`, `hello.py`, `index.html`,
`lib.rs`, `main.c`, `Makefile`, `notes.md`, `plain.txt`, `settings.yaml`,
`table.csv`.

### `documents/`
Office and PDF formats that render as documents:
`letter.odt`, `notes.docx`, `report.pdf`, `sheet.ods`, `sheet.xlsx`,
`slides.odp`, `slides.pptx`.

### `images/`
Raster and vector formats for the image viewer (zoom / pan):
`icon.ico`, `photo.bmp`, `photo.jpg`, `photo.png`, `photo.tiff`, `photo.webp`,
`vector.svg`.

### `media-and-binaries/`
Playable media plus non-previewable binaries (should fall back gracefully):
`audio.ogg`, `clip.mp4`, `song.mp3`, `tone.wav`, `video.webm` (media);
`archive.zip`, `font.ttf`, `program.bin` (binaries).
