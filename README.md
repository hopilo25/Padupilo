# hopilo Padupilo

Aplikasi Windows portable untuk menyatukan video + klip suara dan menambahkan subtitle otomatis beranimasi (Beast, Karaoke, Minimal hopilo). Dibuat dengan Rust (eframe/egui). Panduan pemakaian: [PANDUAN.md](PANDUAN.md). Rancangan: [rencana v0.1](docs/superpowers/plans/2026-11-22-padupilo-mvp.md).

## Mendapatkan Padupilo.exe

- **Lewat GitHub**: setiap push ke `main` menjalankan workflow *Build Padupilo (Windows)*. Unduh `Padupilo-windows.zip` dari tab Actions (artifact) atau dari halaman Releases bila diberi tag `v0.1.0`.
- **Di PC sendiri**: pasang Rust dari https://rustup.rs, lalu di folder ini jalankan `cargo build --release`. Hasilnya `target\release\Padupilo.exe`; salin ke folder kosong bernama `Padupilo` bersama `PANDUAN.md`.

## Struktur kode

| File | Isi |
| --- | --- |
| `src/placement.rs` | urutan klip dan tiga mode penempatan (Per Adegan, Berurutan, Manual) |
| `src/media.rs` | lokasi ffmpeg/whisper, info video, deteksi adegan, ambil frame |
| `src/transcribe.rs` | menjalankan whisper-cli dan membaca waktu tiap kata |
| `src/dict.rs` | kamus koreksi kata |
| `src/subs.rs` | potongan subtitle dan ekspor .srt |
| `src/render.rs` | menggambar subtitle per frame (3 gaya) |
| `src/export.rs` | campur suara, -14 LUFS, gabung jadi MP4 (NVENC atau x264) |
| `src/setup.rs` | unduh komponen saat pertama dipakai |
| `src/gui/` | jendela aplikasi |

## Tes

`cargo test --lib` untuk tes unit. Tes ujung-ke-ujung dengan video asli: set `PADUPILO_FYP_DIR` (folder video + klip) dan `PADUPILO_ASR` (kata per klip), lalu `cargo test --release --test fyp`.

Font Montserrat: SIL Open Font License 1.1 (`font/OFL-Montserrat.txt`).
