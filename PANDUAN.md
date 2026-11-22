# Panduan hopilo Padupilo

Padupilo menyatukan satu video dengan beberapa klip suara (misalnya dari OmniVoice), lalu memberi subtitle otomatis beranimasi. Hasilnya satu MP4 siap upload ke YouTube Shorts.

## Isi folder

```
Padupilo/
  Padupilo.exe        aplikasinya
  PANDUAN.md          panduan ini
  bin/                ffmpeg & whisper.cpp (terisi otomatis)
  model/              model pengenal suara (terisi otomatis)
  kamus/              kamus koreksi kata
  proyek/             file proyek .padupilo
  hasil/              video & .srt hasil ekspor
```

Folder ini portable: boleh dipindah ke mana saja atau disalin ke PC lain. Tidak ada yang dipasang ke Windows.

## Pertama kali

1. Buka `Padupilo.exe`.
2. Di bagian atas muncul kotak kuning **Komponen belum lengkap**. Tekan **Unduh komponen**.
3. Tunggu sampai selesai (sekitar 600 MB: ffmpeg, whisper.cpp, dan model Whisper small). Kalau PC punya GPU NVIDIA, otomatis diunduh versi yang memakai GPU.

Langkah ini hanya sekali.

## Membuat video

1. **Pilih video** di tab *Klip Suara* (atau seret file video ke jendela).
2. **+ Tambah klip suara**: pilih semua klip sekaligus. Urutan mengikuti angka di nama file (Klip1, Klip2, Klip_3, ...). Bisa diubah dengan tombol Naik/Turun.
3. **Mode penempatan**:
   - *Per Adegan*: untuk video yang terbagi per adegan. Klip ke-1 masuk adegan ke-1, dan seterusnya.
   - *Berurutan*: untuk video satu aliran. Klip disambung dengan jeda yang bisa diatur.
   - *Manual*: ketik sendiri detik mulai tiap klip.
4. **Kecepatan** tiap klip (0,85×–1,15×): geser ke kiri kalau narasinya terlalu cepat. Nada suara tidak berubah.
5. Buka tab **Subtitle**, tekan **Transkripsi semua klip**.
6. Periksa teksnya dan betulkan kata yang salah dengar. Tekan **Ingat koreksi ke kamus** supaya kesalahan yang sama otomatis dibetulkan di video berikutnya.
7. Pilih **gaya**: Beast (ala MrBeast), Karaoke, atau Minimal hopilo. Atur warna sorot, ukuran, posisi, dan jumlah kata per potongan. Klik timeline di bawah untuk melihat pratinjau di detik mana saja.
8. Tab **Audio**: atur volume musik latar video asli (bawaan -18 dB) atau bisukan.
9. Tekan **Ekspor MP4**. Hasilnya ada di folder `hasil/`.

Simpan proyek dengan **Simpan** supaya bisa dibuka lagi tanpa transkripsi ulang. File `.padupilo` juga bisa dibuka dengan klik dua kali kalau dikaitkan ke Padupilo.exe.

## Peringatan yang mungkin muncul

| Peringatan | Artinya | Yang bisa dilakukan |
| --- | --- | --- |
| Jumlah adegan tidak sama dengan jumlah klip | Deteksi adegan menemukan lebih banyak/sedikit adegan | Geser *Kepekaan* lalu *Deteksi ulang*, atau pakai mode Berurutan/Manual |
| Klip X melebihi adegan X | Narasi lebih panjang dari adegannya | Naikkan sedikit kecepatan klip itu, atau biarkan |
| Suara lebih panjang dari video | Narasi terakhir akan terpotong | Percepat klip atau kurangi jeda |

## Catatan

- Pratinjau menampilkan gambar diam di detik yang dipilih; suara terdengar di hasil ekspor.
- Ekspor memakai GPU NVIDIA bila tersedia, kalau tidak memakai CPU (lebih lambat).
- Kerasnya suara hasil disetarakan ke -14 LUFS, standar YouTube.
