# Arayüz tasarımı

Kaynak: Bugra design system (`design/bugra/`). Renk, yazı, boşluk ve köşe değerleri `design/bugra/tokens.json`'dan `ui/theme.slint`'e üretilir; `.slint` dosyalarında elle hex yazılmaz.

## Temel kurallar (design system'den)

- Koyu tema varsayılan, sistem ayarını takip eder; açık tema türetilmiş karşılık.
- Tek accent: `accent` (Android yeşili). Dolgu olarak sadece birincil eylemde, ilerleme çubuğunda, seçili switch'te; üzerinde `on-accent` yazı. Yazı/çerçeve olarak `accent-ink`.
- Durum renkleri her zaman bir kelime veya ikonla: `ok` = Kurulu, `info` = Güncel değil / bilgi, `warn` = Hata (metin "Hata:" ile başlar), `muted` = Eksik.
- Gölge yok, ince çizgi var: bölümler `line`, kontroller `border-control`. Gölge sadece yüzen katmanlarda (mini kart, menü, diyalog).
- Köşeler: kart/panel `radius` 14, alan/not/menü `radius-sm` 10, her buton/chip/rozet `pill`.
- Yazı: Geist (400/500/600) ve JetBrains Mono, exe'ye gömülü (OFL lisanslı). Başlıklar 600, sıkı harf aralığı.
- Hareket: 100-300ms, `ease-out`; sistemde "animasyonları azalt" açıksa neredeyse sıfır.
- Odak: 2px `focus-ring`, 3px ofset, her etkileşimli öğede. Klavyeyle her şeye erişilir; Esc diyalog/menüyü kapatır.
- Masaüstü yoğun satırlar `control-sm` (34px) olabilir.
- Metinler: kısa, düz, cümle düzeninde, ünlem yok, emoji yok. Butonlar fiille başlar: "Kur", "Paketi oluştur", "Yeniden başlat".
- İkonlar: 24px ızgara, 1.6px çizgi, yuvarlak uçlu outline SVG (Lucide tabanlı alt küme, ISC lisansı), `muted` renkte, üzerine gelince `ink`.

## Pencereler

### 1. Ana pencere (1040x680, en küçük 880x560)

Yerel Windows başlık çubuğu (koyu mod özniteliği ile). Gövde `bg`.

```
+----------------+-----------------------------------------------------+
| CAIOR          | Runtime paketleri                                   |
|                | Eksik olanları seç, tek tıkla kur.                  |
|  > Kur         |                                                     |
|    Offline     | [Önerilen v] [Oyun] [Geliştirici] [Eski] [Hepsi]  ( Ara... ) |
|    Kayıtlar    | --------------------------------------------------- |
|    Ayarlar     | Visual C++                          6 / 6 kurulu    |
|                |  [x] VC++ 2013 Update 5   x86 x64   13 MB   Kurulu  |
|                |  [x] VC++ v14 (2015-2026) x86 x64   26 MB   Güncel değil |
|                | --------------------------------------------------- |
|                | .NET                                                |
|                |  [ ] .NET Desktop 6       x86 x64   110 MB  Eksik   |
|                |  ...                                                |
| v3.0.0         +-----------------------------------------------------+
| Tema: Sistem   | 14 paket · 412 MB indirilecek   (Sadece indir) (Kur) |
+----------------+-----------------------------------------------------+
```

- Sol menü 220px, `surface`, sağında `line` dikey çizgi. Aktif öğe `surface-2` dolgu + `ink` yazı.
- Profil çipleri: `pill`, seçili olan `accent-quiet` dolgu + check ikonu.
- Paket satırı (48px): checkbox, ad (`body`), mimari rozetleri (`small`, `muted`), boyut (`meta`, tabular), durum (ikon + kelime). x86/x64 aynı `group` ise tek satır; satır genişletilince ayrı ayrı seçilebilir.
- Satıra tıklayınca sağdan detay paneli: sürüm, kaynak URL, imzalayan, tespit sonucu, uyarı (`Note` bileşeni).
- Alt eylem çubuğu yapışkan, `surface`, üstte `line`. Birincil: "Kur" (accent dolgu). İkincil: "Sadece indir" (outline pill).

### 2. Kurulum ilerlemesi (ana pencere içinde)

- Üstte genel ilerleme: "5 / 18 · VC++ 2013 x64 kuruluyor", accent ilerleme çubuğu (6px, `pill`, iz `surface-2`).
- Liste: her paket için durum ikonu (sırada / indiriliyor % / doğrulanıyor / kuruluyor / kuruldu / atlandı / hata).
- Butonlar: "Küçült" (pencereyi gizler, mini kart açılır), "İptal" (onay diyaloğu; mevcut installer bitince durur).

### 3. Mini kart (sağ alt)

- 360x112, çerçevesiz, her zaman üstte, çalışma alanının sağ altında 12px boşlukla (görev çubuğunun üstünde, çoklu ekranda birincil ekran).
- `surface`, `radius` 14, 1px `line-strong`, `shadow-dialog`. Girişte 300ms `ease-enter` kayma.

```
+------------------------------------------+
| CAIOR                          5 / 18  [^] [x] |
| VC++ 2013 Update 5 x64                   |
| [==========-----------------]            |
| Kuruluyor...                             |
+------------------------------------------+
```

- [^] ana pencereyi geri açar, [x] iptal eder (onay ister).
- Bitince: ok ikonu + "Tamamlandı: 17 kuruldu, 1 atlandı". Hata varsa `warn` ile "1 paket kurulamadı" + "Ayrıntılar". Yeniden başlatma gerekiyorsa "Yeniden başlat" (accent) ve "Sonra" butonları; gerekmiyorsa 8 sn sonra kaybolur.
- Tray ikonu: üzerine gelince "CAIOR: 5/18". Menü: Göster, İptal, Çıkış. Bitince Windows bildirimi.
- `--silent` modunda ana pencere hiç açılmaz; sadece mini kart ve tray görünür.

### 4. Offline paket sayfası

Gerçek bir sıra olduğu için numaralı adımlar:

1. **Paketleri seç:** profil çipleri + "Listeyi düzenle". Özet: "32 paket, 1.04 GB".
2. **Çıktı biçimi:** iki seçilebilir kart: "Tek dosya (.exe)" (taşıması kolay) ve "Klasör" (daha hızlı, sonradan güncellenebilir).
3. **Hedef:** yol alanı + "Gözat". Hedef diskte yeterli yer yoksa `warn` uyarısı.

"Paketi oluştur" → ilerleme ekranı (indir → doğrula → yaz). Bitince "Klasörde göster". Sadece online kurulabilen paketler listede gri ve açıklamalı.

### 5. Kayıtlar

Geçmiş oturumlar listesi (tarih, profil, sonuç); seçince log metni `code-bg` üzerinde mono yazıyla, kaydırılabilir. "Log klasörünü aç".

### 6. Ayarlar

Tema (Sistem / Koyu / Açık), dil (Türkçe / English), paralel indirme (1-5), cache konumu, "Kurulu olanları atla" (açık), "Kurulumdan önce geri yükleme noktası oluştur" (kapalı), yeniden başlatma davranışı (sor / asla / otomatik), "Hakkında" (sürüm, lisanslar, Made with Slint).

### Diyaloglar

- "Kurulum iptal edilsin mi?" gövde: "Şu an kurulan paket bitince durur."
- "Yeniden başlatma gerekli" gövde: "3 paket yeniden başlatmadan sonra tamamlanır."
- Hata metinleri "Hata:" ile başlar ve çözümü söyler: "Hata: İndirme tamamlanamadı. İnternet bağlantını kontrol edip tekrar dene."

## Slint'e aktarım

- `xtask theme-gen`: `tokens.json` → `ui/theme.slint` (`global Theme { in-out property <bool> dark; out property <color> bg: dark ? #131110 : #f6f3ee; ... }`). `rgba` değerleri Slint `#rrggbbaa`'ya çevrilir.
- Bileşen eşlemesi: Button (primary/outline/ghost), Chip, Checkbox (design system'de yok; Switch ve Chip'ten türetilecek, `border-control` çerçeve), Switch, TextField, ProgressBar (yeni), Note, Dialog, Snackbar, IconButton, NavItem, PackageRow, StatusBadge.
- Yazı tipleri `ui/fonts/` altında, `import "fonts/Geist-*.ttf"` ile gömülü.
