# iPacked — Release notes

## 1.2.1 (12 November 2026) — hotfix

- **Fixed: endless "iPacked wants to access your Documents folder" prompts.** 1.2.0 shipped with an unsigned app bundle, so macOS couldn't remember the permission and asked again for every helper call, even after you clicked Allow. The app and all its helpers are now signed together (ad-hoc) under one stable identity, so macOS asks once and remembers.
- If you allowed or denied iPacked many times in 1.2.0, reset it once: `tccutil reset SystemPolicyDocumentsFolder com.ipacked.app`
- Cleanup: removed unused code left over from the adowngrader removal.

## 1.2.0 (12 November 2026)

**Universal app**
- iPacked is now a **universal app**: it runs natively on both Apple Silicon and Intel Macs, without Rosetta. This keeps it working after Rosetta is phased out in future macOS releases (macOS 28).
- All bundled helpers (ipatool, the legacy iOS helper, fpdecrypt, tsschecker, bundle-icon) are universal too. On Apple Silicon, App Store downloads and signing run natively and faster.
- The release DMG is now `iPacked_<version>_universal.dmg`.

**Turkish (Türkçe)**
- **Settings → Language** is back, with English and **Turkish** built in.
- The Turkish text is a first draft translated from English. Fix any phrase with **Edit texts**: your changes are saved to `~/Library/Application Support/iPacked/lang/tr.json` and override the built-in translation. You can also add a new language there.
- Technical terms (dump, IPA, blob, jailbreak, SSH…) stay in English.

**Removed**
- adowngrader.deb is no longer bundled. It was a third-party package that iPacked shipped but never used (no button called it). This also makes the app smaller.
- The separate arm64 copy of ipatool is gone (the universal ipatool replaces it).

## 1.1.1 (12 November 2026) — hotfix

- **New disk icon for the DMG:** the mounted iPacked disk now shows its own drive icon instead of the app icon.
- **Settings → About → Special thanks:** credits for Impactor, ipatool, blobsaver, idevice, tsschecker and AppleDB, with links.
- Release DMGs are made by `scripts/make-dmg.sh` (sets the volume icon and no longer needs Finder automation permission).

## 1.1.0 (12 November 2026)

- **Runs on macOS 12 Monterey and later.** The bundled ipatool and fpdecrypt helpers were built only for the newest macOS, so App Store sign-in and downloads failed on older systems. They now target macOS 12, and iPacked itself requires macOS 12 (macOS 11 is no longer supported because ipatool needs macOS 12).
- **App icon fixed:** the icon no longer shows a thin transparent edge. On macOS 12–15 it follows Apple's icon grid with a soft shadow, and on macOS 26+ the Icon Composer layers are now full-bleed.
- Releases now ship as a `.dmg` (`iPacked_<version>_x64.dmg`).
- Open-source release: new README, `.gitignore`, and an icon build script (`scripts/make-icons.swift`).

## 1.0.1 (12 November 2026)

- **Platforms row:** switching back to iPhone & iPad (or any platform) works in the All store; the choice is kept for that app.
- **Apple Watch:** the app page explains that Watch apps usually ship inside the iPhone app, so a Watch download is the iPhone IPA that carries the Watch app.
- **Failed downloads can be retried:** a failed download (e.g. an old version without a Watch app) shows "Failed" with the reason but no longer blocks the download buttons. Downloads also remember the store platform they were queued for.
- **Right-click in the Library:** Open, Show in Archive, Get license, App Store page, Copy App ID / bundle ID, and **Remove from list** for apps added by ID. Apps added by ID whose name couldn't be found are looked up again.
- **Right-click in the Archive:** new **Show in Library**.
- **Esc** closes what's open (menu, window, panel, search, selection, developer page, app details) and never leaves full screen.
- **Top bar:** connected device, downloads and dumps (with counts), refresh this screen, open archive folder, search. Shortcuts: ⌘1–⌘5 switch sections, ⌘R refreshes, ⌘F searches.

## 1.0.0 (12 November 2026) — first release

**Library**
- New **All** store: every purchase on every platform in one list. Search covers iPhone/iPad and Mac apps together. Opening an app picks its own platform for versions and downloads.
- **iPhone & iPad** now shows only iPhone/iPad apps.
- Fixed: switching apps quickly could leave the previous app's version list on screen.

**Platforms in the archive**
- Every IPA shows what it is for: **Apple TV**, **Apple Watch**, **Apple Vision**, **iMessage app** or **iMessage stickers**, plus **+ Watch app** for iPhone apps that carry a Watch app. Details show the platform and "Minimum tvOS / watchOS / visionOS".
- tvOS, watchOS and visionOS icons are drawn by macOS from the app's asset catalog, in their real shapes (wide rounded rectangle for Apple TV, circle for Apple Vision / Watch). Existing archive entries get them on the next archive scan.
- Apple Watch downloads work: most Watch apps ship inside the iPhone app, which used to fail with "downloaded package does not declare WatchOS support".

**Settings**
- Removed the old Language section.
- File name examples use Minecraft. New installs default to `<Appname> v<version> (<BuildNo>) (<ExternalVersionID>)` → "Minecraft v0.3.3 (0.3.3.1) (9470435)".
- About shows the version and its release date.

## 0.1.21 (12 November 2026)

- **Fixed sign-outs and "aes.KeyUnwrap(): integrity check failed":** the App Store session lived in ~/.ipatool, shared with every other ipatool copy. An older iPast build still running re-encrypted it with its own key, so iPacked couldn't read it. iPacked now keeps its session in its own folder (~/Library/Application Support/iPacked/ipatool). Delete the old iPast app.
- **Fixed downloads and lists waiting for minutes:** every App Store call waited in one line behind the shared helper process, so a download could wait for a whole version-name batch or purchases refresh. Calls no longer queue: when the helper is busy they run in their own process (at most two extra at a time), and version-name batches always run separately.
- Unreadable sessions now say "Not signed in… sign in again" instead of the raw error.
- Removed the store country picker: Apple only gives licenses for the account's own country.

## 0.1.20 (12 November 2026)

- **Apple Vision** store added (search, purchases, versions, downloads). Apple Vision has no storefront in some countries (e.g. Turkey); search then lists from the US store.
- **App Store search** now uses Apple's own App Store web search first (same results and order as the App Store), then fills up with the iTunes search API. Exact app names are found much more reliably.
- **One app on several platforms:** universal purchases share one App ID. The app page shows a **Platforms** row (iPhone & iPad, Mac, Apple TV, Apple Watch, Apple Vision, read from the app's App Store page with the minimum OS for each); click one to see and download that platform's versions.
- Developer pages also list Apple Vision apps.
- **Fixed:** Archive.org icons were missing after the rename (cached icon paths still pointed to the old ~/Documents/iPast folder).

## 0.1.19 (12 November 2026)

**New name: iPacked**
- iPast is now **iPacked**, with a new app icon that has a light and a dark version (macOS switches with the system appearance).
- Your data moves automatically on first launch: ~/Documents/iPast → ~/Documents/iPacked (archive, blobs, cache), plus Application Support and Logs. Stored file paths are updated.
- Window settings kept in the web view (theme, store country, sidebar width) start fresh once.

**Speed and memory**
- Idle memory use was ~550 MB: every metadata lookup re-read, parsed and rewrote the whole 18 MB cache file. The cache now stays in memory and is written at most every 3 seconds; old cache entries are dropped and AppleDB entries are stored slimmed down.
- The App Store helper (ipatool, ~400 MB while running) now quits after 90 idle seconds instead of 5 minutes.
- Device plug-in detection checks every 1.5 s instead of 0.8 s.
- Long lists are paged (100 per page): Library, IPA Archive and Archive.org.

**Fixes**
- Apple Watch store: "invalid platform watchos" fixed (it shows your iPhone purchases, where Watch apps live).
- Beta blobs: "could not fetch BuildManifest: end of zip not found" fixed. Some betas are listed with an encrypted OTA (.aea) link; iPacked now uses the build's IPSW from AppleDB instead, and says so when a build has no IPSW.
- Read ApNonce: rewritten as a straight port of blobsaver's GetApnonceTask. It always goes through recovery mode, reads the nonce twice to check it's frozen (retrying once like blobsaver), then reboots and reads the generator after you unlock.

**Release cleanup**
- Example IPs and a personal Archive.org item removed from the code.

## 0.1.18 (11 November 2026)

**App Store**
- New store: **Apple Watch** (next to iPhone & iPad, Mac and Apple TV).
- Store country picker: search and developer pages can use another country's App Store. Licenses still come from your account's country (Apple's rule).
- Developer pages list every app of the developer, grouped under iPhone & iPad, Mac, Apple TV and Apple Watch headings (empty groups are left out). Opening one switches to its store.
- Fixed: a developer page could not be left (the search text filtered it to nothing and hid the Back button).
- App pages show supported devices, the store country the app was found in, and a link to the developer's site. Apps only sold in another country are found and flagged.
- Clearer messages when a license can't be taken: app sold only in another country, or removed from the App Store.
- Add by ID: "id1557595935" and links with "?l=…" are cleaned up automatically.
- Empty searches say "No apps found for …".

**Archive**
- Refresh icon now takes the app's own icon from the IPA (iTunesArtwork only when the app has none).
- Mac apps: bundle ID, minimum macOS, architectures, app name and icon are read from the .pkg (on download and with Refresh icon). Details show "Minimum macOS" and no "Install on device".
- Old Turkish source labels in the archive are shown in English.

**Blobs**
- Beta firmwares come from AppleDB, so current betas can be saved ("Try save") and beta blobs you saved earlier get their version and date.
- Read ApNonce + generator from the connected device (like blobsaver: one trip to recovery mode). Needs irecovery (brew install libirecovery).
- Removed the extra "A" colour in the device editor.

**Devices**
- Set Device shows the methods in one row you scroll sideways, best fit first. Every method has a guide.

**Other**
- Sidebar toggle sits next to the iPast title.
- Buttons in the app details window wrap to a new row with spacing.
- Remaining Turkish texts removed ("Lisans al", "Mimari", "Harici ID", the language editor, some backend messages).

## 0.1.17 (11 November 2026)

**English only**
- The leftover Turkish texts are gone (for example "Son" on the latest version, the dump method notes, some tooltips and errors). Dates are always English.
- The built-in Turkish translation is removed. Translations will be downloadable later. Old language files with app names in them are no longer used.

**Devices**
- **Set Device** replaces the Guide button. It opens the device panel only when you click it.
- The panel lists every dump method with what it needs: archive, Clutch 1.4 / 2.0.4, dumpdecrypted, flexdecrypt, bfdecrypt, CrackerXI+, frida-ios-dump, bagbak + Frida and TrollDecrypt. Methods that don't fit the device's iOS are shown greyed out and can't be used. Guides are on each method.
- Plugging in or removing an iPhone / iPad is detected right away (no Refresh needed).
- Right-click a section in the sidebar for its actions (e.g. Devices → Install IPA, SSH, Set Device, Guides).

**Archive**
- Right-click or the details window → **Refresh icon** re-extracts the icon from the IPA (prefers the 512px iTunesArtwork).
- Archive.org list: Name and Bundle ID are separate columns and sort separately. Every column sorts low → high or high → low (click the header again or use the arrow button).

**Other**
- Settings → Appearance: System / Light / Dark.
- Password fields have a show/hide eye button and a Caps Lock indicator.
- Icons reviewed: decrypting now uses a key instead of the open lock (which stays for jailbreak), plus clearer icons for description, What's New, macOS apps, Clutch install and languages.

## 0.1.16 (11 November 2026)

**English everywhere**
- The whole app is now in English: interface, guides, messages and backend errors. English is the base language.
- The old Turkish texts remain as an optional "Türkçe (incomplete)" translation. A better language system will come later.

**Device log**
- Everything iPast does with a connected device is logged: ideviceinstaller / ideviceinfo calls, the helper (AFC, icons, app data), every SSH / Clutch command and its output, and install steps.
- The log is written to `~/Library/Logs/iPast/device.log` (rotated at 5 MB).
- Devices → **Log** shows it live, with "Copy" and "Show file" buttons.
- Passwords are never logged.

**IPA Archive**
- "Select to dump" switches to list view automatically.
- IPAs that already have a dump in the archive show an **Already dumped** warning.
- Hovering the **Encrypted** label shows the buying Apple ID right away.
- Dump queue: two versions of the same app can be queued back to back. If the previous job left that app on the device, it is removed and the next version is installed and dumped. Apps you installed yourself are still never overwritten.

**Naming**
- New token `<Appname>`: the name shown on the device.
- `<name>` is now the App Store name. Dumps keep the original IPA's store name for it.
- Dumped IPAs are named after the app as it appears on the phone.

**Library**
- The version history decrypt button is now a labelled **Download + Decrypt** button.

**Archive.org importer**
- The list no longer jumps to the top while app info is being read.
- Sorting (Name / Bundle ID, Min iOS, **Size**) is separate from the search box. Column headers are clickable.
- Icons found by the importer are kept for the archived file. Icon detection in IPAs is also more lenient: any `*icon*.png` in the app root counts.

**App Store sign-in**
- Apple returns the same answer for "wrong password" and "2FA code needed", so it can't be told apart on the first try. The message now says both.
- If a code was entered and Apple still refuses, iPast says the email/password is wrong (or the code expired) instead of asking for 2FA again.

**Interface**
- Buttons visibly press in when clicked.
- The Downloads / Dumps counters now sit inside the button instead of jumping to the top-left corner.


## 0.1.15 (11 Kasım 2026)

**Arşiv (IPA Archive)**
- Kaydırma düzeltildi: arşiv listesi (ve Ayarlar sayfası) artık aşağı kayıyor.
- Liste görünümünde Finder'daki gibi klavyeyle gezinme:
  - ↑/↓ satırlar arasında gezer.
  - → uygulamayı açar, ← kapatır veya üst uygulamaya döner.
  - Enter/Boşluk ayrıntıları açar.
- Sağ tık menüsü: Ayrıntılar, Finder'da göster, Cihaza kur, Cihazda çöz, Bilgileri / Yolu / Bundle ID'yi kopyala, App Store sayfası, Sil.
- Ayrıntılar penceresindeki düğmeler artık pencereyi kapatmadan çalışıyor.
- **Dökülecekleri seç** artık filtre gibi çalışıyor: sadece şifreli (FairPlay), cihazın iOS sürümüne uyan ve Apple ID'si cihazda yetkili IPA'lar listeleniyor. Cihaz bilgileri her seferinde yeniden okunmadığı için açılış hızlandı.
- Tıklayınca kopyalama sadece liste görünümünde, Bundle ID hücresinde. Kart görünümünde metin normal şekilde seçilip kopyalanabiliyor. Dosya adı artık kopyalanmıyor.
- "Sürüm" sıralaması kaldırıldı. İşe yaramayan dosya ikonu düğmesi (deleted.log) kaldırıldı.

**Kuyruklar**
- İndirme sırası sadece AppleID Library'de.
- Döküm sırası sadece IPA Archive ve Devices'ta, kendi **Dökümler** düğmesi ve sayacıyla.

**Ayarlar**
- Ayarlar artık iOS tarzı açma/kapama düğmeleriyle:
  - "Döküm alınınca arşivdeki FairPlay IPA'nın yerine döküm konsun": önceki iki onay kutusunun yerine tek ayar.
  - "Döküm alındıktan sonra uygulama cihazdan silinsin": aynı düğme "Dökülecekleri seç" çubuğunda da var.
  - "Sürüm geçmişini otomatik yükle".
- **Archive.org'dan IPA içe aktar:** Bir öğe bağlantısı girilir (ör. `https://archive.org/details/CAFERIUS-IOS-ARCH`).
  - Öğedeki IPA'lar listeleniyor. Her birinin adı, Bundle ID'si, en düşük iOS sürümü ve ikonu dosyanın tamamı indirilmeden, sadece gerekli parçaları çekilerek okunuyor (dosya başına ~0,1 sn).
  - İsim, Bundle ID veya Min iOS'a göre sıralanıp filtrelenebiliyor; zaten arşivde olanlar işaretli.
  - Seçilenler arşive indiriliyor. Altta toplam yüzde ve kalan dosya sayısıyla indirme listesi görünüyor.
- Hakkında: Made by Xmior aka Caferius (github.com/Xmior).

**Devices**
- Dosyalar → **Uygulamalar (iOS ≤7)** ve **Sistem uygulamaları** klasörlerinde, anlaşılmaz klasör adları yerine uygulama ikonu ve adı gösteriliyor.
- **Uygulama verileri** artık iTunes'taki dosya paylaşımı gibi çalışıyor ve jailbreak gerektirmiyor. Uygulamalar ikonlarıyla listeleniyor; birine girince o uygulamanın klasörü (Documents, Library, tmp) açılıyor. Dosya alıp koyma, silme ve yeni klasör oluşturma desteklenir. iPhone 4 / iOS 6.1.3 ile denendi.

**Genel**
- Kenar çubuğu yeniden adlandırıldı ve ikonları yenilendi: AppleID Library, IPA Archive, Devices, Blobs, Settings.
- Kenar çubuğu gizlenebiliyor (sol üstteki düğme) ve kenarından sürüklenerek genişletilebiliyor. Her ikisi de hatırlanıyor.

## 0.1.14 (11 Kasım 2026)

**Cihazda döküm**
- Arşiv → **Dökülecekleri seç**: Bir cihaz seçilir; sadece o cihaza uygun sürümler seçilebilir, uymayanlar soluk görünür ve nedeni yazar. Hiçbir sürümü uymayan uygulamanın tamamı soluk görünür.
  - Engelleyen durumlar: iOS sürümü, mimari, zaten şifresiz IPA, macOS paketi, cihazda zaten kurulu uygulama, cihazın yetkili olmadığı Apple ID.
  - Hangi Apple ID'lerin yetkili olduğu, cihazda kurulu App Store uygulamalarını satın alan hesaplardan anlaşılıyor.
- Seçilenler, İndirilenler panelinde **Döküm sırası** olarak tek tek işleniyor: kurulum → döküm → kaldırma. "Bitince cihazda bırak" seçeneği var. Hatalı işler tekrar denenebiliyor.
- Kütüphane'de her sürümün yanında yeni bir düğme var: **İndir + şifresini çöz**. Sürüm indirilir, sonra otomatik olarak döküm sırasına eklenir. Orijinal IPA silinmez.
- Döküm, orijinal IPA'dan App Store sürüm ID'sini, uygulama ID'sini, yayın tarihini ve Apple ID'yi alıyor. Arşiv'de dökümler orijinalinin hemen altında (↳) gösteriliyor.

**Ayarlar**
- **Dosya adları** şablonu. Kullanılabilen değerler: `<name>`, `<version>`, `<BuildNo>`, `<BundleID>`, `<ExternalVersionID>`. Varsayılan: `<name>_<version>_<BuildNo>_(<ExternalVersionID>)`.
  - Canlı örnek gösteriliyor. Boş kalan değerin parantezi otomatik siliniyor.
  - "Mevcut dosyaları yeniden adlandır" düğmesi var. Dökümlerin adına `.decrypted` ekleniyor.
- "Döküm başarılı olunca orijinal FairPlay IPA'yı sil, yerine dökümü koy" seçeneği.
- "Orijinal FairPlay IPA indirilince eski dökümü sil, yerine orijinali koy" seçeneği.
- "Sürüm geçmişini otomatik yükle" seçeneği.

**Kütüphane**
- Sürüm geçmişi artık **Sürüm geçmişini yükle** düğmesiyle yükleniyor. Yüklenirken ilerleme paneli (x / toplam, kalan süre) gösteriliyor.
- **Sürümleri yenile** düğmesi sürüm geçmişinin hemen üstüne taşındı.
- **Yanlış sürüm adı ve tarihi düzeltildi:** Eski sürümlerin bilgileri aynı anda istenince bazıları zaman aşımına uğruyordu. Bu durumda Apple'ın verdiği güncel sürüm adı ve uygulamanın ilk yayın tarihi gösteriliyordu.
  - Artık bu istekler sırayla tekrar deneniyor. Yanlış bilgi yerine "?" gösteriliyor ve eski önbellek geçersiz sayılıyor.
  - Twitter ile denendi: önceden 12.42.1 / 2009 görünen sürümler artık doğru (2.1 / Kasım 2009, 12.42 / Kasım 2026).
- Bundle ID ve uygulama ID'si tıklanınca kopyalanıyor.

**Arşiv**
- macOS paketleri ayrı bir **macOS uygulamaları** bölümünde.
- Liste görünümünde sütun başlıklarına tıklayınca sıralanıyor: ad, sürüm, yayın tarihi, Bundle ID, boyut. Bundle ID ve tarih seçenekleri sıralama menüsüne de eklendi.
- Bundle ID, dosya adı ve hash değerleri tıklanınca kopyalanıyor; tablo metni seçilebiliyor. Ayrıntılarda "Yolu kopyala" düğmesi var.
- Yayın tarihi: App Store metadata'sı uygulamanın ilk yayın tarihini veriyorsa, IPA içindeki derleme tarihi kullanılıyor.
- Liste görünümünde sürümler gerçek dosya adıyla gösteriliyor.
- "Klasörü aç" artık arşiv klasörünün içini açıyor.
- ⌘F ile aktif ekranın arama kutusuna gidiliyor. Esc ile seçim modundan çıkılıyor.

## 0.1.13 (11 Kasım 2026)

- "Cihazda yetki gerekiyor" penceresi artık çalışan yöntemi anlatıyor. iOS 6 cihazlar FairPlay yetkisini Apple'dan kendileri alamıyor. Bunun yerine iTunes'ta Apple ID yetkilendirilip o hesaptan bir uygulama iTunes ile telefona kurulmalı; bu, her Apple ID için bir kez yeterli. Twitter ile denendi: iTunes kurulumundan sonra Clutch uygulamayı 15 saniyede çözdü.

## 0.1.12 (11 Kasım 2026)

- Arayüzdeki "(iTunes gibi)" ifadeleri kaldırıldı.
- "Cihazda yetki gerekiyor" penceresi: iki adımlı doğrulama açık hesaplarda eski iOS'ta şifrenin sonuna 6 haneli kodun eklenmesi gerektiği yazıyor.

## 0.1.11 (10 Kasım 2026)

- **Mağazadan kaldırılmış Mac uygulamaları** (GarageBand 6.0.5, iBooks Author vb.): "macOS version lookup returned HTTP 404" hatası düzeltildi. Mağaza sayfası artık yoksa, satın alma geçmişindeki en yeni sürüm otomatik seçiliyor. Sürüm listesi de bu uygulamalarda çalışıyor.
  - GarageBand 6.0.5 ve iBooks Author 2.6.1 indirildi; ikisi de "Apple tarafından App Store için imzalanmış" olarak doğrulandı.
- **Kütüphane:** Hesapta olmayan, yalnızca arşivde bulunan uygulamalar ("Arşivden") artık listede görünmüyor. Elle eklenenler ve cihazdakiler görünmeye devam ediyor.

## 0.1.10 (10 Kasım 2026)

- **App Store'dan kaldırılmış uygulamalar** (ör. Talking Roby): "failed to send redownload request" (HTTP 403) hatası düzeltildi. Artık sürüm belirtilmeden ent/download isteği gönderiliyor. Talking Roby'nin son sürümü, sürüm listesi ve 1.0 sürümü indirildi.
- **iPhoto ve diğer Mac uygulamaları** ("hardware ID must contain between 1 and 20 bytes"): Mac paketi, iPhone sekmesinden istendiğinde de Mac'in donanım kimliğiyle çözülüyor. Mac paketlerine artık IPA gibi sinf yazılmaya çalışılmıyor.
- **Kütüphane ikonları:** 1000+ uygulama için tek tek yapılan Apple istekleri kısıtlanıyordu. Artık 150'lik toplu isteklerle soruluyor; başarısız olanlar sonra tekrar deneniyor.
- **Kütüphane açılışı:** Son liste önbellekten anında gösteriliyor, App Store'dan yenileme arkada sürüyor. Yenileme sırasında dönen bir gösterge ve "x / toplam" ilerlemesi görünüyor.

## 0.1.9 (10 Kasım 2026)

- **ipatool RAM:** Sürüm adları artık tek bir ipatool sürecinde alınıyor; başka uygulamaya geçince eskisi kapatılıyor. Önceden her tıklama yeni bir ~400 MB süreç açıyordu (12 tanesi aynı anda çalışırken görüldü). Diğer istekler sıcak sunucuyu bekliyor, iPast kapanınca süreç de kapanıyor.
- **Sticker paketleri** ("failed to replicate sinf… slices have different lengths"): mağaza, Manifest'teki yollardan daha az sinf döndürünce artık hata verilmiyor; eşleşenler yazılıyor. Classic Mac ile denendi.
- **iTunesMetadata:** iTunes gibi `appleId` ve `com.apple.iTunesStore.downloadInfo` (DSPersonID dahil) yazılıyor.
- **Cihazda çöz:** Uygulama cihazda zaten kuruluysa uyarıp duruyor.
- **Clutch "Killed: 9":** Bu, FairPlay'in şifre çözmeyi reddetmesi (-42005: cihaz o Apple ID için yetkili değil). iPast bunu tanıyor; uygulamayı telefonda bir kez açıp şifreyi girmenizi isteyip tekrar deniyor.
- **Diller:** Metinler `~/Library/Application Support/iPast/lang/*.json` dosyalarında. Ayarlar → Dil → "Metinleri düzenle / yeni dil" ile düzenlenebiliyor ve yeni dil eklenebiliyor; ekranda görülen ama çevrilmemiş metinler de listeye ekleniyor.
- SHSH'taki "Betaları göster" seçeneği kaldırıldı.

## 0.1.8 (10 Kasım 2026)

**Arşiv**
- iPast her açılışta arşiv klasörünü yeniden tarıyor. Silinen dosyalar listeden düşüyor; yeni ya da değişen dosyaların bilgileri doğrudan .ipa'dan okunuyor. Mağazadan gelen ek bilgiler (sürüm ID'si, yayın tarihi, kaynak) dosyanın SHA-256'sına göre ayrıca saklanıyor; dosya silinip geri konsa bile kaybolmuyor.
- Yeni klasör düzeni: iOS uygulamaları `Belgeler/iPast/ipa/<bundle>/`, Mac uygulamaları `Belgeler/iPast/pkg/<bundle>/` altında. Eski klasörler ilk açılışta otomatik taşınıyor.
- Boyutu ve SHA-256'sı birebir aynı olan iki dosyadan yenisi siliniyor. Adı `deleted.log` dosyasına yazılıyor (Arşiv → günlük düğmesi).
- Sıralama: A→Z, Z→A, son eklenen, boyut, sürüm sayısı.
- Bir kartın sürümleri açılınca aynı satırdaki diğer kart artık uzamıyor.
- Şifreli IPA'ların etiketinin üstüne gelince onu satın alan Apple ID görünüyor; sürüm ayrıntılarında da yazıyor.

**Cihazlar**
- Dosyalar sekmesi Finder / iFunBox gibi:
  - solda konumlar (Medya, Kök, Rootless) ve kısayollar (DCIM, Downloads, Müzik, Documents, uygulama verileri…);
  - geri/ileri, sütuna tıklayarak sıralama, liste ve ikon görünümü, dosya türüne göre ikon;
  - Cmd/Shift ile çoklu seçim, sağ tık menüsü (aç, Mac'e indir, yeniden adlandır, yolu kopyala, sil), klavye kısayolları;
  - Finder'dan sürükle-bırak ile yükleme, durum çubuğunda öge sayısı ve boyut.
- App Store uygulamalarının etiketinin üstüne gelince hangi Apple ID'ye ait olduğu görünüyor.
- Araç durumu tek satırda: ideviceinstaller, iproxy, bagbak, frida, ssh, clutch. Bir etikete tıklayınca yeniden kontrol ediliyor; yeşilken griye dönerse silinmiş demektir.
- "Clutch dosyasını ver…" düğmesi çalışmıyordu; düzeltildi.
- Log kutusu artık sıkışıp kaybolmuyor; kapatılabiliyor ve yeniden boyutlandırılabiliyor.
- Döküm yöntemleri kartı her pencere genişliğine uyuyor.
- "Cihazda çöz": "İşlem bittikten sonra cihazda tut" seçeneği. İşaretli değilse uygulama döküm sonrası cihazdan kaldırılıyor.

**Bloblar**
- "9.9." ile başlayan sürümler düzeltildi: Apple'ın OTA listesi sürümleri `9.9.<sürüm>` biçiminde veriyor (9.9.26.7.3 = iOS 26.7.3).
- Beta ve OTA blobları kaydedilemiyordu: OTA zip'indeki BuildManifest ikili plist biçiminde ve tsschecker onu okuyamıyordu. Artık XML'e çevriliyor. Beta olmayan OTA sürümlerinde de build numarasıyla olmazsa BuildManifest ile yeniden deneniyor. iPhone12,8 ile 26.7.3 (OTA) ve 27.2.1 için test edildi.
- Cihaz ikonları özelleştirilebiliyor: ikon ve renk seçimi (özel renk dahil).

**Genel**
- Pencere masaüstünde sürüklenemiyordu (izin eksikti); düzeltildi.
- Cmd+F her ekranda o ekranın arama kutusuna gidiyor (Kütüphane, Arşiv, Cihazlar, Bloblar). Esc aramayı temizliyor.
- Kenar çubuğu daraltıldı.
- macOS uygulaması indirilirken, indirme bitip paket işlenirken "Şifre çözülüyor…" yazıyor.
- "iPast yapar" İngilizcede "Done by iPast" oldu.

## 0.1.7 (10 Kasım 2026)

**Disk dolması düzeltildi**
- ipatool, Mac uygulamalarının sürüm adlarını okurken her sürümün .pkg dosyasını baştan sona (yüzlerce MB) geçici klasöre indiriyordu. Süreç kapanınca bu dosyalar silinmeden kalıyordu (bu Mac'te ~17 GB birikmişti).
  - Artık sürüm bilgisi App Store yanıtından okunuyor; hiçbir şey indirilmiyor.
- Başarısız veya iptal edilen indirmelerin yarım dosyaları (`.ipatool-encrypted` / `-decrypted`) siliniyor.
- iPast her açılışta önceki çalışmalardan kalan geçici dosyaları temizliyor.

**Clutch**
- Sadece seçilen uygulama dökülüyor. Clutch 2 için `clutch -n -d <bundle>`, Clutch 1 için `clutch <bundle>` kullanılıyor; sürüm `--version` ile anlaşılıyor.
- iOS'ta `tee` / `head` olmadığı için komutta pipe kullanılmıyor.
- Clutch'ın IPA adlarında parantez olduğu için kopyalama (scp) hata veriyordu; düzeltildi.
- iPhone 4 / iOS 6.1.3 / Clutch 2.0.4 ile uçtan uca test edildi: SSH bağlantısı, döküm, kopyalama ve cihazdaki kopyanın silinmesi.
- Döküm yöntemleri tablosunda Clutch 2.0.4 artık iOS 6–11 olarak geçiyor (iOS 6'da çalıştığı doğrulandı).

**Arayüz**
- Cihazlar ekranında "Uygulamalar / Dosyalar" geçişi, Kütüphane'deki gibi segmentli düğme oldu.

## 0.1.6 (10 Kasım 2026)

**Clutch**
- `/bin/Clutch: Permission denied` hatası düzeltildi. Cihazda Clutch hangi adla ve nerede olursa olsun (`Clutch`, `Clutch2`, `/usr/bin/...`) `/bin/clutch` adına taşınıyor ve `chmod 755` uygulanıyor. Rootless jailbreak'te konum `/var/jb/usr/bin/clutch`.
- Clutch artık internetten indirilmiyor. "Clutch kur…" sizden dosyayı istiyor: programın kendisi ya da onu içeren bir `.deb`. `.deb` verilirse içindeki Clutch programı çıkarılıp kuruluyor.
- Clutch 1.x desteği (iOS 5–7) ve çıktı yolunun (`Dumped` / `Cracked`) daha esnek bulunması.
- SSH çalışmıyorsa "Önce SSH'ı yapılandırın" penceresi çıkıyor ve ayarlara götürüyor.

**Döküm yöntemleri**
- Seçili cihazın iOS sürümüne göre uygun yöntemler gösteriliyor:
  - ideviceinstaller archive (iOS 3–6);
  - Clutch 1.4.x (iOS 5–7) ve Clutch 2.0.4 (iOS 8–11);
  - flexdecrypt, bfdecrypt, CrackerXI+;
  - bagbak + Frida (iOS 12–17);
  - TrollDecrypt.
- Gereken paket için kurulum teklif ediliyor: Clutch dosyası ya da `.deb` paketi seçiliyor, iPast SSH ile `dpkg -i` çalıştırıyor.

**Cihazda çöz** ("Mac'te çöz" düğmesinin yerine)
- Şifreli IPA bağlı cihaza iTunes gibi kuruluyor, ardından iOS sürümüne uygun araçla (Clutch / bagbak) şifresiz dökülüp arşive ekleniyor.
- Uygun olmayan cihazlar seçilemiyor ve nedeni yazıyor, ör.:
  - "iOS sürümü düşük";
  - "32bit uygulama olmaz";
  - "64bit uygulama, cihaz 32bit";
  - "Sadece iPad uygulaması";
  - macOS / tvOS / watchOS uygulaması.

**Dosyalar (AFC)**
- Cihazlar → Dosyalar: Finder benzeri gezgin, üstte "Dosyalar / <cihaz adı> / …" yolu görünüyor.
- Üç kip var: Medya (jailbreak gerekmez), Kök (AFC2, rootful jailbreak) ve Rootless (`/var/jb`).
- İndirme, yükleme, yeni klasör ve silme. Eski iOS'ta da çalışıyor.

**İndirilenler**
- Kütüphane'de iTunes benzeri indirme listesi: ilerleme çubuğu, indirilen / toplam boyut, hız ve kalan süre.
- İndirmeler duraklatılıp devam ettirilebiliyor (kaldığı yerden sürer) ya da iptal edilebiliyor. Aynı anda en fazla 2 indirme yapılıyor.
- Sürüm satırlarındaki düğmeler küçük ikonlara dönüştü. Sürüm ID'sini kopyalamak için ID'ye tıklamak yeterli.

**Diğer**
- Mac App Store uygulamaları `.pkg` olarak indiriliyor ve arşivleniyor. "not a valid zip file" hatası düzeltildi.
- adowngrader düğmeleri kaldırıldı.
- Rehberlerdeki örnek IP'ler `<your_ip>` oldu.
- Dil seçeneği: Ayarlar → Dil / Language (Türkçe / English).

## 0.1.5 (10 Kasım 2026)

**App Store / ipatool**
- ipatool artık kalıcı tek bir süreç olarak çalışıyor (`ipatool serve`). App Store imzası için kullanılan ARM emülatörü bir kez açılıyor ve açık kalıyor.
  - Önceden her istek emülatörü sıfırdan açıyordu: istek başına ~15–20 sn ve ~600 MB–1 GB RAM harcanıyordu.
  - Şimdi ilk istekten sonraki istekler ~2 sn sürüyor. Aynı anda yalnızca tek bir ipatool süreci açık oluyor; 5 dk boşta kalırsa kendiliğinden kapanıyor.
  - Uzun bir indirme sürerken gelen diğer istekler ayrı, geçici bir süreçte çalışıyor; yani indirme diğer işleri bekletmiyor.
- Go tarafında bellek sınırı tanımlandı ve her istekten sonra boşalan bellek sisteme geri veriliyor.

**SHSH Bloblar**
- Apple TV (tvOS) ve HomePod desteği eklendi. OTA olarak çıkan sürümler artık listede; bunlar tsschecker'a `-o` (OTA) ile gönderiliyor.
- Beta sürümler listede, imza durumlarıyla birlikte görünüyor. Bir beta için blob kaydedilirken BuildManifest.plist, OTA/IPSW zip dosyasından HTTP aralık istekleriyle (ZIP64 destekli) birkaç yüz KB indirilerek alınıyor ve `-m` ile veriliyor.
- Yeni filtreler: "Betaları göster", "Sadece imzalananlar". Satırlarda Beta/OTA etiketi var.

**Cihazlar / Clutch**
- Clutch SSH ile, cihazın kendi IP'sine bağlanıyor (ör. 192.168.1.109). Cihaz başına SSH ayarı yapılabiliyor: IP, port, kullanıcı ve şifre. IP boş bırakılırsa bağlantı USB üzerinden (iproxy) kuruluyor.
  - Şifreyle giriş sshpass gerektirmeden yapılıyor (SSH_ASKPASS ile).
  - Eski iOS'taki OpenSSH sürümleriyle uyumlu algoritmalar açık.
- Araç çubuğunda cihazda Clutch olup olmadığı görünüyor. `/bin/clutch`, `/usr/bin/Clutch`, `Clutch2`, `/var/jb/...` gibi yazılış ve konum farkları da algılanıyor.
- "Clutch kur": KJCracks/Clutch 2.0.4 sürümünü indiriyor ya da seçtiğiniz dosyayı alıyor, `clutch` adıyla `/bin`'e koyuyor (rootless jailbreak'te `/var/jb/usr/bin`) ve `chmod 755` uyguluyor.
- Clutch dökümü bitince cihazdaki IPA siliniyor.
- Yeni "Rehber" ekranı:
  - Clutch adım adım (OpenSSH kurulumu, root şifresi, IP, kurulum, döküm, sorun giderme);
  - bagbak + Frida adım adım (build.frida.re deposu, npm ile kurulum, frida-ps testi);
  - Cydia'ya .deb yükleme (AutoInstall, dpkg, Filza).

**Kurulum**
- "iTunes gibi kur": FairPlay'li App Store IPA'ları, IPA'nın içindeki iTunesMetadata.plist ve SINF ile kuruluyor (Windows iTunes'taki gibi). Uygulama, satın alındığı Apple ID'ye bağlı olarak yükleniyor.

## 0.1.4
- Arşivde aynı uygulamanın sürümleri gruplanıyor:
  - kartlarda "Arşivli sürümler" bölümü açılıyor;
  - Finder benzeri liste görünümü (uygulama klasörü → sürümler, geniş satırlar);
  - ikon görünümünde sürüm sayısı rozeti var.
- SHSH Bloblar ekranı:
  - cihaz başına blob koleksiyonu, cihazlara isim verme;
  - Legacy iOS Kit ve blobsaver klasörlerinden okuma, "Koleksiyona topla";
  - ipsw.me'den imza tablosu, tsschecker ile kaydetme;
  - tanımsız ECID'ler ayrı listede.

## 0.1.3
- FairPlay çözme (Mac'teki iOS uygulamaları için, SIP kapatmadan).
- Anahtar zinciri yerine dosya anahtarlığı kullanılıyor; şifre soruları kalktı.
- Satın alma / lisans alma.
- Mac App Store ve Apple TV mağazaları eklendi. Yayıncıya tıklayınca diğer uygulamaları açılıyor.
- What's New ve uygulama bilgileri gösteriliyor.
- Arşivdeki IPA'nın App Store sayfası bulunuyor.
- Uyarı metinleri seçilebiliyor. "Tümünü indir" kaldırıldı.

## 0.1.2
- Daha önce satın alınmış uygulamalar listeleniyor.
- Harici sürüm ID'sinden sürüm adı çözülüyor (toplu ve hızlı).
- Arşivde grid/liste görünümü.

## 0.1.1
- "Country code invalid" hatası düzeltildi.
- Cihaz uygulama listesinde ikonlar gösteriliyor.
- adowngrader, Cydia AutoInstall klasörüne gönderiliyor.
- Eski iOS (≤6) için legacy TLS yardımcısı eklendi.
- Dışa aktarma: archive / bagbak / Clutch.
- SF Symbols ikonları.

## 0.1.0
- İlk sürüm: cihazdaki uygulamaları listeleme, Impactor ile kurulum, ipatool ile App Store girişi, satın alınanlar ve tüm sürümleri indirme, IPA arşivi ve meta veriler.
