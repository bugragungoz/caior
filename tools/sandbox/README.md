# Windows Sandbox test

Gerçek kurulum testi. Release exe Sandbox'a salt okunur bağlanır, paketler sessiz kurulur, sonuçlar host'taki `-Results` klasörüne yazılır.

```
cargo build --release -p caior
powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Profile recommended -Results C:\temp\caior-sbx
powershell -ExecutionPolicy Bypass -File tools\sandbox\start.ps1 -Profile none -Extra "--id vc-*" -Catalog catalog -Results C:\temp\caior-sbx
```

- `-Profile none -Extra "--id ..."`: sadece belirli paketler.
- `-Catalog catalog`: exe'ye gömülü katalog yerine klasördeki TOML'lar (exe'yi yeniden derlemeden katalog denemek için).
- Sonuç klasörü: `install-output.txt`, `install-exit.txt`, `detect-before/after.json`, `second-plan.txt` (ikinci çalıştırmada her şey "kurulu" olmalı), `logs/` (CAIOR oturum logları), `temp-logs/` (MSI ayrıntılı logları; Sandbox içinde Windows Installer loglaması açılır).
- Bir seferde tek Sandbox çalışır. Yeni test için açık olanı kapatın.

## Sandbox'ın sınırı

Windows Sandbox'ta Windows'un bakım yığını (servicing stack) yok (DISM logu: `Failed to bind the online servicing stack 0x80070003`). TrustedInstaller'ı açmak (`-EnableServicing`) bunu çözmez. Bu yüzden Sandbox'ta şunlar **test edilemez**, Hyper-V sanal makine gerekir:

| Paket türü | Sandbox'ta sonuç |
| --- | --- |
| WinSxS assembly kuran MSI'lar: VC++ 2005, VC++ 2008 | MSI `Error 1935` (HRESULT 0x80070003), 1603 |
| `dism-feature`: .NET 3.5 (NetFx3), DirectPlay | DISM 87 (sağlayıcı yüklenemiyor) |
| `dism-capability`: Media Feature Pack | aynı |

Bunların dışındaki her şey (VC++ 2010 - v14, .NET runtime'ları, WebView2, Java, oyun runtime'ları...) Sandbox'ta güvenilir test edilir.

## Bulunan gerçek hatalar

- 2026-10-10: Exe taze Windows'ta açılmıyordu (0xC0000135, VCRUNTIME140.dll yok). Çözüm: C runtime statik bağlandı.
- 2026-10-10: VC++ 2005 IExpress sarmalayıcısı içteki msiexec 1603 hatasını yutup 0 döndürüyor. Motor artık "başarılı ama tespit edilmedi" durumunu hata sayıyor.
