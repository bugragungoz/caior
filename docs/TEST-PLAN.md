# Test planı (3.0.0)

GitHub'dan çekilen sürümü yerel Windows'ta uçtan uca doğrulamak için. Sırayla: önce otomatik kontroller (bu PC'ye hiçbir şey kurmaz), sonra Windows Sandbox'ta gerçek kurulumlar, en son elle GUI kontrolleri. Her adımın beklenen sonucu yanında yazıyor; farklı çıkan her şey hatadır.

Claude Code ile çalışıyorsan: adımları sırayla çalıştır, sonuç dosyalarını oku ve beklenenle karşılaştır, ekran görüntülerine bak. Bulduğun hatayı düzelt, `tools/test/check.ps1`'i yeniden çalıştır, sonra commit'le.

## 0. Hazırlık

```
git pull origin main
```

- Windows 10/11 x64, Rust 1.88+ (MSVC), Git.
- Windows Sandbox açık olmalı (Windows özellikleri > Windows Sandbox). Bir seferde tek Sandbox çalışır.
- Otomatik kontrolleri **yönetici** terminalde çalıştır: uçtan uca kurulum testi (`session::tests::install_runs_a_verified_copy_and_cleans_up`) yalnızca o zaman koşar, yoksa atlanır.

## 1. Otomatik kontroller

```
powershell -ExecutionPolicy Bypass -File tools\test\check.ps1
```

Sonuçlar `C:\temp\caior-check` klasörüne yazılır (`-Results` ile değişir, `-SkipNetwork` indirme adımını atlar, `-NoBuild` release derlemesini tekrar yapmaz). Özet: `summary.txt`.

| Adım | Ne denetler | Beklenen |
| --- | --- | --- |
| `cargo test --workspace` | Çekirdek, başsız UI, GUI yardımcıları ve xtask testleri (Windows'a özel olanlar dahil) | Hepsi geçer |
| clippy | Uyarılar hata sayılır | Temiz |
| catalog check --strict | 94 paket | 0 hata, 0 uyarı |
| release build | `target\release\caior.exe` | Derlenir |
| `caior list` | Katalog | ACE 2016 dahil liste, çıkış 0 |
| `detect --json` | Tespit | Geçerli JSON, çıkış 0 |
| `install --profile all --dry-run` | "Hepsi" profili (eskiden çakışma hatası veriyordu) | Plan basılır, çıkış 0, "cannot be installed together" yok |
| İki çakışan id | `--id ace-2016-x86 --id ace-2016-x64` | Tek satır "Error: ... cannot be installed together", çıkış 2. Bu PC'de 64 bit Office varsa: x86 "cannot sit next to an installed package" ile atlanır, çıkış 0 |
| Bilinmeyen id | | Tek "Error:" satırı (eskiden "Error: Error:"), çıkış 2 |
| `--cache` yöneticisiz | Worker kullanıcının seçtiği yola yazmaz | Çıkış 2, `--download-only` önerir (yönetici terminalde bu adım yok) |
| Tek exe paket | OpenAL ile `pack` (0.6 MB indirir) | Çıkış 0, exe programdan büyük |
| Paketli exe çalışır | Paket exe'si ile `list` | Çıkış 0 |
| Tek exe'den klasör paketi | Eskiden klasördeki `caior.exe` tüm yükü taşıyordu | Klasördeki `caior.exe` boyutu = düz `caior.exe` boyutu, `bundle.json` var |
| Ekran görüntüleri | Kur, Offline, Kayıtlar, Ayarlar sayfaları; EN ve TR | `screenshots\*.png` (8 dosya) |

Ekran görüntülerinde bak:

- Taşan ya da kesilen metin yok; Türkçe sayfalarda İngilizce kalan metin yok.
- Hiçbir kontrolde odak halkası (yeşil çerçeve) görünmüyor.
- Sol altta "Windows 11 ... x64  build 2xxxx" (TR: "derleme 2xxxx").
- Kur sayfasında "Önerilen" seçili; paketlerin durumları (Kurulu / Eksik / Bu PC için değil) doğru.

## 2. Windows Sandbox kurulum testleri

Önce release exe hazır olmalı (`cargo build --release -p caior` ya da 1. bölüm). Her test kendi boş sonuç klasörünü kullanır; Sandbox bitince klasörde `DONE` dosyası oluşur. Ayrıntılar: [tools/sandbox/README.md](../tools/sandbox/README.md).

Her testte kurulumdan sonra bak:

- `staging-acl.txt`: installer'ların çalıştığı `C:\Windows\Temp\CAIOR-...` klasörünün izinleri. Yalnızca iki satır `NT AUTHORITY\SYSTEM:(OI)(CI)(F)` ve `BUILTIN\Administrators:(OI)(CI)(F)`; hiçbir satırda `(I)` yok; `Users` yok.
- `staging-left.txt`: `left=0` (klasör silinmiş).
- `logs-acl.txt` (T2, T3): oturum kayıt klasörü. SYSTEM ve Administrators `(OI)(CI)(F)`, `BUILTIN\Users:(OI)(CI)(RX)`, `(I)` yok. T1'de kayıtlar host'a eşlenmiş klasöre yazıldığı için izinler farklı görünebilir; T1'de bu dosya bilgi amaçlı.

### T1. Önerilen profil, komut satırı

```
powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Profile recommended -Results C:\temp\caior-t1
```

- `install-exit.txt`: `exit=0` ya da `exit=3010`.
- `install-output.txt`: her paket "done" (ya da "done, restart needed"); "Error:" yok.
- `second-plan.txt`: "Nothing to do." (ikinci çalıştırmada her şey kurulu).
- Süre: önceki sürümde 22 paket, yaklaşık 290 sn.

### T2. GUI ile kurulum (worker + UAC yolu)

```
powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Gui -ForceWorker -Results C:\temp\caior-t2
```

- `gui\DONE-GUI`: `done` (`timeout` değil).
- `gui\1-list.png`: Önerilen seçili. `2-progress.png`: ilerleme listesi. `3-card.png`: sağ alt kart. `4-card-finished.png`: kartta "Tamamlandı" ve kapatma (X) düğmesi. `5-finished.png`: "Tamamlandı", "N kuruldu."
- `detect-after.json`: Önerilen profildeki paketler `installed`.

### T3. "Hepsi" profili (önceden hiç başlamıyordu)

```
powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Gui -ForceWorker -GuiProfile 4 -GuiExclude "vc-2005-*,vc-2008-*,netfx-35,directplay,media-feature-pack" -Results C:\temp\caior-t3
```

Hariç tutulanlar Sandbox'ta kurulamayan paketler (bkz. tools/sandbox/README.md). Uzun sürer; çok sayıda paket indirilir.

- Kurulum başlar ve biter: `gui\DONE-GUI` = `done`.
- `2-progress.png` ve `5-finished.png`: ACE 2016'dan yalnızca biri (x64) listede.
- Başarısız paket varsa `logs\<zaman>\<paket>.log` dosyasına bak; kurulamayan paketlerin listesini rapora yaz.

### T4. Offline tek exe, ağ kapalı

Host'ta paketi oluştur (indirir, kurmaz), sonra ağı kapalı Sandbox'ta kur:

```
target\release\caior.exe pack --profile recommended --out C:\temp\caior-offline.exe
powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Exe C:\temp\caior-offline.exe -NoNetwork -Profile recommended -Results C:\temp\caior-t4
```

- `pack` çıkışı 0; "Offline package written: ... (N packages, ~1200 MB)".
- `install-exit.txt`: `exit=0` ya da `3010`; `install-output.txt`'de indirme hatası yok (her şey paketten gelir).
- Önceki sürümde: 23 paket, 1.24 GB, ağsız 22/22 kurulum.

### T5. Tek exe paketten klasör paketi

```
C:\temp\caior-offline.exe pack --profile recommended --folder --out C:\temp\caior-folder
```

- `C:\temp\caior-folder\caior.exe` yaklaşık 13 MB (GB değil); yanında `caior-packages\bundle.json`.

## 3. Elle GUI kontrolleri (bu PC'de, kurulum yapmadan)

`target\release\caior.exe`'yi çift tıkla.

- [ ] Profil çipleri, sol menü, "Sadece indir" ve "Kur" ilk tıklamada tepki veriyor (önceden her biri iki tık istiyordu).
- [ ] Paket satırına tek tık seçimi değiştiriyor; "Bu PC için değil" satırları tıklanmıyor.
- [ ] Tab ile gezinince odaktaki kontrolde yeşil çerçeve var; fareyle tıklayınca çerçeve çıkmıyor. Odaktayken Boşluk/Enter kontrolü çalıştırıyor.
- [ ] "Hepsi": Kur düğmesi etkin; ACE 2016 x86 ve x64 aynı anda işaretli değil. ACE x86'yı işaretleyince x64 kalkıyor.
- [ ] "Kurulu olanlar kontrol ediliyor..." yazarken "Oyun"a tıkla: tarama bitince "Oyun" seçili kalıyor ve liste Oyun profiline göre işaretli.
- [ ] Ayarlar > Tema "Açık", Dil "English": kapat, yeniden aç; ikisi de korunmuş (`%APPDATA%\CAIOR\settings.json`).
- [ ] Offline paket sayfasında hedef kutusunu boşalt: "Paketi oluştur" devre dışı.
- [ ] Büyük bir paket seç (ör. .NET SDK), **Sadece indir**, sonra **İptal**: düğme hemen kapanıyor ve "Bu paket bitince duracak..." yazıyor; iptal sonrası özet "Durduruldu".
- [ ] Küçük bir seçimle **Sadece indir**, hemen **Küçült**: sağ altta kart çıkıyor, bitince kart yaklaşık 8 sn sonra kapanıyor ve `caior.exe` Görev Yöneticisi'nden de kayboluyor (önceden görünmez açık kalıyordu).
- [ ] Aynısını yap ama **İptal** et: kart açık kalıyor, X ile kapanınca uygulama kapanıyor; kartın büyütme simgesi ana pencereyi geri getiriyor.
- [ ] Kurulum yokken pencereyi kapatmak uygulamayı kapatıyor.

## 4. Rapor

Gönder: `C:\temp\caior-check\summary.txt`, Sandbox testlerinin `install-exit.txt`, `staging-acl.txt`, `staging-left.txt`, `logs-acl.txt` ve `gui\*.png` dosyaları, 3. bölümde işaretlenemeyen maddeler. Hata varsa ilgili `logs\<zaman>\` klasörü.

## 5. Bilinen sınırlar

- Sandbox'ta VC++ 2005/2008 ve Windows özellikleri (.NET 3.5, DirectPlay, Media Feature Pack) kurulamaz; bunlar için Hyper-V sanal makine gerekir.
- Kod imzalama yok: SmartScreen ilk açılışta uyarabilir.
