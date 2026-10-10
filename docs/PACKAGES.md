# Paket kataloğu (taslak)

Amaç: eksik kalmasın. Her şey katalogda olur; **profil** neyin varsayılan seçili geleceğini belirler. Faz 1'de her satırın URL'si, sessiz argümanı, tespit kuralı ve imzalayanı resmi kaynaktan doğrulanıp `catalog/*.toml`'a yazılır. Bu tablodaki argümanlar başlangıç bilgisidir, doğrulanmadan kullanılmaz.

**Profiller:** R = Önerilen, G = Oyun (R'yi içerir), D = Geliştirici, L = Eski sistem / eski uygulama, Hepsi = tümü.
**Kaynak:** v1 = eski offline pakette vardı, v2 = Tauri manifest'inde vardı, yeni = bu planda eklendi.
**Mimari kuralı:** x64 sistemde x86 + x64 kurulur (32-bit uygulamalar için x86 şart). ARM64 sistemde varsa arm64 + x64 + x86. x86 sistemde x64/arm64 atlanır.

## 1. Visual C++ Redistributable

| Paket | Mimari | Tür / sessiz argüman | Tespit | Profil | Kaynak |
| --- | --- | --- | --- | --- | --- |
| VC++ 2005 SP1 (ATL/MFC güvenlik güncellemesi, 8.0.61001) | x86, x64 | exe `/q` | uninstall / msi-product | R | v1 |
| VC++ 2008 SP1 (MFC güvenlik güncellemesi, 9.0.30729.6161) | x86, x64 | exe `/q` | msi-product | R | v1 |
| VC++ 2010 SP1 (10.0.40219.325) | x86, x64 | exe `/q /norestart` | registry `VC\VCRedist` / msi | R | v1 |
| VC++ 2012 Update 4 (11.0.61030) | x86, x64 | exe `/install /quiet /norestart` | uninstall | R | v1 |
| VC++ 2013 Update 5 (12.0.40664) | x86, x64 | exe `/install /quiet /norestart` | registry `VisualStudio\12.0\VC\Runtimes` | R | v1 |
| VC++ v14 (2015-2022 / 2026, en son) | x86, x64, arm64 | exe `/install /quiet /norestart` | registry `VisualStudio\14.0\VC\Runtimes\<arch>` `Installed`+`Bld` | R | v1, arm64 yeni |

Not: VS 2026 ile v14 redistributable'ın adı/linki değişmiş olabilir (aka.ms/vc14). Faz 1'de güncel resmi link kontrol edilecek.

## 2. .NET

| Paket | Mimari | Tür | Tespit | Profil | Kaynak |
| --- | --- | --- | --- | --- | --- |
| .NET Framework 3.5 (2.0/3.0 dahil) | - | **DISM** `NetFx3` (exe değil) | dism-feature | R | v1 (dotnetfx35.exe yerine) |
| .NET Framework 4.8.1 | - | exe `/q /norestart` | registry `NDP\v4\Full` `Release >= 533320` | R (sadece eksikse; Win11'de zaten var) | v2 |
| .NET Framework 4.8 | - | exe `/q /norestart` | `Release >= 528040` | yalnızca 4.8.1 desteklenmeyen eski Win10 build'lerinde otomatik | v1 |
| .NET Desktop Runtime 10 (LTS) | x64, x86, arm64 | exe `/install /quiet /norestart` | registry `dotnet\Setup\InstalledVersions\<arch>\sharedfx\Microsoft.WindowsDesktop.App` sürüm | R | yeni |
| .NET Desktop Runtime 9 | x64, x86, arm64 | aynı | aynı | R | v2 |
| .NET Desktop Runtime 8 (LTS) | x64, x86, arm64 | aynı | aynı | R | v2 |
| .NET Desktop Runtime 7 (desteği bitti) | x64, x86 | aynı | aynı | G, L | yeni |
| .NET Desktop Runtime 6 (desteği bitti) | x64, x86 | aynı | aynı | G, L | yeni |
| .NET Desktop Runtime 5 / .NET Core 3.1 (desteği bitti) | x64, x86 | aynı | aynı | L | yeni |
| ASP.NET Core Runtime 8 / 9 / 10 | x64, x86 | exe `/install /quiet /norestart` | `sharedfx\Microsoft.AspNetCore.App` | D | yeni |

Desktop Runtime, temel .NET Runtime'ı da içerir; ayrıca "Runtime" paketi gerekmez.

## 3. Oyun

| Paket | Mimari | Tür | Tespit | Profil | Kaynak |
| --- | --- | --- | --- | --- | --- |
| DirectX End-User Runtime (June 2010, tam offline) | - | directx: `/Q /T:<dir>` aç, `DXSETUP.exe /silent` | dosya `System32\d3dx9_43.dll` + `SysWOW64` | R | v1, v2 |
| DirectPlay (Windows özelliği) | - | DISM `DirectPlay /All` | dism-feature | G | yeni |
| OpenAL 1.1 (Creative oalinst) | - | exe `/s` | dosya `OpenAL32.dll` | G | v1 |
| NVIDIA PhysX System Software (legacy dahil) | - | exe/msi sessiz | uninstall `NVIDIA PhysX` | G | v1 |
| XNA Framework 4.0 Redist | - | msi `/qn` | msi-product | G | v1 |
| XNA Framework 3.1 Redist | - | msi `/qn` | msi-product | L | yeni |
| Vulkan Runtime (LunarG) | x64 | exe `/S` | dosya `vulkan-1.dll` | G (genelde ekran kartı sürücüsüyle gelir; sadece eksikse) | v2 |

## 4. Web ve uygulama platformları

| Paket | Mimari | Tür | Tespit | Profil | Kaynak |
| --- | --- | --- | --- | --- | --- |
| Microsoft Edge WebView2 Runtime (Evergreen) | x64, x86, arm64 (sisteme uygun olan) | online: bootstrapper; offline: standalone `/silent /install` | registry `EdgeUpdate\Clients\{F3017226-...}` `pv` | R (sadece eksikse) | v1, v2 |
| Windows App Runtime (Windows App SDK 1.x) | x64, x86, arm64 | exe `--quiet` | appx `Microsoft.WindowsAppRuntime.*` | R | yeni |
| WinGet / App Installer (+ VCLibs, UI.Xaml bağımlılıkları) | - | appx/msixbundle (Add-AppxProvisionedPackage) | appx `Microsoft.DesktopAppInstaller` | R (sadece eksikse; LTSC/Server) | v2 |
| PowerShell 7 (LTS) | x64, arm64 | msi `/qn ADD_PATH=1` | msi / uninstall | D | yeni |

## 5. Java

| Paket | Mimari | Tür | Tespit | Profil | Kaynak |
| --- | --- | --- | --- | --- | --- |
| Eclipse Temurin JRE 8 | x64, x86 | msi `/qn ADDLOCAL=FeatureMain,FeatureEnvironment,FeatureJarFileRunWith,FeatureJavaHome` | uninstall / registry `Eclipse Adoptium` | G (Minecraft ve eski Java uygulamaları için) | v1 (Oracle yerine) |
| Eclipse Temurin JRE 21 (LTS) | x64 | aynı | aynı | D | yeni |
| Eclipse Temurin JRE 25 (LTS) | x64 | aynı | aynı | D | yeni |
| Eclipse Temurin JDK 21 / 25 | x64 | aynı | aynı | D | yeni |
| Oracle Java 8 JRE | x64, x86 | sadece online, manuel lisans | - | Hepsi (bilgi amaçlı, varsayılan kapalı) | v1 |

Not: v1'deki `java_x64.msi` (2.5 MB) ve `java_x86.msi` (2 MB) gerçek bir JRE için fazla küçük; yeni katalogda kullanılmayacak.

## 6. Office ve veri

| Paket | Mimari | Tür | Tespit | Profil | Kaynak |
| --- | --- | --- | --- | --- | --- |
| Visual Studio 2010 Tools for Office Runtime (VSTOR) | x86+x64 tek paket | exe `/q /norestart` | registry `VSTO Runtime Setup\v4R` | R | v1, v2 |
| Microsoft Access Database Engine 2016 Redist | x64 veya x86 (Office bitliğine göre, ikisi birden kurulamaz) | exe `/quiet` | uninstall | D | yeni |
| Microsoft ODBC Driver 18 for SQL Server | x64, x86 | msi `/qn IACCEPTMSODBCSQLLICENSETERMS=YES` | msi-product | D | yeni |
| Microsoft OLE DB Driver 19 for SQL Server | x64, x86 | msi `/qn IACCEPTMSOLEDBSQLLICENSETERMS=YES` | msi-product | D | yeni |

## 7. Geliştirici (isteğe bağlı)

| Paket | Mimari | Tür | Profil |
| --- | --- | --- | --- |
| Python 3 (güncel kararlı) | x64, arm64 | exe `/quiet InstallAllUsers=1 PrependPath=1` | D |
| Node.js LTS | x64, arm64 | msi `/qn` | D |

## 8. Eski bileşenler (varsayılan kapalı)

| Paket | Tür | Profil | Not |
| --- | --- | --- | --- |
| Visual Basic 6.0 SP6 Runtime | exe sessiz | L | Resmi Microsoft kaynağı kullanılacak |
| MSXML 4.0 SP3 | msi `/qn` | L | Desteği bitmiş, bilinen güvenlik açıkları var; GUI'de uyarı ile |
| Visual F# 4.1 Runtime | msi `/qn` | L | Nadiren gerekir |
| Media Feature Pack (yalnızca Windows N sürümleri) | DISM capability | otomatik (N sürümü tespit edilirse önerilir) | |

## Dahil edilmeyenler

| Paket | Neden |
| --- | --- |
| v1 `vbcrun.msi` | Kaynağı belirsiz repack; yerine resmi VB6 runtime |
| .NET Framework 4.8 tam installer'ı (her sistemde) | Win10 1903+ ve Win11'de zaten var; sadece gerekli eski build'lerde otomatik |
| WebView2 x86 standalone (x64 sistemde) | Sisteme uygun mimari yeterli |
| MSXML 6, UCRT (KB2999226), DirectX 11/12 | Windows 10/11'de yerleşik |
| Silverlight, Flash Player, Games for Windows Live, Adobe AIR | Desteği bitmiş ve/veya resmi indirme yok |
| Codec paketleri, tarayıcılar, genel uygulamalar | Runtime değil; kapsam dışı (ileride ayrı bir "uygulamalar" eklentisi düşünülebilir) |

## Varsayılan profil özeti

- **Önerilen (R):** tüm VC++ (2005 - v14), .NET 3.5, .NET Framework 4.8.1 (eksikse), .NET Desktop 8 / 9 / 10, DirectX June 2010, WebView2 (eksikse), Windows App Runtime, WinGet (eksikse), VSTOR.
- **Oyun (G):** R + DirectPlay, OpenAL, PhysX, XNA 4.0, Vulkan (eksikse), .NET Desktop 6 / 7, Temurin JRE 8.
- **Geliştirici (D):** ASP.NET Core, Temurin JRE/JDK 21 + 25, PowerShell 7, Python, Node.js LTS, veri sürücüleri.
- **Eski (L):** .NET 5 / Core 3.1, XNA 3.1, VB6, MSXML 4, F# 4.1.
