# CAIOR v3 - Plan

Son güncelleme: 2026-10-11. Durum: Faz 1-5 tamam (CLI + GUI + sağ alt kart, Sandbox'ta doğrulandı). Sıradaki: Faz 6 (offline paket).

## 1. Amaç

Format sonrası (veya yeni bir PC'de) işe yarar tüm runtime paketlerini tek tıkla, sorunsuz ve sessiz kuran **tek bir exe**.

- **Şimdi kur (online):** Paketleri resmi kaynaklardan indirir, doğrular, sessiz kurar. GUI görünür; paketler tek tek seçilebilir veya profil seçilir.
- **Offline paket oluştur:** Media Creation Tool gibi. İndirir ve doğrular ama kurmaz; tek dosya `.exe` (veya klasör) üretir. Bu dosya internetsiz başka PC'de aynı arayüzle kurar.
- **Offline kurulum:** Exe içinde gömülü paket bulunursa (veya yanında `caior-packages/` klasörü varsa) internet istemeden kurar.
- **Tam sessiz mod:** `caior.exe install --profile recommended --silent` hiç soru sormaz; sadece sağ alttaki kart ve bildirim görünür.

## 2. Kararlar

| Konu | Karar | Neden |
| --- | --- | --- |
| Dil | Rust (stable, 1.95+) | Tek exe, runtime bağımlılığı yok; v2 Rust kodunun bir kısmı kullanılabilir |
| GUI | **Slint** 1.x (sürüm kodlamada sabitlenecek) | WebView2 gerektirmez (Tauri'nin aksine; taze sistemde WebView2 olmayabilir). Software renderer yedeği sayesinde ekran kartı sürücüsü olmayan taze kurulumda da açılır. Bildirimsel `.slint` dili ile Bugra token'ları birebir aktarılır |
| Tasarım | Bugra design system (`design/bugra/`) | Kullanıcının kendi sistemi; dark-first, tek yeşil accent, Geist + JetBrains Mono |
| Yetki | Uygulama `asInvoker` açılır; kurulum başlayınca kendini **bir kez** yükseltilmiş `--worker` olarak başlatır, ilerleme named pipe ile gelir | Gezinme ve offline paket oluşturma UAC istemez; kurulumda tek UAC |
| İndirme | `reqwest` + rustls, kaldığı yerden devam (Range), 3 paralel indirme | OpenSSL yok, taze sistemde çalışır |
| Doğrulama | Sabit dosyada SHA256; "evergreen" (sürekli güncellenen) linklerde Authenticode imza + imzalayan izin listesi | aka.ms/latest dosyalarının hash'i sabit değil |
| Kurulum sırası | Sıralı (aynı anda tek installer); indirme kurulumla paralel ilerler | MSI/Windows Installer aynı anda iki kurulumu kaldırmaz |
| Katalog | Repo'da kategori başına TOML (`catalog/*.toml`), derlemede doğrulanıp exe'ye gömülür | İnsan okunur, yorum yazılabilir; internet olmadan da katalog var |
| Uzaktan katalog | Sonraki faz: GitHub'daki imzalı `catalog.json` (ed25519/minisign), daha yeniyse kullanılır | Exe yeniden derlenmeden link/sürüm güncellemesi |
| Binary'ler | Repo'ya asla girmez | Boyut ve lisans. Offline paketi kullanıcı kendi makinesinde resmi kaynaklardan üretir; biz hiçbir installer'ı yeniden dağıtmıyoruz |
| Diller | Arayüz TR + EN (Slint `@tr` + gettext) | Design system "baştan çok dilli" diyor |
| Hedef OS | Windows 10 1809+ ve Windows 11; x64 birincil, x86 ve ARM64 farkındalığı | |

## 3. Mimari

```
croxz_aio_runtimes/
  Cargo.toml                 workspace
  crates/
    core/                    GUI'siz çekirdek (test edilebilir)
      catalog/               TOML şema, yükleme, doğrulama, profil çözümleme
      detect/                registry, msi product code, uninstall entry, dosya sürümü, DISM, Appx
      fetch/                 indirme, cache, devam etme, SHA256, Authenticode
      install/               exe/msi/msu/appx/dism/directx yürütücüleri, çıkış kodu eşleme, zaman aşımı
      bundle/                offline paket yazma/okuma (exe + ek yük, klasör)
      plan/                  seçimden kurulum planı: bağımlılık, sıra, mimari filtresi
      events.rs              ilerleme olayları (GUI, CLI ve worker ortak)
    app/                     tek exe: GUI + CLI + worker
      main.rs                argüman ayrıştırma (clap), mod seçimi
      gui/                   Slint köprüsü, mini kart, tray
      worker.rs              yükseltilmiş kurulum süreci, named pipe sunucusu
      app.manifest           asInvoker, PerMonitorV2 DPI, uzun yol desteği
  ui/                        .slint dosyaları
    theme.slint              Bugra token'larından üretilir
    components/              Button, Chip, Checkbox, Switch, TextField, ProgressBar, Note, Dialog, Snackbar
    pages/                   install, offline, logs, settings
    mini-card.slint          sağ alt ilerleme kartı
  catalog/                   *.toml paket tanımları
  i18n/                      tr.po, en.po
  xtask/                     geliştirici araçları: catalog check, hash-refresh, theme-gen
  docs/  design/  assets/  legacy/
```

### Modlar ve CLI

```
caior.exe                                   GUI
caior.exe install --profile recommended     GUI'siz değil: mini kart + tray ile kurar
caior.exe install --profile gaming --silent tamamen sorusuz; bitince bildirim
caior.exe install --only vc-2013-x64,dx-jun2010 --exclude java-*
caior.exe pack --profile all --format exe --out D:\caior-offline.exe
caior.exe pack --profile recommended --format folder --out E:\caior
caior.exe detect --json                     neler kurulu, makine okunur çıktı
caior.exe verify                            gömülü/yan klasördeki paketleri doğrula
```

Ortak seçenekler: `--source auto|online|offline`, `--no-reboot`, `--log <dir>`, `--lang tr|en`. Çıkış kodları: 0 başarı, 3010 başarı + yeniden başlatma gerekli, 1 kısmi hata, 2 kullanım hatası, 1223 UAC reddedildi.

### Kurulum akışı

1. Katalog yüklenir (gömülü → varsa daha yeni uzaktan katalog → offline pakette gömülü katalog önceliklidir).
2. Sistem taranır: OS build, mimari, her paketin durumu (kurulu / güncel değil / eksik / bu sisteme uygun değil).
3. Seçim → plan: mimari filtresi (x86 sistemde x64 atlanır), bağımlılıklar, sıra, "zaten kurulu olanı atla".
4. Kaynak çözümleme: offline yük → yerel cache → online indirme.
5. Doğrulama (SHA256 veya imza). Başarısızsa paket kurulmaz.
6. Worker (tek UAC) sırayla kurar; her paket için zaman aşımı, çıkış kodu eşleme, MSI'larda `/l*v` log.
7. Kurulum sonrası tekrar tespit (gerçekten kuruldu mu?).
8. Özet: kuruldu / atlandı / hata; yeniden başlatma gerekiyorsa sorulur (sessiz modda sadece bildirim).

Seçenekler (Ayarlar): kurulum öncesi sistem geri yükleme noktası (varsayılan kapalı), paralel indirme sayısı, cache konumu, kurulu olanları yeniden kur.

### Offline paket formatı

- **Klasör:** `caior.exe` + `caior-packages/` (installer'lar + `bundle.json`: katalog kopyası, her dosyanın SHA256'sı, oluşturma tarihi, kaynak URL'ler).
- **Tek exe:** `caior.exe` baytları + sıkıştırılmamış ek yük (installer'lar zaten sıkıştırılmış) + sonda footer: `magic "CAIORPK1" | sürüm | index ofseti | index uzunluğu | index SHA256`. Exe çalışınca kendi dosyasının sonunu okur; her installer'ı kurmadan hemen önce temp'e çıkarır, kurulunca siler. Tüm yükü baştan açmaz.
- İmzasız exe'ye ek yük eklemek sorun değildir. İleride imzalanırsa ek yük imzadan sonra eklenemez; o zaman yükü imza kapsamı dışında tutan yöntem gerekir (faz 7'de not).

### Durum tespiti türleri

`registry` (anahtar/değer, DWORD min, sürüm karşılaştırma, 32/64 görünüm), `uninstall` (Uninstall anahtarlarında DisplayName regex + DisplayVersion), `msi-product` (ProductCode / UpgradeCode), `file-version`, `dism-feature`, `dism-capability`, `appx` (paket ailesi adı + min sürüm), `none`.

### Güvenlik kararları (Faz 3)

- Yükseltilmiş worker planı kendisi kurar: yükseltilmemiş süreçten sadece seçim (profil, id) gelir, plan veya katalog gelmez. Release derlemede worker `--catalog` kabul etmez; sadece exe'ye gömülü kataloğu kullanır.
- İlerleme, yükseltilmemiş sürecin oluşturduğu tek örnekli bir named pipe üzerinden gelir (uzak istemciler reddedilir). Worker kullanıcının seçtiği bir yola dosya yazmaz.
- Kurulumdan hemen önce installer dosyası yazma/silme/yeniden adlandırmaya kapalı olarak açılır ve yeniden doğrulanır; dosya installer bitene kadar kilitli kalır. Doğrulanan dosya ile çalışan dosya aynıdır.
- Installer'lar bir job object içinde başlar. Zaman aşımında installer ve alt süreçleri birlikte sonlandırılır; CAIOR kapanırsa çalışan installer öldürülmez (yarım kurulum riski).
- 1618 (başka kurulum sürüyor) alınırsa 30 sn arayla 3 kez yeniden denenir. İptal, çalışan installer'ı kesmez; sıradaki pakete geçmeden durur.
- İmza kontrolü WinVerifyTrust ile, iptal listesi ve ağ erişimi olmadan yapılır; offline paket de aynı şekilde doğrulanır.
- Exe C runtime'ını statik bağlar (`.cargo/config.toml`, `+crt-static`). İlk Sandbox denemesi bunu gösterdi: dinamik bağlı exe taze Windows'ta `VCRUNTIME140.dll` bulamayıp hiç açılmıyordu. Kural: exe yalnızca Windows'un kendi DLL'lerine bağlı olabilir.

## 4. Riskler ve notlar

- **Antivirüs / SmartScreen:** Kendini yükseltip sessiz installer çalıştıran imzasız exe şüpheli görünür. Yayında kod imzalama (ör. SignPath açık kaynak programı) ve VirusTotal kontrolü planlanmalı.
- **.NET 3.5:** DISM ile Windows Update'ten gelir. Tam offline için kurulu Windows sürümüne uygun `sources\sxs` cab'ı gerekir; ISO yolu gösterme seçeneği eklenecek, yoksa "online gerekli" yazılacak.
- **Evergreen linkler** (aka.ms, fwlink) zamanla farklı dosya döndürür; offline pakete o anki dosyanın hash'i yazılır, imza her seferinde kontrol edilir.
- **Lisans:** Biz installer barındırmıyoruz; offline paket kullanıcının kendi makinesinde üretiliyor. Oracle Java gibi tıklamalı lisans isteyen paketler katalogda "sadece online / manuel" olarak işaretlenir.
- **Slint lisansı:** Slint Royalty-free lisansı kullanılır; Hakkında ekranında "Made with Slint" atfı zorunlu. Proje kodu MIT.
- **Windows Sandbox:** Temiz test için kullanılır; bu makinede açık (2026-10-08).

## 4b. Katalog iş listesi (araştırma ajanlarının bulduğu şema eksikleri)

| Konu | Örnek | Öneri |
| --- | --- | --- |
| Sabit sürümler sessizce eskir | Python, Node.js, ODBC/OLE DB, VC++ v14 `min_version` | `latest_feed` alanı (python.org / nodejs.org/dist/index.json / Microsoft sayfası) + `xtask catalog outdated` |
| Eski sürümü kaldırmadan kurmak | Temurin yeniden derlemeleri (21.0.12.8 -> 21.0.12.101) MajorUpgrade tetiklemiyor | msi için `remove_related_below = true` |
| Plan dışındaki çakışmalar | ACE x86, 64-bit Office kuruluysa başarısız olur | `blocked_by` (tespit kuralı doğruysa "bu PC için değil") |
| Mimariye göre bağımlılık | OLE DB 19 sadece ARM64'te vc-v14-arm64 ister | `depends` için mimari koşulu; şimdilik planlayıcı uygulanamaz bağımlılığı sessizce atlıyor (belgelendi) |
| Bir paket diğerini kapsar | ARM64'te vc-v14-x64, ARM64 runtime'ı da kurar | `covers = [...]` |
| MSI tespitinde mimari filtresi | Node.js x64/arm64 aynı UpgradeCode | `os_arch` ile çözüldü; istenirse `msi_platform` |
| Kullanıcı başına kurulum | Python install manager sadece kullanıcı başına | Python 3.16'dan önce (tam installer kalkınca) yeni yöntem |
| IExpress sarmalayıcıları hata kodunu yutar | VC++ 2005 | Motor "başarılı ama tespit yok"u hata sayıyor (2026-10-10) |

## 5. Yol haritası

Her faz sonunda çalışır, test edilmiş bir ara sonuç olur.

| Faz | İçerik | Bitti sayılır |
| --- | --- | --- |
| **0. Hazırlık** | Klasör düzeni, `.gitignore`, belgeler, design kopyası | Tamam (2026-10-08) |
| **1. Katalog** | Workspace iskeleti, TOML şema + doğrulayıcı, `docs/PACKAGES.md`'deki tüm paketlerin URL / argüman / tespit bilgilerinin resmi kaynaklardan doğrulanması, `xtask catalog check` | Tamam (2026-10-11): 8 kategori, 94 paket, strict kontrol temiz, 100 URL yanıt veriyor. Doğrulama Sandbox kurulumlarıyla yapıldı |
| **2. Tespit + indirme** | detect modülü, fetch + cache + doğrulama, `caior detect --json` | Tamam (2026-10-10): tespit bu PC'de doğru, SHA256 + WinVerifyTrust çalışıyor |
| **3. Kurulum motoru + CLI** | Yürütücüler, plan, worker + UAC, log, `caior install` | Tamam (2026-10-11): önerilen profil temiz Sandbox'ta 22/22 paket, 287 sn, ikinci çalıştırmada "yapılacak bir şey yok". VC++ 2005/2008 ve DISM özellikleri Sandbox'ta test edilemiyor (bkz. `tools/sandbox/README.md`), Hyper-V VM gerekiyor |
| **4. GUI ana pencere** | theme.slint, bileşenler, Kur sayfası, Ayarlar, Kayıtlar | Tamam (2026-10-11): Slint (yazılımsal renderer), Bugra token'larından üretilen tema, TR/EN, koyu/açık; Sandbox'ta GUI'den 22/22 kurulum (worker + pipe yolu) |
| **5. Sağ alt kart** | Mini kart, bitiş durumu, yeniden başlatma butonu | Tamam (2026-10-11): Faz 4 ile birlikte; pencere küçültülünce ya da kapatılınca kurulum kartta sürüyor. Tray ikonu ve Windows bildirimi yapılmadı (gerekirse sonra) |
| **6. Offline paket** | `pack` (klasör, sonra tek exe), offline kaynak çözümleme, `verify` | Paket internetsiz Sandbox'ta kuruyor |
| **7. Cila + yayın** | TR/EN, uzaktan imzalı katalog, devam etme, GitHub repo, Actions ile derleme, Release, imzalama araştırması | Kullanıcı onayıyla GitHub'da v3.0.0 |

## 6. Kapanan sorular

- Ürün adı: **CAIOR** (croxz all-in-one runtimes), exe `caior.exe`. Klasör adı `croxz_aio_runtimes` şimdilik kalır; GitHub repo adı yayında `caior` olabilir.
- Proje lisansı: **MIT** (en serbest ve yaygın lisans; istenirse yayından önce değiştirilebilir).
- .NET 6/7 (desteği bitmiş): Oyun ve Eski profillerinde, Önerilen'de değil.
