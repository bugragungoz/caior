# Katalog formatı (v3)

Kategori başına bir dosya: `catalog/<kategori>.toml`. Şema kodda `crates/core/src/catalog/model.rs`, kurallar `validate.rs` içinde; bu belge ikisiyle aynı olmalı. Bilinmeyen alan hata verir.

Araçlar:

```
cargo xtask catalog check [--strict] [--file x.toml] [--id vc-*]   şema + kurallar
cargo xtask catalog probe [--file x.toml] [--json]                 URL'ler yanıt veriyor mu, nereye yönleniyor
cargo xtask catalog refresh [--file x.toml] [--force]              indir, size + sha256 + signer yaz
cargo xtask catalog summary                                        markdown tablo
```

`check` taslakta eksik hash/boyutu uyarı sayar; `--strict` (yayın) hata sayar. `refresh` dosyaları `cache/<id>/` altına indirir, SHA256 ve boyutu hesaplar, Authenticode imzasını okur ve TOML'a geri yazar (yorumlar korunur).

## Kategori dosyası

```toml
[category]
id = "vcredist"
name = { en = "Visual C++ Redistributables", tr = "Visual C++ Yeniden Dağıtılabilir Paketleri" }
description = { en = "...", tr = "..." }   # isteğe bağlı
order = 10
icon = "cpu"

[[package]]
id = "vc-2013-x64"                    # tüm katalogda benzersiz: [a-z0-9][a-z0-9-]*
name = "Visual C++ 2013 Redistributable (x64)"
description = { en = "...", tr = "..." }   # isteğe bağlı, GUI detay paneli
version = "12.0.40664.0"              # gösterilen sürüm; evergreen için "latest" olabilir
arch = "x64"                          # x86 | x64 | arm64 | neutral
os_min = "10.0.17763"                 # isteğe bağlı
os_max = "10.0.18362"                 # isteğe bağlı, dahil
os_arch = []                          # isteğe bağlı: hangi Windows mimarilerinde sunulur. Boş = varsayılan
                                      # (x86 paket: x86/x64/arm64, x64: x64/arm64, arm64: arm64). Ör. WebView2 x86: ["x86"]
only_n_edition = false                # true: sadece Windows N sürümlerinde önerilir
profiles = ["recommended", "gaming"]  # recommended | gaming | developer | legacy ("hepsi" örtük)
order = 130                           # kurulum sırası, küçük önce
depends = []                          # paket id'leri
conflicts = []                        # birlikte kurulamayan paketler (iki tarafta da yazılır)
group = "vc-2013"                     # aynı ürünün x86/x64'ü GUI'de tek satır
legacy = false                        # true: "desteği bitti" rozeti
hidden = false                        # true: listede görünmez, sadece bağımlılık olarak gelir
warning = { en = "...", tr = "..." }  # isteğe bağlı GUI uyarısı
homepage = "https://..."              # isteğe bağlı, https
notes = "Kaynaklar, tuhaflıklar"      # bakımcı notu, GUI'de görünmez

[package.source]                      # dism-feature / dism-capability'de YOK
url = "https://aka.ms/highdpimfc2013x64enu"
fallback_urls = []                    # isteğe bağlı, sırayla denenir
filename = "vcredist_2013_x64.exe"    # cache ve offline paketteki ad
size = 0                              # refresh doldurur
sha256 = "..."                        # refresh doldurur; evergreen = false ise strict'te zorunlu
evergreen = false                     # true: link hep en yeni dosyayı verir, hash sabit değil
signer = ["Microsoft Corporation"]    # kabul edilen imzalayan CN'leri; evergreen ise zorunlu
online_only = false                   # true: offline pakete konamaz
extract = { archive = "zip", entry = "oalinst.exe" }   # isteğe bağlı: indirme bir zip ise

[package.install]
kind = "exe"
args = "/install /quiet /norestart"
success_codes = [0, 1638]             # varsayılan [0]
reboot_codes = [3010, 1641]           # varsayılan [3010, 1641]; success ile kesişemez
timeout_sec = 900                     # varsayılan 1200, en az 30

[package.detect]
kind = "registry"
hive = "HKLM"
key = 'SOFTWARE\Microsoft\VisualStudio\12.0\VC\Runtimes\x64'
view = "64"
value = "Installed"
equals = 1
```

## `install.kind`

| kind | Alanlar | Motorun yaptığı |
| --- | --- | --- |
| `exe` | `args` (zorunlu, boş olabilir) | dosyayı `args` ile çalıştırır. `{log}` yer tutucusu paket başına bir log dosyası yoluna dönüşür (ör. Burn: `/quiet /norestart /log "{log}"`) |
| `msi` | `properties` (isteğe bağlı) | `msiexec /i <dosya> /qn /norestart /l*v <log> <properties>`; `properties` içine `/qn` vb. yazılmaz |
| `msu` | - | `wusa.exe <dosya> /quiet /norestart` |
| `appx` | `provisioned` | `Add-AppxPackage` / `provisioned = true` ise `Add-AppxProvisionedPackage -Online` (tüm kullanıcılar). Bağımlılık appx'leri ayrı paket + `depends` |
| `dism-feature` | `feature` (zorunlu), `all` (`/All`), `source_hint` | `DISM /Online /Enable-Feature /FeatureName:<f> [/All] /NoRestart` |
| `dism-capability` | `capability` (zorunlu) | `DISM /Online /Add-Capability /CapabilityName:<c> /NoRestart` |
| `directx` | - | `/Q /T:<temp>` ile açar, `DXSETUP.exe /silent` |

Dosya uzantısı türle eşleşmeli: exe/directx → `.exe`, msi → `.msi`, msu → `.msu`, appx → `.appx/.appxbundle/.msix/.msixbundle`. `extract` varsa `filename` `.zip`, `entry` installer'ın uzantısını taşır.

## `detect.kind`

| kind | Alanlar | Anlamı |
| --- | --- | --- |
| `registry` | `hive` (HKLM/HKCU), `key` (hive'sız yol), `view` ("32"/"64", neutral olmayan pakette zorunlu), `value?`, `equals?` (sayı veya metin), `min_dword?`, `version_value?` veya `version_name_prefix?`, `min_version?` | Anahtar (ve değer) varsa kurulu. `min_version` için `version_value` (sürüm metni tutan değer) veya `version_name_prefix` gerekir. `version_name_prefix`: anahtarın değer veya alt anahtar adları sürüm numarasıdır (ör. dotnet `InstalledVersions\x64\sharedfx\Microsoft.WindowsDesktop.App` altında `8.0.11` değeri); bu öneki taşıyan en yüksek ad bulunmalı ("8." veya her sürüm için "") |
| `uninstall` | `display_name` (regex), `publisher?` (regex), `view` ("32"/"64"/"both"), `min_version?` | Uninstall anahtarlarında eşleşen kayıt; `DisplayVersion` karşılaştırılır |
| `msi-product` | `product_code` veya `upgrade_code` (`{GUID}`), `min_version?` | MSI API ile ürün sorgusu |
| `file-version` | `path` (ortam değişkenli), `min_version?` | Dosya var mı, sürümü yeterli mi |
| `dism-feature` | `feature` | Özellik "Enabled" mı |
| `dism-capability` | `capability` | Yetenek "Installed" mı |
| `appx` | `family`, `min_version?` | Paket ailesi kurulu mu |
| `any` / `all` | `rules = [ {...}, {...} ]` (en az 2) | Kurallardan biri / hepsi |
| `none` | - | Tespit yok: "bilinmiyor", varsayılan seçili gelmez |

Kurallar: `key` başında `\`, `HKLM` veya `HKEY` olmaz. `Wow6432Node` yazmak yerine `view = "32"` kullanılır (uyarı verir).

## Doğrulama kuralları

- Kategori ve paket id'leri desen ve benzersizlik; `group` tek kategoride.
- `depends` var olan id'lere işaret eder, döngü yok; paket kendine bağlı olamaz.
- Bir bağımlılığın `order` değeri, ona bağlı paketinkinden küçük olmalı (planlayıcı `order`'a göre sıralar).
- `conflicts` var olan id'lere işaret eder, iki tarafta da yazılmalı (uyarı), bağımlılıkla çakışamaz.
- `os_arch` içinde `neutral` olamaz.
- URL'ler https (http uyarı); `filename` çıplak dosya adı.
- `sha256` 64 küçük harf hex; evergreen değilse strict'te zorunlu. `evergreen = true` → `signer` zorunlu.
- `success_codes` boş olamaz, `reboot_codes` ile kesişemez; `timeout_sec >= 30`.
- Türe ait olmayan install alanı hata (ör. msi'de `args`).
- `min_version`, `os_min`, `os_max` noktalı sayısal sürüm.
- Regex'ler derlenebilir; GUID'ler süslü parantezli.
- Her `Text` alanında `en` zorunlu, `tr` eksikse uyarı.
- `hidden` paket profile konmaz (uyarı).

## Offline paket `bundle.json`

```json
{
  "format": 1,
  "created": "2026-10-08T21:00:00Z",
  "app_version": "3.0.0",
  "catalog": { "...": "o anki katalogun tam kopyası" },
  "files": [
    { "id": "vc-2013-x64", "path": "vc-2013-x64/vcredist_2013_x64.exe", "size": 7200744,
      "sha256": "...", "source_url": "...", "signer": "Microsoft Corporation" }
  ]
}
```
